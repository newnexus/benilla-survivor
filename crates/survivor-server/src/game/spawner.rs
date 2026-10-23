//! A run's spawner, and the creatures' idle walks.
//!
//! Out of town the dead keep coming ([`crate::waves`]): a wave kind at a time, the next kind
//! every one to three minutes, a boss every four. They spawn out of sight around the player and
//! chase at once, more of them and faster the longer the run lasts. In a marked area (a Scarlet
//! camp) the area's own people come instead. Entering a town takes every spawned creature away.
//!
//! Spawn spots are the world's own creature spots near the player, so a newcomer stands on the
//! ground: the server knows no terrain.

use std::time::{Duration, Instant};

use benilla_protocol::messages::opcode;

use super::{dist2d, Game, Mob, MobState, Path};
use crate::packets;
use crate::waves::{area_at, AREAS, BOSSES, HERALDS, HERALD_LEAD, HERALD_SHARE, KINDS};

/// A spawn lands this far from the player, yards.
const SPAWN_NEAR: f32 = 25.0;
const SPAWN_FAR: f32 = 45.0;
/// The wave kind moves on after this many seconds, at random between the two.
const KIND_EVERY: (f32, f32) = (60.0, 180.0);
/// The share of a run's spawns, outside the marked areas, that are creatures the player's
/// quests want killed, while any are wanted.
const QUEST_SHARE: f32 = 0.25;
/// A boss every this many seconds.
const BOSS_EVERY: Duration = Duration::from_secs(240);
/// Spawned creatures alive at once: a start, more each minute, and a ceiling.
const CAP: (f32, f32, usize) = (10.0, 3.0, 40);
/// Seconds between spawns: a start, quicker each minute, and a floor.
const INTERVAL: (f32, f32, f32) = (3.5, 0.3, 0.8);
/// A boss's health and damage over a creature of its level, and more each round of the three.
const BOSS_HEALTH: f32 = 8.0;
const BOSS_DAMAGE: f32 = 1.4;
/// An idle stroller waits this long between strolls, ms.
const STROLL_WAIT: (u64, u64) = (3000, 10_000);

/// The spawner's clock for one run.
pub(crate) struct Waves {
    started: Instant,
    kind: usize,
    next_kind: Instant,
    next_spawn: Instant,
    next_boss: Instant,
    bosses: usize,
    /// The marked area the player was last in.
    area: Option<usize>,
    next_guid: u64,
    /// `$SURVIVOR_WAVE_PACE=8`: the spawner's clock runs that many times faster, for trying the
    /// kinds and bosses without waiting minutes.
    pace: f32,
}

impl Waves {
    pub(crate) fn new(now: Instant, first_kind_in: Duration) -> Self {
        let pace = std::env::var("SURVIVOR_WAVE_PACE")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|&p| p > 0.0)
            .unwrap_or(1.0);
        Self {
            started: now,
            kind: 0,
            next_kind: now + first_kind_in.div_f32(pace),
            next_spawn: now + Duration::from_secs(2),
            next_boss: now + BOSS_EVERY.div_f32(pace),
            bosses: 0,
            area: None,
            next_guid: 0,
            pace,
        }
    }

    /// Seconds the run has lasted, pauses left out.
    pub(super) fn survived(&self, now: Instant) -> f32 {
        now.saturating_duration_since(self.started).as_secs_f32()
    }

    pub(crate) fn shift(&mut self, by: Duration) {
        self.started += by;
        self.next_kind += by;
        self.next_spawn += by;
        self.next_boss += by;
    }
}

impl Game {
    /// A fresh spawner for a run starting now.
    pub(super) fn new_waves(&mut self, now: Instant) -> Waves {
        let first = self.rng.range(KIND_EVERY.0, KIND_EVERY.1);
        Waves::new(now, Duration::from_secs_f32(first))
    }

    /// The spawner's tick: the wave kind, the boss, the next spawn.
    pub(super) fn tick_waves(&mut self, now: Instant) {
        if !self.in_run() {
            crate::run_clock::set(None);
            return;
        }
        if self.player.dead_until.is_some() {
            return;
        }
        let Some(mut w) = self.waves.take() else { return };
        let minutes = now.saturating_duration_since(w.started).as_secs_f32() / 60.0 * w.pace;
        let area = area_at(self.player.pos);
        if area != w.area {
            w.area = area;
            if let Some(a) = area {
                eprintln!("survivor: entered {}", AREAS[a].name);
                self.send(opcode::SMSG_NOTIFICATION, &packets::notification(AREAS[a].entered));
            }
        }
        if now >= w.next_kind && w.kind + 1 < KINDS.len() {
            w.kind += 1;
            w.next_kind =
                now + Duration::from_secs_f32(self.rng.range(KIND_EVERY.0, KIND_EVERY.1) / w.pace);
            let name = self.data.creatures.get(&KINDS[w.kind]).map_or("", |t| t.name.as_str());
            eprintln!("survivor: the wave turns to {name}");
            let line = format!("The dead grow stronger: {name}");
            self.send(opcode::SMSG_NOTIFICATION, &packets::notification(&line));
        }
        // The level the run's creatures come at: with the minutes, and never far below the
        // player's own.
        let level = ((1.0 + minutes * 1.2) as u32).max(self.player.level.saturating_sub(1));
        if now >= w.next_boss {
            w.next_boss = now + BOSS_EVERY.div_f32(w.pace);
            let round = w.bosses / BOSSES.len();
            let (entry, scale) = BOSSES[w.bosses % BOSSES.len()];
            w.bosses += 1;
            self.spawn(&mut w, entry, level + 2 + round as u32 * 3, Some((scale, round)), now);
        }
        let (next, _) = BOSSES[w.bosses % BOSSES.len()];
        crate::run_clock::set(Some(crate::run_clock::Clock {
            survived: now.saturating_duration_since(w.started).as_secs_f32(),
            boss_in: w.next_boss.saturating_duration_since(now).as_secs_f32(),
            boss: self.data.creatures.get(&next).map_or_else(String::new, |t| t.name.clone()),
        }));
        let alive = self.mobs.iter().filter(|m| m.spawned && m.alive()).count();
        let cap = ((CAP.0 + CAP.1 * minutes) as usize).min(CAP.2);
        if now >= w.next_spawn && alive < cap {
            let wait = (INTERVAL.0 - INTERVAL.1 * minutes).max(INTERVAL.2);
            w.next_spawn = now + Duration::from_secs_f32(wait);
            let boss_in = w.next_boss.saturating_duration_since(now).as_secs_f32() * w.pace;
            let heralds: &[u32] = if boss_in <= HERALD_LEAD {
                HERALDS.get(w.bosses % BOSSES.len()).copied().unwrap_or(&[])
            } else {
                &[]
            };
            let targets = self.quest_targets();
            let entry = match area {
                Some(a) => {
                    let kinds = AREAS[a].kinds;
                    kinds[(self.rng.next() % kinds.len() as u64) as usize]
                }
                // The coming boss's heralds, in its last minute.
                None if !heralds.is_empty() && self.rng.unit() < HERALD_SHARE => {
                    let e = heralds[(self.rng.next() % heralds.len() as u64) as usize];
                    eprintln!("survivor: a herald of the boss, {e}");
                    e
                }
                // Now and then a creature the player's quests want.
                None if !targets.is_empty() && self.rng.unit() < QUEST_SHARE => {
                    let e = targets[(self.rng.next() % targets.len() as u64) as usize];
                    eprintln!("survivor: a creature a quest wants, {e}");
                    e
                }
                // Now and then one of the kind before, so the waves blend.
                None if w.kind > 0 && self.rng.unit() < 0.3 => KINDS[w.kind - 1],
                None => KINDS[w.kind],
            };
            self.spawn(&mut w, entry, level, None, now);
        }
        self.waves = Some(w);
    }

    /// One creature of `entry` at `level` out of sight round the player, chasing; a boss when
    /// `boss` says how much bigger and which round.
    fn spawn(&mut self, w: &mut Waves, entry: u32, level: u32, boss: Option<(f32, usize)>, now: Instant) {
        // Our own handle on the shared world data, so `self` stays free to change.
        let data = std::sync::Arc::clone(&self.data);
        let Some(t) = data.creatures.get(&entry) else { return };
        let Some(pos) = self.spawn_spot() else { return };
        let me = self.player.pos;
        let facing = (me[1] - pos[1]).atan2(me[0] - pos[0]);
        w.next_guid = (w.next_guid + 1) & 0x3F_FFFF;
        let guid = 0xF130_0000_0000_0000 | u64::from(entry) << 24 | 0x40_0000 | w.next_guid;
        let level = level.max(t.level.0);
        let mut m = Mob::new(t, level, guid, pos, facing, now);
        m.spawned = true;
        m.state = MobState::Chasing;
        m.hostile = true;
        if let Some((scale, round)) = boss {
            let more = 1.0 + round as f32;
            // `$SURVIVOR_BOSS_HEALTH=0.05`: in place of the boss's health factor, for trying a kill.
            let factor = std::env::var("SURVIVOR_BOSS_HEALTH")
                .ok()
                .and_then(|v| v.parse::<f32>().ok())
                .unwrap_or(BOSS_HEALTH);
            m.max_health = ((m.max_health as f32 * factor * more) as u32).max(1);
            m.health = m.max_health;
            m.damage = (m.damage.0 * BOSS_DAMAGE * more, m.damage.1 * BOSS_DAMAGE * more);
            m.scale *= scale;
            m.boss = true;
            eprintln!("survivor: boss {} (level {level}, {} health)", t.name, m.max_health);
            let line = format!("{} has come for you!", t.name);
            self.send(opcode::SMSG_NOTIFICATION, &packets::notification(&line));
        }
        // A dead spawned creature's slot, out of view, is reused.
        let free = self.mobs.iter().enumerate().position(|(i, o)| {
            o.spawned
                && matches!(o.state, MobState::Dead { despawn, .. } if now >= despawn)
                && !self.visible.contains(&i)
        });
        match free {
            Some(i) => self.mobs[i] = m,
            None => self.mobs.push(m),
        }
    }

    /// A world creature spot out of sight but near, not in a town and with no living creature
    /// on it: where a spawn can stand, nudged a little off the spot.
    fn spawn_spot(&mut self) -> Option<[f32; 3]> {
        let me = self.player.pos;
        let taken: Vec<[f32; 3]> = self
            .mobs
            .iter()
            .filter(|m| m.alive())
            .map(|m| m.pos)
            .collect();
        let spots: Vec<[f32; 3]> = self
            .data
            .spawns
            .iter()
            .map(|s| s.pos)
            .filter(|&p| (SPAWN_NEAR..=SPAWN_FAR).contains(&dist2d(p, me)))
            .filter(|&p| crate::town::town_at(p).is_none())
            .filter(|&p| taken.iter().all(|&t| dist2d(t, p) >= super::SPREAD))
            .collect();
        if spots.is_empty() {
            // Nowhere known: a ring round the player, at their height.
            let a = self.rng.range(0.0, std::f32::consts::TAU);
            let d = self.rng.range(SPAWN_NEAR, SPAWN_FAR);
            let p = [me[0] + a.cos() * d, me[1] + a.sin() * d, me[2]];
            return crate::town::town_at(p).is_none().then_some(p);
        }
        let p = spots[(self.rng.next() % spots.len() as u64) as usize];
        let (a, r) = (self.rng.range(0.0, std::f32::consts::TAU), self.rng.range(0.0, 1.5));
        Some([p[0] + a.cos() * r, p[1] + a.sin() * r, p[2]])
    }

    /// Into town: every spawned creature goes.
    pub(super) fn clear_spawned(&mut self, now: Instant) {
        for m in self.mobs.iter_mut().filter(|m| m.spawned && m.alive()) {
            m.state = MobState::Dead {
                despawn: now,
                respawn: now,
            };
            m.path = None;
        }
    }

    /// An idle creature's walk, as in the live game: the next point of its path, or a stroll to
    /// a spot round home, then a wait.
    pub(super) fn idle_walk(&mut self, i: usize, now: Instant) {
        let m = &self.mobs[i];
        if m.path.is_some() || now < m.idle_until {
            return;
        }
        let (to, wait_ms) = if let Some(route) = m.route {
            let points = &self.data.mob_paths[&route];
            let p = points[m.route_next % points.len()];
            (p.pos, u64::from(p.wait_ms))
        } else if m.wander > 0.5 {
            let (home, wander) = (m.home, m.wander);
            let a = self.rng.range(0.0, std::f32::consts::TAU);
            let d = self.rng.range(1.0, wander);
            let wait = STROLL_WAIT.0 + self.rng.next() % (STROLL_WAIT.1 - STROLL_WAIT.0);
            ([home[0] + a.cos() * d, home[1] + a.sin() * d, home[2]], wait)
        } else {
            return;
        };
        let m = &mut self.mobs[i];
        let from = m.pos;
        let duration = Duration::from_secs_f32((dist2d(from, to) / m.walk_speed).max(0.05));
        m.path = Some(Path {
            from,
            to,
            start: now,
            duration,
        });
        m.facing = (to[1] - from[1]).atan2(to[0] - from[0]);
        m.idle_until = now + duration + Duration::from_millis(wait_ms);
        m.route_next += 1;
        let guid = m.guid;
        if self.visible.contains(&i) {
            let id = self.spline_id();
            self.send(
                opcode::SMSG_MONSTER_MOVE,
                &packets::monster_walk(guid, from, id, to, duration.as_millis() as u32),
            );
        }
    }
}
