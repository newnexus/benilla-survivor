//! Gather quests' objects (`gathers.tsv`): Deathknell's Equipment Boxes and their like. They stand
//! where 1.12 has them, in town or out. Standing by one while a quest still wants it gathers it
//! over [`GATHER_TIME`] (the screen draws the charge over the object, from [`crate::pickup`]);
//! stepping off breaks it. Gathered, the quest counts one, and that object is gone for a while.

use std::time::{Duration, Instant};

use benilla_protocol::messages::opcode;

use super::{dist2d, Game, VISIBLE, VISIBLE_DROP};
use crate::data::OBJECTIVE_GAMEOBJECT;
use crate::packets::{self, Update};

/// How close the player walks to take one, and how long it is gone after.
const TAKE_RANGE: f32 = 2.5;
const TAKE_HEIGHT: f32 = 4.0;
const BACK: Duration = Duration::from_secs(60);
/// How long a gathering takes.
const GATHER_TIME: Duration = Duration::from_millis(2000);
/// The charge's icon, over the object.
const GATHER_ICON: &str = r"Interface\Icons\INV_Crate_02";

/// A gathering under way: which object, when it began and when it is done.
pub(crate) struct Gathering {
    i: usize,
    start: Instant,
    done: Instant,
}

/// A gather object in the world.
pub(crate) struct Gather {
    guid: u64,
    entry: u32,
    pos: [f32; 3],
    facing: f32,
    rotation: [f32; 4],
    /// Taken: when it is back.
    back: Option<Instant>,
}

/// The gather objects from the world data, with guids of their own, clear of the chests'.
pub(crate) fn gathers(data: &crate::data::WorldData) -> Vec<Gather> {
    data.gathers
        .iter()
        .enumerate()
        .map(|(i, c)| Gather {
            guid: 0xF110_0000_0000_0000 | u64::from(c.entry) << 24 | 0x80_0000 | i as u64,
            entry: c.entry,
            pos: c.pos,
            facing: c.facing,
            rotation: c.rotation,
            back: None,
        })
        .collect()
}

impl Game {
    /// The gather objects coming into view and leaving it; a taken one is gone until it is back.
    pub(super) fn update_gathers(&mut self, now: Instant) {
        let me = self.player.pos;
        let mut creates = Vec::new();
        let mut gone = Vec::new();
        for (i, g) in self.gathers.iter_mut().enumerate() {
            if g.back.is_some_and(|t| now >= t) {
                g.back = None;
            }
            let d = dist2d(g.pos, me);
            let shown = self.gathers_shown.contains(&i);
            if shown && (d > VISIBLE_DROP || g.back.is_some()) {
                gone.push(i);
            } else if !shown && d <= VISIBLE && g.back.is_none() {
                creates.push(i);
            }
        }
        if !gone.is_empty() {
            let guids = gone.iter().map(|&i| self.gathers[i].guid).collect();
            for i in &gone {
                self.gathers_shown.remove(i);
            }
            self.send(
                opcode::SMSG_UPDATE_OBJECT,
                &packets::update_object(&[Update::OutOfRange(guids)]),
            );
        }
        if !creates.is_empty() {
            let updates: Vec<Update> = creates
                .iter()
                .filter_map(|&i| {
                    let g = &self.gathers[i];
                    let t = self.data.chest_templates.get(&g.entry)?;
                    Some(packets::gameobject_create(g.guid, t, g.pos, g.facing, g.rotation))
                })
                .collect();
            self.send(opcode::SMSG_UPDATE_OBJECT, &packets::update_object(&updates));
            self.gathers_shown.extend(creates);
        }
    }

    /// Whether gather object `i` is there to gather: back, within reach, and still wanted.
    fn gatherable(&self, i: usize) -> bool {
        let (g, me) = (&self.gathers[i], self.player.pos);
        g.back.is_none()
            && dist2d(g.pos, me) <= TAKE_RANGE
            && (g.pos[2] - me[2]).abs() <= TAKE_HEIGHT
            && self.quest_wants(g.entry | OBJECTIVE_GAMEOBJECT)
    }

    /// The player by a gather object a quest of theirs still wants: it charges, then counts and
    /// is taken.
    pub(super) fn take_nearby_gather(&mut self, now: Instant) {
        if self.player.dead_until.is_some() {
            self.gathering = None;
            crate::pickup::set(None);
            return;
        }
        if let Some(h) = &self.gathering {
            let i = h.i;
            if !self.gatherable(i) {
                self.gathering = None;
                crate::pickup::set(None);
                return;
            }
            if now < h.done {
                let part = now.saturating_duration_since(h.start).as_secs_f32()
                    / GATHER_TIME.as_secs_f32();
                crate::pickup::set(Some(crate::pickup::Pickup {
                    pos: self.gathers[i].pos,
                    part,
                    icon: GATHER_ICON,
                }));
                return;
            }
            self.gathering = None;
            crate::pickup::set(None);
            self.gather(i, now);
            return;
        }
        if let Some(i) = (0..self.gathers.len()).find(|&i| self.gatherable(i)) {
            self.gathering = Some(Gathering {
                i,
                start: now,
                done: now + GATHER_TIME,
            });
        }
    }

    /// A pause holds a gathering where it is.
    pub(super) fn shift_gathering(&mut self, by: Duration) {
        if let Some(h) = &mut self.gathering {
            h.start += by;
            h.done += by;
        }
    }

    /// Gather object `i` gathered: the quest counts one, and it is gone for a while.
    fn gather(&mut self, i: usize, now: Instant) {
        let (entry, guid) = (self.gathers[i].entry, self.gathers[i].guid);
        self.gathers[i].back = Some(now + BACK);
        eprintln!("survivor: took gather object {entry}");
        self.quest_kill(entry | OBJECTIVE_GAMEOBJECT, guid);
    }
}
