//! Food and drink, the survivor's way: no sitting down. Using one starts its 1.12 regeneration
//! (the use spell's `MOD_REGEN` or `MOD_POWER_REGEN` aura, the same total over the same time)
//! while the player keeps running and fighting. The buff shows in the player's aura row with its
//! timer, and the stack in the bag goes down by one.

use std::time::{Duration, Instant};

use benilla_protocol::messages::opcode;

use super::Game;
use crate::packets::{self, f, Fields};

/// `SPELL_AURA_MOD_REGEN` (food) and `SPELL_AURA_MOD_POWER_REGEN` (drink).
const AURA_MOD_REGEN: u32 = 84;
const AURA_MOD_POWER_REGEN: u32 = 85;
/// Both auras give their points once every five seconds.
const REGEN_PERIOD_MS: u32 = 5000;
/// The aura slots the food and drink buffs show in.
const FOOD_SLOT: u16 = 0;
const DRINK_SLOT: u16 = 1;
/// `INVENTORY_SLOT_BAG_0`: the bag byte of an item in the backpack or worn.
const BACKPACK_BAG: u8 = 255;

/// An item in the player's bags or equipment, as the client was told.
pub(crate) struct PackItem {
    pub(crate) guid: u64,
    pub(crate) entry: u32,
    pub(crate) count: u32,
    /// The player field holding its guid (`PLAYER_FIELD_INV_SLOT_HEAD + slot * 2`).
    pub(crate) field: u16,
}

/// Food or drink being taken.
pub(crate) struct Meal {
    /// Mana rather than health.
    drink: bool,
    pub(super) spell: u32,
    per_second: f32,
    /// The part of a point owed but not yet given.
    carry: f32,
    /// Points actually restored, short of the total when the bar was already full.
    given: u32,
    next: Instant,
    pub(super) ends: Instant,
}

impl Meal {
    pub(super) fn slot(&self) -> u16 {
        if self.drink {
            DRINK_SLOT
        } else {
            FOOD_SLOT
        }
    }
}

impl Game {
    /// `CMSG_USE_ITEM` on the item at `bag`/`slot`.
    pub(crate) fn use_item(&mut self, bag: u8, slot: u8) {
        if self.paused() || self.player.dead_until.is_some() || bag != BACKPACK_BAG {
            return;
        }
        let field = f::PLAYER_INV_SLOT_HEAD + u16::from(slot) * 2;
        let Some(i) = self.pack.iter().position(|p| p.field == field) else {
            return;
        };
        let spell_id = self.data.items.get(&self.pack[i].entry).map_or(0, |t| t.spell);
        let Some(s) = self.spell(spell_id).cloned() else { return };
        let Some(e) = (0..3).find(|&e| {
            s.effects[e] == 6 && matches!(s.auras[e], AURA_MOD_REGEN | AURA_MOD_POWER_REGEN)
        }) else {
            return;
        };
        let drink = s.auras[e] == AURA_MOD_POWER_REGEN;
        // Drink is for mana: a rage or energy class gets nothing from it.
        if drink && self.player.power.kind != 0 {
            return;
        }
        let now = Instant::now();
        let per_tick = (s.base_points[e] + 1).max(1) as f32;
        let seconds = (s.duration_ms / 1000).max(1);
        let total = per_tick * s.duration_ms as f32 / REGEN_PERIOD_MS as f32;
        eprintln!(
            "survivor: {} {} restores {total:.0} over {seconds} s",
            if drink { "drinking" } else { "eating" },
            self.data.items[&self.pack[i].entry].name
        );
        self.send(opcode::SMSG_CAST_RESULT, &packets::cast_result(spell_id, None));
        // A second one of the same kind starts over, as in 1.12.
        self.meals.retain(|m| m.drink != drink);
        self.meals.push(Meal {
            drink,
            spell: spell_id,
            per_second: total / seconds as f32,
            carry: 0.0,
            given: 0,
            next: now + Duration::from_secs(1),
            ends: now + Duration::from_millis(u64::from(s.duration_ms)),
        });
        self.send_auras(now);
        self.spend_item(i);
    }

    /// One fewer of the item; the last one leaves the bag.
    fn spend_item(&mut self, i: usize) {
        let item = &mut self.pack[i];
        item.count = item.count.saturating_sub(1);
        let (guid, count, field) = (item.guid, item.count, item.field);
        if count > 0 {
            let mut fl = Fields::default();
            fl.set(f::ITEM_STACK_COUNT, count);
            self.values(guid, fl);
            return;
        }
        self.pack.remove(i);
        let mut fl = Fields::default();
        fl.guid(field, 0);
        self.values(self.player.guid, fl);
        self.send(opcode::SMSG_DESTROY_OBJECT, &packets::destroy(guid));
    }

    /// Once a second, each meal gives its share; one past its end comes off the aura row.
    pub(super) fn tick_meals(&mut self, now: Instant) {
        if self.meals.is_empty() {
            return;
        }
        let alive = self.player.dead_until.is_none();
        let mut health = None;
        let mut mana = None;
        for m in &mut self.meals {
            if now < m.next || now > m.ends + Duration::from_millis(500) {
                continue;
            }
            m.next += Duration::from_secs(1);
            if !alive {
                continue;
            }
            m.carry += m.per_second;
            let gain = m.carry.floor();
            m.carry -= gain;
            let gain = gain as u32;
            if m.drink {
                let p = &mut self.player.power;
                let before = p.current;
                p.current = (p.current + gain).min(p.max);
                m.given += p.current - before;
                mana = Some(p.current);
            } else {
                let p = &mut self.player;
                let before = p.health;
                p.health = (p.health + gain).min(p.max_health);
                m.given += p.health - before;
                health = Some(p.health);
            }
        }
        if health.is_some() || mana.is_some() {
            let mut fl = Fields::default();
            if let Some(h) = health {
                fl.set(f::HEALTH, h);
            }
            if let Some(m) = mana {
                fl.set(f::POWER1, m);
            }
            self.values(self.player.guid, fl);
        }
        let before = self.meals.len();
        for m in self.meals.iter().filter(|m| now >= m.ends) {
            let what = if m.drink { "mana" } else { "health" };
            eprintln!("survivor: finished, restored {} {what}", m.given);
        }
        self.meals.retain(|m| now < m.ends);
        if self.meals.len() != before {
            self.send_auras(now);
        }
    }

    /// The pause moved the clock: every meal ends later by as much.
    pub(super) fn shift_meals(&mut self, by: Duration) {
        for m in &mut self.meals {
            m.next += by;
            m.ends += by;
        }
    }
}
