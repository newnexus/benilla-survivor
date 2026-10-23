//! The profession trainers' events ([`crate::events`]): at a run's start each trainer not yet
//! freed may be found at their spot (the first chance whole, each further one 0.8 of the last;
//! a chain begun is always there). Their window opens as the player walks up, the world paused
//! while it is open.
//!
//! The second quest is a defence: as the player nears the spot the attack comes, in waves, and
//! the player holds it off for [`DEFEND_SECS`] without walking away. On the third the trainer
//! follows the player; as they reach Deathknell's barrier the trainer thanks them, the quest is
//! done, and they run on alone to where they will stand. Home, they stand there for good.
//!
//! While a defence or an escort is on, the trainer talks (`/say`), and their bar shows at the
//! top of the screen in green, in place of the focus frame 1.12 does not have.

use std::time::{Duration, Instant};

use benilla_protocol::messages::opcode;

use super::{dist2d, Game, Path};
use crate::events::{quest_id, DEFEND_SECS, EVENTS};
use crate::packets::{self, Update};

/// How near the spot the second quest's attack comes, and how far off the player may go before
/// it breaks off (to begin again on coming back).
const AMBUSH_RANGE: f32 = 22.0;
const DEFEND_LEASH: f32 = 45.0;
/// The attack comes again this often while the defence lasts.
const WAVE_EVERY: Duration = Duration::from_secs(20);
/// A trainer says something about this often during a defence or an escort.
const SAY_EVERY: Duration = Duration::from_secs(11);
/// An escorted trainer sets off with the player from this near, keeps this far behind, and
/// runs a little faster than the player to keep up.
const FOLLOW_START: f32 = 8.0;
const FOLLOW_GAP: f32 = 3.0;
const FOLLOW_REACH: f32 = 30.0;
const FOLLOW_SPEED: f32 = 1.3;
/// The trainer leaves the player this far outside Deathknell's people (its town radius).
const HOME_REACH: f32 = 22.0;
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
    /// The defence under way.
    defence: Option<Defence>,
    /// The trainer walking home with the player, and when they speak next.
    following: Option<usize>,
    next_say: Option<Instant>,
    /// The trainer running on alone into Deathknell, left where they get to.
    homeward: Option<usize>,
}

/// A defence: its trainer, when it is won, the next wave, the next line, and the line said.
struct Defence {
    event: usize,
    ends: Instant,
    next_wave: Instant,
    next_say: Instant,
    said: usize,
    /// The seconds-left warnings given.
    warned: u32,
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
        self.events.defence = None;
        for e in order {
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
        // Into town with them still following (past the barrier before they left): home all
        // the same. Dead, the escort starts again next run.
        if let Some(e) = self.events.following.take() {
            if !died {
                self.bring_home(e);
            }
        }
        self.events.defence = None;
        self.events.homeward = None;
        self.events.next_say = None;
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
            if self.events.following == Some(e) || self.events.homeward == Some(e) {
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
            // The defence: the second quest taken and not yet won, the player at the spot.
            let attack = quest_id(e, 2);
            if self.events.defence.is_none()
                && self.quest_in_log(attack)
                && !self.quest_finished_in_log(attack)
                && dist2d(me, spot) <= AMBUSH_RANGE
            {
                self.start_defence(e, now);
            }
            // The escort: the third quest taken, the player by the trainer.
            let escort = quest_id(e, 3);
            if self.events.following.is_none()
                && self.events.homeward != Some(e)
                && self.quest_in_log(escort)
                && !self.quest_finished_in_log(escort)
                && dist2d(me, self.trainer_here(e, now)) <= FOLLOW_START
            {
                self.events.following = Some(e);
                self.events.next_say = Some(now + SAY_EVERY / 2);
                let line = format!("{} follows you. Lead them to Deathknell.", self.trainer_name(e));
                self.send(opcode::SMSG_NOTIFICATION, &packets::notification(&line));
            }
        }
        self.tick_defence(now);
        if let Some(e) = self.events.following {
            self.follow_player(e, now);
            if self.events.next_say.is_some_and(|t| now >= t) {
                self.events.next_say = Some(now + SAY_EVERY);
                let lines = EVENTS[e].escort_lines;
                let k = (self.rng.next() % lines.len() as u64) as usize;
                self.trainer_say(e, lines[k]);
            }
            // At the barrier: thanks, the quest done, and on alone.
            let town = &crate::town::TOWNS[0];
            let here = self.trainer_here(e, now);
            let d = (here[0] - town.centre[0]).hypot(here[1] - town.centre[1]);
            if d <= town.radius + HOME_REACH {
                self.trainer_say(e, EVENTS[e].home_line);
                self.events.following = None;
                self.events.next_say = None;
                self.events.homeward = Some(e);
                self.bring_home(e);
                self.run_home(e, now);
            }
        }
    }

    /// The third quest done: the trainer brought home, handed in at once (the quest log is not
    /// saved, the done list is).
    fn bring_home(&mut self, e: usize) {
        let (entry, guid) = (EVENTS[e].trainer, self.trainer_guid(e));
        self.quest_kill(entry, guid);
        eprintln!("survivor: {} brought home", self.trainer_name(e));
        self.quest_turn_in(guid, quest_id(e, 3));
    }

    /// The trainer runs from where they are to their place in Deathknell.
    fn run_home(&mut self, e: usize, now: Instant) {
        let h = EVENTS[e].home;
        self.lead_npc(self.events.npc[e], [h[0], h[1], h[2]], packets::STOCK_SPEEDS[1], now);
    }

    /// The attack comes: a wave now and every [`WAVE_EVERY`], [`DEFEND_SECS`] to hold out.
    fn start_defence(&mut self, e: usize, now: Instant) {
        let ev = &EVENTS[e];
        eprintln!("survivor: the attack on {} comes", self.trainer_name(e));
        let line = format!("{} Hold out for {DEFEND_SECS} seconds!", ev.ambush_line);
        self.send(opcode::SMSG_NOTIFICATION, &packets::notification(&line));
        self.events.defence = Some(Defence {
            event: e,
            ends: now + Duration::from_secs(DEFEND_SECS),
            next_wave: now + WAVE_EVERY,
            next_say: now + Duration::from_secs(3),
            said: 0,
            warned: 0,
        });
        self.defence_wave(e, now);
    }

    fn defence_wave(&mut self, e: usize, now: Instant) {
        let level = self.player.level.max(2);
        if let Some(mut w) = self.waves.take() {
            for &(entry, count) in EVENTS[e].ambush {
                for _ in 0..count {
                    self.spawn(&mut w, entry, level, None, now);
                }
            }
            self.waves = Some(w);
        }
    }

    /// The defence's clock: waves, lines, warnings, and its end, won or broken off.
    fn tick_defence(&mut self, now: Instant) {
        let Some(d) = self.events.defence.as_ref() else { return };
        let e = d.event;
        let ev = &EVENTS[e];
        let spot = [ev.spot[0], ev.spot[1], ev.spot[2]];
        if dist2d(self.player.pos, spot) > DEFEND_LEASH {
            self.events.defence = None;
            let line = format!(
                "You left {} behind. Go back to face the attack again.",
                self.trainer_name(e)
            );
            self.send(opcode::SMSG_NOTIFICATION, &packets::notification(&line));
            return;
        }
        if now >= d.ends {
            self.events.defence = None;
            let (entry, guid) = (ev.trainer, self.trainer_guid(e));
            self.quest_kill(entry, guid);
            self.trainer_say(e, "That's the last of them! We held!");
            eprintln!("survivor: the defence of {} is won", self.trainer_name(e));
            return;
        }
        let left = d.ends.saturating_duration_since(now).as_secs();
        let (wave, say, said, warned) = (now >= d.next_wave, now >= d.next_say, d.said, d.warned);
        if wave {
            self.defence_wave(e, now);
        }
        if say {
            let lines = ev.defend_lines;
            self.trainer_say(e, lines[said % lines.len()]);
        }
        let warn = [(30, 1), (10, 2)].into_iter().find(|&(s, w)| left < s && warned < w);
        if let Some((s, w)) = warn {
            let line = format!("{s} seconds left!");
            self.send(opcode::SMSG_NOTIFICATION, &packets::notification(&line));
            if let Some(d) = self.events.defence.as_mut() {
                d.warned = w;
            }
        }
        if let Some(d) = self.events.defence.as_mut() {
            if wave {
                d.next_wave = now + WAVE_EVERY;
            }
            if say {
                d.next_say = now + SAY_EVERY;
                d.said += 1;
            }
        }
    }

    /// The trainer says `text` aloud (`/say`), `$N` the player's name.
    fn trainer_say(&self, e: usize, text: &str) {
        let text = self.fill(text);
        let name = self.trainer_name(e);
        let mut b = crate::wire::Body::new();
        b.u8(benilla_protocol::messages::CHAT_MSG_MONSTER_SAY)
            .u32(0)
            .u64(self.trainer_guid(e))
            .u32(name.len() as u32 + 1)
            .cstr(&name)
            .u64(0)
            .u32(text.len() as u32 + 1)
            .cstr(&text)
            .u8(0);
        self.send(opcode::SMSG_MESSAGECHAT, &b.0);
    }

    /// The pause moved the clock: the defence's timers move with it.
    pub(super) fn shift_events(&mut self, by: Duration) {
        if let Some(d) = self.events.defence.as_mut() {
            d.ends += by;
            d.next_wave += by;
            d.next_say += by;
        }
        if let Some(t) = self.events.next_say.as_mut() {
            *t += by;
        }
    }

    /// The trainer to watch over, for the green bar at the top: the one defended or escorted.
    pub(super) fn event_focus(&self) -> Option<(u64, String)> {
        let e = self.events.defence.as_ref().map(|d| d.event).or(self.events.following)?;
        Some((self.trainer_guid(e), self.trainer_name(e)))
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
        self.lead_npc(i, to, packets::STOCK_SPEEDS[1] * FOLLOW_SPEED, now);
    }

    /// Town person `i` runs from where they are to `to`.
    fn lead_npc(&mut self, i: usize, to: [f32; 3], speed: f32, now: Instant) {
        let here = self.npcs[i].here(now);
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
