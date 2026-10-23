//! The profession trainers' events ([`crate::events`]): at a run's start each trainer not yet
//! freed may be found at their spot (the first chance whole, each further one 0.8 of the last;
//! a chain begun is always there). Their window opens as the player walks up, the world paused
//! while it is open. The second quest's attack comes as the player nears the spot again; on the
//! third the trainer follows the player, and walking into Deathknell with them along brings
//! them home. Home, they stand in Deathknell for good.

use std::time::{Duration, Instant};

use benilla_protocol::messages::opcode;

use super::{dist2d, Game, Path};
use crate::events::{quest_id, EVENTS};
use crate::packets::{self, Update};

/// How near the spot the second quest's attack comes.
const AMBUSH_RANGE: f32 = 22.0;
/// An escorted trainer sets off with the player from this near, keeps this far behind, and is
/// brought home if this near as the player walks into town.
const FOLLOW_START: f32 = 8.0;
const FOLLOW_GAP: f32 = 3.0;
const FOLLOW_REACH: f32 = 30.0;
/// The share of the chance each further trainer's spot has of the one before.
const NEXT_CHANCE: f32 = 0.8;
/// Out of the world: a trainer not at their spot this run.
const PARK: [f32; 3] = [0.0, 0.0, -500.0];

/// The events' state for this session.
#[derive(Default)]
pub(crate) struct Events {
    /// Each trainer's place in `npcs`.
    npc: Vec<usize>,
    /// This run's spots, by event.
    active: Vec<bool>,
    /// The second quest's attack came this run, by event.
    ambushed: Vec<bool>,
    /// The trainer walking home with the player.
    following: Option<usize>,
}

impl Game {
    /// At login: every trainer among the town's people, where they belong now.
    pub(super) fn add_event_trainers(&mut self, now: Instant) {
        if !self.events.npc.is_empty() {
            return;
        }
        for (k, e) in EVENTS.iter().enumerate() {
            let guid = 0xF130_0000_0000_0000 | u64::from(e.trainer) << 24 | 0x90_0000 | k as u64;
            self.events.npc.push(self.npcs.len());
            self.npcs
                .push(super::town::Npc::standing(guid, e.trainer, PARK, e.spot[3]));
        }
        self.events.active = vec![false; EVENTS.len()];
        self.events.ambushed = vec![false; EVENTS.len()];
        // Logged in out on a run: its trainers are rolled now.
        if self.in_run() {
            self.roll_events(now);
        }
        self.place_trainers(now);
    }

    fn step_done(&self, e: usize, step: usize) -> bool {
        self.quest_done(quest_id(e, step))
    }

    fn freed(&self, e: usize) -> bool {
        self.step_done(e, 3)
    }

    /// The chain is under way: a step taken or done.
    fn begun(&self, e: usize) -> bool {
        (1..=3).any(|s| self.step_done(e, s) || self.quest_in_log(quest_id(e, s)))
    }

    /// The escort is over: the third quest's trainer brought home, not yet handed in.
    fn brought_home(&self, e: usize) -> bool {
        self.quest_finished_in_log(quest_id(e, 3))
    }

    /// A run begins: which trainers can be found on it.
    pub(super) fn roll_events(&mut self, now: Instant) {
        if self.events.npc.is_empty() {
            return;
        }
        let freed = (0..EVENTS.len()).filter(|&e| self.freed(e)).count();
        let mut chance = NEXT_CHANCE.powi(freed as i32);
        let mut order: Vec<usize> = (0..EVENTS.len()).collect();
        for i in (1..order.len()).rev() {
            let j = (self.rng.next() % (i as u64 + 1)) as usize;
            order.swap(i, j);
        }
        // `$SURVIVOR_EVENT=<n>`: event n's spot every run and no other, for trying a chain.
        let forced: Option<usize> = std::env::var("SURVIVOR_EVENT").ok().and_then(|v| v.parse().ok());
        for e in order {
            self.events.ambushed[e] = false;
            let on = if self.freed(e) || self.brought_home(e) {
                false
            } else if let Some(f) = forced {
                f == e
            } else if self.begun(e) {
                true
            } else if self.rng.unit() < chance {
                chance *= NEXT_CHANCE;
                true
            } else {
                false
            };
            self.events.active[e] = on;
            if on {
                eprintln!("survivor: {} can be found this run", self.trainer_name(e));
            }
        }
        self.events.following = None;
        self.place_trainers(now);
    }

    /// The run is over: an escort not brought in starts again next time.
    pub(super) fn end_run_events(&mut self, died: bool, now: Instant) {
        if self.events.npc.is_empty() {
            return;
        }
        if let Some(e) = self.events.following.take() {
            // Walked in with them close: they are home.
            let near = dist2d(self.trainer_here(e, now), self.player.pos) <= FOLLOW_REACH;
            if near && !died {
                let (entry, guid) = (EVENTS[e].trainer, self.trainer_guid(e));
                self.quest_kill(entry, guid);
                eprintln!("survivor: {} brought home", self.trainer_name(e));
                // Handed in at once: the quest log is not saved, the done list is.
                self.quest_turn_in(guid, quest_id(e, 3));
            }
        }
        self.events.active.iter_mut().for_each(|a| *a = false);
        self.place_trainers(now);
    }

    fn trainer_name(&self, e: usize) -> String {
        self.data.creatures.get(&EVENTS[e].trainer).map_or_else(String::new, |t| t.name.clone())
    }

    fn trainer_guid(&self, e: usize) -> u64 {
        self.npcs[self.events.npc[e]].guid
    }

    fn trainer_here(&self, e: usize, now: Instant) -> [f32; 3] {
        self.npcs[self.events.npc[e]].here(now)
    }

    /// Whether `entry` is a trainer at their spot this run: they may speak on a run.
    pub(super) fn event_trainer_out(&self, entry: u32) -> bool {
        EVENTS
            .iter()
            .enumerate()
            .any(|(e, ev)| ev.trainer == entry && self.events.active.get(e) == Some(&true))
    }

    /// Each trainer where they belong: home once brought home, at their spot on a run that has
    /// them, else out of the world.
    fn place_trainers(&mut self, now: Instant) {
        for e in 0..EVENTS.len() {
            if self.events.following == Some(e) {
                continue;
            }
            let ev = &EVENTS[e];
            let at = if self.freed(e) || self.brought_home(e) {
                Some(ev.home)
            } else if self.in_run() && self.events.active[e] {
                Some(ev.spot)
            } else {
                None
            };
            let (pos, facing) = at.map_or((PARK, ev.spot[3]), |p| ([p[0], p[1], p[2]], p[3]));
            self.move_npc_to(self.events.npc[e], pos, facing, now);
        }
    }

    /// Put town person `i` at `pos` at once: out of view and back in, if it moved.
    fn move_npc_to(&mut self, i: usize, pos: [f32; 3], facing: f32, now: Instant) {
        let n = &mut self.npcs[i];
        if dist2d(n.here(now), pos) < 0.5 && (n.here(now)[2] - pos[2]).abs() < 0.5 {
            return;
        }
        n.pos = pos;
        n.set_facing(facing);
        n.lead = None;
        let guid = n.guid;
        if self.npcs_shown.remove(&i) {
            self.send(
                opcode::SMSG_UPDATE_OBJECT,
                &packets::update_object(&[Update::OutOfRange(vec![guid])]),
            );
        }
    }

    /// Each tick of a run: the second quest's attack, and the escort.
    pub(super) fn tick_events(&mut self, now: Instant) {
        if self.events.npc.is_empty() || !self.in_run() || self.player.dead_until.is_some() {
            return;
        }
        let me = self.player.pos;
        for e in 0..EVENTS.len() {
            if !self.events.active[e] {
                continue;
            }
            let ev = &EVENTS[e];
            let spot = [ev.spot[0], ev.spot[1], ev.spot[2]];
            // The attack: the second quest taken and not yet won, the player at the spot.
            let attack = quest_id(e, 2);
            if !self.events.ambushed[e]
                && self.quest_in_log(attack)
                && !self.quest_finished_in_log(attack)
                && dist2d(me, spot) <= AMBUSH_RANGE
            {
                self.events.ambushed[e] = true;
                eprintln!("survivor: the attack on {} comes", self.trainer_name(e));
                self.send(opcode::SMSG_NOTIFICATION, &packets::notification(ev.ambush_line));
                let level = self.player.level.max(2);
                if let Some(mut w) = self.waves.take() {
                    for &(entry, count) in ev.ambush {
                        for _ in 0..count {
                            self.spawn(&mut w, entry, level, None, now);
                        }
                    }
                    self.waves = Some(w);
                }
            }
            // The escort: the third quest taken, the player by the trainer.
            let escort = quest_id(e, 3);
            if self.events.following.is_none()
                && self.quest_in_log(escort)
                && !self.quest_finished_in_log(escort)
                && dist2d(me, self.trainer_here(e, now)) <= FOLLOW_START
            {
                self.events.following = Some(e);
                let line = format!("{} follows you. Lead them to Deathknell.", self.trainer_name(e));
                self.send(opcode::SMSG_NOTIFICATION, &packets::notification(&line));
            }
        }
        if let Some(e) = self.events.following {
            self.follow_player(e, now);
        }
    }

    /// The escorted trainer runs after the player, a few yards behind.
    fn follow_player(&mut self, e: usize, now: Instant) {
        let i = self.events.npc[e];
        let me = self.player.pos;
        let here = self.npcs[i].here(now);
        let still = self.npcs[i].lead.is_none_or(|l| l.done(now));
        let d = dist2d(here, me);
        if !still && d < FOLLOW_REACH {
            return;
        }
        if d <= FOLLOW_GAP + 0.5 {
            if let Some(l) = self.npcs[i].lead.take() {
                self.npcs[i].pos = l.at(now);
            }
            return;
        }
        let k = (d - FOLLOW_GAP) / d;
        let to = [
            here[0] + (me[0] - here[0]) * k,
            here[1] + (me[1] - here[1]) * k,
            me[2],
        ];
        let speed = packets::STOCK_SPEEDS[1];
        let duration = Duration::from_secs_f32((dist2d(here, to) / speed).max(0.05));
        let n = &mut self.npcs[i];
        n.pos = here;
        n.lead = Some(Path {
            from: here,
            to,
            start: now,
            duration,
        });
        let guid = n.guid;
        if self.npcs_shown.contains(&i) {
            let id = self.spline_id();
            self.send(
                opcode::SMSG_MONSTER_MOVE,
                &packets::monster_move(guid, here, id, to, duration.as_millis() as u32),
            );
        }
    }

    /// A trainer's chain handed in: where they stand now (home, after the last).
    pub(super) fn event_quest_done(&mut self, id: u32) {
        let Some(e) = (0..EVENTS.len()).find(|&e| (1..=3).any(|s| quest_id(e, s) == id)) else {
            return;
        };
        if id == quest_id(e, 3) {
            let line = format!(
                "{} now teaches {} in Deathknell",
                self.trainer_name(e),
                EVENTS[e].profession
            );
            self.send(opcode::SMSG_NOTIFICATION, &packets::notification(&line));
        }
        self.place_trainers(Instant::now());
    }
}
