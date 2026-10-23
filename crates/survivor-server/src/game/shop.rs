//! Innkeepers: in town, standing by one opens their window ([`crate::shop`]). The town purse buys
//! food and drink (Innkeeper Renee's 1.12 wares, each from its level), one at a time into the
//! backpack; whatever the backpack holds sells, every one of an item at once, at its 1.12 price.

use std::time::Instant;

use super::Game;
use crate::shop::{Action, Ware, Window};

/// How close the player stands to open the window, and how far to walk off to close it.
const RANGE: f32 = 4.0;
const RESET: f32 = 7.0;
/// The innkeepers: Deathknell's (Joshua Kien, the general supplier, out in front of his house)
/// and Brill's.
pub(crate) const KEEPERS: &[u32] = &[2115, 5688];

impl Game {
    /// The keeper within `range`: their name and guid.
    fn keeper_near(&self, now: Instant, range: f32) -> Option<(String, u64)> {
        let me = self.player.pos;
        self.npcs.iter().find_map(|n| {
            (KEEPERS.contains(&n.entry) && super::dist2d(n.here(now), me) <= range)
                .then(|| self.data.creatures.get(&n.entry).map(|t| (t.name.clone(), n.guid)))
                .flatten()
        })
    }

    pub(super) fn open_shop_nearby(&mut self, now: Instant) {
        if self.shop_open.is_some() {
            if self.in_run() || self.keeper_near(now, RESET).is_none() {
                self.shop_open = None;
                crate::shop::clear();
            }
            return;
        }
        if self.shop_closed {
            self.shop_closed = self.keeper_near(now, RESET).is_some();
            return;
        }
        if self.in_run()
            || self.paused()
            || !self.walked
            || self.armory_open.is_some()
            || self.spirit_open.is_some()
            || self.trainer_open.is_some()
        {
            return;
        }
        if let Some(keeper) = self.keeper_near(now, RANGE) {
            self.close_quest_panel();
            self.post_shop(keeper);
        }
    }

    fn ware(&self, entry: u32, count: u32, price: u32, level: u32) -> Option<Ware> {
        let t = self.data.items.get(&entry)?;
        Some(Ware {
            entry,
            name: t.name.clone(),
            display: t.display,
            quality: t.quality,
            count,
            price,
            level,
            about: self.food_text(entry),
        })
    }

    /// How many of `entry` the backpack holds.
    fn in_pack(&self, entry: u32) -> u32 {
        self.pack
            .iter()
            .filter(|p| p.entry == entry && super::loot::is_backpack(p.field))
            .map(|p| p.count)
            .sum()
    }

    fn post_shop(&mut self, (keeper, keeper_guid): (String, u64)) {
        let buy = self
            .data
            .shop
            .iter()
            .filter_map(|&(entry, level)| {
                let price = self.data.items.get(&entry)?.buy;
                self.ware(entry, self.in_pack(entry), price, level)
            })
            .collect();
        let mut entries: Vec<u32> = Vec::new();
        for p in self.pack.iter().filter(|p| super::loot::is_backpack(p.field)) {
            if !entries.contains(&p.entry) && p.entry != super::hearth::HEARTHSTONE {
                entries.push(p.entry);
            }
        }
        let sell = entries
            .into_iter()
            .filter_map(|entry| {
                let each = self.data.items.get(&entry)?.sell;
                let count = self.in_pack(entry);
                (each > 0).then(|| self.ware(entry, count, each * count, 0)).flatten()
            })
            .collect();
        self.shop_open = Some(crate::shop::post(Window {
            id: 0,
            keeper,
            keeper_guid,
            money: self.money,
            level: self.meta.level,
            buy,
            sell,
        }));
    }

    /// Every one of `entry` in the backpack sold at its 1.12 price; whether any was.
    fn sell_all_of(&mut self, entry: u32) -> bool {
        let count = self.in_pack(entry);
        let each = self.data.items.get(&entry).map_or(0, |t| t.sell);
        if count == 0 || each == 0 || entry == super::hearth::HEARTHSTONE {
            return false;
        }
        self.remove_from_pack(entry, count);
        self.money += each * count;
        eprintln!("survivor: sold item {entry} x{count} for {}c", each * count);
        true
    }

    /// The window's answer: buy one, sell them all, sell the grey ones, or close.
    pub(super) fn take_shop_action(&mut self) {
        let Some((id, action)) = crate::shop::take_action() else { return };
        if self.shop_open != Some(id) {
            return;
        }
        match action {
            Action::Close => {
                self.shop_open = None;
                crate::shop::clear();
                self.shop_closed = true;
                return;
            }
            Action::Buy(entry) => {
                let Some(&(_, level)) = self.data.shop.iter().find(|s| s.0 == entry) else {
                    return;
                };
                let price = self.data.items.get(&entry).map_or(0, |t| t.buy);
                if level > self.meta.level || self.money < price {
                    return;
                }
                if self.add_to_pack(entry, 1) == 0 {
                    let line = "Your bags are full";
                    self.send(
                        benilla_protocol::messages::opcode::SMSG_NOTIFICATION,
                        &crate::packets::notification(line),
                    );
                    return;
                }
                self.money -= price;
                eprintln!("survivor: bought item {entry} for {price}c");
            }
            Action::Sell(entry) => {
                if !self.sell_all_of(entry) {
                    return;
                }
            }
            Action::SellTrash => {
                let mut trash: Vec<u32> = self
                    .pack
                    .iter()
                    .filter(|p| super::loot::is_backpack(p.field))
                    .map(|p| p.entry)
                    .filter(|e| self.data.items.get(e).is_some_and(|t| t.quality == 0))
                    .collect();
                trash.sort_unstable();
                trash.dedup();
                let mut sold = false;
                for entry in trash {
                    sold |= self.sell_all_of(entry);
                }
                if !sold {
                    return;
                }
            }
        }
        self.send_money();
        self.save();
        let keeper = crate::shop::current().map_or((String::new(), 0), |w| (w.keeper, w.keeper_guid));
        self.post_shop(keeper);
    }
}
