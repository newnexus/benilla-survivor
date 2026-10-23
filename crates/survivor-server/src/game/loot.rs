//! Loot: what an enemy carried goes straight into the player's backpack when it dies, rolled from
//! its 1.12 loot table (`loot.tsv`) at a part of the 1.12 chances ([`LOOT_RATE`]): a run kills
//! hundreds. Each pickup is posted for the screen's feed over the bag
//! ([`crate::loot_feed`]); with the backpack full the find is lost and the feed says so.
//!
//! What a run picked up is the player's at once, but dying on it loses about half of each find.
//! The backpack's finds are saved with the character.

use benilla_protocol::messages::opcode;

use super::Game;
use crate::data::LootRow;
use crate::packets::{self, f, Fields};

/// The part of each 1.12 chance a kill rolls with.
const LOOT_RATE: f32 = 0.25;
/// The backpack's slots, `INVENTORY_SLOT_ITEM_START..END`.
const BACKPACK: std::ops::Range<u16> = 23..39;
/// How deep reference tables may name one another.
const MAX_DEPTH: u32 = 4;

impl Game {
    /// Creature `entry` died: its loot into the backpack.
    pub(super) fn drop_loot(&mut self, entry: u32) {
        // `$SURVIVOR_LOOT_RATE=1`: the 1.12 chances themselves, for trying the feed out.
        let rate = std::env::var("SURVIVOR_LOOT_RATE")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(LOOT_RATE);
        let mut found = Vec::new();
        self.roll_loot(&(false, entry), rate, 0, &mut found);
        for (item, count) in found {
            let added = self.add_to_pack(item, count);
            let t = self.data.items.get(&item);
            let pickup = crate::loot_feed::Pickup {
                entry: item,
                count: added,
                display: t.map_or(0, |t| t.display),
                quality: t.map_or(1, |t| t.quality),
                name: t.map_or_else(String::new, |t| t.name.clone()),
            };
            if added == 0 {
                crate::loot_feed::post(crate::loot_feed::Pickup {
                    count,
                    ..pickup
                }, true);
                continue;
            }
            if self.in_run() {
                *self.run_loot.entry(item).or_insert(0) += added;
            }
            eprintln!("survivor: looted {} x{added}", pickup.name);
            crate::loot_feed::post(pickup, added < count);
        }
    }

    /// Roll table `key` (`(reference, id)`): each loose row on its own chance, each group once
    /// for one of its rows. `rate` scales the chances of this table's rows, not of the tables
    /// they name.
    fn roll_loot(&mut self, key: &(bool, u32), rate: f32, depth: u32, out: &mut Vec<(u32, u32)>) {
        let Some(rows) = self.data.loot.get(key).cloned() else { return };
        let mut groups: Vec<u32> = rows.iter().map(|r| r.group).filter(|&g| g != 0).collect();
        groups.sort_unstable();
        groups.dedup();
        for r in rows.iter().filter(|r| r.group == 0) {
            if self.rng.unit() * 100.0 < r.chance * rate {
                self.take_loot(r, depth, out);
            }
        }
        for g in groups {
            if self.rng.unit() >= rate {
                continue;
            }
            let members: Vec<&LootRow> = rows.iter().filter(|r| r.group == g).collect();
            // The explicit chances first, out of a hundred; the rest share what is left.
            let mut roll = self.rng.unit() * 100.0;
            let mut picked = None;
            for r in members.iter().filter(|r| r.chance > 0.0) {
                if roll < r.chance {
                    picked = Some(*r);
                    break;
                }
                roll -= r.chance;
            }
            let equal: Vec<&&LootRow> = members.iter().filter(|r| r.chance == 0.0).collect();
            if picked.is_none() && !equal.is_empty() {
                picked = Some(*equal[(self.rng.next() % equal.len() as u64) as usize]);
            }
            if let Some(r) = picked {
                self.take_loot(r, depth, out);
            }
        }
    }

    fn take_loot(&mut self, r: &LootRow, depth: u32, out: &mut Vec<(u32, u32)>) {
        if r.min < 0 {
            if depth < MAX_DEPTH {
                for _ in 0..r.max.max(1) {
                    self.roll_loot(&(true, r.min.unsigned_abs()), 1.0, depth + 1, out);
                }
            }
            return;
        }
        let lo = r.min.max(1) as u32;
        let hi = r.max.max(lo);
        let count = lo + (self.rng.next() % u64::from(hi - lo + 1)) as u32;
        match out.iter_mut().find(|o| o.0 == r.item) {
            Some(o) => o.1 += count,
            None => out.push((r.item, count)),
        }
    }

    /// Put `count` of `entry` in the backpack: on its stacks first, then in free slots. Returns
    /// how many went in; the rest did not fit.
    pub(super) fn add_to_pack(&mut self, entry: u32, count: u32) -> u32 {
        let Some(t) = self.data.items.get(&entry).cloned() else { return 0 };
        let most = t.stackable.max(1);
        let mut left = count;
        for i in 0..self.pack.len() {
            let p = &mut self.pack[i];
            if left == 0 || p.entry != entry || p.count >= most || !is_backpack(p.field) {
                continue;
            }
            let n = left.min(most - p.count);
            p.count += n;
            left -= n;
            let (guid, now) = (p.guid, p.count);
            let mut fl = Fields::default();
            fl.set(f::ITEM_STACK_COUNT, now);
            self.values(guid, fl);
        }
        while left > 0 {
            let Some(field) = BACKPACK
                .map(|s| f::PLAYER_INV_SLOT_HEAD + s * 2)
                .find(|&fd| !self.pack.iter().any(|p| p.field == fd))
            else {
                break;
            };
            let n = left.min(most);
            left -= n;
            self.new_stack(entry, n, field);
        }
        self.refresh_consumables();
        count - left
    }

    /// Take `count` of `entry` out of the backpack, its last stacks first.
    pub(super) fn remove_from_pack(&mut self, entry: u32, mut count: u32) {
        while count > 0 {
            let Some(i) = self.pack.iter().rposition(|p| p.entry == entry && is_backpack(p.field))
            else {
                return;
            };
            let p = &mut self.pack[i];
            let n = count.min(p.count);
            p.count -= n;
            count -= n;
            let (guid, now, field) = (p.guid, p.count, p.field);
            if now > 0 {
                let mut fl = Fields::default();
                fl.set(f::ITEM_STACK_COUNT, now);
                self.values(guid, fl);
                continue;
            }
            self.pack.remove(i);
            let mut fl = Fields::default();
            fl.guid(field, 0);
            self.values(self.player.guid, fl);
            self.send(opcode::SMSG_DESTROY_OBJECT, &packets::destroy(guid));
            self.refresh_consumables();
        }
    }

    /// The run is over: back alive, its finds stay; dead, about half of each is lost.
    pub(super) fn end_run_loot(&mut self, died: bool) {
        let finds: Vec<(u32, u32)> = self.run_loot.drain().collect();
        if !died || finds.is_empty() {
            return;
        }
        let mut lost_any = false;
        for (entry, count) in finds {
            // Half, an odd one's last on a coin toss.
            let lost = count / 2 + u32::from(count % 2 == 1 && self.rng.next() % 2 == 0);
            if lost > 0 {
                self.remove_from_pack(entry, lost);
                lost_any = true;
            }
        }
        if lost_any {
            self.send(
                opcode::SMSG_NOTIFICATION,
                &packets::notification("Half of what you gathered was lost"),
            );
        }
        self.save();
    }

    /// The backpack's finds to save: every item but the starting ones (the starting amounts of
    /// food and drink come back at each login anyway), as `(entry, count)`.
    pub(super) fn bag_to_save(&self) -> Vec<(u32, u32)> {
        let mut totals: Vec<(u32, u32)> = Vec::new();
        for p in self.pack.iter().filter(|p| is_backpack(p.field)) {
            match totals.iter_mut().find(|t| t.0 == p.entry) {
                Some(t) => t.1 += p.count,
                None => totals.push((p.entry, p.count)),
            }
        }
        for (class, entry, amount) in &self.data.start_items {
            if *class == self.player.class {
                if let Some(t) = totals.iter_mut().find(|t| t.0 == *entry) {
                    t.1 = t.1.saturating_sub(*amount);
                }
            }
        }
        totals.retain(|t| t.1 > 0 && t.0 != super::hearth::HEARTHSTONE);
        totals
    }

    /// At login: the saved finds back into the backpack, beside the starting items.
    pub(super) fn restore_bag(&mut self) {
        for (entry, count) in std::mem::take(&mut self.saved_bag) {
            self.add_to_pack(entry, count);
        }
    }
}

/// `INVENTORY_SLOT_BAG_0`: the player's own slots (equipment and backpack) as a bag byte.
const OWN_BAG: u8 = 255;
/// `EQUIP_ERR_ITEM_DOESNT_GO_TO_SLOT`, and `EQUIP_ERR_COULDNT_SPLIT_ITEMS`.
const ERR_WRONG_SLOT: u8 = 3;
const ERR_CANT_SPLIT: u8 = 26;

impl Game {
    /// The pack item in the player's slot `slot`.
    fn pack_index(&self, slot: u8) -> Option<usize> {
        let field = f::PLAYER_INV_SLOT_HEAD + u16::from(slot) * 2;
        self.pack.iter().position(|p| p.field == field)
    }

    /// `SMSG_INVENTORY_CHANGE_FAILURE`: the move is refused and the client's items unlock.
    fn refuse_move(&self, reason: u8, a: Option<usize>, b: Option<usize>) {
        let guid = |i: Option<usize>| i.map_or(0, |i| self.pack[i].guid);
        let mut body = crate::wire::Body::new();
        body.u8(reason).u64(guid(a)).u64(guid(b)).u8(0);
        self.send(opcode::SMSG_INVENTORY_CHANGE_FAILURE, &body.0);
    }

    /// `CMSG_SWAP_ITEM`: only within the player's own slots here (there are no other bags).
    pub(crate) fn swap_item(&mut self, dst_bag: u8, dst_slot: u8, src_bag: u8, src_slot: u8) {
        if dst_bag != OWN_BAG || src_bag != OWN_BAG {
            let src = if src_bag == OWN_BAG { self.pack_index(src_slot) } else { None };
            self.refuse_move(ERR_WRONG_SLOT, src, None);
            return;
        }
        self.swap_inv_item(src_slot, dst_slot);
    }

    /// `CMSG_SWAP_INV_ITEM`: a backpack item moved to another backpack slot: into an empty one,
    /// onto a stack of its own kind (as much as fits), or swapped with what is there. Worn gear
    /// is the collection's business, not the bag's: anything else is refused.
    pub(crate) fn swap_inv_item(&mut self, src: u8, dst: u8) {
        let (a, b) = (self.pack_index(src), self.pack_index(dst));
        let in_pack = |s: u8| BACKPACK.contains(&u16::from(s));
        let Some(a) = a.filter(|_| in_pack(src) && in_pack(dst) && src != dst) else {
            self.refuse_move(ERR_WRONG_SLOT, a, b);
            return;
        };
        let field = |s: u8| f::PLAYER_INV_SLOT_HEAD + u16::from(s) * 2;
        let entry = self.pack[a].entry;
        let most = self.data.items.get(&entry).map_or(1, |t| t.stackable.max(1));
        let mut fl = Fields::default();
        match b {
            // Onto a stack of the same: as much as fits.
            Some(b) if self.pack[b].entry == entry && most > 1 && self.pack[b].count < most => {
                let n = self.pack[a].count.min(most - self.pack[b].count);
                self.pack[b].count += n;
                self.pack[a].count -= n;
                let (bg, bc) = (self.pack[b].guid, self.pack[b].count);
                let mut sf = Fields::default();
                sf.set(f::ITEM_STACK_COUNT, bc);
                self.values(bg, sf);
                let (ag, ac) = (self.pack[a].guid, self.pack[a].count);
                if ac > 0 {
                    let mut sf = Fields::default();
                    sf.set(f::ITEM_STACK_COUNT, ac);
                    self.values(ag, sf);
                    return;
                }
                self.pack.remove(a);
                fl.guid(field(src), 0);
                self.values(self.player.guid, fl);
                self.send(opcode::SMSG_DESTROY_OBJECT, &packets::destroy(ag));
                return;
            }
            Some(b) => {
                self.pack[b].field = field(src);
                fl.guid(field(src), self.pack[b].guid);
            }
            None => {
                fl.guid(field(src), 0);
            }
        }
        self.pack[a].field = field(dst);
        fl.guid(field(dst), self.pack[a].guid);
        self.values(self.player.guid, fl);
    }

    /// `CMSG_SPLIT_ITEM`: `count` of a backpack stack into an empty backpack slot.
    pub(crate) fn split_item(&mut self, src_bag: u8, src: u8, dst_bag: u8, dst: u8, count: u8) {
        let a = if src_bag == OWN_BAG { self.pack_index(src) } else { None };
        let ok = src_bag == OWN_BAG
            && dst_bag == OWN_BAG
            && BACKPACK.contains(&u16::from(dst))
            && self.pack_index(dst).is_none()
            && a.is_some_and(|a| u32::from(count) > 0 && self.pack[a].count > u32::from(count));
        let Some(a) = a.filter(|_| ok) else {
            self.refuse_move(ERR_CANT_SPLIT, a, None);
            return;
        };
        let (entry, n) = (self.pack[a].entry, u32::from(count));
        self.pack[a].count -= n;
        let (ag, ac) = (self.pack[a].guid, self.pack[a].count);
        let mut sf = Fields::default();
        sf.set(f::ITEM_STACK_COUNT, ac);
        self.values(ag, sf);
        let field = f::PLAYER_INV_SLOT_HEAD + u16::from(dst) * 2;
        self.new_stack(entry, n, field);
    }

    /// `CMSG_DESTROYITEM`: `count` of the item in a backpack slot thrown away (0: all of it).
    pub(crate) fn destroy_item(&mut self, bag: u8, slot: u8, count: u8) {
        let Some(i) = self.pack_index(slot).filter(|_| bag == OWN_BAG && BACKPACK.contains(&u16::from(slot)))
        else {
            return;
        };
        let p = &mut self.pack[i];
        let n = if count == 0 { p.count } else { u32::from(count).min(p.count) };
        p.count -= n;
        let (guid, left, field) = (p.guid, p.count, p.field);
        if left > 0 {
            let mut sf = Fields::default();
            sf.set(f::ITEM_STACK_COUNT, left);
            self.values(guid, sf);
            return;
        }
        self.pack.remove(i);
        let mut fl = Fields::default();
        fl.guid(field, 0);
        self.values(self.player.guid, fl);
        self.send(opcode::SMSG_DESTROY_OBJECT, &packets::destroy(guid));
        self.save();
    }

    /// A new stack of `n` of `entry` in the player field `field` (an empty backpack slot).
    fn new_stack(&mut self, entry: u32, n: u32, field: u16) {
        let Some(t) = self.data.items.get(&entry).cloned() else { return };
        self.next_loot += 1;
        // Beside the starting items' and the worn chest gear's guids, clear of both.
        let guid = 0x4000_0000_0000_0000 | (self.player.guid << 16) | (0x2000 + self.next_loot % 0x2000);
        let u = packets::item_create(guid, self.player.guid, &t, n);
        self.send(opcode::SMSG_UPDATE_OBJECT, &packets::update_object(&[u]));
        let mut fl = Fields::default();
        fl.guid(field, guid);
        self.values(self.player.guid, fl);
        self.pack.push(super::PackItem { guid, entry, count: n, field });
    }
}

pub(super) fn is_backpack(field: u16) -> bool {
    field >= f::PLAYER_INV_SLOT_HEAD && BACKPACK.contains(&((field - f::PLAYER_INV_SLOT_HEAD) / 2))
}
