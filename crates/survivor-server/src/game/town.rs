//! The safe towns ([`crate::town`]) and the runs between them.
//!
//! - In a town the player is at their **meta level**: the lasting one, earned by quests and by
//!   every kill of every run, on 1.12's own XP table. No creature follows them in, nothing is
//!   attacked or cast by itself, and the town's people stand about, the quest givers among them.
//! - Stepping out through the barrier starts a **run**: level 1 on the survivor curve, the
//!   class's starting spells and bar, level-up picks as before.
//! - Stepping back in ends the run, and so does dying: the player stands up again, alive, in the
//!   nearest town.

use std::time::Instant;

use benilla_protocol::messages::opcode;

use std::time::Duration;

use super::{
    base_health, class_power, dist2d, Game, Path, HEALTH_PER_LEVEL, MANA_PER_LEVEL, VISIBLE,
    VISIBLE_DROP,
};
use crate::data::PatrolPoint;
use crate::packets::{self, f, Fields, Update};
use crate::picks;
use crate::town::{nearest, town_at, TOWNS};

/// 1.12's XP from each level to the next (`player_xp_for_level`), levels 1-29; the meta level
/// climbs it as a character would.
const META_XP: [u32; 29] = [
    400, 900, 1400, 2100, 2800, 3600, 4500, 5400, 6500, 7600, 8800, 10100, 11400, 12900, 14400,
    16000, 17700, 19400, 21300, 23200, 25200, 27300, 29400, 31700, 34000, 36400, 38900, 41400,
    44300,
];
/// The meta level's cap, for now.
const META_MAX: u32 = META_XP.len() as u32 + 1;

/// Where the player is playing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Mode {
    /// Safe inside `TOWNS[i]`.
    Town(usize),
    Run,
}

/// The lasting level and its XP.
pub(crate) struct Meta {
    pub(crate) level: u32,
    pub(crate) xp: u32,
}

impl Default for Meta {
    fn default() -> Self {
        Self { level: 1, xp: 0 }
    }
}

fn meta_next(level: u32) -> u32 {
    META_XP.get(level as usize - 1).copied().unwrap_or(0)
}

/// A guard's walking pace, yards a second (1.12's walk speed).
const WALK: f32 = 2.5;

/// One of a town's people.
pub(crate) struct Npc {
    pub(crate) guid: u64,
    pub(crate) entry: u32,
    /// Where they stand, or where their beat's last leg began.
    pub(crate) pos: [f32; 3],
    facing: f32,
    beat: Option<Walk>,
}

impl Npc {
    pub(crate) fn here(&self, now: Instant) -> [f32; 3] {
        self.beat
            .as_ref()
            .and_then(|w| w.leg)
            .map_or(self.pos, |leg| leg.at(now))
    }
}

/// A guard walking a patrol path round and round, `side` yards to its right.
struct Walk {
    points: Vec<PatrolPoint>,
    side: f32,
    /// The point walked to next.
    next: usize,
    leg: Option<Path>,
    wait_until: Instant,
}

/// The town's people from the world data, with guids clear of the creatures'. A guard with a
/// beat starts at its path's first point.
pub(crate) fn npcs(data: &crate::data::WorldData, now: Instant) -> Vec<Npc> {
    data.npcs
        .iter()
        .enumerate()
        .map(|(i, (s, beat))| {
            let walk = data.patrols.get(&beat.patrol).filter(|p| p.len() > 1).map(|points| Walk {
                points: points.clone(),
                side: beat.side,
                next: 1,
                leg: None,
                wait_until: now,
            });
            let pos = walk.as_ref().map_or(s.pos, |w| beside(&w.points, 1, w.side));
            Npc {
                guid: 0xF130_0000_0000_0000 | u64::from(s.entry) << 24 | 0x80_0000 | i as u64,
                entry: s.entry,
                pos,
                facing: s.facing,
                beat: walk,
            }
        })
        .collect()
}

/// Point `to` of a path, moved `side` yards to the right of the leg that arrives there (WoW's x
/// is north and y west, so the right of a heading `(dx, dy)` is `(dy, -dx)`).
fn beside(points: &[PatrolPoint], to: usize, side: f32) -> [f32; 3] {
    let n = points.len();
    let (a, b) = (points[(to + n - 1) % n].pos, points[to % n].pos);
    if side == 0.0 {
        return b;
    }
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy);
    if len < 0.01 {
        return b;
    }
    [b[0] + dy / len * side, b[1] - dx / len * side, b[2]]
}

/// The mode the player is in at `pos`.
pub(crate) fn mode_at(pos: [f32; 3]) -> Mode {
    town_at(pos).map_or(Mode::Run, Mode::Town)
}

impl Game {
    pub(crate) fn in_run(&self) -> bool {
        self.mode == Mode::Run
    }

    /// Crossing a barrier, either way; asked every tick.
    pub(super) fn check_barrier(&mut self, now: Instant) {
        if self.player.dead_until.is_some() {
            return;
        }
        match (self.mode, town_at(self.player.pos)) {
            (Mode::Town(_), None) => self.start_run(),
            (Mode::Run, Some(i)) => self.enter_town(i, false, now),
            _ => {}
        }
    }

    /// Out through the barrier: a fresh run at level 1.
    fn start_run(&mut self) {
        self.mode = Mode::Run;
        self.player.level = 1;
        self.player.xp = 0;
        self.reset_character(self.curve.next_level(1));
        eprintln!("survivor: run started");
        self.send(opcode::SMSG_NOTIFICATION, &packets::notification("The run begins: level 1"));
    }

    /// Into town `i`: the run is over, the meta level shows again. `died` when the run ended in
    /// death (the player was carried here).
    fn enter_town(&mut self, i: usize, died: bool, now: Instant) {
        let reached = self.player.level;
        self.mode = Mode::Town(i);
        // An open level-up pick goes with the run.
        if self.pick.is_some() {
            picks::clear();
            self.resume(now);
        }
        self.picks_pending = 0;
        // The player's own swing stops, or the client keeps them in combat.
        if let Some(t) = self.player.target.take() {
            self.send(opcode::SMSG_ATTACKSTOP, &packets::attack_stop(self.player.guid, t));
            let mut fl = Fields::default();
            fl.guid(f::TARGET, 0);
            self.values(self.player.guid, fl);
        }
        crate::aim::set(None);
        self.player.level = self.meta.level;
        self.player.xp = self.meta.xp;
        self.reset_character(meta_next(self.meta.level));
        let name = TOWNS[i].name;
        eprintln!("survivor: safe in {name}, the run ended at level {reached}");
        let line = if died {
            format!("You fell at level {reached}. Safe in {name}.")
        } else {
            format!("Safe in {name}. The run ended at level {reached}.")
        };
        self.send(opcode::SMSG_NOTIFICATION, &packets::notification(&line));
    }

    /// The character as a level start has them: health, power, the starting spells and bar, and
    /// the level fields for `self.player.level` and `xp`, `next` to go.
    fn reset_character(&mut self, next: u32) {
        let level = self.player.level;
        self.cancel_cast();
        self.player.max_health = base_health(self.player.class) + HEALTH_PER_LEVEL * (level - 1);
        self.player.health = self.player.max_health;
        let (kind, base) = class_power(self.player.class);
        let max = if kind == 0 { base + MANA_PER_LEVEL * (level - 1) } else { base };
        self.player.power.max = max;
        self.player.power.current = if kind == 1 { 0 } else { max };
        // A run's picks are forgotten: back to the class's first spells.
        for &s in &self.player.spells {
            if !self.start_spells.contains(&s) {
                self.send(opcode::SMSG_REMOVED_SPELL, &packets::removed_spell(s));
            }
        }
        for &s in &self.start_spells {
            if !self.player.spells.contains(&s) {
                self.send(opcode::SMSG_LEARNED_SPELL, &packets::learned_spell(s));
            }
        }
        self.player.spells = self.start_spells.clone();
        let start = self.start_buttons;
        self.sort_start_bar(&start);
        self.send(opcode::SMSG_ACTION_BUTTONS, &packets::action_buttons(&self.buttons));
        let pt = u16::from(kind);
        let mut fl = Fields::default();
        fl.set(f::LEVEL, level)
            .set(f::PLAYER_XP, self.player.xp)
            .set(f::PLAYER_NEXT_LEVEL_XP, next)
            .set(f::MAXHEALTH, self.player.max_health)
            .set(f::HEALTH, self.player.health)
            .set(f::MAXPOWER1 + pt, max)
            .set(f::POWER1 + pt, self.player.power.current);
        self.values(self.player.guid, fl);
    }

    /// A dead run ends in the nearest town, standing.
    pub(super) fn carry_to_town(&mut self, now: Instant) {
        let i = nearest(self.player.pos);
        let t = &TOWNS[i];
        self.teleport_counter += 1;
        let ms = now.saturating_duration_since(self.started).as_millis() as u32;
        self.send(
            opcode::MSG_MOVE_TELEPORT_ACK,
            &packets::teleport(self.player.guid, self.teleport_counter, t.arrive, t.arrive_facing, ms),
        );
        self.player.pos = t.arrive;
        self.player.facing = t.arrive_facing;
        if self.in_run() {
            self.enter_town(i, true, now);
        } else {
            self.player.health = self.player.max_health;
            let mut fl = Fields::default();
            fl.set(f::HEALTH, self.player.health);
            self.values(self.player.guid, fl);
        }
    }

    /// Meta XP: kept whatever the mode, shown (with its level-ups) only in town.
    pub(super) fn grant_meta_xp(&mut self, xp: u32) {
        if xp == 0 || self.meta.level >= META_MAX {
            return;
        }
        self.meta.xp += xp;
        let mut gained = 0;
        while self.meta.level < META_MAX && self.meta.xp >= meta_next(self.meta.level) {
            self.meta.xp -= meta_next(self.meta.level);
            self.meta.level += 1;
            gained += 1;
        }
        if gained > 0 {
            eprintln!("survivor: meta level {}", self.meta.level);
        }
        if self.in_run() {
            return;
        }
        self.player.level = self.meta.level;
        self.player.xp = self.meta.xp;
        let mut fl = Fields::default();
        if gained > 0 {
            self.player.max_health += HEALTH_PER_LEVEL * gained;
            self.player.health = self.player.max_health;
            if self.player.power.kind == 0 {
                self.player.power.max += MANA_PER_LEVEL * gained;
                self.player.power.current = self.player.power.max;
                fl.set(f::MAXPOWER1, self.player.power.max)
                    .set(f::POWER1, self.player.power.current);
            }
            self.send(
                opcode::SMSG_LEVELUP_INFO,
                &packets::levelup_info(self.meta.level, HEALTH_PER_LEVEL),
            );
            fl.set(f::LEVEL, self.meta.level)
                .set(f::MAXHEALTH, self.player.max_health)
                .set(f::HEALTH, self.player.health)
                .set(f::PLAYER_NEXT_LEVEL_XP, meta_next(self.meta.level));
        }
        fl.set(f::PLAYER_XP, self.meta.xp);
        self.values(self.player.guid, fl);
    }

    /// Meta XP for a kill: 1.12's same-level base, nothing for a creature grey to the meta level.
    pub(super) fn meta_kill_xp(&self, level: u32) -> u32 {
        if level + 5 <= self.meta.level {
            0
        } else {
            crate::data::kill_xp(level)
        }
    }

    /// The guards on their beats: a leg walked, a wait at the point, the next leg. The pace is
    /// the path's own, so a guard and their partner at its side keep step.
    pub(super) fn walk_beats(&mut self, now: Instant) {
        for i in 0..self.npcs.len() {
            let n = &mut self.npcs[i];
            let Some(w) = n.beat.as_mut() else { continue };
            if let Some(leg) = w.leg {
                if !leg.done(now) {
                    continue;
                }
                n.pos = leg.to;
                w.leg = None;
                w.wait_until = now + Duration::from_millis(u64::from(w.points[w.next].wait_ms));
                w.next = (w.next + 1) % w.points.len();
            }
            if now < w.wait_until {
                continue;
            }
            let len = w.points.len();
            let (a, b) = (w.points[(w.next + len - 1) % len].pos, w.points[w.next].pos);
            let duration = Duration::from_secs_f32((dist2d(a, b) / WALK).max(0.05));
            let to = beside(&w.points, w.next, w.side);
            let from = n.pos;
            w.leg = Some(Path {
                from,
                to,
                start: now,
                duration,
            });
            let guid = n.guid;
            if self.npcs_shown.contains(&i) {
                let id = self.spline_id();
                self.send(
                    opcode::SMSG_MONSTER_MOVE,
                    &packets::monster_walk(guid, from, id, to, duration.as_millis() as u32),
                );
            }
        }
    }

    /// The town's people coming into view and leaving it.
    pub(super) fn update_npcs(&mut self, now: Instant) {
        let me = self.player.pos;
        let mut creates = Vec::new();
        let mut gone = Vec::new();
        for (i, n) in self.npcs.iter().enumerate() {
            let d = dist2d(n.here(now), me);
            let shown = self.npcs_shown.contains(&i);
            if shown && d > VISIBLE_DROP {
                gone.push(n.guid);
                self.npcs_shown.remove(&i);
            } else if !shown && d <= VISIBLE {
                creates.push(i);
            }
        }
        if !gone.is_empty() {
            self.send(
                opcode::SMSG_UPDATE_OBJECT,
                &packets::update_object(&[Update::OutOfRange(gone)]),
            );
        }
        for chunk in creates.chunks(40) {
            let updates: Vec<Update> = chunk
                .iter()
                .map(|&i| {
                    let n = &self.npcs[i];
                    let t = &self.data.creatures[&n.entry];
                    let health = t.health.0.max(1);
                    packets::creature_create(n.guid, t, t.level.0, health, health, n.here(now), n.facing)
                })
                .collect();
            self.send(opcode::SMSG_UPDATE_OBJECT, &packets::update_object(&updates));
            self.npcs_shown.extend(chunk.iter().copied());
        }
        if !creates.is_empty() {
            self.send_giver_statuses();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_meta_table_is_1_12s_and_climbs() {
        assert_eq!(meta_next(1), 400);
        assert_eq!(meta_next(10), 7600);
        assert!(META_XP.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(mode_at([1846.0, 1606.0, 99.0]), Mode::Town(0));
        assert_eq!(mode_at([1679.0, 1668.0, 136.0]), Mode::Run);
    }
}
