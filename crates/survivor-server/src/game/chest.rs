//! Chests and gold. Every creature killed drops a little gold; the world's chests (vmangos's
//! own, `chests.tsv`) stand where they always stood, and one opens for its price when the player
//! stands by it during a run. Each chest opened makes the next dearer; a new run starts the
//! prices over and closes every chest again.

use std::time::Instant;

use benilla_protocol::messages::opcode;

use super::{dist2d, Game, VISIBLE, VISIBLE_DROP};
use crate::packets::{self, f, Fields, Update};

/// How close the player stands to open a chest, and how far to walk off before it asks again.
const OPEN_RANGE: f32 = 3.5;
const ASK_RESET: f32 = 7.0;
/// The first chest of a run costs this many copper, each one after this much more.
const FIRST_PRICE: u32 = 50;
const PRICE_GROWTH: f32 = 1.6;
/// Copper a kill drops, per level of the creature: between these.
const GOLD_PER_LEVEL: (u32, u32) = (3, 8);

/// A chest in the world.
pub(crate) struct Chest {
    pub(crate) guid: u64,
    pub(crate) entry: u32,
    pub(crate) pos: [f32; 3],
    facing: f32,
    rotation: [f32; 4],
    opened: bool,
}

impl Chest {
    pub(crate) fn is_opened(&self) -> bool {
        self.opened
    }
}

/// The chests from the world data, with guids of their own.
pub(crate) fn chests(data: &crate::data::WorldData) -> Vec<Chest> {
    data.chests
        .iter()
        .enumerate()
        .map(|(i, c)| Chest {
            guid: 0xF110_0000_0000_0000 | u64::from(c.entry) << 24 | i as u64,
            entry: c.entry,
            pos: c.pos,
            facing: c.facing,
            rotation: c.rotation,
            opened: false,
        })
        .collect()
}

/// `1g 20s 5c`.
pub(crate) fn money_text(copper: u32) -> String {
    let (g, s, c) = (copper / 10_000, copper / 100 % 100, copper % 100);
    let mut parts = Vec::new();
    if g > 0 {
        parts.push(format!("{g}g"));
    }
    if s > 0 {
        parts.push(format!("{s}s"));
    }
    if c > 0 || parts.is_empty() {
        parts.push(format!("{c}c"));
    }
    parts.join(" ")
}

impl Game {
    /// A kill's gold, copper: it drops as a coin ([`super::orbs`]).
    pub(super) fn gold_for(&mut self, level: u32) -> u32 {
        let per = GOLD_PER_LEVEL.0
            + (self.rng.next() % u64::from(GOLD_PER_LEVEL.1 - GOLD_PER_LEVEL.0 + 1)) as u32;
        level.max(1) * per
    }

    /// `copper` more in the purse, said as WoW says a loot.
    pub(super) fn add_money(&mut self, copper: u32) {
        self.money += copper;
        self.send(opcode::SMSG_LOOT_MONEY_NOTIFY, &copper.to_le_bytes());
        self.send_money();
    }

    pub(super) fn send_money(&self) {
        crate::purse::set(self.money);
        let mut fl = Fields::default();
        fl.set(f::PLAYER_COINAGE, self.money);
        self.values(self.player.guid, fl);
    }

    /// A new run: every chest closed again, the prices from the start.
    pub(super) fn reset_chests(&mut self) {
        self.chest_price = FIRST_PRICE;
        self.chest_asked = None;
        for c in &mut self.chests {
            c.opened = false;
        }
        self.roll_chest_loot();
    }

    /// The chests coming into view and leaving it; an opened one is gone.
    pub(super) fn update_chests(&mut self) {
        let me = self.player.pos;
        let mut creates = Vec::new();
        let mut gone = Vec::new();
        for (i, c) in self.chests.iter().enumerate() {
            let d = dist2d(c.pos, me);
            let shown = self.chests_shown.contains(&i);
            if shown && (d > VISIBLE_DROP || c.opened) {
                gone.push(i);
            } else if !shown && d <= VISIBLE && !c.opened {
                creates.push(i);
            }
        }
        if !gone.is_empty() {
            let guids = gone.iter().map(|&i| self.chests[i].guid).collect();
            for i in &gone {
                self.chests_shown.remove(i);
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
                    let c = &self.chests[i];
                    let t = self.data.chest_templates.get(&c.entry)?;
                    Some(packets::gameobject_create(c.guid, t, c.pos, c.facing, c.rotation))
                })
                .collect();
            self.send(opcode::SMSG_UPDATE_OBJECT, &packets::update_object(&updates));
            self.chests_shown.extend(creates);
        }
    }

    /// During a run, a chest the player stands by opens if they can pay; else it says its price,
    /// once until they walk off.
    pub(super) fn open_nearby_chest(&mut self, now: Instant) {
        if !self.in_run() || self.player.dead_until.is_some() || self.paused() {
            return;
        }
        let me = self.player.pos;
        if let Some(i) = self.chest_asked {
            if dist2d(self.chests[i].pos, me) > ASK_RESET {
                self.chest_asked = None;
            }
            return;
        }
        let Some(i) = self
            .chests
            .iter()
            .position(|c| !c.opened && dist2d(c.pos, me) <= OPEN_RANGE)
        else {
            return;
        };
        let price = self.chest_price;
        if self.money < price {
            self.chest_asked = Some(i);
            let line = format!(
                "This chest costs {} (you have {})",
                money_text(price),
                money_text(self.money)
            );
            self.send(opcode::SMSG_NOTIFICATION, &packets::notification(&line));
            return;
        }
        self.money -= price;
        self.send_money();
        self.chest_price = (price as f32 * PRICE_GROWTH).round() as u32;
        self.chests[i].opened = true;
        let name = self
            .data
            .chest_templates
            .get(&self.chests[i].entry)
            .map_or("Chest", |t| t.name.as_str())
            .to_owned();
        eprintln!(
            "survivor: opened {name} for {}, the next costs {}",
            money_text(price),
            money_text(self.chest_price)
        );
        self.open_chest_offer(i, now);
    }

    /// `CMSG_GAMEOBJECT_QUERY`: a chest's name and look.
    pub(crate) fn gameobject_query(&self, entry: u32) {
        let t = self.data.chest_templates.get(&entry);
        self.send(
            opcode::SMSG_GAMEOBJECT_QUERY_RESPONSE,
            &packets::gameobject_query_response(entry, t),
        );
    }
}
