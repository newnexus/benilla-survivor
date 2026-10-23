//! Food and drink, the survivor's way: no sitting down. Using one starts its 1.12 regeneration
//! (the use spell's `MOD_REGEN` or `MOD_POWER_REGEN` aura, the same total over the same time)
//! while the player keeps running and fighting. The buff shows in the player's aura row with its
//! timer, and the stack in the bag goes down by one.
//!
//! Each also leaves a food buff, Valheim's way, until the run ends: food raises max health and
//! gives health back every second, drink the same for mana (for health, to a class without
//! mana). The three most recently eaten count; a fourth puts out the oldest, and the same food
//! again only moves to the newest.

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
/// How many food buffs count at once.
const FOOD_BUFFS: usize = 3;
/// A food buff, as shares of what the food restores: the max it adds, and what it gives back
/// every five seconds.
const FOOD_MAX: f32 = 0.5;
const FOOD_REGEN: f32 = 0.1;

/// What an eaten food or drink does: restores `total` over `seconds`, then while its buff lasts
/// adds `max` to the bar and gives back `regen` every five seconds.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FoodEffect {
    /// Mana rather than health.
    pub(crate) drink: bool,
    pub(crate) total: u32,
    pub(crate) seconds: u32,
    pub(crate) max: u32,
    pub(crate) regen: u32,
}

/// A food buff: the item eaten and what it gives while the run lasts.
pub(crate) struct FoodBuff {
    entry: u32,
    effect: FoodEffect,
    /// Its buff is to mana (a drink, for a class with mana).
    mana: bool,
    carry: f32,
}

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
    /// What eating item `entry` does, from its use spell; none for an item that is not food or
    /// drink.
    pub(crate) fn food_effect(&self, entry: u32) -> Option<FoodEffect> {
        let spell_id = self.data.items.get(&entry)?.spell;
        let s = self.spell(spell_id)?;
        let e = (0..3).find(|&e| {
            s.effects[e] == 6 && matches!(s.auras[e], AURA_MOD_REGEN | AURA_MOD_POWER_REGEN)
        })?;
        let per_tick = (s.base_points[e] + 1).max(1) as f32;
        let total = per_tick * s.duration_ms as f32 / REGEN_PERIOD_MS as f32;
        Some(FoodEffect {
            drink: s.auras[e] == AURA_MOD_POWER_REGEN,
            total: total.round() as u32,
            seconds: (s.duration_ms / 1000).max(1),
            max: (total * FOOD_MAX).round() as u32,
            regen: (total * FOOD_REGEN).round().max(1.0) as u32,
        })
    }

    /// A food's line for the innkeeper's window: what it restores and its buff, for this class.
    pub(crate) fn food_text(&self, entry: u32) -> String {
        let Some(e) = self.food_effect(entry) else { return String::new() };
        let mana = e.drink && self.player.power.kind == 0;
        let what = if mana { "mana" } else { "health" };
        let restore = if e.drink && !mana {
            String::new()
        } else {
            format!("Restores {} {what} over {} sec. ", e.total, e.seconds)
        };
        format!("{restore}Run buff: +{} max {what}, +{} {what} per 5 sec", e.max, e.regen)
    }

    /// The food buffs' sum: max health, max mana, and health and mana per five seconds.
    fn food_totals(&self) -> (u32, u32, u32, u32) {
        let mut t = (0, 0, 0, 0);
        for f in &self.foods {
            if f.mana {
                t.1 += f.effect.max;
                t.3 += f.effect.regen;
            } else {
                t.0 += f.effect.max;
                t.2 += f.effect.regen;
            }
        }
        t
    }

    pub(super) fn food_max_health(&self) -> u32 {
        self.food_totals().0
    }

    /// The food buffs on the bars: max mana by what changed since last time, max health through
    /// the gear's sum.
    pub(super) fn apply_food(&mut self) {
        let mana = self.food_totals().1;
        if self.player.power.kind == 0 && mana != self.food_mana {
            let p = &mut self.player.power;
            p.max = (p.max + mana).saturating_sub(self.food_mana);
            if mana > self.food_mana {
                p.current += mana - self.food_mana;
            }
            p.current = p.current.min(p.max);
            let mut fl = Fields::default();
            fl.set(f::MAXPOWER1, p.max).set(f::POWER1, p.current);
            self.values(self.player.guid, fl);
            self.food_mana = mana;
        }
        self.apply_gear();
    }

    /// The run is over: its food buffs go.
    pub(super) fn end_run_food(&mut self) {
        if !self.foods.is_empty() {
            self.foods.clear();
            self.apply_food();
        }
    }

    /// Eaten `entry`: its buff is the newest, the oldest of four going.
    fn add_food_buff(&mut self, entry: u32, effect: FoodEffect) {
        self.foods.retain(|f| f.entry != entry);
        if self.foods.len() >= FOOD_BUFFS {
            self.foods.remove(0);
        }
        let mana = effect.drink && self.player.power.kind == 0;
        self.foods.push(FoodBuff {
            entry,
            effect,
            mana,
            carry: 0.0,
        });
        self.apply_food();
    }

    /// Once a second: what the food buffs give back.
    pub(super) fn tick_food_buffs(&mut self) {
        if self.foods.is_empty() || self.player.dead_until.is_some() {
            return;
        }
        let (mut health, mut mana) = (0, 0);
        for f in &mut self.foods {
            f.carry += f.effect.regen as f32 / 5.0;
            let gain = f.carry.floor();
            f.carry -= gain;
            if f.mana {
                mana += gain as u32;
            } else {
                health += gain as u32;
            }
        }
        let mut fl = Fields::default();
        let mut any = false;
        if health > 0 && self.player.health < self.player.max_health {
            self.player.health = (self.player.health + health).min(self.player.max_health);
            fl.set(f::HEALTH, self.player.health);
            any = true;
        }
        let p = &mut self.player.power;
        if mana > 0 && p.kind == 0 && p.current < p.max {
            p.current = (p.current + mana).min(p.max);
            fl.set(f::POWER1, p.current);
            any = true;
        }
        if any {
            self.values(self.player.guid, fl);
        }
    }

    /// `CMSG_USE_ITEM` on the item at `bag`/`slot`.
    pub(crate) fn use_item(&mut self, bag: u8, slot: u8) {
        if self.paused() || self.player.dead_until.is_some() || bag != BACKPACK_BAG {
            return;
        }
        let field = f::PLAYER_INV_SLOT_HEAD + u16::from(slot) * 2;
        let Some(i) = self.pack.iter().position(|p| p.field == field) else {
            return;
        };
        if self.pack[i].entry == super::hearth::HEARTHSTONE {
            self.use_hearthstone(Instant::now());
            return;
        }
        let spell_id = self.data.items.get(&self.pack[i].entry).map_or(0, |t| t.spell);
        let Some(s) = self.spell(spell_id).cloned() else { return };
        let Some(e) = (0..3).find(|&e| {
            s.effects[e] == 6 && matches!(s.auras[e], AURA_MOD_REGEN | AURA_MOD_POWER_REGEN)
        }) else {
            return;
        };
        let drink = s.auras[e] == AURA_MOD_POWER_REGEN;
        let entry = self.pack[i].entry;
        // The buff, whoever eats it.
        if let Some(effect) = self.food_effect(entry) {
            self.add_food_buff(entry, effect);
        }
        // Drink restores mana: a rage or energy class gets only its buff.
        if drink && self.player.power.kind != 0 {
            self.send(opcode::SMSG_CAST_RESULT, &packets::cast_result(spell_id, None));
            self.spend_item(i);
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
        self.refresh_consumables();
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
