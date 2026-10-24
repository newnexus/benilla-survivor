//! What a character keeps between sessions: the meta level, the purse, the quests handed in and
//! the collection items it wears, in one plain text file per character name in
//! `$SURVIVOR_SAVE_DIR` (the launchers point it into the client's own state folder); no folder,
//! no saving. The collection itself (each item's rarity, stats, upgrades and favourite) and the
//! items only seen are every character's, in `_collection.txt` beside them.
//!
//! ```text
//! meta 3 450
//! money 1234
//! done 364,380
//! wear 1
//! bag 1476 7
//! ```
//! ```text
//! item 1 2652 2 1 1 4:0.168,1:0.06
//! seen 2653
//! ```
//! An item line is `id entry rarity upgrades favourite stats`. A character file from before the
//! collection was shared holds its own item lines, `id entry rarity upgrades favourite worn
//! stats`: they join the shared collection, under new ids, the first time it loads.

use std::fmt::Write as _;
use std::path::PathBuf;

use super::gear::{GearItem, GearStat, OwnedItem};
use super::Game;

fn path(name: &str) -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("SURVIVOR_SAVE_DIR")?);
    let safe: String = name.chars().filter(char::is_ascii_alphanumeric).collect();
    (!safe.is_empty()).then(|| dir.join(format!("{safe}.txt")))
}

/// The shared collection's file: no character's name has an underscore.
fn collection_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("SURVIVOR_SAVE_DIR")?).join("_collection.txt"))
}

/// An item line's fields after `item`: `id entry rarity upgrades favourite`, `worn` in an old
/// character file, then the stats.
struct ItemLine {
    id: u64,
    entry: u32,
    rarity: u8,
    upgrades: u32,
    favourite: bool,
    worn: bool,
    stats: Vec<(GearStat, f32)>,
}

fn item_line<'a>(mut w: impl Iterator<Item = &'a str>, with_worn: bool) -> ItemLine {
    let mut num = || w.next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    let (id, entry, rarity, upgrades, favourite) =
        (num(), num() as u32, num() as u8, num() as u32, num() != 0);
    let worn = with_worn && num() != 0;
    let stats = w
        .next()
        .unwrap_or("")
        .split(',')
        .filter_map(|p| {
            let (s, x) = p.split_once(':')?;
            Some((GearStat::from_index(s.parse().ok()?)?, x.parse().ok()?))
        })
        .collect();
    ItemLine {
        id,
        entry,
        rarity,
        upgrades,
        favourite,
        worn,
        stats,
    }
}

fn write_file(path: &std::path::Path, s: String) {
    let write = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(path, s));
    if let Err(e) = write {
        eprintln!("survivor: cannot save {}: {e}", path.display());
    }
}

impl Game {
    /// Write the character's file and the collection's; a failure is said and play goes on.
    pub(super) fn save(&self) {
        let Some(path) = path(&self.name) else { return };
        let mut s = String::new();
        let _ = writeln!(s, "meta {} {}", self.meta.level, self.meta.xp);
        let _ = writeln!(s, "money {}", self.banked_money());
        let _ = writeln!(s, "souls {}", self.souls);
        let _ = writeln!(s, "healer {}", self.healer.to_line());
        let order: Vec<String> = self.queue_order.iter().map(u32::to_string).collect();
        let _ = writeln!(s, "queue {}", order.join(","));
        for (chain, ranks) in &self.owned {
            let _ = writeln!(s, "owned {chain} {ranks}");
        }
        for (id, rank) in &self.talents {
            let _ = writeln!(s, "talent {id} {rank}");
        }
        let done: Vec<String> = self.quests_done().iter().map(u32::to_string).collect();
        let _ = writeln!(s, "done {}", done.join(","));
        for id in self.loadout.values() {
            let _ = writeln!(s, "wear {id}");
        }
        for (entry, count) in self.bag_to_save() {
            let _ = writeln!(s, "bag {entry} {count}");
        }
        write_file(&path, s);
        let Some(path) = collection_path() else { return };
        let mut s = String::new();
        for o in &self.collection {
            let stats: Vec<String> = o
                .item
                .stats()
                .iter()
                .map(|&(st, x)| format!("{}:{x:.4}", st.index()))
                .collect();
            let _ = writeln!(
                s,
                "item {} {} {} {} {} {}",
                o.id,
                o.item.entry,
                o.item.rarity,
                o.item.upgrades,
                u8::from(o.favourite),
                stats.join(",")
            );
        }
        for &entry in self.seen.keys() {
            let _ = writeln!(s, "seen {entry}");
        }
        write_file(&path, s);
    }

    /// An item read back, brought to the present rules, into the collection.
    fn restore_item(&mut self, l: ItemLine) {
        let mut item = GearItem::restored(l.entry, l.rarity, l.stats, l.upgrades, l.id);
        self.rerate(&mut item);
        self.next_owned = self.next_owned.max(l.id + 1);
        self.collection.push(OwnedItem {
            id: l.id,
            item,
            favourite: l.favourite,
        });
    }

    /// Wear owned item `id`, when the class can.
    fn restore_worn(&mut self, id: u64) {
        let Some(entry) = self.collection.iter().find(|o| o.id == id).map(|o| o.item.entry) else {
            return;
        };
        if let Some(slot) = self.slot_of_item(entry).filter(|_| self.can_wear(entry)) {
            self.loadout.insert(slot, id);
        }
    }

    /// Read the shared collection's file, if there is one.
    fn load_collection(&mut self) {
        let Some(path) = collection_path() else { return };
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        for line in text.lines() {
            let mut w = line.split_whitespace();
            match w.next() {
                Some("item") => {
                    let l = item_line(w, false);
                    if self.data.items.contains_key(&l.entry) {
                        self.restore_item(l);
                    }
                }
                Some("seen") => {
                    let entry = w.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                    self.seen.insert(entry, self.rarity_of(entry));
                }
                _ => {}
            }
        }
    }

    /// Read the character's file, if there is one.
    pub(super) fn load(&mut self) {
        self.load_collection();
        let Some(path) = path(&self.name) else { return };
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        let mut imported = 0;
        for line in text.lines() {
            let mut w = line.split_whitespace();
            let num = |v: Option<&str>| v.and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
            match w.next() {
                Some("meta") => {
                    self.meta.level = (num(w.next()) as u32).max(1);
                    self.meta.xp = num(w.next()) as u32;
                }
                Some("money") => self.money = num(w.next()) as u32,
                Some("souls") => {
                    self.souls = num(w.next()) as u32;
                    crate::spirit::souls::set(self.souls);
                }
                Some("healer") => self.healer = super::spirit::Healer::from_line(&mut w),
                Some("queue") => {
                    self.queue_order =
                        w.next().unwrap_or("").split(',').filter_map(|v| v.parse().ok()).collect();
                }
                Some("owned") => {
                    let chain = num(w.next()) as u32;
                    let ranks = num(w.next()) as u32;
                    if ranks > 0 {
                        self.owned.insert(chain, ranks);
                    }
                }
                Some("talent") => {
                    let id = num(w.next()) as u32;
                    let rank = num(w.next()) as u32;
                    if rank > 0 {
                        self.talents.insert(id, rank);
                    }
                }
                Some("done") => {
                    let ids = w.next().unwrap_or("").split(',').filter_map(|v| v.parse().ok());
                    self.set_quests_done(ids);
                }
                Some("wear") => self.restore_worn(num(w.next())),
                Some("bag") => {
                    let (entry, count) = (num(w.next()) as u32, num(w.next()) as u32);
                    if count > 0 && self.data.items.contains_key(&entry) {
                        self.saved_bag.push((entry, count));
                    }
                }
                // A character file from before the collection was shared: its items join it.
                Some("item") => {
                    let mut l = item_line(w, true);
                    if !self.data.items.contains_key(&l.entry) {
                        continue;
                    }
                    l.id = self.next_owned;
                    let (id, worn) = (l.id, l.worn);
                    self.restore_item(l);
                    if worn {
                        self.restore_worn(id);
                    }
                    imported += 1;
                }
                Some("seen") => {
                    let entry = num(w.next()) as u32;
                    self.seen.insert(entry, self.rarity_of(entry));
                }
                _ => {}
            }
        }
        // A login out on a run starts with the healer's charges, as a run walked into does.
        self.rerolls_left = self.rerolls_a_run();
        self.banishes_left = self.healer.banishes;
        eprintln!(
            "survivor: loaded {} (meta level {}, {} items, {} seen)",
            path.display(),
            self.meta.level,
            self.collection.len(),
            self.seen.len()
        );
        if imported > 0 {
            eprintln!("survivor: {imported} items moved into the shared collection");
            self.save();
        }
    }
}
