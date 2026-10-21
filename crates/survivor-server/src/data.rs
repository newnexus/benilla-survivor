//! The world rows `survivor-data` exports from vmangos (`spawns.tsv`, `creatures.tsv`,
//! `start_items.tsv`, `items.tsv`), and the XP curve derived from them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One creature kind, at its 1.12 template row.
#[derive(Clone, Debug)]
pub(crate) struct CreatureTemplate {
    pub(crate) entry: u32,
    pub(crate) name: String,
    pub(crate) level: (u32, u32),
    pub(crate) faction: u32,
    pub(crate) display: u32,
    pub(crate) scale: f32,
    /// Walk and run speed multipliers over the stock 2.5 and 7.0 yd/s.
    pub(crate) speed: (f32, f32),
    pub(crate) rank: u32,
    pub(crate) kind: u32,
    pub(crate) attack_ms: u32,
    /// Health at the lowest and highest level, before the survivor scaling.
    pub(crate) health: (u32, u32),
    pub(crate) damage: (f32, f32),
    /// How near the player must come for the creature to attack, in yards.
    pub(crate) detection: f32,
    /// Attacks on sight; a neutral creature only fights back.
    pub(crate) hostile: bool,
    /// `UNIT_NPC_FLAGS` of a town's people (gossip, quests, vendor...), 0 for a foe.
    pub(crate) npc_flags: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct Spawn {
    pub(crate) entry: u32,
    pub(crate) pos: [f32; 3],
    pub(crate) facing: f32,
    pub(crate) respawn_s: u32,
}

/// A town person's beat (`npcs.tsv`'s patrol and side): the path in [`WorldData::patrols`] they
/// walk, and how far to its right.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Beat {
    pub(crate) patrol: u32,
    pub(crate) side: f32,
}

/// A patrol path's point and how long to stand there, ms.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PatrolPoint {
    pub(crate) pos: [f32; 3],
    pub(crate) wait_ms: u32,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ItemTemplate {
    pub(crate) entry: u32,
    pub(crate) class: u32,
    pub(crate) subclass: u32,
    pub(crate) name: String,
    pub(crate) display: u32,
    pub(crate) quality: u32,
    pub(crate) inventory_type: u32,
    pub(crate) delay: u32,
    pub(crate) damage: (f32, f32),
    pub(crate) damage_type: u32,
    pub(crate) armor: u32,
    pub(crate) stackable: u32,
    pub(crate) sheath: u32,
    pub(crate) durability: u32,
    pub(crate) material: i32,
    pub(crate) item_level: u32,
    pub(crate) flags: u32,
    pub(crate) buy: u32,
    pub(crate) sell: u32,
    pub(crate) max_count: u32,
    /// The spell using the item casts (food, drink), 0 for none.
    pub(crate) spell: u32,
}

/// What a spell is for, in the level-up picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum SpellKind {
    Damage,
    Defensive,
    #[default]
    Other,
}

/// One spell's cast data, at its 1.12 row.
#[derive(Clone, Debug, Default)]
pub(crate) struct SpellInfo {
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) rank_text: String,
    pub(crate) kind: SpellKind,
    pub(crate) school: u8,
    pub(crate) cast_ms: u32,
    pub(crate) range: f32,
    pub(crate) power_type: u8,
    pub(crate) cost: u32,
    pub(crate) recovery_ms: u32,
    pub(crate) duration_ms: u32,
    /// Missile speed in yards per second, 0 for an instant hit.
    pub(crate) speed: f32,
    /// `dmgClass`: 2 is a melee ability, swung with the weapon.
    pub(crate) dmg_class: u32,
    pub(crate) effects: [u32; 3],
    pub(crate) base_points: [i32; 3],
    pub(crate) die_sides: [i32; 3],
    pub(crate) auras: [u32; 3],
    pub(crate) amplitudes: [u32; 3],
    pub(crate) targets: [u32; 3],
}

/// `UNIT_NPC_FLAG_QUESTGIVER`.
pub(crate) const NPC_QUESTGIVER: u32 = 0x2;

/// A kill quest a town's quest giver offers (`quests.tsv`).
#[derive(Clone, Debug, Default)]
pub(crate) struct Quest {
    pub(crate) id: u32,
    pub(crate) level: u32,
    pub(crate) min_level: u32,
    /// The quest that must be turned in first, 0 for none.
    pub(crate) prev: u32,
    pub(crate) giver: u32,
    pub(crate) ender: u32,
    pub(crate) xp: u32,
    pub(crate) money: u32,
    pub(crate) title: String,
    pub(crate) details: String,
    pub(crate) objectives: String,
    pub(crate) offer_text: String,
    pub(crate) end_text: String,
    /// Up to four `(creature entry, count)` kills, `(0, 0)` where unused.
    pub(crate) kills: [(u32, u32); 4],
    pub(crate) objective_texts: [String; 4],
}

/// A class's trainer spell with its rank chain.
#[derive(Clone, Debug)]
pub(crate) struct ClassSpell {
    pub(crate) class: u8,
    pub(crate) spell: u32,
    /// Rank 1 of the chain (the spell itself when it has no ranks).
    pub(crate) first: u32,
    pub(crate) rank: u32,
    pub(crate) req_level: u32,
}

#[derive(Default)]
pub(crate) struct WorldData {
    pub(crate) creatures: HashMap<u32, CreatureTemplate>,
    pub(crate) spawns: Vec<Spawn>,
    /// The safe towns' people.
    pub(crate) npcs: Vec<(Spawn, Beat)>,
    /// Patrol paths by id, their points in order.
    pub(crate) patrols: HashMap<u32, Vec<PatrolPoint>>,
    pub(crate) quests: Vec<Quest>,
    pub(crate) items: HashMap<u32, ItemTemplate>,
    /// `(class, item, amount)`.
    pub(crate) start_items: Vec<(u8, u32, u32)>,
    pub(crate) spells: HashMap<u32, SpellInfo>,
    pub(crate) class_spells: Vec<ClassSpell>,
    /// `(class, button, action, type)`: the starting action bar.
    pub(crate) start_actions: Vec<(u8, u8, u32, u8)>,
}

/// Three comma-separated values.
fn three<T: std::str::FromStr + Default + Copy>(s: Option<&String>) -> [T; 3] {
    let mut out = [T::default(); 3];
    for (o, v) in out
        .iter_mut()
        .zip(s.map(|v| v.split(',')).into_iter().flatten())
    {
        *o = v.trim().parse().unwrap_or_default();
    }
    out
}

/// Where the exported tables live: `$SURVIVOR_DATA`, else `Data\survivor` beside the binary (a
/// packaged build), else this crate's own `data` folder (a dev build).
fn data_dir() -> Option<PathBuf> {
    let candidates = [
        std::env::var_os("SURVIVOR_DATA").map(PathBuf::from),
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| d.join("Data").join("survivor"))),
        Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("data")),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|d| d.join("spawns.tsv").is_file())
}

/// The rows of a TSV file below its header, split on tabs.
fn rows(dir: &Path, file: &str) -> Vec<Vec<String>> {
    let Ok(text) = std::fs::read_to_string(dir.join(file)) else {
        return Vec::new();
    };
    text.lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split('\t').map(str::to_owned).collect())
        .collect()
}

fn num<T: std::str::FromStr + Default>(s: Option<&String>) -> T {
    s.and_then(|v| v.trim().parse().ok()).unwrap_or_default()
}

/// A `min,max` pair.
fn pair<T: std::str::FromStr + Default + Copy>(s: Option<&String>) -> (T, T) {
    let mut it = s.map(|v| v.split(',')).into_iter().flatten();
    let a = it.next().and_then(|v| v.trim().parse().ok()).unwrap_or_default();
    let b = it.next().and_then(|v| v.trim().parse().ok()).unwrap_or(a);
    (a, b)
}

impl WorldData {
    pub(crate) fn load() -> Self {
        let Some(dir) = data_dir() else {
            eprintln!("survivor-server: no world data found (run survivor-data); the world is empty");
            return Self::default();
        };
        let mut d = Self::default();
        for r in rows(&dir, "creatures.tsv") {
            let t = CreatureTemplate {
                entry: num(r.first()),
                name: r.get(1).cloned().unwrap_or_default(),
                level: (num(r.get(2)), num(r.get(3))),
                faction: num(r.get(4)),
                display: num(r.get(5)),
                scale: num(r.get(6)),
                speed: (num(r.get(7)), num(r.get(8))),
                rank: num(r.get(9)),
                kind: num(r.get(10)),
                attack_ms: num(r.get(11)),
                health: pair(r.get(12)),
                damage: pair(r.get(13)),
                detection: num(r.get(14)),
                hostile: num::<u8>(r.get(15)) != 0,
                npc_flags: num(r.get(16)),
            };
            d.creatures.insert(t.entry, t);
        }
        for r in rows(&dir, "spawns.tsv") {
            let entry = num(r.get(1));
            if !d.creatures.contains_key(&entry) {
                continue;
            }
            d.spawns.push(Spawn {
                entry,
                pos: [num(r.get(2)), num(r.get(3)), num(r.get(4))],
                facing: num(r.get(5)),
                respawn_s: num(r.get(6)),
            });
        }
        for r in rows(&dir, "npcs.tsv") {
            let entry = num(r.get(1));
            if !d.creatures.contains_key(&entry) {
                continue;
            }
            d.npcs.push((
                Spawn {
                    entry,
                    pos: [num(r.get(2)), num(r.get(3)), num(r.get(4))],
                    facing: num(r.get(5)),
                    respawn_s: 0,
                },
                Beat {
                    patrol: num(r.get(6)),
                    side: num(r.get(7)),
                },
            ));
        }
        for r in rows(&dir, "patrols.tsv") {
            d.patrols.entry(num(r.first())).or_default().push(PatrolPoint {
                pos: [num(r.get(2)), num(r.get(3)), num(r.get(4))],
                wait_ms: num(r.get(5)),
            });
        }
        for r in rows(&dir, "quests.tsv") {
            let text = |i: usize| r.get(i).cloned().unwrap_or_default();
            d.quests.push(Quest {
                id: num(r.first()),
                level: num(r.get(1)),
                min_level: num(r.get(2)),
                prev: num(r.get(3)),
                giver: num(r.get(4)),
                ender: num(r.get(5)),
                xp: num(r.get(6)),
                money: num(r.get(7)),
                title: text(8),
                details: text(9),
                objectives: text(10),
                offer_text: text(11),
                end_text: text(12),
                kills: std::array::from_fn(|k| pair(r.get(13 + k))),
                objective_texts: std::array::from_fn(|k| text(17 + k)),
            });
        }
        for r in rows(&dir, "items.tsv") {
            let i = ItemTemplate {
                entry: num(r.first()),
                class: num(r.get(1)),
                subclass: num(r.get(2)),
                name: r.get(3).cloned().unwrap_or_default(),
                display: num(r.get(4)),
                quality: num(r.get(5)),
                inventory_type: num(r.get(6)),
                delay: num(r.get(7)),
                damage: (num(r.get(8)), num(r.get(9))),
                damage_type: num(r.get(10)),
                armor: num(r.get(11)),
                stackable: num(r.get(12)),
                sheath: num(r.get(13)),
                durability: num(r.get(14)),
                material: num(r.get(15)),
                item_level: num(r.get(16)),
                flags: num(r.get(17)),
                buy: num(r.get(18)),
                sell: num(r.get(19)),
                max_count: num(r.get(20)),
                spell: num(r.get(21)),
            };
            d.items.insert(i.entry, i);
        }
        for r in rows(&dir, "start_items.tsv") {
            d.start_items
                .push((num(r.first()), num(r.get(1)), num(r.get(2))));
        }
        for r in rows(&dir, "spells.tsv") {
            let s = SpellInfo {
                id: num(r.first()),
                name: r.get(1).cloned().unwrap_or_default(),
                rank_text: r.get(2).cloned().unwrap_or_default(),
                kind: match r.get(3).map(String::as_str) {
                    Some("damage") => SpellKind::Damage,
                    Some("defensive") => SpellKind::Defensive,
                    _ => SpellKind::Other,
                },
                school: num(r.get(4)),
                cast_ms: num::<i32>(r.get(5)).max(0) as u32,
                range: num(r.get(6)),
                power_type: num(r.get(7)),
                cost: num(r.get(8)),
                recovery_ms: num::<u32>(r.get(10)).max(num(r.get(11))),
                duration_ms: num::<i32>(r.get(13)).max(0) as u32,
                speed: num(r.get(14)),
                dmg_class: num(r.get(15)),
                effects: three(r.get(18)),
                base_points: three(r.get(19)),
                die_sides: three(r.get(20)),
                auras: three(r.get(21)),
                amplitudes: three(r.get(22)),
                targets: three(r.get(23)),
            };
            d.spells.insert(s.id, s);
        }
        for r in rows(&dir, "class_spells.tsv") {
            d.class_spells.push(ClassSpell {
                class: num(r.first()),
                spell: num(r.get(1)),
                first: num(r.get(2)),
                rank: num(r.get(4)),
                req_level: num(r.get(5)),
            });
        }
        for r in rows(&dir, "start_actions.tsv") {
            d.start_actions.push((
                num(r.first()),
                num(r.get(1)),
                num(r.get(2)),
                num(r.get(3)),
            ));
        }
        // A town's people offer only what the server answers: the quest givers keep their mark
        // (`UNIT_NPC_FLAG_QUESTGIVER`), vendors and trainers stay plain townsfolk for now.
        let talks: std::collections::HashSet<u32> =
            d.quests.iter().flat_map(|q| [q.giver, q.ender]).collect();
        for t in d.creatures.values_mut() {
            t.npc_flags = if talks.contains(&t.entry) { NPC_QUESTGIVER } else { 0 };
        }
        eprintln!(
            "survivor-server: {} spawns of {} creature kinds, {} town people, {} quests, {} items, \
             {} spells, from {}",
            d.spawns.len(),
            d.creatures.len(),
            d.npcs.len(),
            d.quests.len(),
            d.items.len(),
            d.spells.len(),
            dir.display()
        );
        d
    }
}

/// Kill XP for a creature of `level`: 1.12's same-level base (`level * 5 + 45`), whatever the
/// killer's level, so the zone's low creatures keep paying.
pub(crate) fn kill_xp(level: u32) -> u32 {
    level * 5 + 45
}

/// The level every Tirisfall creature killed once should reach.
pub(crate) const ZONE_CLEAR_LEVEL: u32 = 24;

/// Level 2 takes this many level-1 kills.
const KILLS_FOR_LEVEL_2: f64 = 2.5;

/// The survivor XP curve: level 2 after [`KILLS_FOR_LEVEL_2`] easy kills, then each level costs
/// a fixed multiple of the one before, the multiple solved so that killing every spawn in the
/// zone once adds up to exactly [`ZONE_CLEAR_LEVEL`]. Fast at first, then long.
pub(crate) struct XpCurve {
    first: f64,
    ratio: f64,
}

impl XpCurve {
    pub(crate) fn for_world(d: &WorldData) -> Self {
        let total: f64 = d
            .spawns
            .iter()
            .map(|s| {
                let t = &d.creatures[&s.entry];
                f64::from(kill_xp((t.level.0 + t.level.1) / 2))
            })
            .sum();
        let first = KILLS_FOR_LEVEL_2 * f64::from(kill_xp(1));
        let levels = (ZONE_CLEAR_LEVEL - 1) as i32;
        let sum = |r: f64| (0..levels).map(|k| first * r.powi(k)).sum::<f64>();
        // `sum` grows with the ratio: bisect for the one that matches the zone's total.
        let (mut lo, mut hi) = (1.0f64, 10.0f64);
        if sum(lo) >= total {
            return Self { first, ratio: 1.0 };
        }
        for _ in 0..100 {
            let mid = (lo + hi) / 2.0;
            if sum(mid) < total {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Self { first, ratio: lo }
    }

    /// XP from `level` to the next.
    pub(crate) fn next_level(&self, level: u32) -> u32 {
        let k = level.max(1) as i32 - 1;
        ((self.first * self.ratio.powi(k)).floor() as u32).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearing_the_zone_once_reaches_level_twelve() {
        let mut d = WorldData::default();
        d.creatures.insert(
            1,
            CreatureTemplate {
                entry: 1,
                name: "Duskbat".into(),
                level: (5, 5),
                faction: 7,
                display: 1,
                scale: 1.0,
                speed: (1.0, 1.0),
                rank: 0,
                kind: 1,
                attack_ms: 2000,
                health: (100, 100),
                damage: (2.0, 3.0),
                detection: 10.0,
                hostile: true,
                npc_flags: 0,
            },
        );
        for _ in 0..1500 {
            d.spawns.push(Spawn {
                entry: 1,
                pos: [0.0; 3],
                facing: 0.0,
                respawn_s: 300,
            });
        }
        let curve = XpCurve::for_world(&d);
        // Level 2 after two and a half level-1 kills.
        assert_eq!(curve.next_level(1), 125);
        let (mut level, mut xp) = (1, 0u32);
        for _ in &d.spawns {
            xp += kill_xp(5);
            while xp >= curve.next_level(level) {
                xp -= curve.next_level(level);
                level += 1;
            }
        }
        assert_eq!(level, ZONE_CLEAR_LEVEL);
    }
}
