//! `survivor-data <mangos.sqlite> --schema <table>`: print a table's columns.
//! `survivor-data <mangos.sqlite> <wow Data dir> <out dir>`: write the TSV files survivor-server
//! loads, from vmangos's world database at patch 1.12:
//!
//! - `creatures.tsv`: the hostile and neutral creatures spawned in Tirisfall Glades, by template;
//! - `spawns.tsv`: where each of them stands;
//! - `start_items.tsv` and `items.tsv`: each Forsaken class's starting gear.
//!
//! Friendly creatures (the Forsaken's own) and anything with a service (vendors, trainers,
//! quest givers) are left out: the survivor-like has no faction NPCs yet.

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
         effects\tbase_points\tdie_sides\tauras\tamplitudes\ttargets\n",
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
            "{id}\t{}\t{}\t{}\t{}\t{cast_ms}\t{range:.1}\t{}\t{}\t{}\t{}\t{}\t{}\t{duration}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
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
        )?;
        count += 1;
    }
    std::fs::write(out.join("spells.tsv"), spells)?;
    Ok(count)
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
                wander_distance
         FROM creature
         WHERE map = ?1 AND patch_min <= ?2 AND patch_max >= ?2
           AND position_x BETWEEN ?3 AND ?4 AND position_y BETWEEN ?5 AND ?6
         ORDER BY guid",
    )?;
    let mut spawns = String::from("guid\tentry\tx\ty\tz\to\trespawn_s\twander\n");
    let mut used = std::collections::BTreeSet::new();
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
            ))
        },
    )?;
    let areas = benilla_formats::load_area_table_catalog(&mut chain)?;
    let mut zones = ZoneLookup::default();
    let mut outside = 0;
    for row in rows {
        let (guid, entry, [x, y, z, o], respawn, wander) = row?;
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
            "{guid}\t{entry}\t{x:.2}\t{y:.2}\t{z:.2}\t{o:.3}\t{respawn}\t{wander:.1}"
        )?;
    }
    std::fs::write(out.join("spawns.tsv"), spawns)?;

    let mut creatures = String::from(
        "entry\tname\tlevel_min\tlevel_max\tfaction\tdisplay\tscale\twalk\trun\trank\ttype\t\
         attack_ms\thealth_per_level\tdamage_per_level\tdetection\thostile\n",
    );
    for entry in &used {
        let t = &templates[entry];
        // Health and damage at the template's lowest and highest level, `min,max`.
        let at = |level: i64| class_stats.get(&(t.unit_class.max(1), level)).copied();
        let (lo, hi) = (at(t.levels.0), at(t.levels.1));
        let health = |s: Option<(f64, f64)>| s.map_or(1.0, |s| (s.0 * t.health_mult).ceil());
        let damage = |s: Option<(f64, f64)>| s.map_or(1.0, |s| s.1 * t.damage_mult);
        writeln!(
            creatures,
            "{entry}\t{}\t{}\t{}\t{}\t{}\t{:.2}\t{:.2}\t{:.2}\t{}\t{}\t{}\t{},{}\t{:.1},{:.1}\t{:.0}\t{}",
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
        )?;
    }
    std::fs::write(out.join("creatures.tsv"), creatures)?;

    // Starting gear: every Forsaken class's rows, and the items they name.
    let mut stmt = conn.prepare(
        "SELECT class, itemid, amount FROM playercreateinfo_item WHERE race = ?1 ORDER BY class",
    )?;
    let mut start = String::from("class\titem\tamount\n");
    let mut items_used = std::collections::BTreeSet::new();
    for row in stmt.query_map([UNDEAD], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
    })? {
        let (class, item, amount) = row?;
        items_used.insert(item);
        writeln!(start, "{class}\t{item}\t{amount}")?;
    }
    std::fs::write(out.join("start_items.tsv"), start)?;

    let mut stmt = conn.prepare(
        "SELECT entry, class, subclass, name, display_id, quality, inventory_type, delay,
                dmg_min1, dmg_max1, dmg_type1, armor, stackable, sheath, max_durability,
                material, item_level, flags, buy_price, sell_price, max_count, spellid_1
         FROM item_template t
         WHERE entry = ?1 AND patch = (SELECT MAX(patch) FROM item_template u
                                       WHERE u.entry = t.entry AND u.patch <= ?2)",
    )?;
    let mut items = String::from(
        "entry\tclass\tsubclass\tname\tdisplay\tquality\tinventory_type\tdelay\tdmg_min\tdmg_max\t\
         dmg_type\tarmor\tstackable\tsheath\tdurability\tmaterial\titem_level\tflags\tbuy\tsell\t\
         max_count\tspell\n",
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
            writeln!(items, "{}", cols.join("\t"))?;
        }
    }
    std::fs::write(out.join("items.tsv"), items)?;

    let spells = export_spells(&conn, &chain, out, &item_spells)?;

    println!(
        "survivor-data: {spells} class spells and ranks -> {}",
        out.display()
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
