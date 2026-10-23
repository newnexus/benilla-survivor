//! Chest gear. Each chest holds three items, rolled when a run starts: a weapon or a piece of
//! armour the class can wear (`gear.tsv`, the zone's own). An item's rarity is its 1.12 quality
//! (white common, green uncommon, blue rare, purple epic, orange legendary). Besides its own 1.12
//! armour, damage and bonuses every item gives one survivor stat by its slot, stronger the rarer
//! it is; a rare one or better gives a second, picked at random.
//!
//! | slot | stat | | slot | stat |
//! |---|---|---|---|---|
//! | head | crit chance | | feet | run speed |
//! | shoulders, weapons | spell or physical damage | | wrists | a chance to hit one more enemy |
//! | chest | health | | hands | pickup range |
//! | waist | cooldowns | | back | experience |
//! | legs | costs, or regeneration | | | |
//!
//! Costs and regeneration are of whatever the wearer's class spends: mana, energy or rage. Damage
//! is spell damage on wands, staves and items with a caster's bonus (intellect, spirit, mana),
//! physical on the rest.
//! Half the legs lower costs; the other half quicken the resource.
//!
//! Of the armour found, most ([`OWN_ARMOUR_PERCENT`]) is the class's own kind (warriors mail,
//! rogues leather, the casters cloth); the rest is any other kind, often one the class cannot
//! wear: that is for the player's other characters, who share the collection.
//!
//! Taking a card wears the item for the run; taking the one already worn raises its item level
//! instead (up to [`MAX_UPGRADES`]); taking one the class cannot wear keeps it for the alts. What
//! is worn or kept when the player walks back into town, alive, joins the collection; dying loses
//! it.

use std::collections::HashMap;
use std::time::Instant;

use benilla_protocol::messages::opcode;

use super::Game;
use crate::packets::{self, f, Fields};
use crate::picks::{Card, ItemCard};

/// The cards a chest offers.
const LOOT: usize = 3;
/// The share of armour found that is the class's own kind, percent.
const OWN_ARMOUR_PERCENT: u64 = 80;
/// A chest recycled gives back this part of its price, percent.
const RECYCLE_PERCENT: u32 = 20;
/// How far each rarity, common to legendary, pushes the slot's stat and the second one.
const PRIMARY_RATE: [f32; 5] = [1.0, 1.4, 1.8, 2.2, 2.5];
const SECONDARY_RATE: [f32; 5] = [0.0, 0.0, 0.5, 1.0, 1.5];
pub(crate) const RARITY_NAMES: [&str; 5] = ["Common", "Uncommon", "Rare", "Epic", "Legendary"];
/// Item level upgrades one item can take, each this much stronger and this many levels.
pub(crate) const MAX_UPGRADES: u32 = 5;
const UPGRADE_STEP: f32 = 0.15;
const UPGRADE_LEVELS: u32 = 3;
/// Base run speed, yards a second.
const RUN_SPEED: f32 = 7.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub(crate) enum GearStat {
    Crit,
    Damage,
    Health,
    Cooldown,
    Cost,
    Speed,
    ExtraTarget,
    Pickup,
    Experience,
    /// Before the split by resource and by damage kind: read from old saves, turned into
    /// [`Self::Regen`] and [`Self::SpellDamage`] or [`Self::PhysicalDamage`] on load.
    ManaRegen,
    EnergyRegen,
    RageRegen,
    /// The wearer's own resource quickens: mana, energy, or rage in combat.
    Regen,
    /// Spells of any school but physical, heals too.
    SpellDamage,
    /// Weapon swings and physical skills.
    PhysicalDamage,
}

impl GearStat {
    /// Its place in [`ALL_STATS`], for the save file.
    pub(crate) fn index(self) -> usize {
        ALL_STATS.iter().position(|&s| s == self).unwrap_or(0)
    }

    pub(crate) fn from_index(i: usize) -> Option<Self> {
        ALL_STATS.get(i).copied()
    }

    /// A common item's amount (a share, 0.05 = 5%).
    fn base(self) -> f32 {
        match self {
            Self::Crit => 0.03,
            Self::Damage | Self::SpellDamage | Self::PhysicalDamage => 0.06,
            Self::Health => 0.12,
            Self::Cooldown => 0.06,
            Self::Cost => 0.08,
            Self::Speed => 0.06,
            Self::ExtraTarget => 0.15,
            Self::Pickup => 0.2,
            Self::Experience => 0.08,
            Self::ManaRegen | Self::EnergyRegen | Self::RageRegen | Self::Regen => 0.15,
        }
    }

    /// Its line for a wearer whose resource is `power` (`UnitPower` kind: 0 mana, 1 rage,
    /// 3 energy).
    fn line(self, x: f32, power: u8) -> String {
        let p = (x * 100.0).round();
        let resource = match power {
            1 => "rage",
            3 => "energy",
            _ => "mana",
        };
        match self {
            Self::Crit => format!("+{p}% critical strike chance"),
            Self::Damage => format!("+{p}% damage"),
            Self::SpellDamage => format!("+{p}% spell damage"),
            Self::PhysicalDamage => format!("+{p}% physical damage"),
            Self::Health => format!("+{p}% health"),
            Self::Cooldown => format!("-{p}% cooldowns"),
            Self::Cost => format!("-{p}% {resource} cost"),
            Self::Speed => format!("+{p}% run speed"),
            Self::ExtraTarget => format!("{p}% chance to hit one more enemy"),
            Self::Pickup => format!("+{p}% pickup range"),
            Self::Experience => format!("+{p}% experience"),
            Self::ManaRegen => format!("+{p}% mana regeneration"),
            Self::EnergyRegen => format!("+{p}% energy regeneration"),
            Self::RageRegen => format!("+{:.1} rage a second in combat", x * RAGE_REGEN_SCALE),
            Self::Regen if power == 1 => Self::RageRegen.line(x, power),
            Self::Regen => format!("+{p}% {resource} regeneration"),
        }
    }

    /// The stat an equipment slot gives.
    fn of_slot(slot: u16) -> Self {
        match slot {
            0 => Self::Crit,
            4 => Self::Health,
            5 => Self::Cooldown,
            6 => Self::Cost,
            7 => Self::Speed,
            8 => Self::ExtraTarget,
            9 => Self::Pickup,
            14 => Self::Experience,
            _ => Self::Damage,
        }
    }
}

/// A share of [`GearStat::RageRegen`] in rage a second: 0.15 is 1.5.
pub(crate) const RAGE_REGEN_SCALE: f32 = 10.0;

/// In this order in the save file: new stats go at the end.
const ALL_STATS: [GearStat; 15] = [
    GearStat::Crit,
    GearStat::Damage,
    GearStat::Health,
    GearStat::Cooldown,
    GearStat::Cost,
    GearStat::Speed,
    GearStat::ExtraTarget,
    GearStat::Pickup,
    GearStat::Experience,
    GearStat::ManaRegen,
    GearStat::EnergyRegen,
    GearStat::RageRegen,
    GearStat::Regen,
    GearStat::SpellDamage,
    GearStat::PhysicalDamage,
];

/// The stats an item may roll: the old ones only stand in for these.
const ROLLED_STATS: [GearStat; 10] = [
    GearStat::Crit,
    GearStat::Damage,
    GearStat::Health,
    GearStat::Cooldown,
    GearStat::Cost,
    GearStat::Speed,
    GearStat::ExtraTarget,
    GearStat::Pickup,
    GearStat::Experience,
    GearStat::Regen,
];

/// `ItemSubclass` of armour: cloth, leather, mail, plate.
const CLOTH: u32 = 1;
const LEATHER: u32 = 2;

/// One rolled item: in a chest, worn, or brought home.
#[derive(Clone, Debug)]
pub(crate) struct GearItem {
    pub(crate) entry: u32,
    pub(crate) rarity: u8,
    /// Its stats at no upgrades.
    stats: Vec<(GearStat, f32)>,
    pub(crate) upgrades: u32,
    /// The collection item it is, when worn from the collection.
    pub(crate) owned: Option<u64>,
}

/// An item in the collection.
#[derive(Clone, Debug)]
pub(crate) struct OwnedItem {
    pub(crate) id: u64,
    pub(crate) item: GearItem,
    pub(crate) favourite: bool,
}

/// The slots the collection chest has tabs for, in order.
const ARMORY_SLOTS: [u16; 12] = [0, 2, 4, 5, 6, 7, 8, 9, 14, 15, 16, 17];
/// How close the player stands to open the collection chest, and how far to walk off to close it.
const ARMORY_RANGE: f32 = 3.5;
const ARMORY_RESET: f32 = 7.0;

impl GearItem {
    /// An item read back from the save file.
    pub(crate) fn restored(
        entry: u32,
        rarity: u8,
        stats: Vec<(GearStat, f32)>,
        upgrades: u32,
        id: u64,
    ) -> Self {
        Self {
            entry,
            rarity,
            stats,
            upgrades,
            owned: Some(id),
        }
    }

    pub(crate) fn stats(&self) -> &[(GearStat, f32)] {
        &self.stats
    }

    /// Its stats at `rarity`: the first at the primary rate, the second (if any) at the
    /// secondary one.
    fn rate(&mut self, rarity: u8) {
        self.rarity = rarity;
        let r = usize::from(rarity);
        for (k, (s, x)) in self.stats.iter_mut().enumerate() {
            *x = s.base() * if k == 0 { PRIMARY_RATE[r] } else { SECONDARY_RATE[r] };
        }
    }

    /// Its stats, a line each, for the item tooltip's description, led by [`STAT_LINES_MARK`]:
    /// the client's tooltip draws them as white stat lines above the item's own instead of a
    /// quoted gold description. `power` is the wearer's resource.
    fn description(&self, power: u8) -> String {
        let mut lines: Vec<String> =
            self.stat_values().map(|(s, x)| s.line(x, power)).collect();
        if self.upgrades > 0 {
            lines.push(format!("Upgraded +{}", self.upgrades));
        }
        format!("{STAT_LINES_MARK}{}", lines.join("\n"))
    }

    fn stat_values(&self) -> impl Iterator<Item = (GearStat, f32)> + '_ {
        let k = 1.0 + UPGRADE_STEP * self.upgrades as f32;
        self.stats.iter().map(move |&(s, x)| (s, x * k))
    }
}

/// Leads an item description the client's tooltip shows as stat lines (`benilla-ui`
/// `tooltip_item/render.rs`, marked `// survivor:`).
const STAT_LINES_MARK: char = '\u{1}';

/// An item's own 1.12 bonus as its tooltip words it: `+3 Agility`.
fn bonus_line(kind: u32, value: i32) -> Option<String> {
    let name = match kind {
        0 => "Mana",
        1 => "Health",
        3 => "Agility",
        4 => "Strength",
        5 => "Intellect",
        6 => "Spirit",
        7 => "Stamina",
        _ => return None,
    };
    Some(format!("{value:+} {name}"))
}

/// Where an item of `inventory_type` is worn, as its tooltip says.
fn inventory_text(inventory_type: u32) -> &'static str {
    match inventory_type {
        1 => "Head",
        3 => "Shoulder",
        5 | 20 => "Chest",
        6 => "Waist",
        7 => "Legs",
        8 => "Feet",
        9 => "Wrist",
        10 => "Hands",
        16 => "Back",
        13 => "One-Hand",
        17 => "Two-Hand",
        21 => "Main Hand",
        14 | 22 => "Off Hand",
        23 => "Held In Off-hand",
        15 | 26 => "Ranged",
        25 => "Thrown",
        _ => "",
    }
}

/// What an item is, as its tooltip says: the armour's kind or the weapon's.
fn kind_text(class: u32, subclass: u32) -> &'static str {
    match (class, subclass) {
        (4, 1) => "Cloth",
        (4, 2) => "Leather",
        (4, 3) => "Mail",
        (4, 4) => "Plate",
        (4, 6) => "Shield",
        (2, 0 | 1) => "Axe",
        (2, 2) => "Bow",
        (2, 3) => "Gun",
        (2, 4 | 5) => "Mace",
        (2, 6) => "Polearm",
        (2, 7 | 8) => "Sword",
        (2, 10) => "Staff",
        (2, 13) => "Fist Weapon",
        (2, 15) => "Dagger",
        (2, 16) => "Thrown",
        (2, 18) => "Crossbow",
        (2, 19) => "Wand",
        (2, 20) => "Fishing Pole",
        _ => "",
    }
}

/// The equipment slot names on a card.
fn slot_name(slot: u16) -> &'static str {
    match slot {
        0 => "Head",
        2 => "Shoulders",
        4 => "Chest",
        5 => "Waist",
        6 => "Legs",
        7 => "Feet",
        8 => "Wrists",
        9 => "Hands",
        14 => "Back",
        15 => "Main Hand",
        16 => "Off Hand",
        17 => "Ranged",
        _ => "Item",
    }
}

/// The armour `class` is best in: warriors mail, rogues leather, the casters cloth.
fn class_armour(class: u8) -> u32 {
    match class {
        1 => 3,
        4 => LEATHER,
        _ => CLOTH,
    }
}

/// Whether an item is `class`'s own find: a weapon it wields, or armour of its own kind (any
/// cloak, a warrior's shield).
fn own_find(class: u8, item_class: u32, subclass: u32, inventory_type: u32) -> bool {
    match item_class {
        4 => {
            subclass == class_armour(class)
                || inventory_type == 16
                || (subclass == 6 && class == 1)
        }
        _ => wearable(class, item_class, subclass, inventory_type),
    }
}

/// Whether `class` wears or wields an item of `item_class`/`subclass` in `inventory_type`.
fn wearable(class: u8, item_class: u32, subclass: u32, inventory_type: u32) -> bool {
    match item_class {
        // Armour: cloth for all, leather up from rogues, mail for warriors, shields for warriors.
        4 => match subclass {
            0 => true,
            1 => true,
            2 => matches!(class, 1 | 4),
            3 => class == 1,
            6 => class == 1,
            _ => false,
        },
        // Weapons a class trains.
        2 => {
            let ok: &[u32] = match class {
                1 => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 13, 15, 16, 18],
                4 => &[2, 3, 4, 7, 13, 15, 16, 18],
                5 => &[4, 10, 15, 19],
                8 | 9 => &[7, 10, 15, 19],
                _ => &[],
            };
            ok.contains(&subclass) && !(inventory_type == 17 && class == 4)
        }
        _ => false,
    }
}

impl Game {
    /// Three items for every chest, for this run.
    pub(super) fn roll_chest_loot(&mut self) {
        self.chest_loot = (0..self.chests.len()).map(|_| self.roll_gear_set()).collect();
        self.publish_chest_glow();
    }

    /// Three items to choose from, as a chest holds: mostly the class's own armour kind.
    fn roll_gear_set(&mut self) -> Vec<GearItem> {
        let class = self.player.class;
        let gear = |own: bool| -> Vec<u32> {
            self.data
                .gear
                .iter()
                .filter(|&&(entry, _)| {
                    self.data.items.get(&entry).is_some_and(|t| {
                        super::super::world::equip_slot(t.inventory_type).is_some()
                            && if own {
                                own_find(class, t.class, t.subclass, t.inventory_type)
                            } else {
                                t.class == 4
                                    && matches!(t.subclass, 1..=4 | 6)
                                    && !own_find(class, t.class, t.subclass, t.inventory_type)
                            }
                    })
                })
                .map(|g| g.0)
                .collect()
        };
        let (pool, others) = (gear(true), gear(false));
        let mut items: Vec<GearItem> = Vec::new();
        while items.len() < LOOT && !pool.is_empty() {
            let mut entry = pool[(self.rng.next() % pool.len() as u64) as usize];
            let armour = self.data.items.get(&entry).is_some_and(|t| t.class == 4);
            if armour && !others.is_empty() && self.rng.next() % 100 >= OWN_ARMOUR_PERCENT {
                entry = others[(self.rng.next() % others.len() as u64) as usize];
            }
            if items.iter().any(|i| i.entry == entry) {
                continue;
            }
            items.push(self.roll_item(entry));
        }
        items
    }

    /// The rarity of `entry`: its 1.12 quality, poor and common both common.
    pub(super) fn rarity_of(&self, entry: u32) -> u8 {
        let quality = self.data.items.get(&entry).map_or(1, |t| t.quality);
        quality.saturating_sub(1).min(4) as u8
    }

    /// An item of `entry` with its stats rolled.
    pub(super) fn roll_item(&mut self, entry: u32) -> GearItem {
        let rarity = self.rarity_of(entry);
        let template = self.data.items.get(&entry);
        let slot = template
            .and_then(|t| super::super::world::equip_slot(t.inventory_type))
            .unwrap_or(15);
        let mut main = GearStat::of_slot(slot);
        // Legs: half lower costs, half quicken the resource.
        if main == GearStat::Cost && self.rng.next() % 2 == 0 {
            main = GearStat::Regen;
        }
        let main = self.fit_stat(entry, main);
        let mut item = GearItem {
            entry,
            rarity,
            stats: vec![(main, 0.0)],
            upgrades: 0,
            owned: None,
        };
        self.second_stat(&mut item);
        item.rate(rarity);
        item
    }

    /// A second stat for a rare item or better, unlike its first; none below rare.
    fn second_stat(&mut self, item: &mut GearItem) {
        if SECONDARY_RATE[usize::from(item.rarity)] == 0.0 {
            item.stats.truncate(1);
        } else if item.stats.len() < 2 {
            let main = item.stats[0].0;
            let others: Vec<GearStat> = ROLLED_STATS
                .iter()
                .map(|&s| self.fit_stat(item.entry, s))
                .filter(|&s| s != main)
                .collect();
            let second = others[(self.rng.next() % others.len() as u64) as usize];
            item.stats.push((second, 0.0));
        }
    }

    /// A saved item brought to the present rules: its rarity its quality, its stats rated by it
    /// (a second added or dropped to suit). Its stat kinds are kept.
    pub(super) fn rerate(&mut self, item: &mut GearItem) {
        item.rarity = self.rarity_of(item.entry);
        if item.stats.is_empty() {
            return;
        }
        // Saved before the split, or under an older rule: fitted to the present one.
        for k in 0..item.stats.len() {
            let s = match item.stats[k].0 {
                GearStat::Damage | GearStat::SpellDamage | GearStat::PhysicalDamage => {
                    self.damage_by_item(item.entry)
                }
                GearStat::ManaRegen | GearStat::EnergyRegen | GearStat::RageRegen => GearStat::Regen,
                s => s,
            };
            item.stats[k].0 = s;
        }
        item.stats.dedup_by_key(|s| s.0);
        self.second_stat(item);
        item.rate(item.rarity);
    }

    /// `stat` made concrete for `entry`: damage becomes spell damage or physical.
    fn fit_stat(&self, entry: u32, stat: GearStat) -> GearStat {
        match stat {
            GearStat::Damage => self.damage_by_item(entry),
            s => s,
        }
    }

    /// The damage an item gives: spell damage for a wand or a staff, or an item with a caster's
    /// bonus (intellect, spirit, mana); physical for the rest.
    fn damage_by_item(&self, entry: u32) -> GearStat {
        let spell = self.data.items.get(&entry).is_some_and(|t| {
            (t.class == 2 && matches!(t.subclass, 10 | 19))
                || t.stats.iter().any(|&(k, v)| v > 0 && matches!(k, 0 | 5 | 6))
        });
        if spell {
            GearStat::SpellDamage
        } else {
            GearStat::PhysicalDamage
        }
    }

    /// Whether the player's class wears or wields `entry`.
    pub(super) fn can_wear(&self, entry: u32) -> bool {
        self.data.items.get(&entry).is_some_and(|t| {
            wearable(self.player.class, t.class, t.subclass, t.inventory_type)
        })
    }

    /// The equipment slot `entry` goes in.
    pub(super) fn slot_of_item(&self, entry: u32) -> Option<u16> {
        self.data
            .items
            .get(&entry)
            .and_then(|t| super::super::world::equip_slot(t.inventory_type))
    }

    /// Tell the screen where the closed chests are and how good their best item is.
    pub(super) fn publish_chest_glow(&self) {
        let list = self
            .chests
            .iter()
            .zip(&self.chest_loot)
            .filter(|(c, _)| !c.is_opened())
            .map(|(c, loot)| (c.pos, loot.iter().map(|i| i.rarity).max().unwrap_or(0)));
        let mut list: Vec<([f32; 3], u8)> = if self.in_run() { list.collect() } else { Vec::new() };
        list.extend(
            crate::town::TOWNS
                .iter()
                .map(|t| ([t.armory[0], t.armory[1], t.armory[2]], crate::chest_glow::COLLECTION)),
        );
        crate::chest_glow::set(list);
    }

    /// The cards of chest `i`'s items.
    pub(super) fn chest_cards(&mut self, i: usize) -> (Vec<Card>, Vec<GearItem>) {
        let loot = self.chest_loot.get(i).cloned().unwrap_or_default();
        // Each item shown is seen, for the collection chest's shadows.
        for g in &loot {
            let r = self.seen.entry(g.entry).or_insert(g.rarity);
            *r = (*r).max(g.rarity);
        }
        let cards = loot.iter().map(|g| self.item_card(g)).collect();
        (cards, loot)
    }

    fn item_card(&self, g: &GearItem) -> Card {
        let t = self.data.items.get(&g.entry);
        let slot = t
            .and_then(|t| super::super::world::equip_slot(t.inventory_type))
            .unwrap_or(15);
        let worn = self.run_gear.get(&slot).filter(|w| w.entry == g.entry);
        let mut lines = Vec::new();
        if let Some(t) = t {
            if t.damage.1 > 0.0 {
                lines.push(format!(
                    "{:.0} - {:.0} damage, {:.1} speed",
                    t.damage.0,
                    t.damage.1,
                    t.delay as f32 / 1000.0
                ));
            } else if t.armor > 0 {
                lines.push(format!("{} armor", t.armor));
            }
        }
        if !self.can_wear(g.entry) {
            lines.push("For your other characters: kept if you get home".to_owned());
        }
        match worn {
            Some(w) if w.upgrades < MAX_UPGRADES => lines.push(format!(
                "Worn: item level +{} (upgrade {} of {MAX_UPGRADES})",
                UPGRADE_LEVELS,
                w.upgrades + 1
            )),
            _ => {}
        }
        let shown = worn.unwrap_or(g);
        let power = self.player.power.kind;
        lines.extend(shown.stat_values().map(|(s, x)| s.line(x, power)));
        if let Some(t) = t {
            lines.extend(t.stats.iter().filter_map(|&(k, v)| bonus_line(k, v)));
        }
        Card {
            spell: 0,
            replaces: None,
            defensive: false,
            rarity: shown.rarity,
            boost: Vec::new(),
            item: Some(ItemCard {
                entry: g.entry,
                name: t.map_or_else(|| format!("Item {}", g.entry), |t| t.name.clone()),
                display: t.map_or(0, |t| t.display),
                slot: slot_name(slot).to_owned(),
                lines,
                upgrade: worn.is_some(),
            }),
        }
    }

    /// Keep a chest's item without wearing it: into the collection if the run gets home.
    pub(super) fn keep_item(&mut self, g: GearItem) {
        let name = self.data.items.get(&g.entry).map_or_else(String::new, |t| t.name.clone());
        eprintln!("survivor: keeping {name} for the collection");
        let line = format!("{name}: kept, it goes to your collection when you get home");
        self.send(opcode::SMSG_NOTIFICATION, &packets::notification(&line));
        self.run_alt.push(g);
    }

    /// Take a chest's item: wear it for the run, or raise the one already worn.
    pub(super) fn take_item(&mut self, g: GearItem) {
        let Some(t) = self.data.items.get(&g.entry).cloned() else { return };
        let Some(slot) = super::super::world::equip_slot(t.inventory_type) else { return };
        let name = t.name.clone();
        if !self.can_wear(g.entry) {
            eprintln!("survivor: keeping {name} for the alts");
            let line = format!("{name}: kept for your other characters");
            self.send(opcode::SMSG_NOTIFICATION, &packets::notification(&line));
            self.run_alt.push(g);
            return;
        }
        match self.run_gear.get_mut(&slot) {
            Some(w) if w.entry == g.entry => {
                w.upgrades = (w.upgrades + 1).min(MAX_UPGRADES);
                eprintln!("survivor: {name} upgraded to +{}", w.upgrades);
                let line = format!("{name}: item level +{}", w.upgrades * UPGRADE_LEVELS);
                self.send(opcode::SMSG_NOTIFICATION, &packets::notification(&line));
            }
            _ => {
                eprintln!(
                    "survivor: wearing {} {name} in slot {slot}",
                    RARITY_NAMES[usize::from(g.rarity)]
                );
                self.run_gear.insert(slot, g);
                let mut fl = Fields::default();
                self.wear_in_slot(slot, &mut fl);
                self.values(self.player.guid, fl);
                if slot == 15 && t.damage.1 > 0.0 {
                    self.player.weapon = (
                        t.damage.0.max(1.0),
                        t.damage.1.max(2.0),
                        std::time::Duration::from_millis(u64::from(t.delay.max(1000))),
                    );
                    let mut fl = Fields::default();
                    fl.setf(f::MINDAMAGE, self.player.weapon.0)
                        .setf(f::MAXDAMAGE, self.player.weapon.1)
                        .set(f::BASEATTACKTIME, t.delay.max(1000));
                    self.values(self.player.guid, fl);
                }
            }
        }
        self.apply_gear();
    }

    /// The run gear's total for `stat`.
    pub(super) fn gear(&self, stat: GearStat) -> f32 {
        self.run_gear
            .values()
            .flat_map(GearItem::stat_values)
            .filter(|&(s, _)| s == stat)
            .map(|(_, x)| x)
            .sum::<f32>()
            + self.rune_bonus(stat)
            + self.talent_bonus(stat)
    }

    /// What the gear changes on the character itself: health and run speed.
    pub(super) fn apply_gear(&mut self) {
        let base = super::base_health(self.player.class)
            + super::HEALTH_PER_LEVEL * (self.player.level - 1);
        let max = (base as f32 * (1.0 + self.gear(GearStat::Health))).round() as u32
            + self.food_max_health();
        if max != self.player.max_health {
            let gained = max.saturating_sub(self.player.max_health);
            self.player.max_health = max;
            self.player.health = (self.player.health + gained).min(max);
            let mut fl = Fields::default();
            fl.set(f::MAXHEALTH, max).set(f::HEALTH, self.player.health);
            self.values(self.player.guid, fl);
        }
        let speed = RUN_SPEED * (1.0 + self.gear(GearStat::Speed));
        if (speed - self.run_speed).abs() > 0.01 {
            self.run_speed = speed;
            self.speed_counter += 1;
            self.send(
                opcode::SMSG_FORCE_RUN_SPEED_CHANGE,
                &packets::force_run_speed(self.player.guid, self.speed_counter, speed),
            );
        }
    }

    /// Back in town alive: the run's finds join the collection (worn if their slot is free), and
    /// an owned item keeps the upgrades it took. Dead, the finds and the upgrades are lost, the
    /// owned items kept. Either way the character wears their collection's things again.
    pub(super) fn end_run_gear(&mut self, died: bool) {
        let worn: Vec<(u16, GearItem)> = self.run_gear.drain().collect();
        for (slot, g) in worn {
            let name = self.data.items.get(&g.entry).map_or("", |t| t.name.as_str()).to_owned();
            if died {
                if g.owned.is_none() {
                    eprintln!("survivor: lost {name}");
                }
                continue;
            }
            match g.owned {
                Some(id) => {
                    if let Some(o) = self.collection.iter_mut().find(|o| o.id == id) {
                        o.item.upgrades = g.upgrades;
                    }
                }
                None => {
                    let id = self.next_owned;
                    self.next_owned += 1;
                    eprintln!("survivor: brought home {name}");
                    let line = format!(
                        "Brought home: {name} ({})",
                        RARITY_NAMES[usize::from(g.rarity)]
                    );
                    self.send(opcode::SMSG_NOTIFICATION, &packets::notification(&line));
                    let mut item = g;
                    item.owned = Some(id);
                    self.collection.push(OwnedItem {
                        id,
                        item,
                        favourite: false,
                    });
                    self.loadout.entry(slot).or_insert(id);
                }
            }
        }
        for mut item in std::mem::take(&mut self.run_alt) {
            let name = self.data.items.get(&item.entry).map_or("", |t| t.name.as_str()).to_owned();
            if died {
                eprintln!("survivor: lost {name}");
                continue;
            }
            let id = self.next_owned;
            self.next_owned += 1;
            eprintln!("survivor: brought home {name} for the collection");
            let line = format!("Brought home to your collection: {name}");
            self.send(opcode::SMSG_NOTIFICATION, &packets::notification(&line));
            item.owned = Some(id);
            self.collection.push(OwnedItem {
                id,
                item,
                favourite: false,
            });
        }
        self.wear_loadout();
        self.save();
    }

    /// A quest's reward: into the collection, its survivor stats rolled, worn when its slot is
    /// empty.
    pub(super) fn quest_reward_item(&mut self, entry: u32) {
        let Some(slot) = self.slot_of_item(entry) else { return };
        // As its preview showed it, when it had one.
        let mut item = match self.reward_rolls.remove(&entry) {
            Some(g) => g,
            None => self.roll_item(entry),
        };
        let id = self.next_owned;
        self.next_owned += 1;
        item.owned = Some(id);
        let name = self.data.items.get(&entry).map_or_else(String::new, |t| t.name.clone());
        eprintln!("survivor: quest reward {name}");
        self.send(
            opcode::SMSG_NOTIFICATION,
            &packets::notification(&format!("Received: {name}, in your collection")),
        );
        self.collection.push(OwnedItem {
            id,
            item,
            favourite: false,
        });
        if self.can_wear(entry) {
            self.loadout.entry(slot).or_insert(id);
        }
        self.wear_loadout();
        self.save();
    }

    /// The stats of the chest gear worn now as `entry`, for its tooltip.
    pub(crate) fn item_description(&self, entry: u32) -> Option<String> {
        let power = self.player.power.kind;
        self.run_gear.values().find(|g| g.entry == entry).map(|g| g.description(power))
    }

    /// Put what `slot` now holds on the character: the chest gear worn there, or else the
    /// starting gear. Chest gear gets an item object of its own in the equipment slot, so the
    /// character window shows it with its tooltip; the model shows its look. The fields go in
    /// `fl`; the item object, made or dropped, is sent at once, before them.
    fn wear_in_slot(&mut self, slot: u16, fl: &mut Fields) {
        let start = self.start_visible.get(&slot).copied().unwrap_or((0, 0));
        let (entry, guid) = match self.run_gear.get(&slot).map(|g| g.entry) {
            Some(entry) if entry == start.0 => start,
            Some(entry) if self.worn_items.get(&slot).is_some_and(|w| w.1 == entry) => {
                (entry, self.worn_items[&slot].0)
            }
            Some(entry) => {
                let Some(t) = self.data.items.get(&entry) else { return };
                self.next_worn += 1;
                // Beside the starting items' guids (`world::self_create`), clear of them.
                let guid = 0x4000_0000_0000_0000
                    | (self.player.guid << 16)
                    | (0x8000 + self.next_worn % 0x8000);
                let u = packets::item_create(guid, self.player.guid, t, 1);
                self.send(opcode::SMSG_UPDATE_OBJECT, &packets::update_object(&[u]));
                (entry, guid)
            }
            None => start,
        };
        let made = guid != start.1;
        let old = if made {
            self.worn_items.insert(slot, (guid, entry))
        } else {
            self.worn_items.remove(&slot)
        };
        if let Some((old, _)) = old.filter(|o| o.0 != guid) {
            self.send(
                opcode::SMSG_UPDATE_OBJECT,
                &packets::update_object(&[packets::Update::OutOfRange(vec![old])]),
            );
        }
        // The item's tooltip with our own stats: the client takes a fresh answer for an entry
        // at any time, over what it cached.
        if let Some(g) = self.run_gear.get(&slot) {
            let reply = packets::item_query_response_described(
                entry,
                self.data.items.get(&entry),
                &g.description(self.player.power.kind),
            );
            self.send(opcode::SMSG_ITEM_QUERY_SINGLE_RESPONSE, &reply);
        }
        fl.guid(f::PLAYER_INV_SLOT_HEAD + slot * 2, guid);
        fl.set(f::PLAYER_VISIBLE_ITEM_ENTRY + slot * f::PLAYER_VISIBLE_ITEM_STRIDE, entry);
    }

    /// The collection's worn things on the character: the run gear, the looks, the weapon, the
    /// stats.
    pub(super) fn wear_loadout(&mut self) {
        self.run_gear = self
            .loadout
            .iter()
            .filter_map(|(&slot, &id)| {
                let o = self.collection.iter().find(|o| o.id == id)?;
                Some((slot, o.item.clone()))
            })
            .collect();
        let slots: std::collections::BTreeSet<u16> = self
            .start_visible
            .keys()
            .chain(ARMORY_SLOTS.iter())
            .copied()
            .collect();
        let mut fl = Fields::default();
        for slot in slots {
            self.wear_in_slot(slot, &mut fl);
        }
        self.values(self.player.guid, fl);
        self.player.weapon = self.start_weapon;
        if let Some(t) = self
            .run_gear
            .get(&15)
            .and_then(|g| self.data.items.get(&g.entry))
            .filter(|t| t.damage.1 > 0.0)
        {
            self.player.weapon = (
                t.damage.0.max(1.0),
                t.damage.1.max(2.0),
                std::time::Duration::from_millis(u64::from(t.delay.max(1000))),
            );
        }
        let mut fl = Fields::default();
        fl.setf(f::MINDAMAGE, self.player.weapon.0)
            .setf(f::MAXDAMAGE, self.player.weapon.1)
            .set(f::BASEATTACKTIME, self.player.weapon.2.as_millis() as u32);
        self.values(self.player.guid, fl);
        self.apply_gear();
    }

    /// In town, standing by the collection chest opens it; walking off closes it.
    pub(super) fn open_armory_nearby(&mut self) {
        let me = self.player.pos;
        let near = |t: &crate::town::Town| {
            super::dist2d([t.armory[0], t.armory[1], t.armory[2]], me)
        };
        let far = crate::town::TOWNS.iter().all(|t| near(t) > ARMORY_RESET);
        if self.armory_open.is_some() {
            if far || self.in_run() {
                self.armory_open = None;
                crate::armory::clear();
            }
            return;
        }
        if self.armory_closed {
            self.armory_closed = !far;
            return;
        }
        if self.in_run()
            || self.paused()
            || self.player.dead_until.is_some()
            || !self.walked
        {
            return;
        }
        if crate::town::TOWNS.iter().any(|t| near(t) <= ARMORY_RANGE) {
            self.close_quest_panel();
            self.post_armory();
        }
    }

    /// The collection chest's tabs: every owned item of each slot the class can wear, and in
    /// shadow the ones only seen. Favourites first, then what is worn, then by rarity and
    /// upgrades.
    fn post_armory(&mut self) {
        let tabs = ARMORY_SLOTS
            .iter()
            .map(|&slot| {
                let mut rows: Vec<crate::armory::Row> = self
                    .collection
                    .iter()
                    .filter(|o| {
                        self.slot_of_item(o.item.entry) == Some(slot) && self.can_wear(o.item.entry)
                    })
                    .map(|o| {
                        let worn = self.loadout.get(&slot) == Some(&o.id);
                        self.armory_row(&o.item, o.id, o.favourite, worn, false)
                    })
                    .collect();
                let owned: std::collections::HashSet<u32> =
                    self.collection.iter().map(|o| o.item.entry).collect();
                rows.extend(
                    self.seen
                        .iter()
                        .filter(|(e, _)| {
                            !owned.contains(e)
                                && self.slot_of_item(**e) == Some(slot)
                                && self.can_wear(**e)
                        })
                        .map(|(&entry, &rarity)| {
                            let g = GearItem {
                                entry,
                                rarity,
                                stats: Vec::new(),
                                upgrades: 0,
                                owned: None,
                            };
                            self.armory_row(&g, 0, false, false, true)
                        }),
                );
                rows.sort_by(|a, b| {
                    (b.favourite, b.worn, !b.seen_only, b.rarity, b.upgrades)
                        .cmp(&(a.favourite, a.worn, !a.seen_only, a.rarity, a.upgrades))
                        .then_with(|| a.name.cmp(&b.name))
                });
                crate::armory::Tab {
                    slot,
                    name: slot_name(slot).to_owned(),
                    rows,
                }
            })
            .collect();
        self.armory_open = Some(crate::armory::post(tabs));
    }

    pub(super) fn armory_row(
        &self,
        g: &GearItem,
        key: u64,
        favourite: bool,
        worn: bool,
        seen_only: bool,
    ) -> crate::armory::Row {
        let t = self.data.items.get(&g.entry);
        let mut base = Vec::new();
        if let Some(t) = t {
            if t.damage.1 > 0.0 {
                base.push(format!("{:.0} - {:.0} Damage", t.damage.0, t.damage.1));
                base.push(format!("Speed {:.2}", t.delay as f32 / 1000.0));
            } else if t.armor > 0 {
                base.push(format!("{} Armor", t.armor));
            }
        }
        let power = self.player.power.kind;
        let own = g.stat_values().map(|(s, x)| s.line(x, power)).collect();
        let bonus = t.map_or_else(Vec::new, |t| {
            t.stats.iter().filter_map(|&(k, v)| bonus_line(k, v)).collect()
        });
        crate::armory::Row {
            key,
            entry: g.entry,
            name: t.map_or_else(|| format!("Item {}", g.entry), |t| t.name.clone()),
            display: t.map_or(0, |t| t.display),
            rarity: g.rarity,
            upgrades: g.upgrades,
            favourite,
            worn,
            seen_only,
            slot_text: t.map_or("", |t| inventory_text(t.inventory_type)).to_owned(),
            kind: t.map_or("", |t| kind_text(t.class, t.subclass)).to_owned(),
            durability: t.map_or(0, |t| t.durability),
            base,
            own,
            bonus,
        }
    }

    /// The collection chest's answer.
    pub(super) fn take_armory_action(&mut self) {
        let Some((id, action)) = crate::armory::take_action() else { return };
        if self.armory_open != Some(id) {
            return;
        }
        match action {
            crate::armory::Action::Close => {
                self.armory_open = None;
                crate::armory::clear();
                // Quiet until walked off and back.
                self.armory_closed = true;
                return;
            }
            crate::armory::Action::Favourite(key) => {
                if let Some(o) = self.collection.iter_mut().find(|o| o.id == key) {
                    o.favourite = !o.favourite;
                }
            }
            crate::armory::Action::Toggle(key) => {
                let Some(entry) = self.collection.iter().find(|o| o.id == key).map(|o| o.item.entry)
                else {
                    return;
                };
                let Some(slot) = self.slot_of_item(entry) else { return };
                if self.loadout.get(&slot) == Some(&key) {
                    self.loadout.remove(&slot);
                } else {
                    self.loadout.insert(slot, key);
                }
                self.wear_loadout();
            }
        }
        self.save();
        self.post_armory();
    }

    /// The collection chests in the towns, as game objects.
    pub(super) fn update_armory_chests(&mut self) {
        let me = self.player.pos;
        for (i, t) in crate::town::TOWNS.iter().enumerate() {
            let at = [t.armory[0], t.armory[1], t.armory[2]];
            let guid = 0xF110_0000_0000_0000 | u64::from(ARMORY_ENTRY) << 24 | 0x80_0000 | i as u64;
            let d = super::dist2d(at, me);
            let shown = self.armory_shown.contains(&i);
            if !shown && d <= super::VISIBLE {
                if let Some(tpl) = self.data.chest_templates.get(&ARMORY_ENTRY) {
                    let rot = [0.0, 0.0, (t.armory[3] / 2.0).sin(), (t.armory[3] / 2.0).cos()];
                    let u = packets::gameobject_create(guid, tpl, at, t.armory[3], rot);
                    self.send(opcode::SMSG_UPDATE_OBJECT, &packets::update_object(&[u]));
                    self.armory_shown.insert(i);
                }
            } else if shown && d > super::VISIBLE_DROP {
                self.armory_shown.remove(&i);
                self.send(
                    opcode::SMSG_UPDATE_OBJECT,
                    &packets::update_object(&[packets::Update::OutOfRange(vec![guid])]),
                );
            }
        }
    }

    /// An elite or a boss slain: a choice of items on the card screen, the world paused, no
    /// chest to open and nothing to pay. One must be taken.
    pub(super) fn open_elite_offer(&mut self, name: &str, now: Instant) {
        let loot = self.roll_gear_set();
        for g in &loot {
            let r = self.seen.entry(g.entry).or_insert(g.rarity);
            *r = (*r).max(g.rarity);
        }
        let cards: Vec<Card> = loot.iter().map(|g| self.item_card(g)).collect();
        if cards.is_empty() {
            return;
        }
        eprintln!("survivor: {name}'s spoils offered");
        let title = Some(format!("{name}'s spoils"));
        let id = crate::picks::post_titled(self.player.level, cards, title, None);
        self.pick = Some(super::pick::Pick::chest(id, now, loot, 0));
        self.freeze(now);
    }

    /// A chest opened: its items on the card screen, the world paused.
    /// Chest `i`, bought for `price`: its cards, or recycled for [`RECYCLE_PERCENT`] of it.
    pub(super) fn open_chest_offer(&mut self, i: usize, price: u32, now: Instant) {
        let (cards, loot) = self.chest_cards(i);
        if cards.is_empty() {
            return;
        }
        let title = self
            .data
            .chest_templates
            .get(&self.chests[i].entry)
            .map(|t| t.name.clone());
        let refund = price * RECYCLE_PERCENT / 100;
        let recycle = format!("Recycle: {} back", super::chest::money_text(refund));
        let id = crate::picks::post_titled(self.player.level, cards, title, Some(recycle));
        self.pick = Some(super::pick::Pick::chest(id, now, loot, refund));
        self.freeze(now);
        self.publish_chest_glow();
    }
}

/// The collection chest's template entry, the server's own.
pub(crate) const ARMORY_ENTRY: u32 = 900_001;

/// Run gear by equipment slot.
pub(crate) type RunGear = HashMap<u16, GearItem>;
