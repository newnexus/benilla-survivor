//! `survivor-data <mangos.sqlite> --schema <table>`: print a table's columns.
//! `survivor-data <mangos.sqlite> <wow Data dir> <out dir>`: write the TSV files survivor-server
//! loads, from vmangos's world database at patch 1.12:
//!
//! - `creatures.tsv`: the hostile and neutral creatures spawned in Tirisfall Glades, by template;
//! - `spawns.tsv`: where each of them stands;
//! - `start_items.tsv` and `items.tsv`: each Forsaken class's starting gear.
//! - `loot.tsv`: what those creatures drop, with the reference tables they name;
//! - `shop.tsv`: the innkeepers' food and drink, by the level each sells from.
//!
//! - `npcs.tsv`: the friendly people of the safe towns (`survivor_server::town`), quest givers
//!   among them; `quests.tsv`: their kill quests, and their gather quests;
//! - `gathers.tsv`: the things those are gathered from (Deathknell's Equipment Boxes).
//!
//! Friendly creatures and anything with a service outside the towns are left out, and so is
//! every foe inside one.

use std::fmt::Write as _;
use std::path::Path;

use anyhow::{bail, Context, Result};
use benilla_formats::Reaction;
use rusqlite::Connection;

/// vmangos's patch index for 1.12.
const PATCH: i64 = 10;
/// `ChrRaces.dbc` id of the Forsaken, and their player faction template (`FactionTemplate` 5).
const UNDEAD: i64 = 5;
const UNDEAD_FACTION_TEMPLATE: u32 = 5;
/// Map 0 (Eastern Kingdoms) and Tirisfall Glades' extent (`WorldMapArea.dbc`).
const MAP: i64 = 0;
const TIRISFALL_X: (f64, f64) = (824.0, 3838.0);
const TIRISFALL_Y: (f64, f64) = (-1486.0, 3034.0);
/// `CREATURE_TYPE_CRITTER`: rats and rabbits, no fight in them.
const CRITTER: i64 = 8;
/// `AreaTable.dbc` id of Tirisfall Glades.
const TIRISFALL_ZONE: u32 = 85;
/// Creatures above this level are left out: the Scarlet Monastery's level-30 gate guards would
/// end a level 1-12 run on sight.
const MAX_LEVEL: i64 = 15;
/// A map tile's edge and a terrain chunk's, in yards.
const TILE: f32 = 533.333_3;
const CHUNK: f32 = TILE / 16.0;

/// The client build vmangos's `build` columns are compared with.
const BUILD: i64 = 5875;
/// The Forsaken classes: warrior, rogue, priest, mage, warlock.
const CLASSES: [i64; 5] = [1, 4, 5, 8, 9];
/// `SPELL_EFFECT_LEARN_SPELL`: a trainer's teaching spell, which names the real one.
const EFFECT_LEARN_SPELL: i64 = 36;

/// A DBC's records as `u32` fields: `id -> fields`.
fn dbc_rows(
    chain: &benilla_formats::Chain,
    name: &str,
) -> Result<std::collections::HashMap<u32, Vec<u32>>> {
    let d = chain.read(name)?;
    let u = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().expect("4"));
    let (n, fields, size) = (u(4) as usize, u(8) as usize, u(12) as usize);
    Ok((0..n)
        .map(|i| {
            let rec: Vec<u32> = (0..fields).map(|f| u(20 + i * size + f * 4)).collect();
            (rec[0], rec)
        })
        .collect())
}

/// What a spell does, for the level-up picks.
fn spell_kind(effects: &[i64; 3], auras: &[i64; 3], targets: &[i64; 3]) -> &'static str {
    // School damage, health leech, weapon damage (four kinds), periodic damage or a periodic
    // trigger (Arcane Missiles) make an attack.
    let damage = effects.iter().zip(auras).any(|(&e, &a)| {
        matches!(e, 2 | 9 | 17 | 31 | 58 | 121) || (e == 6 && matches!(a, 3 | 23 | 89))
    });
    if damage {
        return "damage";
    }
    // A heal, or an aura cast on oneself or a friend: shields, armours, buffs.
    let on_friend = targets.iter().any(|t| matches!(t, 1 | 21 | 25));
    let helpful = effects.iter().any(|&e| matches!(e, 10 | 6 | 35));
    if on_friend && helpful {
        return "defensive";
    }
    "other"
}

/// `spells.tsv`, `class_spells.tsv` and `start_actions.tsv`: every Forsaken class's trainer
/// spells with their whole rank chains, the data a cast needs, and each class's starting bar.
/// `item_spells` are the starting items' use spells (food, drink), exported with the rest.
fn export_spells(
    conn: &Connection,
    chain: &benilla_formats::Chain,
    out: &Path,
    item_spells: &std::collections::BTreeSet<i64>,
) -> Result<usize> {
    let cast_times = dbc_rows(chain, "DBFilesClient\\SpellCastTimes.dbc")?;
    let ranges = dbc_rows(chain, "DBFilesClient\\SpellRange.dbc")?;
    let durations = dbc_rows(chain, "DBFilesClient\\SpellDuration.dbc")?;

    // Rank chains.
    let mut prev = std::collections::HashMap::new();
    let mut first = std::collections::HashMap::new();
    let mut rank = std::collections::HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT spell_id, prev_spell, first_spell, rank FROM spell_chain
         WHERE build_min <= ?1 AND build_max >= ?1",
    )?;
    for r in stmt.query_map([BUILD], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?))
    })? {
        let (s, p, f, k) = r?;
        prev.insert(s, p);
        first.insert(s, f);
        rank.insert(s, k);
    }

    // Race-locked spells (a priest's racials) the Forsaken do not get.
    let mut race_locked = std::collections::HashSet::new();
    let mut stmt = conn.prepare("SELECT spell_id, race_mask FROM skill_line_ability")?;
    for r in stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
        let (s, mask) = r?;
        if mask != 0 && mask & (1 << (UNDEAD - 1)) == 0 {
            race_locked.insert(s);
        }
    }

    // The spell row at 1.12.
    let spell_sql = "SELECT * FROM spell_template t WHERE entry = ?1 AND build =
        (SELECT MAX(build) FROM spell_template u WHERE u.entry = t.entry AND u.build <= ?2)";
    let mut spell_stmt = conn.prepare(spell_sql)?;
    let mut learn_target = |id: i64| -> Result<Option<i64>> {
        let mut rows = spell_stmt.query(rusqlite::params![id, BUILD])?;
        let Some(r) = rows.next()? else { return Ok(None) };
        let effect: i64 = r.get("effect1")?;
        Ok(Some(if effect == EFFECT_LEARN_SPELL {
            r.get("effectTriggerSpell1")?
        } else {
            id
        }))
    };

    // Each class's trainer spells, with the level a trainer first offers them.
    let mut class_spells = String::from("class\tspell\tfirst\tprev\trank\treq_level\n");
    let mut wanted = item_spells.clone();
    let mut stmt = conn.prepare(
        "SELECT spell, reqlevel FROM npc_trainer_template
           WHERE build_min <= ?2 AND build_max >= ?2 AND entry IN
             (SELECT trainer_id FROM creature_template WHERE trainer_type = 0 AND trainer_class = ?1)
         UNION
         SELECT spell, reqlevel FROM npc_trainer
           WHERE build_min <= ?2 AND build_max >= ?2 AND entry IN
             (SELECT entry FROM creature_template WHERE trainer_type = 0 AND trainer_class = ?1)",
    )?;
    for class in CLASSES {
        let rows: Vec<(i64, i64)> = stmt
            .query_map(rusqlite::params![class, BUILD], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        let mut seen = std::collections::BTreeMap::new();
        for (teach, level) in rows {
            let Some(spell) = learn_target(teach)? else { continue };
            if race_locked.contains(&spell) {
                continue;
            }
            let e = seen.entry(spell).or_insert(level);
            *e = (*e).min(level);
        }
        for (spell, level) in seen {
            wanted.insert(spell);
            writeln!(
                class_spells,
                "{class}\t{spell}\t{}\t{}\t{}\t{level}",
                first.get(&spell).copied().unwrap_or(spell),
                prev.get(&spell).copied().unwrap_or(0),
                rank.get(&spell).copied().unwrap_or(1),
            )?;
        }
    }
    std::fs::write(out.join("class_spells.tsv"), class_spells)?;

    // Each class's starting action bar.
    let mut start = String::from("class\tbutton\taction\ttype\n");
    let mut stmt = conn.prepare(
        "SELECT class, button, action, type FROM playercreateinfo_action WHERE race = ?1
         ORDER BY class, button",
    )?;
    for r in stmt.query_map([UNDEAD], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?))
    })? {
        let (class, button, action, kind) = r?;
        if kind == 0 {
            wanted.insert(action);
        }
        writeln!(start, "{class}\t{button}\t{action}\t{kind}")?;
    }
    std::fs::write(out.join("start_actions.tsv"), start)?;

    // The cast data of every spell named.
    let mut spells = String::from(
        "id\tname\trank_text\tkind\tschool\tcast_ms\trange\tpower_type\tcost\tcost_pct\t\
         recovery_ms\tcategory_recovery_ms\tgcd_ms\tduration_ms\tspeed\tdmg_class\tvisual\ticon\t\
         effects\tbase_points\tdie_sides\tauras\tamplitudes\ttargets\tfinisher\tper_combo\n",
    );
    let mut stmt = conn.prepare(spell_sql)?;
    let mut count = 0;
    for &id in &wanted {
        let mut rows = stmt.query(rusqlite::params![id, BUILD])?;
        let Some(r) = rows.next()? else { continue };
        let three = |prefix: &str| -> Result<[i64; 3]> {
            Ok([
                r.get(format!("{prefix}1").as_str())?,
                r.get(format!("{prefix}2").as_str())?,
                r.get(format!("{prefix}3").as_str())?,
            ])
        };
        let effects = three("effect")?;
        let base = three("effectBasePoints")?;
        let dice = three("effectDieSides")?;
        let auras = three("effectApplyAuraName")?;
        let amps = three("effectAmplitude")?;
        let targets = three("effectImplicitTargetA")?;
        let join = |a: [i64; 3]| format!("{},{},{}", a[0], a[1], a[2]);
        // A finishing move (`AttributesEx` REQ_TARGET_COMBO_POINTS or REQ_COMBO_POINTS), and what
        // each effect gains per combo point.
        let finisher = r.get::<_, i64>("attributesEx")? & 0x0050_0000 != 0;
        let per_combo: Vec<String> = (1..=3)
            .map(|k| {
                r.get::<_, f64>(format!("effectPointsPerComboPoint{k}").as_str())
                    .map(|v| format!("{v}"))
            })
            .collect::<std::result::Result<_, _>>()?;
        let idx = |col: &str| -> Result<u32> { Ok(r.get::<_, i64>(col)? as u32) };
        let cast_ms = cast_times
            .get(&idx("castingTimeIndex")?)
            .map_or(0, |row| row[1] as i32);
        let range = ranges
            .get(&idx("rangeIndex")?)
            .map_or(0.0, |row| f32::from_bits(row[2]));
        let duration = durations
            .get(&idx("durationIndex")?)
            .map_or(0, |row| row[1] as i32);
        let text = |col: &str| -> Result<String> {
            Ok(r.get::<_, Option<String>>(col)?
                .unwrap_or_default()
                .replace(['\t', '\n', '\r'], " "))
        };
        writeln!(
            spells,
            "{id}\t{}\t{}\t{}\t{}\t{cast_ms}\t{range:.1}\t{}\t{}\t{}\t{}\t{}\t{}\t{duration}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            text("name")?,
            text("nameSubtext")?,
            spell_kind(&effects, &auras, &targets),
            r.get::<_, i64>("school")?,
            r.get::<_, i64>("powerType")?,
            r.get::<_, i64>("manaCost")?,
            r.get::<_, i64>("manaCostPercentage")?,
            r.get::<_, i64>("recoveryTime")?,
            r.get::<_, i64>("categoryRecoveryTime")?,
            r.get::<_, i64>("startRecoveryTime")?,
            r.get::<_, f64>("speed")?,
            r.get::<_, i64>("dmgClass")?,
            r.get::<_, i64>("spellVisual1")?,
            r.get::<_, i64>("spellIconId")?,
            join(effects),
            join(base),
            join(dice),
            join(auras),
            join(amps),
            join(targets),
            u8::from(finisher),
            per_combo.join(","),
        )?;
        count += 1;
    }
    std::fs::write(out.join("spells.tsv"), spells)?;
    Ok(count)
}

/// Chest gear goes up to this wearing level: Tirisfall's own.
const GEAR_MAX_LEVEL: i64 = 14;

/// `loot.tsv`: the loot tables of `creatures` (by creature entry, owner `c<entry>`) and every
/// reference table they reach (owner `r<id>`), at patch 1.12. A row is `owner item chance group
/// min max`; a reference row has `item` the table and a negative `min`, `max` its times. Quest
/// items (a negative chance) are left out: the survivor's quests are its own. Returns the items.
fn export_loot(
    conn: &Connection,
    creatures: &std::collections::BTreeSet<i64>,
    out: &Path,
) -> Result<std::collections::BTreeSet<i64>> {
    let rows = |table: &str, entry: i64| -> Result<Vec<(i64, f64, i64, i64, i64)>> {
        let mut stmt = conn.prepare(&format!(
            "SELECT item, ChanceOrQuestChance, groupid, mincountOrRef, maxcount FROM {table}
             WHERE entry = ?1 AND patch_min <= ?2 AND patch_max >= ?2 AND condition_id = 0
             ORDER BY groupid, item"
        ))?;
        let found = stmt
            .query_map(rusqlite::params![entry, PATCH], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(found)
    };
    let mut tsv = String::from("owner\titem\tchance\tgroup\tmin\tmax\n");
    let mut items = std::collections::BTreeSet::new();
    let mut refs: Vec<i64> = Vec::new();
    let mut done_refs = std::collections::BTreeSet::new();
    let mut write = |owner: String, list: Vec<(i64, f64, i64, i64, i64)>, refs: &mut Vec<i64>| {
        for (item, chance, group, min, max) in list {
            if chance < 0.0 {
                continue;
            }
            if min < 0 {
                refs.push(-min);
            } else {
                items.insert(item);
            }
            let _ = writeln!(tsv, "{owner}\t{item}\t{chance}\t{group}\t{min}\t{max}");
        }
    };
    for &entry in creatures {
        let loot_id: i64 = conn
            .query_row(
                "SELECT loot_id FROM creature_template WHERE entry = ?1 AND patch <= ?2
                 ORDER BY patch DESC LIMIT 1",
                rusqlite::params![entry, PATCH],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if loot_id != 0 {
            write(format!("c{entry}"), rows("creature_loot_template", loot_id)?, &mut refs);
        }
    }
    while let Some(id) = refs.pop() {
        if done_refs.insert(id) {
            write(format!("r{id}"), rows("reference_loot_template", id)?, &mut refs);
        }
    }
    drop(write);
    std::fs::write(out.join("loot.tsv"), tsv)?;
    Ok(items)
}

/// `chests.tsv`: every chest vmangos places on Tirisfall's ground (a `GAMEOBJECT_TYPE_CHEST` named
/// a chest, lockbox or coffer), outside the towns, with its template's look.
fn export_chests(
    conn: &Connection,
    chain: &benilla_formats::Chain,
    zones: &mut ZoneLookup,
    areas: &benilla_formats::AreaTableCatalog,
    out: &Path,
) -> Result<usize> {
    let mut stmt = conn.prepare(
        "SELECT g.guid, g.id, t.name, t.displayId, t.size, g.position_x, g.position_y,
                g.position_z, g.orientation, g.rotation0, g.rotation1, g.rotation2, g.rotation3
         FROM gameobject g JOIN gameobject_template t ON t.entry = g.id
         WHERE g.map = ?1 AND g.patch_min <= ?2 AND g.patch_max >= ?2 AND t.type = 3
           AND g.position_x BETWEEN ?3 AND ?4 AND g.position_y BETWEEN ?5 AND ?6
           AND (t.name LIKE '%Chest%' OR t.name LIKE '%Lockbox%' OR t.name LIKE '%Coffer%')
           AND t.patch = (SELECT MAX(patch) FROM gameobject_template u
                          WHERE u.entry = t.entry AND u.patch <= ?2)
         ORDER BY g.guid",
    )?;
    let mut tsv = String::from("guid\tentry\tname\tdisplay\tsize\tx\ty\tz\to\tr0\tr1\tr2\tr3\n");
    let mut count = 0;
    let mut rows = stmt.query(rusqlite::params![
        MAP,
        PATCH,
        TIRISFALL_X.0,
        TIRISFALL_X.1,
        TIRISFALL_Y.0,
        TIRISFALL_Y.1
    ])?;
    while let Some(r) = rows.next()? {
        let (x, y, z): (f64, f64, f64) = (r.get(5)?, r.get(6)?, r.get(7)?);
        let area = zones.area_at(chain, x as f32, y as f32);
        if area.and_then(|a| areas.top_zone(a)) != Some(TIRISFALL_ZONE)
            || survivor_server::town::town_at([x as f32, y as f32, z as f32]).is_some()
        {
            continue;
        }
        let name: String = r.get(2)?;
        writeln!(
            tsv,
            "{}\t{}\t{name}\t{}\t{:.2}\t{x:.2}\t{y:.2}\t{z:.2}\t{:.3}\t{:.4}\t{:.4}\t{:.4}\t{:.4}",
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(3)?,
            r.get::<_, f64>(4)?,
            r.get::<_, f64>(8)?,
            r.get::<_, f64>(9)?,
            r.get::<_, f64>(10)?,
            r.get::<_, f64>(11)?,
            r.get::<_, f64>(12)?,
        )?;
        count += 1;
    }
    std::fs::write(out.join("chests.tsv"), tsv)?;
    Ok(count)
}

/// `gathers.tsv`: every object in Tirisfall a quest item is looted from (a `GAMEOBJECT_TYPE_CHEST`
/// whose loot holds a quest-only drop), towns included, with the item. Returns `item -> object`,
/// the object with the most spawns where several give the same item.
fn export_gathers(
    conn: &Connection,
    chain: &benilla_formats::Chain,
    zones: &mut ZoneLookup,
    areas: &benilla_formats::AreaTableCatalog,
    out: &Path,
) -> Result<std::collections::BTreeMap<i64, i64>> {
    let mut stmt = conn.prepare(
        "SELECT g.guid, g.id, t.name, t.displayId, t.size, g.position_x, g.position_y,
                g.position_z, g.orientation, g.rotation0, g.rotation1, g.rotation2, g.rotation3,
                l.item
         FROM gameobject g JOIN gameobject_template t ON t.entry = g.id
              JOIN gameobject_loot_template l ON l.entry = t.data1
         WHERE g.map = ?1 AND g.patch_min <= ?2 AND g.patch_max >= ?2 AND t.type = 3
           AND l.ChanceOrQuestChance < 0 AND l.patch_min <= ?2 AND l.patch_max >= ?2
           AND g.position_x BETWEEN ?3 AND ?4 AND g.position_y BETWEEN ?5 AND ?6
           AND t.patch = (SELECT MAX(patch) FROM gameobject_template u
                          WHERE u.entry = t.entry AND u.patch <= ?2)
         ORDER BY g.guid",
    )?;
    let mut tsv =
        String::from("guid	entry	name	display	size	x	y	z	o	r0	r1	r2	r3	item
");
    let mut spawns: std::collections::BTreeMap<(i64, i64), usize> = Default::default();
    let mut seen = std::collections::BTreeSet::new();
    let mut rows = stmt.query(rusqlite::params![
        MAP,
        PATCH,
        TIRISFALL_X.0,
        TIRISFALL_X.1,
        TIRISFALL_Y.0,
        TIRISFALL_Y.1
    ])?;
    while let Some(r) = rows.next()? {
        let (x, y, z): (f64, f64, f64) = (r.get(5)?, r.get(6)?, r.get(7)?);
        let area = zones.area_at(chain, x as f32, y as f32);
        if area.and_then(|a| areas.top_zone(a)) != Some(TIRISFALL_ZONE) {
            continue;
        }
        let (guid, entry, item): (i64, i64, i64) = (r.get(0)?, r.get(1)?, r.get(13)?);
        // vmangos stacks a few spawns on the same spot twice: one of them.
        let spot = (entry, (x * 2.0).round() as i64 * 100_000 + (y * 2.0).round() as i64);
        if !seen.insert(spot) {
            continue;
        }
        *spawns.entry((item, entry)).or_default() += 1;
        let name: String = r.get(2)?;
        writeln!(
            tsv,
            "{guid}\t{entry}\t{name}\t{}\t{:.2}\t{x:.2}\t{y:.2}\t{z:.2}\t{:.3}\t{:.4}\t{:.4}\t{:.4}\t{:.4}\t{item}",
            r.get::<_, i64>(3)?,
            r.get::<_, f64>(4)?,
            r.get::<_, f64>(8)?,
            r.get::<_, f64>(9)?,
            r.get::<_, f64>(10)?,
            r.get::<_, f64>(11)?,
            r.get::<_, f64>(12)?,
        )?;
    }
    std::fs::write(out.join("gathers.tsv"), tsv)?;
    let mut best: std::collections::BTreeMap<i64, (usize, i64)> = Default::default();
    for (&(item, entry), &n) in &spawns {
        let b = best.entry(item).or_insert((0, entry));
        if n > b.0 {
            *b = (n, entry);
        }
    }
    Ok(best.into_iter().map(|(item, (_, entry))| (item, entry)).collect())
}

/// A quest objective naming a game object rather than a creature: its entry with this bit, as
/// `SMSG_QUEST_QUERY_RESPONSE` and `SMSG_QUESTUPDATE_ADD_KILL` carry it.
const OBJECTIVE_GAMEOBJECT: i64 = 0x8000_0000;

/// `voice/`: the VoiceOver addon's spoken lines (`$VOICEOVER_DIR`, its unzipped
/// `AI_VoiceOverData_Vanilla`) for every quest in `quests.tsv`, as it names them:
/// `<id>-accept.mp3` and `<id>-complete.mp3`, and their lengths from the pack's own table
/// (`voice/lengths.tsv`: name, seconds), for the window to pace its text by. Returns how many
/// lines it copied.
fn copy_quest_voices(pack: &Path, out: &Path) -> Result<usize> {
    let from = pack.join("generated").join("sounds").join("quests");
    let to = out.join("voice");
    std::fs::create_dir_all(&to)?;
    let tsv = std::fs::read_to_string(out.join("quests.tsv"))?;
    // `["3902-accept"] = 28.368875,` lines of the addon's Lua table.
    let table = std::fs::read_to_string(pack.join("generated").join("sound_length_table.lua"))
        .unwrap_or_default();
    let seconds = |name: &str| {
        let key = format!("[\"{name}\"] = ");
        table.lines().find_map(|l| {
            let rest = l.trim().strip_prefix(&key)?;
            rest.trim_end_matches(',').trim().parse::<f32>().ok()
        })
    };
    let mut lengths = String::from("name\tseconds\n");
    let mut n = 0;
    for id in tsv.lines().skip(1).filter_map(|l| l.split('\t').next()) {
        for kind in ["accept", "complete"] {
            let name = format!("{id}-{kind}.mp3");
            if from.join(&name).is_file() {
                std::fs::copy(from.join(&name), to.join(&name))?;
                n += 1;
                if let Some(s) = seconds(&format!("{id}-{kind}")) {
                    writeln!(lengths, "{id}-{kind}\t{s:.3}")?;
                }
            }
        }
    }
    std::fs::write(to.join("lengths.tsv"), lengths)?;
    Ok(n)
}

/// The towns' guards that walk a beat: vmangos's own patrols of Deathguard Oliver and Deathguard
/// Phillip through Deathknell (`creature_movement_template`). Each gets a Deathguard Elite at
/// their side.
const PATROLLERS: [i64; 2] = [1737, 1739];
/// The guard every post and patrol partner is.
const DEATHGUARD_ELITE: i64 = 7980;
/// How far outside the barrier the entrance guards stand, and from the road's middle.
const POST_OUT: f32 = 3.5;
const POST_ACROSS: [f32; 2] = [4.5, 8.0];
/// A patrol partner walks this far to the right of the path.
const PARTNER_SIDE: f32 = 2.2;

/// The guards the towns add: four at every road into a town ([`survivor_server::town::Town`]'s
/// `entrances`), two each side, facing out; and a partner for every patroller, whose path goes
/// to `patrols.tsv`. Heights come from the map tiles. Guids count down from the top of the
/// range, clear of vmangos's.
fn add_guards(
    conn: &Connection,
    chain: &benilla_formats::Chain,
    out: &Path,
    npcs: &mut String,
) -> Result<usize> {
    let mut ground = Heights::default();
    let mut guid = 9_999_999i64;
    let mut count = 0;
    for town in survivor_server::town::TOWNS {
        for &a in town.entrances {
            let (sin, cos) = a.sin_cos();
            let r = survivor_server::town::edge(town, a) + POST_OUT;
            for across in POST_ACROSS {
                for side in [-1.0f32, 1.0] {
                    let x = town.centre[0] + cos * r - sin * across * side;
                    let y = town.centre[1] + sin * r + cos * across * side;
                    let z = ground.at(chain, x, y).unwrap_or(town.ground);
                    writeln!(npcs, "{guid}\t{DEATHGUARD_ELITE}\t{x:.2}\t{y:.2}\t{z:.2}\t{a:.3}\t0\t0")?;
                    guid -= 1;
                    count += 1;
                }
            }
        }
    }
    let mut patrols = String::from("patrol\tpoint\tx\ty\tz\twait_ms\n");
    let mut stmt = conn.prepare(
        "SELECT point, position_x, position_y, position_z, waittime
         FROM creature_movement_template WHERE entry = ?1 ORDER BY point",
    )?;
    for entry in PATROLLERS {
        let points: Vec<(i64, f64, f64, f64, i64)> = stmt
            .query_map([entry], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
            .collect::<Result<_, _>>()?;
        let Some(&(_, x, y, z, _)) = points.first() else { continue };
        for (point, x, y, z, wait) in &points {
            writeln!(patrols, "{entry}\t{point}\t{x:.2}\t{y:.2}\t{z:.2}\t{wait}")?;
        }
        writeln!(npcs, "{guid}\t{DEATHGUARD_ELITE}\t{x:.2}\t{y:.2}\t{z:.2}\t0\t{entry}\t{PARTNER_SIDE}")?;
        guid -= 1;
        count += 1;
    }
    std::fs::write(out.join("patrols.tsv"), patrols)?;
    Ok(count)
}

/// Terrain heights from the Eastern Kingdoms' map tiles (`MCVT`), read as asked.
#[derive(Default)]
struct Heights {
    tiles: std::collections::HashMap<(i32, i32), Option<Vec<benilla_adt::McnkChunk>>>,
}

impl Heights {
    /// The ground at `(x, y)`, blended across the outer height grid's cell.
    fn at(&mut self, chain: &benilla_formats::Chain, x: f32, y: f32) -> Option<f32> {
        let (col, row) = (
            (32.0 - y / TILE).floor() as i32,
            (32.0 - x / TILE).floor() as i32,
        );
        let chunks = self.tiles.entry((col, row)).or_insert_with(|| {
            let bytes = chain
                .read(&format!("World\\Maps\\Azeroth\\Azeroth_{col}_{row}.adt"))
                .ok()?;
            let benilla_adt::ParsedAdt::Root(adt) =
                benilla_adt::parse_adt(&mut std::io::Cursor::new(&bytes[..])).ok()?;
            Some(adt.mcnk_chunks)
        });
        let c = chunks.as_ref()?.iter().find(|c| {
            let p = c.header.position;
            x <= p[0] && x > p[0] - CHUNK && y <= p[1] && y > p[1] - CHUNK
        })?;
        let h = &c.heights.as_ref()?.heights;
        let p = c.header.position;
        let unit = CHUNK / 8.0;
        let (fi, fj) = ((p[0] - x) / unit, (p[1] - y) / unit);
        let (i, j) = ((fi.floor() as usize).min(7), (fj.floor() as usize).min(7));
        let (ti, tj) = (fi - i as f32, fj - j as f32);
        // Outer vertex (i, j) is row i of 17 (nine outer, eight inner), column j.
        let at = |i: usize, j: usize| h.get(i * 17 + j).copied();
        let top = at(i, j)? * (1.0 - tj) + at(i, j + 1)? * tj;
        let bottom = at(i + 1, j)? * (1.0 - tj) + at(i + 1, j + 1)? * tj;
        Some(p[2] + top * (1.0 - ti) + bottom * ti)
    }
}

/// `quests.tsv`: the kill-only quests the towns' people give, for the Forsaken, of a level the
/// zone's creatures suit, whose every target is spawned; and the gather-only ones whose every
/// item comes from an object in `gathers` (`item -> object`), written as objectives to use that
/// object, the item's name their text. A prerequisite outside the set is dropped, so a chain
/// starts wherever its first quest is. Text keeps WoW's `$B` breaks.
fn export_quests(
    conn: &Connection,
    out: &Path,
    givers: &std::collections::BTreeSet<i64>,
    spawned: &std::collections::BTreeSet<i64>,
    gathers: &std::collections::BTreeMap<i64, i64>,
    rewards_used: &mut std::collections::BTreeSet<i64>,
) -> Result<usize> {
    let mut item_name = conn.prepare(
        "SELECT name FROM item_template WHERE entry = ?1 AND patch <= ?2
         ORDER BY patch DESC LIMIT 1",
    )?;
    let mut stmt = conn.prepare(
        "SELECT q.entry, q.QuestLevel, q.MinLevel, q.PrevQuestId, q.RewXP, q.RewOrReqMoney,
                q.Title, q.Details, q.Objectives, q.OfferRewardText, q.EndText,
                q.ReqCreatureOrGOId1, q.ReqCreatureOrGOId2, q.ReqCreatureOrGOId3,
                q.ReqCreatureOrGOId4, q.ReqCreatureOrGOCount1, q.ReqCreatureOrGOCount2,
                q.ReqCreatureOrGOCount3, q.ReqCreatureOrGOCount4, q.ObjectiveText1,
                q.ObjectiveText2, q.ObjectiveText3, q.ObjectiveText4, q.RequiredRaces,
                q.ReqItemId1, q.ReqItemId2, q.ReqItemId3, q.ReqItemId4, q.RequiredClasses, r.id,
                (SELECT e.id FROM creature_involvedrelation e
                  WHERE e.quest = q.entry AND e.patch_min <= ?1 AND e.patch_max >= ?1 LIMIT 1),
                q.ReqItemCount1, q.ReqItemCount2, q.ReqItemCount3, q.ReqItemCount4,
                q.RewChoiceItemId1, q.RewChoiceItemId2, q.RewChoiceItemId3,
                q.RewChoiceItemId4, q.RewChoiceItemId5, q.RewChoiceItemId6
         FROM quest_template q JOIN creature_questrelation r ON r.quest = q.entry
         WHERE r.patch_min <= ?1 AND r.patch_max >= ?1
           AND q.patch = (SELECT MAX(patch) FROM quest_template u
                          WHERE u.entry = q.entry AND u.patch <= ?1)
         ORDER BY q.QuestLevel, q.entry",
    )?;
    struct Row {
        id: i64,
        cols: Vec<String>,
        prev: i64,
    }
    let text = |v: Option<String>| {
        v.unwrap_or_default()
            .replace("\r\n", "$B")
            .replace(['\n', '\r'], "$B")
            .replace('\t', " ")
    };
    let mut kept: Vec<Row> = Vec::new();
    let mut rows = stmt.query([PATCH])?;
    while let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        let level: i64 = r.get(1)?;
        let races: i64 = r.get(23)?;
        let items: [i64; 4] = [r.get(24)?, r.get(25)?, r.get(26)?, r.get(27)?];
        let classes: i64 = r.get(28)?;
        let giver: i64 = r.get(29)?;
        let mut targets: [i64; 4] = [r.get(11)?, r.get(12)?, r.get(13)?, r.get(14)?];
        let mut counts: [i64; 4] = [r.get(15)?, r.get(16)?, r.get(17)?, r.get(18)?];
        let mut texts: [String; 4] = [
            text(r.get(19)?),
            text(r.get(20)?),
            text(r.get(21)?),
            text(r.get(22)?),
        ];
        let kill_only = items.iter().all(|&i| i == 0) && targets[0] > 0;
        let gather_only = targets.iter().all(|&t| t == 0)
            && items[0] > 0
            && items.iter().all(|&i| i == 0 || gathers.contains_key(&i));
        if gather_only {
            let item_counts: [i64; 4] = [r.get(31)?, r.get(32)?, r.get(33)?, r.get(34)?];
            for k in 0..4 {
                if items[k] == 0 {
                    continue;
                }
                targets[k] = gathers[&items[k]] | OBJECTIVE_GAMEOBJECT;
                counts[k] = item_counts[k];
                if texts[k].is_empty() {
                    texts[k] = item_name
                        .query_row(rusqlite::params![items[k], PATCH], |r| r.get(0))
                        .unwrap_or_default();
                }
            }
        }
        let all_spawned = targets
            .iter()
            .all(|&t| t == 0 || t & OBJECTIVE_GAMEOBJECT != 0 || spawned.contains(&t));
        let for_us = (races == 0 || races & (1 << (UNDEAD - 1)) != 0) && classes == 0;
        if !givers.contains(&giver)
            || !(kill_only || gather_only)
            || !all_spawned
            || !for_us
            || level > MAX_LEVEL
            || kept.iter().any(|k| k.id == id)
        {
            continue;
        }
        let ender: Option<i64> = r.get(30)?;
        let ender = ender.filter(|e| givers.contains(e)).unwrap_or(giver);
        let mut cols = vec![
            id.to_string(),
            level.to_string(),
            r.get::<_, i64>(2)?.to_string(),
            String::new(), // the prerequisite, settled below
            giver.to_string(),
            ender.to_string(),
            r.get::<_, i64>(4)?.to_string(),
            r.get::<_, i64>(5)?.max(0).to_string(),
        ];
        for c in 6..=10 {
            cols.push(text(r.get(c)?));
        }
        for k in 0..4 {
            cols.push(format!("{},{}", targets[k].max(0), counts[k].max(0)));
        }
        cols.extend(texts);
        // The items to choose one of on handing it in.
        let rewards: Vec<i64> = (35..41)
            .map(|c| r.get::<_, i64>(c))
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .filter(|&i| i > 0)
            .collect();
        rewards_used.extend(&rewards);
        cols.push(rewards.iter().map(i64::to_string).collect::<Vec<_>>().join(","));
        kept.push(Row {
            id,
            cols,
            prev: r.get(3)?,
        });
    }
    let ids: std::collections::BTreeSet<i64> = kept.iter().map(|k| k.id).collect();
    let mut tsv = String::from(
        "id\tlevel\tmin_level\tprev\tgiver\tender\txp\tmoney\ttitle\tdetails\tobjectives\t\
         offer_text\tend_text\tkill1\tkill2\tkill3\tkill4\tobjective1\tobjective2\tobjective3\t\
         objective4\trewards\n",
    );
    for mut k in kept {
        k.cols[3] = if ids.contains(&k.prev) { k.prev } else { 0 }.to_string();
        writeln!(tsv, "{}", k.cols.join("\t"))?;
    }
    std::fs::write(out.join("quests.tsv"), tsv)?;
    Ok(ids.len())
}

/// The `AreaTable` id the terrain names under a world position, tile by tile as asked.
#[derive(Default)]
struct ZoneLookup {
    tiles: std::collections::HashMap<(i32, i32), Option<Vec<benilla_adt::McnkHeader>>>,
}

impl ZoneLookup {
    fn area_at(&mut self, chain: &benilla_formats::Chain, x: f32, y: f32) -> Option<u32> {
        let (col, row) = (
            (32.0 - y / TILE).floor() as i32,
            (32.0 - x / TILE).floor() as i32,
        );
        let headers = self.tiles.entry((col, row)).or_insert_with(|| {
            let bytes = chain
                .read(&format!("World\\Maps\\Azeroth\\Azeroth_{col}_{row}.adt"))
                .ok()?;
            let benilla_adt::ParsedAdt::Root(adt) =
                benilla_adt::parse_adt(&mut std::io::Cursor::new(&bytes[..])).ok()?;
            Some(adt.mcnk_chunks.into_iter().map(|c| c.header).collect())
        });
        // A chunk's `position` is its corner at the largest x and y.
        headers.as_ref()?.iter().find_map(|h| {
            let inside = x <= h.position[0]
                && x > h.position[0] - CHUNK
                && y <= h.position[1]
                && y > h.position[1] - CHUNK;
            inside.then_some(h.area_id)
        })
    }
}

/// The Forsaken classes' talents (`Talent.dbc` by `TalentTab.dbc`) to `talents.tsv`: the
/// class, the talent, its tab's place among the class's three, its tier and column, its ranks'
/// spells, and the talent (and 0-based rank) it needs first.
fn export_talents(chain: &mut benilla_formats::Chain, out: &Path) -> Result<()> {
    let catalog = benilla_formats::load_talent_catalog(chain)?;
    let mut tsv = String::from("class\ttalent\ttab\trow\tcol\tranks\tprereq\tprereq_rank\n");
    let mut n = 0;
    for class in [1u8, 4, 5, 8, 9] {
        for (page, tab) in catalog.tabs_for_class(UNDEAD as u8, class).iter().enumerate() {
            for t in catalog.talents_in_tab(tab.id) {
                let ranks: Vec<String> =
                    t.ranks.iter().filter(|&&r| r != 0).map(u32::to_string).collect();
                tsv.push_str(&format!(
                    "{class}\t{}\t{page}\t{}\t{}\t{}\t{}\t{}\n",
                    t.id,
                    t.row,
                    t.col,
                    ranks.join(","),
                    t.prereq_talent,
                    t.prereq_rank
                ));
                n += 1;
            }
        }
    }
    std::fs::write(out.join("talents.tsv"), tsv)?;
    eprintln!("survivor-data: {n} talents");
    Ok(())
}

/// Innkeeper Renee, Brill's: her wares are every innkeeper's (`shop.tsv`).
const INNKEEPER: i64 = 5688;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(db) = args.first() else {
        bail!("usage: survivor-data <mangos.sqlite> (--schema <table> | <wow Data> <out dir>)");
    };
    let conn = Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    if args.get(1).map(String::as_str) == Some("--tables") {
        let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table'")?;
        for t in stmt.query_map([], |r| r.get::<_, String>(0))? {
            println!("{}", t?);
        }
        return Ok(());
    }
    if args.get(1).map(String::as_str) == Some("--schema") {
        let table = args.get(2).map_or("", String::as_str);
        let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let cols = stmt.query_map([], |r| {
            Ok(format!("{} {}", r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })?;
        for c in cols {
            println!("{}", c?);
        }
        return Ok(());
    }
    let (Some(data), Some(out)) = (args.get(1), args.get(2)) else {
        bail!("usage: survivor-data <mangos.sqlite> <wow Data dir> <out dir>");
    };
    let out = Path::new(out);
    std::fs::create_dir_all(out)?;
    let mut chain = benilla_formats::open_chain(Path::new(data)).context("opening the install")?;
    let factions = benilla_formats::load_faction_catalog(&mut chain)?;
    let player = factions
        .template(UNDEAD_FACTION_TEMPLATE)
        .context("the Forsaken faction template")?;

    // Creature templates at their 1.12 row, with health and melee damage from the class-level
    // table times the template's multipliers.
    let mut stmt = conn.prepare(
        "SELECT t.entry, t.name, t.level_min, t.level_max, t.faction, t.npc_flags,
                t.display_id1, t.display_id2, t.display_id3, t.display_id4, t.display_scale1,
                t.speed_walk, t.speed_run, t.rank, t.type, t.base_attack_time,
                t.health_multiplier, t.damage_multiplier, t.unit_class, t.detection_range
         FROM creature_template t
         WHERE t.patch = (SELECT MAX(patch) FROM creature_template u
                          WHERE u.entry = t.entry AND u.patch <= ?1)",
    )?;
    struct Template {
        name: String,
        levels: (i64, i64),
        faction: i64,
        npc_flags: i64,
        display: i64,
        scale: f64,
        speeds: (f64, f64),
        rank: i64,
        kind: i64,
        attack_time: i64,
        health_mult: f64,
        damage_mult: f64,
        unit_class: i64,
        detection: f64,
    }
    let mut templates = std::collections::HashMap::new();
    let rows = stmt.query_map([PATCH], |r| {
        let displays: [i64; 4] = [r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?];
        Ok((
            r.get::<_, i64>(0)?,
            Template {
                name: r.get(1)?,
                levels: (r.get(2)?, r.get(3)?),
                faction: r.get(4)?,
                npc_flags: r.get(5)?,
                display: displays.into_iter().find(|&d| d != 0).unwrap_or(0),
                scale: r.get(10)?,
                speeds: (r.get(11)?, r.get(12)?),
                rank: r.get(13)?,
                kind: r.get(14)?,
                attack_time: r.get(15)?,
                health_mult: r.get(16)?,
                damage_mult: r.get(17)?,
                unit_class: r.get(18)?,
                detection: r.get(19)?,
            },
        ))
    })?;
    for row in rows {
        let (entry, t) = row?;
        templates.insert(entry, t);
    }

    let mut class_stats = std::collections::HashMap::new();
    let mut stmt =
        conn.prepare("SELECT class, level, health, melee_damage FROM creature_classlevelstats")?;
    for row in stmt.query_map([], |r| {
        Ok((
            (r.get::<_, i64>(0)?, r.get::<_, i64>(1)?),
            (r.get::<_, f64>(2)?, r.get::<_, f64>(3)?),
        ))
    })? {
        let (k, v) = row?;
        class_stats.insert(k, v);
    }

    // Spawns inside the zone at 1.12, kept when their creature is a foe and serves nothing.
    let mut stmt = conn.prepare(
        "SELECT guid, id, position_x, position_y, position_z, orientation, spawntimesecsmin,
                wander_distance, movement_type
         FROM creature
         WHERE map = ?1 AND patch_min <= ?2 AND patch_max >= ?2
           AND position_x BETWEEN ?3 AND ?4 AND position_y BETWEEN ?5 AND ?6
         ORDER BY guid",
    )?;
    let mut spawns = String::from("guid\tentry\tx\ty\tz\to\trespawn_s\twander\tmove\n");
    // Spawns that walk a waypoint path (`movement_type` 2).
    let mut walkers = Vec::new();
    let mut used = std::collections::BTreeSet::new();
    let mut npcs = String::from("guid\tentry\tx\ty\tz\to\tpatrol\tside\n");
    let mut npcs_used = std::collections::BTreeSet::new();
    let mut hostile = std::collections::BTreeSet::new();
    let (mut kept, mut dropped) = (0, 0);
    let rows = stmt.query_map(
        rusqlite::params![
            MAP,
            PATCH,
            TIRISFALL_X.0,
            TIRISFALL_X.1,
            TIRISFALL_Y.0,
            TIRISFALL_Y.1
        ],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                [
                    r.get::<_, f64>(2)?,
                    r.get::<_, f64>(3)?,
                    r.get::<_, f64>(4)?,
                    r.get::<_, f64>(5)?,
                ],
                r.get::<_, i64>(6)?,
                r.get::<_, f64>(7)?,
                r.get::<_, i64>(8)?,
            ))
        },
    )?;
    let areas = benilla_formats::load_area_table_catalog(&mut chain)?;
    let mut zones = ZoneLookup::default();
    let mut outside = 0;
    for row in rows {
        let (guid, entry, [x, y, z, o], respawn, wander, movement) = row?;
        let Some(t) = templates.get(&entry) else { continue };
        // The rectangle overlaps the neighbours: keep only what stands on Tirisfall's own ground.
        let area = zones.area_at(&chain, x as f32, y as f32);
        if area.and_then(|a| areas.top_zone(a)) != Some(TIRISFALL_ZONE) {
            outside += 1;
            continue;
        }
        let reaction = factions
            .template(t.faction as u32)
            .map(|f| f.reaction_toward(player));
        let foe = matches!(reaction, Some(Reaction::Hostile | Reaction::Neutral));
        // A town keeps its own people (the quest givers among them) and no foe.
        let in_town = survivor_server::town::near_town([x as f32, y as f32, z as f32]).is_some();
        if in_town {
            if !foe && t.kind != CRITTER && t.display != 0 {
                npcs_used.insert(entry);
                let patrol = if PATROLLERS.contains(&entry) { entry } else { 0 };
                writeln!(npcs, "{guid}\t{entry}\t{x:.2}\t{y:.2}\t{z:.2}\t{o:.3}\t{patrol}\t0")?;
            }
            dropped += 1;
            continue;
        }
        if !foe
            || t.npc_flags != 0
            || t.kind == CRITTER
            || t.display == 0
            || t.levels.1 > MAX_LEVEL
        {
            dropped += 1;
            continue;
        }
        kept += 1;
        used.insert(entry);
        if reaction == Some(Reaction::Hostile) {
            hostile.insert(entry);
        }
        writeln!(
            spawns,
            "{guid}\t{entry}\t{x:.2}\t{y:.2}\t{z:.2}\t{o:.3}\t{respawn}\t{wander:.1}\t{movement}"
        )?;
        if movement == 2 {
            walkers.push(guid);
        }
    }
    std::fs::write(out.join("spawns.tsv"), spawns)?;
    let chests = export_chests(&conn, &chain, &mut zones, &areas, out)?;
    println!("survivor-data: {chests} chests");
    // The waypoint paths of the creatures that walk one, by spawn guid.
    let mut paths = String::from("guid\tpoint\tx\ty\tz\twait_ms\n");
    let mut stmt = conn.prepare(
        "SELECT point, position_x, position_y, position_z, waittime
         FROM creature_movement WHERE id = ?1 ORDER BY point",
    )?;
    for guid in &walkers {
        let points: Vec<(i64, f64, f64, f64, i64)> = stmt
            .query_map([guid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
            .collect::<Result<_, _>>()?;
        for (point, x, y, z, wait) in points {
            writeln!(paths, "{guid}\t{point}\t{x:.2}\t{y:.2}\t{z:.2}\t{wait}")?;
        }
    }
    std::fs::write(out.join("mob_paths.tsv"), paths)?;
    // Every kind a run's spawner sends, spawned in the zone or not (the bosses).
    for e in survivor_server::waves::all_kinds() {
        used.insert(i64::from(e));
    }
    if add_guards(&conn, &chain, out, &mut npcs)? > 0 {
        npcs_used.insert(DEATHGUARD_ELITE);
    }
    std::fs::write(out.join("npcs.tsv"), &npcs)?;
    let gathers = export_gathers(&conn, &chain, &mut zones, &areas, out)?;
    export_talents(&mut chain, out)?;
    let mut rewards_used = std::collections::BTreeSet::new();
    let quests = export_quests(&conn, out, &npcs_used, &used, &gathers, &mut rewards_used)?;
    // The profession trainers held out in the zone (`survivor_server::events`): their own
    // quests only, so they join after the quests are picked.
    for e in survivor_server::events::trainers() {
        npcs_used.insert(i64::from(e));
    }
    if let Some(dir) = std::env::var_os("VOICEOVER_DIR") {
        let lines = copy_quest_voices(Path::new(&dir), out)?;
        println!("survivor-data: {lines} quest voice lines");
    }

    let mut creatures = String::from(
        "entry\tname\tlevel_min\tlevel_max\tfaction\tdisplay\tscale\twalk\trun\trank\ttype\t\
         attack_ms\thealth_per_level\tdamage_per_level\tdetection\thostile\tnpc_flags\n",
    );
    for entry in used.iter().chain(&npcs_used) {
        let t = &templates[entry];
        // Health and damage at the template's lowest and highest level, `min,max`.
        let at = |level: i64| class_stats.get(&(t.unit_class.max(1), level)).copied();
        let (lo, hi) = (at(t.levels.0), at(t.levels.1));
        let health = |s: Option<(f64, f64)>| s.map_or(1.0, |s| (s.0 * t.health_mult).ceil());
        let damage = |s: Option<(f64, f64)>| s.map_or(1.0, |s| s.1 * t.damage_mult);
        writeln!(
            creatures,
            "{entry}\t{}\t{}\t{}\t{}\t{}\t{:.2}\t{:.2}\t{:.2}\t{}\t{}\t{}\t{},{}\t{:.1},{:.1}\t{:.0}\t{}\t{}",
            t.name,
            t.levels.0,
            t.levels.1,
            t.faction,
            t.display,
            if t.scale > 0.0 { t.scale } else { 1.0 },
            t.speeds.0,
            t.speeds.1,
            t.rank,
            t.kind,
            t.attack_time,
            health(lo),
            health(hi),
            damage(lo),
            damage(hi),
            t.detection,
            u8::from(hostile.contains(entry)),
            t.npc_flags,
        )?;
    }
    std::fs::write(out.join("creatures.tsv"), creatures)?;
    let loot_items = export_loot(&conn, &used, out)?;

    // The innkeepers' food and drink: Innkeeper Renee's 1.12 list, with the level each may be
    // bought from (its required level, at least 1).
    let mut stmt = conn.prepare(
        "SELECT DISTINCT v.item, MAX(i.required_level, 1) FROM npc_vendor v
         JOIN item_template i ON i.entry = v.item
         WHERE v.entry = ?1 AND i.patch = (SELECT MAX(patch) FROM item_template u
                                           WHERE u.entry = i.entry AND u.patch <= ?2)
         ORDER BY 2, 1",
    )?;
    let mut shop = String::from("item	level
");
    let mut shop_items = Vec::new();
    for r in stmt.query_map(rusqlite::params![INNKEEPER, PATCH], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
    })? {
        let (item, level) = r?;
        shop_items.push(item);
        writeln!(shop, "{item}	{level}")?;
    }
    std::fs::write(out.join("shop.tsv"), shop)?;

    // Starting gear: every Forsaken class's rows, and the items they name.
    let mut stmt = conn.prepare(
        "SELECT class, itemid, amount FROM playercreateinfo_item WHERE race = ?1 ORDER BY class",
    )?;
    let mut start = String::from("class\titem\tamount\n");
    // The quests' reward items, beside the starting gear and the chests'.
    let mut items_used = rewards_used;
    items_used.extend(loot_items);
    items_used.extend(shop_items);
    for row in stmt.query_map([UNDEAD], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
    })? {
        let (class, item, amount) = row?;
        items_used.insert(item);
        writeln!(start, "{class}\t{item}\t{amount}")?;
    }
    std::fs::write(out.join("start_items.tsv"), start)?;

    // Chest gear: the zone's weapons and armour of common and uncommon make, for any Forsaken
    // class, by the level that may wear them. Their rarity in a run is their quality.
    let mut stmt = conn.prepare(
        "SELECT entry, required_level FROM item_template t
         WHERE patch = (SELECT MAX(patch) FROM item_template u
                        WHERE u.entry = t.entry AND u.patch <= ?1)
           AND class IN (2, 4) AND quality IN (1, 2) AND display_id > 0
           AND required_level BETWEEN 1 AND ?2
           AND inventory_type IN (1, 3, 5, 6, 7, 8, 9, 10, 16, 20, 13, 17, 21, 14, 15, 26, 22, 23)
           AND (allowable_class = -1 OR allowable_class & ?3 != 0)
           AND start_quest = 0
           AND name NOT LIKE '%Test%' AND name NOT LIKE '%Deprecated%'
           AND name NOT LIKE '%(old)%' AND name NOT LIKE '%[PH]%'
           AND name NOT LIKE 'Monster -%' AND name NOT LIKE '%Unused%' AND name NOT LIKE '%NPC%'
           AND name NOT LIKE '%QA%' AND name NOT LIKE '%DEBUG%'
         ORDER BY entry",
    )?;
    let class_mask: i64 = CLASSES.iter().map(|c| 1i64 << (c - 1)).sum();
    let mut gear = String::from("entry\trequired_level\n");
    for r in stmt.query_map(rusqlite::params![PATCH, GEAR_MAX_LEVEL, class_mask], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
    })? {
        let (entry, level) = r?;
        items_used.insert(entry);
        writeln!(gear, "{entry}\t{level}")?;
    }
    std::fs::write(out.join("gear.tsv"), gear)?;

    let mut stmt = conn.prepare(
        "SELECT entry, class, subclass, name, display_id, quality, inventory_type, delay,
                dmg_min1, dmg_max1, dmg_type1, armor, stackable, sheath, max_durability,
                material, item_level, flags, buy_price, sell_price, max_count, spellid_1,
                stat_type1, stat_value1, stat_type2, stat_value2, stat_type3, stat_value3,
                stat_type4, stat_value4, stat_type5, stat_value5, stat_type6, stat_value6,
                stat_type7, stat_value7, stat_type8, stat_value8, stat_type9, stat_value9,
                stat_type10, stat_value10,
                holy_res, fire_res, nature_res, frost_res, shadow_res, arcane_res
         FROM item_template t
         WHERE entry = ?1 AND patch = (SELECT MAX(patch) FROM item_template u
                                       WHERE u.entry = t.entry AND u.patch <= ?2)",
    )?;
    let mut items = String::from(
        "entry\tclass\tsubclass\tname\tdisplay\tquality\tinventory_type\tdelay\tdmg_min\tdmg_max\t\
         dmg_type\tarmor\tstackable\tsheath\tdurability\tmaterial\titem_level\tflags\tbuy\tsell\t\
         max_count\tspell\tstats\tresistances\n",
    );
    // The items' use spells (food, drink), exported with the class spells.
    let mut item_spells = std::collections::BTreeSet::new();
    for &entry in &items_used {
        let mut rows = stmt.query(rusqlite::params![entry, PATCH])?;
        if let Some(r) = rows.next()? {
            let spell: i64 = r.get(21)?;
            if spell != 0 {
                item_spells.insert(spell);
            }
            let cols: Vec<String> = (0..22)
                .map(|i| match r.get_ref(i) {
                    Ok(rusqlite::types::ValueRef::Integer(v)) => v.to_string(),
                    Ok(rusqlite::types::ValueRef::Real(v)) => format!("{v}"),
                    Ok(rusqlite::types::ValueRef::Text(t)) => {
                        String::from_utf8_lossy(t).replace(['\t', '\n'], " ")
                    }
                    _ => String::new(),
                })
                .collect();
            // The item's own bonuses, as `type:value` pairs (`ItemModType`: 3 agility, 4
            // strength, 5 intellect, 6 spirit, 7 stamina), and its six resistances, holy to arcane.
            let mut stats = Vec::new();
            for k in 0..10 {
                let (kind, value): (i64, i64) = (r.get(22 + 2 * k)?, r.get(23 + 2 * k)?);
                if value != 0 {
                    stats.push(format!("{kind}:{value}"));
                }
            }
            let resists = (42..48)
                .map(|i| r.get::<_, i64>(i).map(|v| v.to_string()))
                .collect::<rusqlite::Result<Vec<_>>>()?;
            writeln!(items, "{}\t{}\t{}", cols.join("\t"), stats.join(","), resists.join(","))?;
        }
    }
    std::fs::write(out.join("items.tsv"), items)?;

    let spells = export_spells(&conn, &chain, out, &item_spells)?;

    println!(
        "survivor-data: {spells} class spells and ranks -> {}",
        out.display()
    );
    println!(
        "survivor-data: {} town NPCs of {} kinds, {quests} kill quests",
        npcs.lines().count() - 1,
        npcs_used.len()
    );
    println!(
        "survivor-data: {kept} spawns kept ({outside} outside the zone, {dropped} friendly, \
         service or critter dropped), {} \
         creature kinds, {} starting items -> {}",
        used.len(),
        items_used.len(),
        out.display()
    );
    Ok(())
}
