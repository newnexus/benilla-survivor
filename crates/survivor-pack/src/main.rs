//! `survivor-pack <wow Data dir> <out Data dir> [read-log ...]`: writes `<out>/patch.MPQ`, an
//! archive holding only what the survivor-like reads, so the game fits on a small drive.
//!
//! What goes in:
//! 1. every path in the read logs (`$SURVIVOR_READ_LOG` runs of the game);
//! 2. every data table, interface and font file, and the Forsaken character art, so a panel or
//!    a look the logged run never opened still works;
//! 3. Tirisfall Glades' map tiles with a one-tile border, and every model, building and texture
//!    they place, followed down through the models' own references;
//! 4. those tiles' minimap images.
//!
//! The output is the user's own install, trimmed, for their own machine. Never commit it.

mod mpq;

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use benilla_formats::Chain;

/// Prefixes kept whole, from the archives' listfiles.
const KEEP_PREFIXES: &[&str] = &[
    "dbfilesclient\\",
    "interface\\",
    "fonts\\",
    "character\\scourge\\",
    // The Scarlet Crusade and other human NPCs (18 MB).
    "character\\human\\",
    // Sound: the interface, a Forsaken's own voice and body, weapons, the login music. Zone
    // music and ambience come from Tirisfall's own areas ([`zone_sounds`]).
    "sound\\interface\\",
    "sound\\character\\scourge\\",
    "sound\\character\\playerexertions\\",
    "sound\\character\\footsteps\\",
    "sound\\character\\playerroars\\",
    "sound\\item\\",
    "sound\\music\\gluescreenmusic\\",
    // Every spell's effect models and sounds (16 + 47 MB): a level-up pick can teach any class
    // spell, so no logged run covers them all.
    "spells\\",
    "sound\\spells\\",
    // Death thuds and other shared hits (2 MB).
    "sound\\effects\\",
];

/// `AreaTable.dbc` id of Tirisfall Glades.
const TIRISFALL_ZONE: u32 = 85;

/// Every sound file Tirisfall's areas play: their zone music, intro music and ambience.
fn zone_sounds(chain: &mut Chain) -> Result<Vec<String>> {
    let areas = benilla_formats::load_area_sound_catalog(chain)?;
    let kits = benilla_formats::load_sound_kit_catalog(chain)?;
    let mut kit_ids = Vec::new();
    for id in 1..=8192 {
        let Some(area) = areas.area(id) else { continue };
        if area.id != TIRISFALL_ZONE && area.parent != TIRISFALL_ZONE {
            continue;
        }
        let Some(audio) = areas.resolve(id) else { continue };
        if let Some(m) = audio.music {
            kit_ids.extend(m.sounds);
        }
        if let Some(i) = audio.intro {
            kit_ids.push(i.sound_id);
        }
        if let Some(a) = audio.ambience {
            kit_ids.extend(a.kits);
        }
    }
    Ok(kit_ids
        .into_iter()
        .filter_map(|k| kits.get(k))
        .flat_map(|k| k.files.iter().map(|(path, _)| path.clone()))
        .collect())
}

/// Tirisfall Glades' extent in world coordinates (`WorldMapArea.dbc`, area 85): x is north, y
/// is west.
const TIRISFALL_X: (f32, f32) = (824.0, 3838.0);
const TIRISFALL_Y: (f32, f32) = (-1486.0, 3034.0);
/// A map tile's edge, in yards.
const TILE: f32 = 533.333_3;

fn norm(name: &str) -> String {
    name.replace('/', "\\").to_ascii_lowercase()
}

/// Map tile `(column, row)` of the `Azeroth_<column>_<row>.adt` holding world `(x, y)`.
fn tile_of(x: f32, y: f32) -> (i32, i32) {
    ((32.0 - y / TILE).floor() as i32, (32.0 - x / TILE).floor() as i32)
}

/// Every NUL-terminated printable run in `bytes` that names an asset file by its extension.
fn referenced_names(bytes: &[u8]) -> Vec<String> {
    const EXTS: &[&str] = &[".blp", ".mdx", ".m2", ".wmo", ".mdl"];
    let mut out = Vec::new();
    for run in bytes.split(|&b| b == 0) {
        let start = run
            .iter()
            .rposition(|&b| !(0x20..0x7f).contains(&b))
            .map_or(0, |i| i + 1);
        let s = &run[start..];
        if s.len() < 5 {
            continue;
        }
        let Ok(s) = std::str::from_utf8(s) else { continue };
        let lower = s.to_ascii_lowercase();
        if EXTS.iter().any(|e| lower.ends_with(e)) && lower.contains('\\') {
            out.push(s.to_owned());
        }
    }
    out
}

struct Picker<'a> {
    chain: &'a Chain,
    /// Lowercased path to the spelling first seen.
    picked: BTreeMap<String, String>,
}

impl Picker<'_> {
    /// Keep `name` if the chain has it; true when newly kept.
    fn keep(&mut self, name: &str) -> bool {
        let key = norm(name);
        if self.picked.contains_key(&key) {
            return false;
        }
        if !self.chain.contains(name) {
            return false;
        }
        self.picked.insert(key, name.to_owned());
        true
    }

    /// Keep a model, building or texture and everything it names, recursively.
    fn keep_asset(&mut self, name: &str) {
        let mut work = vec![name.to_owned()];
        while let Some(name) = work.pop() {
            let lower = norm(&name);
            // Placements and models name `.mdx`; the archives hold `.m2`.
            let candidates: Vec<String> = if let Some(stem) = lower
                .strip_suffix(".mdx")
                .or_else(|| lower.strip_suffix(".mdl"))
            {
                vec![format!("{stem}.m2"), lower.clone()]
            } else {
                vec![lower.clone()]
            };
            for c in candidates {
                if !self.keep(&c) {
                    continue;
                }
                // Only placements and models are followed: a data table names every asset in the
                // game, which would pull the whole install back in.
                if ![".adt", ".wmo", ".m2"].iter().any(|e| c.ends_with(e)) {
                    continue;
                }
                let Ok(bytes) = self.chain.read(&c) else { continue };
                work.extend(referenced_names(&bytes));
                // A building's root names its group files only by count: `<root>_NNN.wmo`.
                if let Some(stem) = c.strip_suffix(".wmo") {
                    if !stem.rsplit('_').next().is_some_and(|t| {
                        t.len() == 3 && t.bytes().all(|b| b.is_ascii_digit())
                    }) {
                        for g in 0..512 {
                            let group = format!("{stem}_{g:03}.wmo");
                            if !self.chain.contains(&group) {
                                break;
                            }
                            work.push(group);
                        }
                    }
                }
            }
        }
    }
}

/// `survivor-pack --du <Data dir> <prefix>`: the listed size under each folder one level below
/// `prefix`, largest first, for choosing what to keep.
fn disk_usage(data: &str, prefix: &str) -> Result<()> {
    let chain = Chain::open(std::path::Path::new(data))?;
    let prefix = norm(prefix);
    let mut sizes: BTreeMap<String, (u64, usize)> = BTreeMap::new();
    for e in chain.list()? {
        let key = norm(&e.name);
        let Some(rest) = key.strip_prefix(&prefix) else { continue };
        let folder = rest.split('\\').next().unwrap_or_default().to_owned();
        let s = sizes.entry(folder).or_default();
        s.0 += e.size;
        s.1 += 1;
    }
    let mut rows: Vec<_> = sizes.into_iter().collect();
    rows.sort_by_key(|(_, (b, _))| std::cmp::Reverse(*b));
    for (folder, (bytes, n)) in rows {
        println!("{:>8.1} MB {n:>6} files  {prefix}{folder}", bytes as f64 / 1e6);
    }
    Ok(())
}

/// The raw records of a DBC: `(field count, record bytes, string block)`.
fn dbc(chain: &Chain, name: &str) -> Result<(usize, Vec<Vec<u8>>, Vec<u8>)> {
    let d = chain.read(name)?;
    let u = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().expect("4")) as usize;
    let (n, fields, size, strings) = (u(4), u(8), u(12), u(16));
    let recs = (0..n)
        .map(|i| d[20 + i * size..20 + (i + 1) * size].to_vec())
        .collect();
    let block = d[20 + n * size..20 + n * size + strings].to_vec();
    Ok((fields, recs, block))
}

/// The model file of each `CreatureDisplayInfo` id in `displays`, via `CreatureModelData`.
fn creature_models(chain: &Chain, displays: &[u32]) -> Result<Vec<String>> {
    let field = |r: &[u8], i: usize| u32::from_le_bytes(r[i * 4..i * 4 + 4].try_into().expect("4"));
    let (_, cdi, _) = dbc(chain, "DBFilesClient\\CreatureDisplayInfo.dbc")?;
    let (_, cmd, strings) = dbc(chain, "DBFilesClient\\CreatureModelData.dbc")?;
    // A humanoid NPC's face and clothes come baked into one texture, named by the last field of
    // its `CreatureDisplayInfoExtra` row (`ExtendedDisplayInfoID`, field 3).
    let (extra_fields, extra, extra_strings) =
        dbc(chain, "DBFilesClient\\CreatureDisplayInfoExtra.dbc")?;
    let string_at = |block: &[u8], off: usize| {
        let end = block[off..].iter().position(|&b| b == 0).map_or(block.len(), |e| off + e);
        String::from_utf8_lossy(&block[off..end]).into_owned()
    };
    let mut out = Vec::new();
    for &d in displays {
        let Some(row) = cdi.iter().find(|r| field(r, 0) == d) else { continue };
        if let Some(x) = extra.iter().find(|r| field(r, 0) == field(row, 3) && field(row, 3) != 0) {
            let bake = string_at(&extra_strings, field(x, extra_fields - 1) as usize);
            if !bake.is_empty() {
                out.push(format!("Textures\\BakedNpcTextures\\{bake}"));
            }
        }
        let model = field(row, 1);
        let Some(m) = cmd.iter().find(|r| field(r, 0) == model) else { continue };
        let off = field(m, 2) as usize;
        let end = strings[off..].iter().position(|&b| b == 0).map_or(strings.len(), |e| off + e);
        out.push(String::from_utf8_lossy(&strings[off..end]).into_owned());
    }
    Ok(out)
}

/// Every sound file the creatures of `displays` play: each `CreatureDisplayInfo` row's
/// `CreatureSoundData` row (field 2) names its kits (aggro, wounds, death, wing flaps, fidgets),
/// which live in shared folders as often as in the creature's own. A field that is not a kit
/// (a delay, an impact type) names a small kit or none, which costs little.
fn creature_sounds(chain: &mut Chain, displays: &[u32]) -> Result<Vec<String>> {
    let field = |r: &[u8], i: usize| u32::from_le_bytes(r[i * 4..i * 4 + 4].try_into().expect("4"));
    let (_, cdi, _) = dbc(chain, "DBFilesClient\\CreatureDisplayInfo.dbc")?;
    let (fields, csd, _) = dbc(chain, "DBFilesClient\\CreatureSoundData.dbc")?;
    let kits = benilla_formats::load_sound_kit_catalog(chain)?;
    let mut out = Vec::new();
    for &d in displays {
        let Some(row) = cdi.iter().find(|r| field(r, 0) == d) else { continue };
        let Some(sound) = csd.iter().find(|r| field(r, 0) == field(row, 2) && field(row, 2) != 0)
        else {
            continue;
        };
        for i in 1..fields {
            if let Some(kit) = kits.get(field(sound, i)) {
                out.extend(kit.files.iter().map(|(path, _)| path.clone()));
            }
        }
    }
    Ok(out)
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    if std::env::args().nth(1).as_deref() == Some("--du") {
        let (Some(data), Some(prefix)) = (std::env::args().nth(2), std::env::args().nth(3)) else {
            bail!("usage: survivor-pack --du <Data dir> <prefix>");
        };
        return disk_usage(&data, &prefix);
    }
    let (Some(data), Some(out)) = (args.next(), args.next()) else {
        bail!("usage: survivor-pack <wow Data dir> <out Data dir> [read-log ...]");
    };
    // `--creatures <creatures.tsv>` adds every model (and its sound folder) the server spawns.
    let mut logs: Vec<PathBuf> = Vec::new();
    let mut creatures_tsv: Option<PathBuf> = None;
    // `--gameobjects <chests.tsv>`: the models of the objects the server places (column 4); once
    // per table (chests, gathers).
    let mut objects_tsv: Vec<PathBuf> = Vec::new();
    // `--items <items.tsv>`: the worn and held looks of the items the server deals (column 5).
    let mut items_tsv: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        if a == "--creatures" {
            creatures_tsv = args.next().map(PathBuf::from);
        } else if a == "--gameobjects" {
            objects_tsv.extend(args.next().map(PathBuf::from));
        } else if a == "--items" {
            items_tsv = args.next().map(PathBuf::from);
        } else {
            logs.push(PathBuf::from(a));
        }
    }
    let chain = Chain::open(std::path::Path::new(&data)).context("opening the install")?;
    let mut p = Picker {
        chain: &chain,
        picked: BTreeMap::new(),
    };

    // 1. The logged reads, followed through their references.
    for log in &logs {
        let text = std::fs::read_to_string(log).with_context(|| log.display().to_string())?;
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            p.keep_asset(line);
        }
    }
    let logged = p.picked.len();

    // 2. Whole folders, from the listfiles.
    for entry in chain.list()? {
        let key = norm(&entry.name);
        if KEEP_PREFIXES.iter().any(|pre| key.starts_with(pre)) {
            p.keep_asset(&entry.name);
        }
    }
    // Every spawned creature's model folder (model, skins, texture variations) and the
    // `Sound\Creature\<same name>` folder of its voice.
    if let Some(tsv) = &creatures_tsv {
        let text = std::fs::read_to_string(tsv).with_context(|| tsv.display().to_string())?;
        let displays: Vec<u32> = text
            .lines()
            .skip(1)
            .filter_map(|l| l.split('\t').nth(5)?.parse().ok())
            .collect();
        let mut folders = std::collections::BTreeSet::new();
        for model in creature_models(&chain, &displays)? {
            let key = norm(&model);
            // A baked face is one file: its folder holds every NPC in the game.
            if key.starts_with("textures\\bakednpctextures\\") {
                p.keep(&model);
                continue;
            }
            if let Some((dir, _)) = key.rsplit_once('\\') {
                folders.insert(format!("{dir}\\"));
                if let Some(name) = dir.rsplit('\\').next() {
                    folders.insert(format!("sound\\creature\\{name}\\"));
                }
            }
            p.keep_asset(&model);
        }
        for entry in chain.list()? {
            let key = norm(&entry.name);
            if folders.iter().any(|f| key.starts_with(f)) {
                p.keep_asset(&entry.name);
            }
        }
        let mut sound_chain = Chain::open(std::path::Path::new(&data))?;
        for path in creature_sounds(&mut sound_chain, &displays)? {
            p.keep(&path);
        }
    }
    for tsv in &objects_tsv {
        let text = std::fs::read_to_string(tsv).with_context(|| tsv.display().to_string())?;
        let displays: std::collections::BTreeSet<u32> = text
            .lines()
            .skip(1)
            .filter_map(|l| l.split('\t').nth(3)?.parse().ok())
            .collect();
        let field = |r: &[u8], i: usize| u32::from_le_bytes(r[i * 4..i * 4 + 4].try_into().expect("4"));
        let (_, rows, strings) = dbc(&chain, "DBFilesClient\\GameObjectDisplayInfo.dbc")?;
        for d in displays {
            let Some(row) = rows.iter().find(|r| field(r, 0) == d) else { continue };
            let off = field(row, 1) as usize;
            let end = strings[off..].iter().position(|&b| b == 0).map_or(strings.len(), |e| off + e);
            let model = String::from_utf8_lossy(&strings[off..end]).into_owned();
            if !model.is_empty() {
                p.keep_asset(&model);
            }
        }
    }
    if let Some(tsv) = &items_tsv {
        let text = std::fs::read_to_string(tsv).with_context(|| tsv.display().to_string())?;
        let displays: std::collections::BTreeSet<u32> = text
            .lines()
            .skip(1)
            .filter_map(|l| l.split('\t').nth(4)?.parse().ok())
            .collect();
        let mut item_chain = Chain::open(std::path::Path::new(&data))?;
        let catalog = benilla_formats::load_item_display_catalog(&mut item_chain)?;
        // An item's model, skin and body-region textures are files named after it, in a folder
        // by slot or region: keep every one whose name starts with one of the item's names.
        let mut names = std::collections::BTreeSet::new();
        for d in displays.iter().filter_map(|&d| catalog.get(d)) {
            for n in d.model.iter().chain(&d.model_texture).chain(&d.region_textures).flatten() {
                let base = norm(n);
                let base = base.rsplit('\\').next().unwrap_or(&base);
                let base = base.trim_end_matches(".mdx").trim_end_matches(".m2");
                if !base.is_empty() {
                    names.insert(base.to_owned());
                }
            }
        }
        for entry in chain.list()? {
            let key = norm(&entry.name);
            if !key.starts_with("item\\") {
                continue;
            }
            let file = key.rsplit('\\').next().unwrap_or(&key);
            if names.iter().any(|n| file.starts_with(n.as_str())) {
                p.keep_asset(&entry.name);
            }
        }
    }
    let mut sound_chain = Chain::open(std::path::Path::new(&data))?;
    for path in zone_sounds(&mut sound_chain)? {
        p.keep(&path);
    }
    let foldered = p.picked.len();

    // 3. The zone's tiles and everything placed on them.
    let (c0, r0) = tile_of(TIRISFALL_X.1, TIRISFALL_Y.1);
    let (c1, r1) = tile_of(TIRISFALL_X.0, TIRISFALL_Y.0);
    let mut tiles = Vec::new();
    for col in c0 - 1..=c1 + 1 {
        for row in r0 - 1..=r1 + 1 {
            tiles.push((col, row));
            p.keep_asset(&format!("World\\Maps\\Azeroth\\Azeroth_{col}_{row}.adt"));
        }
    }
    for f in [
        "World\\Maps\\Azeroth\\Azeroth.wdt",
        "World\\Maps\\Azeroth\\Azeroth.wdl",
    ] {
        p.keep(f);
    }
    let mapped = p.picked.len();

    // 4. The minimap: `md5translate.trs` maps `Azeroth\mapCC_RR.blp` to its hashed file.
    if let Ok(trs) = chain.read("Textures\\Minimap\\md5translate.trs") {
        p.keep("Textures\\Minimap\\md5translate.trs");
        for line in String::from_utf8_lossy(&trs).lines() {
            let mut parts = line.split('\t');
            let (Some(plain), Some(hashed)) = (parts.next(), parts.next()) else {
                continue;
            };
            let plain = norm(plain);
            if tiles
                .iter()
                .any(|(c, r)| plain == format!("azeroth\\map{c}_{r}.blp"))
            {
                p.keep(&format!("Textures\\Minimap\\{}", hashed.trim()));
            }
        }
    }

    // The archive, with a listfile so interface enumeration finds what it holds.
    let out_dir = PathBuf::from(out);
    std::fs::create_dir_all(&out_dir)?;
    let mpq_path = out_dir.join("patch.MPQ");
    let mut w = mpq::Writer::create(&mpq_path)?;
    let mut raw_total = 0u64;
    for name in p.picked.values() {
        let bytes = chain
            .read(name)
            .with_context(|| format!("reading {name}"))?;
        raw_total += bytes.len() as u64;
        w.add(name, &bytes)?;
    }
    let listfile = p
        .picked
        .values()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\r\n");
    w.add("(listfile)", listfile.as_bytes())?;
    let size = w.finish()?;
    println!(
        "survivor-pack: {} files ({} from logs, +{} whole folders, +{} Tirisfall map, +{} \
         minimap), {:.0} MB raw -> {} at {:.0} MB",
        p.picked.len(),
        logged,
        foldered - logged,
        mapped - foldered,
        p.picked.len() - mapped,
        raw_total as f64 / 1e6,
        mpq_path.display(),
        size as f64 / 1e6
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deathknell_sits_on_tile_28_28() {
        assert_eq!(tile_of(1676.71, 1678.31), (28, 28));
    }

    #[test]
    fn names_are_found_in_a_string_block() {
        let block = b"\x01\x02World\\Azeroth\\Tree.mdx\0Tileset\\Grass.blp\0junk\0";
        assert_eq!(
            referenced_names(block),
            vec!["World\\Azeroth\\Tree.mdx", "Tileset\\Grass.blp"]
        );
    }
}
