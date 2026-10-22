//! What a character keeps between sessions: the meta level, the purse, the quests handed in,
//! the collection (each item's rarity, stats, upgrades, favourite and whether it is worn) and
//! the items only seen. One plain text file per character name in `$SURVIVOR_SAVE_DIR` (the
//! launchers point it into the client's own state folder); no folder, no saving.
//!
//! ```text
//! meta 3 450
//! money 1234
//! done 364,380
//! item 1 2652 2 1 1 1 4:0.168,1:0.06
//! seen 2653 3
//! ```
//! An item line is `id entry rarity upgrades favourite worn stats`.

use std::fmt::Write as _;
use std::path::PathBuf;

use super::gear::{GearItem, GearStat, OwnedItem};
use super::Game;

fn path(name: &str) -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("SURVIVOR_SAVE_DIR")?);
    let safe: String = name.chars().filter(char::is_ascii_alphanumeric).collect();
    (!safe.is_empty()).then(|| dir.join(format!("{safe}.txt")))
}

impl Game {
    /// Write the character's file; a failure is said and play goes on.
    pub(super) fn save(&self) {
        let Some(path) = path(&self.name) else { return };
        let mut s = String::new();
        let _ = writeln!(s, "meta {} {}", self.meta.level, self.meta.xp);
        let _ = writeln!(s, "money {}", self.banked_money());
        let _ = writeln!(s, "souls {}", self.souls);
        let _ = writeln!(s, "healer {}", self.healer.to_line());
        let done: Vec<String> = self.quests_done().iter().map(u32::to_string).collect();
        let _ = writeln!(s, "done {}", done.join(","));
        for o in &self.collection {
            let worn = self.loadout.values().any(|&id| id == o.id);
            let stats: Vec<String> = o
                .item
                .stats()
                .iter()
                .map(|&(st, x)| format!("{}:{x:.4}", st.index()))
                .collect();
            let _ = writeln!(
                s,
                "item {} {} {} {} {} {} {}",
                o.id,
                o.item.entry,
                o.item.rarity,
                o.item.upgrades,
                u8::from(o.favourite),
                u8::from(worn),
                stats.join(",")
            );
        }
        for (&entry, &rarity) in &self.seen {
            let _ = writeln!(s, "seen {entry} {rarity}");
        }
        let write = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, s));
        if let Err(e) = write {
            eprintln!("survivor: cannot save {}: {e}", path.display());
        }
    }

    /// Read the character's file, if there is one.
    pub(super) fn load(&mut self) {
        let Some(path) = path(&self.name) else { return };
        let Ok(text) = std::fs::read_to_string(&path) else { return };
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
                Some("done") => {
                    let ids = w.next().unwrap_or("").split(',').filter_map(|v| v.parse().ok());
                    self.set_quests_done(ids);
                }
                Some("item") => {
                    let id = num(w.next());
                    let entry = num(w.next()) as u32;
                    let rarity = num(w.next()) as u8;
                    let upgrades = num(w.next()) as u32;
                    let favourite = num(w.next()) != 0;
                    let worn = num(w.next()) != 0;
                    let stats = w
                        .next()
                        .unwrap_or("")
                        .split(',')
                        .filter_map(|p| {
                            let (s, x) = p.split_once(':')?;
                            Some((GearStat::from_index(s.parse().ok()?)?, x.parse().ok()?))
                        })
                        .collect();
                    if !self.data.items.contains_key(&entry) {
                        continue;
                    }
                    let item = GearItem::restored(entry, rarity, stats, upgrades, id);
                    if worn {
                        if let Some(slot) = self.slot_of_item(entry) {
                            self.loadout.insert(slot, id);
                        }
                    }
                    self.next_owned = self.next_owned.max(id + 1);
                    self.collection.push(OwnedItem {
                        id,
                        item,
                        favourite,
                    });
                }
                Some("seen") => {
                    let entry = num(w.next()) as u32;
                    let rarity = num(w.next()) as u8;
                    self.seen.insert(entry, rarity);
                }
                _ => {}
            }
        }
        // A login out on a run starts with the healer's charges, as a run walked into does.
        self.rerolls_left = self.healer.rerolls;
        self.banishes_left = self.healer.banishes;
        eprintln!(
            "survivor: loaded {} (meta level {}, {} items, {} seen)",
            path.display(),
            self.meta.level,
            self.collection.len(),
            self.seen.len()
        );
    }
}
