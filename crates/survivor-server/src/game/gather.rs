//! Gather quests' objects (`gathers.tsv`): Deathknell's Equipment Boxes and their like. They stand
//! where 1.12 has them, in town or out. Walking up to one while a quest still wants it takes it:
//! the quest counts one, and that object is gone for a while.

use std::time::{Duration, Instant};

use benilla_protocol::messages::opcode;

use super::{dist2d, Game, VISIBLE, VISIBLE_DROP};
use crate::data::OBJECTIVE_GAMEOBJECT;
use crate::packets::{self, Update};

/// How close the player walks to take one, and how long it is gone after.
const TAKE_RANGE: f32 = 2.5;
const TAKE_HEIGHT: f32 = 4.0;
const BACK: Duration = Duration::from_secs(60);

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

    /// The player by a gather object a quest of theirs still wants: it counts, and is taken.
    pub(super) fn take_nearby_gather(&mut self, now: Instant) {
        if self.player.dead_until.is_some() {
            return;
        }
        let me = self.player.pos;
        let Some(i) = self.gathers.iter().position(|g| {
            g.back.is_none()
                && dist2d(g.pos, me) <= TAKE_RANGE
                && (g.pos[2] - me[2]).abs() <= TAKE_HEIGHT
                && self.quest_wants(g.entry | OBJECTIVE_GAMEOBJECT)
        }) else {
            return;
        };
        let (entry, guid) = (self.gathers[i].entry, self.gathers[i].guid);
        self.gathers[i].back = Some(now + BACK);
        eprintln!("survivor: took gather object {entry}");
        self.quest_kill(entry | OBJECTIVE_GAMEOBJECT, guid);
    }
}
