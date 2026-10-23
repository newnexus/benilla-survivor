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
    /// vmangos's spawn guid, which names its waypoint path.
    pub(crate) guid: u32,
    pub(crate) entry: u32,
    pub(crate) pos: [f32; 3],
    pub(crate) facing: f32,
    pub(crate) respawn_s: u32,
    /// How far from home it wanders while idle, yards (0 stands still).
    pub(crate) wander: f32,
    /// It walks its waypoint path in [`WorldData::mob_paths`].
    pub(crate) walks_path: bool,
}

/// A chest standing in the world (`chests.tsv`).
#[derive(Clone, Debug)]
pub(crate) struct ChestSpawn {
    pub(crate) entry: u32,
    pub(crate) pos: [f32; 3],
    pub(crate) facing: f32,
    pub(crate) rotation: [f32; 4],
}

/// A chest's look and name.
#[derive(Clone, Debug)]
pub(crate) struct ChestTemplate {
    pub(crate) entry: u32,
    pub(crate) name: String,
    pub(crate) display: u32,
    pub(crate) size: f32,
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
    /// Its own bonuses, `(ItemModType, value)`: 3 agility, 4 strength, 5 intellect, 6 spirit,
    /// 7 stamina.
    pub(crate) stats: Vec<(u32, i32)>,
    /// Holy, fire, nature, frost, shadow and arcane resistance.
    pub(crate) resistances: [i32; 6],
}

/// One row of a loot table (`loot.tsv`).
#[derive(Clone, Debug)]
pub(crate) struct LootRow {
    pub(crate) item: u32,
    /// Percent; 0 in a group shares what its other rows leave.
    pub(crate) chance: f32,
    /// 0 for a row rolled on its own; rows of one group give one of them.
    pub(crate) group: u32,
    /// The count's range; a negative `min` names reference table `-min`, rolled `max` times.
    pub(crate) min: i32,
    pub(crate) max: u32,
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

/// `FactionTemplate.dbc` 14, "Monster": hostile to every player.
const FACTION_MONSTER: u32 = 14;

/// A survivor's quests want many more kills than 1.12's: this many times, up to what the quest
/// log's 6-bit counter can show. Gather quests want only this many times as many.
const QUEST_KILLS_TIMES: u32 = 4;
const QUEST_GATHER_TIMES: u32 = 2;
const QUEST_KILLS_MAX: u32 = 60;

/// The survivor's own starting spells, in place of 1.12's: `(class, 1.12's spell, ours)` on the
/// same button, and `(class, button, spell)` added. A mage starts with Frostbolt and Fire Blast,
/// not Fireball.
const START_SWAPS: &[(u8, u32, u32)] = &[(8, 133, 116)];
const START_ADDS: &[(u8, u8, u32)] = &[(8, 4, 2136)];

/// A quest objective naming a game object rather than a creature: its entry with this bit, as
/// `SMSG_QUEST_QUERY_RESPONSE` and `SMSG_QUESTUPDATE_ADD_KILL` carry it (`quests.tsv` too).
pub(crate) const OBJECTIVE_GAMEOBJECT: u32 = 0x8000_0000;

/// How many a survivor's quest wants of `target`, where 1.12's wanted `count`.
fn survivor_count(target: u32, count: u32) -> u32 {
    let times = if target & OBJECTIVE_GAMEOBJECT != 0 {
        QUEST_GATHER_TIMES
    } else {
        QUEST_KILLS_TIMES
    };
    (count * times).min(QUEST_KILLS_MAX)
}

/// `UNIT_NPC_FLAG_QUESTGIVER`.
pub(crate) const NPC_QUESTGIVER: u32 = 0x2;

/// A kill quest a town's quest giver offers (`quests.tsv`).
#[derive(Clone, Debug, Default)]
pub(crate) struct Quest {
    pub(crate) id: u32,
    pub(crate) level: u32,
    /// 1.12's level and chain gates, kept from the data; the survivor offers every quest at once.
    #[allow(dead_code)]
    pub(crate) min_level: u32,
    /// The quest that must be turned in first, 0 for none.
    #[allow(dead_code)]
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
    /// The items to choose one of on handing it in.
    pub(crate) rewards: Vec<u32>,
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

impl WorldData {
    /// The rank chains, from the spells themselves: the world data's `spell_chain` misses most
    /// of the warrior's and the rogue's. A class's trainer spells and starting spells that share
    /// a name are one chain, ranked by their "Rank N"; a starting rank no trainer teaches joins
    /// the class's list (at level 1) so its next rank is known as its upgrade.
    fn survivor_starts(&mut self) {
        for &(class, from, to) in START_SWAPS {
            for a in self.start_actions.iter_mut() {
                if a.0 == class && a.2 == from && a.3 == 0 {
                    a.2 = to;
                }
            }
            // The 1.12 starting rank no trainer taught still sells, from level 1.
            if !self.class_spells.iter().any(|c| c.class == class && c.spell == from) {
                self.class_spells.push(ClassSpell {
                    class,
                    spell: from,
                    first: from,
                    rank: 1,
                    req_level: 1,
                });
            }
        }
        for &(class, button, spell) in START_ADDS {
            self.start_actions.push((class, button, spell, 0));
        }
        // A spell started with is had from level 1.
        let starts: Vec<(u8, u32)> = START_SWAPS
            .iter()
            .map(|&(c, _, s)| (c, s))
            .chain(START_ADDS.iter().map(|&(c, _, s)| (c, s)))
            .collect();
        for c in &mut self.class_spells {
            if starts.contains(&(c.class, c.spell)) {
                c.req_level = 1;
            }
        }
    }

    fn link_rank_chains(&mut self) {
        let rank_of = |s: &SpellInfo| -> u32 {
            s.rank_text
                .trim()
                .strip_prefix("Rank ")
                .and_then(|n| n.trim().parse().ok())
                .unwrap_or(1)
        };
        let classes: std::collections::BTreeSet<u8> = self.class_spells.iter().map(|c| c.class).collect();
        for class in classes {
            let known: std::collections::HashSet<u32> = self
                .class_spells
                .iter()
                .filter(|c| c.class == class)
                .map(|c| c.spell)
                .collect();
            let starts: Vec<u32> = self
                .start_actions
                .iter()
                .filter(|a| a.0 == class && a.3 == 0 && !known.contains(&a.2))
                .map(|a| a.2)
                .filter(|s| self.spells.contains_key(s))
                .collect();
            for s in starts {
                self.class_spells.push(ClassSpell {
                    class,
                    spell: s,
                    first: s,
                    rank: 1,
                    req_level: 1,
                });
            }
            let mut by_name: HashMap<String, Vec<(u32, u32)>> = HashMap::new();
            for c in self.class_spells.iter().filter(|c| c.class == class) {
                if let Some(info) = self.spells.get(&c.spell) {
                    by_name.entry(info.name.clone()).or_default().push((rank_of(info), c.spell));
                }
            }
            for mut chain in by_name.into_values() {
                chain.sort_unstable();
                let first = chain[0].1;
                for (k, &(_, spell)) in chain.iter().enumerate() {
                    for c in self.class_spells.iter_mut().filter(|c| c.class == class && c.spell == spell) {
                        c.first = first;
                        c.rank = k as u32 + 1;
                    }
                }
            }
        }
    }
}

/// A talent of a class (`talents.tsv`): its tree (0-2), tier and column, its ranks' spells, and
/// the talent (and 0-based rank) it needs first.
#[derive(Clone, Debug)]
pub(crate) struct TalentRow {
    pub(crate) class: u8,
    pub(crate) id: u32,
    pub(crate) tab: u32,
    pub(crate) row: u32,
    pub(crate) ranks: Vec<u32>,
    pub(crate) prereq: u32,
    pub(crate) prereq_rank: u32,
}

#[derive(Default)]
pub(crate) struct WorldData {
    pub(crate) creatures: HashMap<u32, CreatureTemplate>,
    pub(crate) spawns: Vec<Spawn>,
    /// The safe towns' people.
    pub(crate) npcs: Vec<(Spawn, Beat)>,
    /// Patrol paths by id, their points in order.
    pub(crate) patrols: HashMap<u32, Vec<PatrolPoint>>,
    /// The waypoint paths creatures walk while idle, by spawn guid.
    pub(crate) mob_paths: HashMap<u32, Vec<PatrolPoint>>,
    pub(crate) chests: Vec<ChestSpawn>,
    /// The objects the gather quests want (`gathers.tsv`), their templates in `chest_templates`.
    pub(crate) gathers: Vec<ChestSpawn>,
    pub(crate) chest_templates: HashMap<u32, ChestTemplate>,
    /// Chest gear (`gear.tsv`): `(item, level to wear it)`, the templates in `items`.
    pub(crate) gear: Vec<(u32, u32)>,
    pub(crate) quests: Vec<Quest>,
    pub(crate) items: HashMap<u32, ItemTemplate>,
    /// `(class, item, amount)`.
    pub(crate) start_items: Vec<(u8, u32, u32)>,
    /// The innkeepers' wares (`shop.tsv`): item and the camp level it sells from.
    pub(crate) shop: Vec<(u32, u32)>,
    /// Loot tables by `(reference, id)`: a creature's by its entry, a reference table by its id.
    pub(crate) loot: HashMap<(bool, u32), Vec<LootRow>>,
    pub(crate) spells: HashMap<u32, SpellInfo>,
    pub(crate) class_spells: Vec<ClassSpell>,
    pub(crate) talents: Vec<TalentRow>,
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

/// A quest giver's spoken line for `quest` (`voice/<id>-accept.mp3`, or `-complete` when
/// handing in), from the exported tables' folder, if it has one.
pub fn quest_voice(quest: u32, complete: bool) -> Option<Vec<u8>> {
    let kind = if complete { "complete" } else { "accept" };
    std::fs::read(data_dir()?.join("voice").join(format!("{quest}-{kind}.mp3"))).ok()
}

/// How long `quest`'s spoken line lasts, seconds (`voice/lengths.tsv`).
pub fn quest_voice_seconds(quest: u32, complete: bool) -> Option<f32> {
    let kind = if complete { "complete" } else { "accept" };
    let name = format!("{quest}-{kind}");
    let text = std::fs::read_to_string(data_dir()?.join("voice").join("lengths.tsv")).ok()?;
    text.lines().find_map(|l| {
        let (n, s) = l.split_once('\t')?;
        (n == name).then(|| s.trim().parse().ok()).flatten()
    })
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
                guid: num(r.first()),
                entry,
                pos: [num(r.get(2)), num(r.get(3)), num(r.get(4))],
                facing: num(r.get(5)),
                respawn_s: num(r.get(6)),
                wander: num(r.get(7)),
                walks_path: num::<u32>(r.get(8)) == 2,
            });
        }
        for r in rows(&dir, "npcs.tsv") {
            let entry = num(r.get(1));
            if !d.creatures.contains_key(&entry) {
                continue;
            }
            d.npcs.push((
                Spawn {
                    guid: num(r.first()),
                    entry,
                    pos: [num(r.get(2)), num(r.get(3)), num(r.get(4))],
                    facing: num(r.get(5)),
                    respawn_s: 0,
                    wander: 0.0,
                    walks_path: false,
                },
                Beat {
                    patrol: num(r.get(6)),
                    side: num(r.get(7)),
                },
            ));
        }
        for r in rows(&dir, "chests.tsv") {
            let entry = num(r.get(1));
            d.chest_templates.entry(entry).or_insert_with(|| ChestTemplate {
                entry,
                name: r.get(2).cloned().unwrap_or_default(),
                display: num(r.get(3)),
                size: num(r.get(4)),
            });
            d.chests.push(ChestSpawn {
                entry,
                pos: [num(r.get(5)), num(r.get(6)), num(r.get(7))],
                facing: num(r.get(8)),
                rotation: [num(r.get(9)), num(r.get(10)), num(r.get(11)), num(r.get(12))],
            });
        }
        // The collection chest, the server's own object, in the battered chest's look.
        d.chest_templates.insert(
            crate::game::ARMORY_ENTRY,
            ChestTemplate {
                entry: crate::game::ARMORY_ENTRY,
                name: "Collection Chest".to_owned(),
                display: 259,
                size: 1.5,
            },
        );
        for r in rows(&dir, "gear.tsv") {
            d.gear.push((num(r.first()), num(r.get(1))));
        }
        for r in rows(&dir, "mob_paths.tsv") {
            d.mob_paths.entry(num(r.first())).or_default().push(PatrolPoint {
                pos: [num(r.get(2)), num(r.get(3)), num(r.get(4))],
                wait_ms: num(r.get(5)),
            });
        }
        for r in rows(&dir, "patrols.tsv") {
            d.patrols.entry(num(r.first())).or_default().push(PatrolPoint {
                pos: [num(r.get(2)), num(r.get(3)), num(r.get(4))],
                wait_ms: num(r.get(5)),
            });
        }
        for r in rows(&dir, "quests.tsv") {
            let text = |i: usize| r.get(i).cloned().unwrap_or_default();
            // The objective sentence names 1.12's counts: it names the survivor's instead.
            let mut objectives = text(10);
            for k in 0..4 {
                let (target, count): (u32, u32) = pair(r.get(13 + k));
                if count > 0 {
                    let more = survivor_count(target, count);
                    objectives = objectives.replace(&format!(" {count} "), &format!(" {more} "));
                }
            }
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
                objectives,
                offer_text: text(11),
                end_text: text(12),
                kills: std::array::from_fn(|k| {
                    let (entry, count): (u32, u32) = pair(r.get(13 + k));
                    (entry, survivor_count(entry, count))
                }),
                objective_texts: std::array::from_fn(|k| text(17 + k)),
                rewards: r
                    .get(21)
                    .map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect())
                    .unwrap_or_default(),
            });
        }
        // The profession trainers' chains (`crate::events`), each step after the one before.
        for (e, ev) in crate::events::EVENTS.iter().enumerate() {
            for (k, s) in ev.steps.iter().enumerate() {
                let id = crate::events::quest_id(e, k + 1);
                d.quests.push(Quest {
                    id,
                    level: 3,
                    min_level: 1,
                    prev: if k == 0 { 0 } else { id - 1 },
                    giver: ev.trainer,
                    ender: ev.trainer,
                    xp: s.xp,
                    money: s.money,
                    title: s.title.to_owned(),
                    details: s.details.to_owned(),
                    objectives: s.objectives.to_owned(),
                    offer_text: s.done.to_owned(),
                    end_text: String::new(),
                    kills: [s.kill, (0, 0), (0, 0), (0, 0)],
                    objective_texts: [s.objective.to_owned(), String::new(), String::new(), String::new()],
                    rewards: Vec::new(),
                });
            }
        }
        // Only the objects a quest wants.
        for r in rows(&dir, "gathers.tsv") {
            let entry: u32 = num(r.get(1));
            let wanted = d
                .quests
                .iter()
                .any(|q| q.kills.iter().any(|&(e, _)| e == entry | OBJECTIVE_GAMEOBJECT));
            if !wanted {
                continue;
            }
            d.chest_templates.entry(entry).or_insert_with(|| ChestTemplate {
                entry,
                name: r.get(2).cloned().unwrap_or_default(),
                display: num(r.get(3)),
                size: num(r.get(4)),
            });
            d.gathers.push(ChestSpawn {
                entry,
                pos: [num(r.get(5)), num(r.get(6)), num(r.get(7))],
                facing: num(r.get(8)),
                rotation: [num(r.get(9)), num(r.get(10)), num(r.get(11)), num(r.get(12))],
            });
        }
        for r in rows(&dir, "items.tsv") {
            let mut i = ItemTemplate {
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
                stats: r
                    .get(22)
                    .map(|v| v.split(','))
                    .into_iter()
                    .flatten()
                    .filter_map(|p| {
                        let (k, v) = p.split_once(':')?;
                        Some((k.trim().parse().ok()?, v.trim().parse().ok()?))
                    })
                    .collect(),
                resistances: {
                    let mut res = [0; 6];
                    let mut it = r.get(23).map(|v| v.split(',')).into_iter().flatten();
                    for x in &mut res {
                        *x = it.next().and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                    }
                    res
                },
            };
            // An item keeps as many of its own 1.12 bonuses as its quality allows, the biggest:
            // none for common, one uncommon, two rare, three epic and up.
            i.stats.sort_by_key(|&(_, v)| std::cmp::Reverse(v));
            i.stats.truncate([0, 0, 1, 2, 3, 3, 3][i.quality.min(6) as usize]);
            d.items.insert(i.entry, i);
        }
        for r in rows(&dir, "loot.tsv") {
            let owner = r.first().map_or("", String::as_str);
            let Some(id) = owner.get(1..).and_then(|v| v.parse().ok()) else { continue };
            d.loot.entry((owner.starts_with('r'), id)).or_default().push(LootRow {
                item: num(r.get(1)),
                chance: num(r.get(2)),
                group: num(r.get(3)),
                min: num(r.get(4)),
                max: num(r.get(5)),
            });
        }
        for r in rows(&dir, "shop.tsv") {
            d.shop.push((num(r.first()), num(r.get(1))));
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
        for r in rows(&dir, "talents.tsv") {
            d.talents.push(TalentRow {
                class: num(r.first()),
                id: num(r.get(1)),
                tab: num(r.get(2)),
                row: num(r.get(3)),
                ranks: r
                    .get(5)
                    .map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect())
                    .unwrap_or_default(),
                prereq: num(r.get(6)),
                prereq_rank: num(r.get(7)),
            });
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
        d.survivor_starts();
        d.link_rank_chains();
        // A town's people offer only what the server answers: the quest givers keep their mark
        // (`UNIT_NPC_FLAG_QUESTGIVER`), vendors and trainers stay plain townsfolk for now.
        let talks: std::collections::HashSet<u32> =
            d.quests.iter().flat_map(|q| [q.giver, q.ender]).collect();
        // The town's people, and the profession trainers held out in the zone, stay friendly.
        let people: std::collections::HashSet<u32> = d
            .npcs
            .iter()
            .map(|n| n.0.entry)
            .chain(crate::events::trainers())
            .collect();
        let bosses: std::collections::HashSet<u32> =
            crate::waves::BOSSES.iter().map(|b| b.0).collect();
        for t in d.creatures.values_mut() {
            t.npc_flags = if talks.contains(&t.entry) { NPC_QUESTGIVER } else { 0 };
            // Every foe attacks on sight: a neutral one takes the hostile "Monster" faction, so
            // the client names it in red too.
            if !people.contains(&t.entry) && !t.hostile {
                t.hostile = true;
                t.faction = FACTION_MONSTER;
            }
            // A boss shows the elite frame.
            if bosses.contains(&t.entry) {
                t.rank = t.rank.max(1);
            }
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
                guid: 0,
                entry: 1,
                pos: [0.0; 3],
                facing: 0.0,
                respawn_s: 300,
                wander: 0.0,
                walks_path: false,
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
