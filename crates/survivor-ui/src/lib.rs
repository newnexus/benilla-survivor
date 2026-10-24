//! The survivor-like's own screens, drawn by Bevy over the benilla client. For now one: the
//! level-up pick, three cards side by side over the dimmed, paused world. A card is taken with a
//! click or the 1-3 keys, and the answer goes back to the in-process server through
//! [`survivor_server::picks`].
//!
//! The cards read the client's own data: each spell's name, rank, icon and tooltip text come
//! from `Spell.dbc` and `SpellIcon.dbc` in the install, and the text is in the game's font.

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{Context, Result};
use benilla_formats::{
    load_spell_catalog, load_spell_durations, load_spell_radii, load_spell_ranges, substitute,
    SpellCatalog, SpellDisplay, SpellDurationCatalog, SpellRadiusCatalog, SpellRangeCatalog,
    TokenContext, TokenNumber,
};
use benilla_formats::Chain;
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use survivor_server::picks::{self, Card, Offer};

mod barrier;
mod ground;
mod roads;
mod countdown;
mod menu_pause;
mod mob_bars;
mod run_clock;
mod quest_frame;
mod quest_panel;
mod chest_glow;
mod armory;
mod orbs;
mod trails;
mod haven;
mod talents;
mod hearth;
mod spirit;
mod big_bars;
mod focus_frame;
mod trade_icons;
mod run_summary;
mod shop;
mod trainer;
mod queue;
mod loot_feed;
mod settings;
mod action_bar;
mod bag_window;

/// The level-up screen.
pub struct SurvivorUiPlugin;

impl Plugin for SurvivorUiPlugin {
    fn build(&self, app: &mut App) {
        settings::seed();
        // A run starting or ending resets the level: only a rise is a level-up.
        benilla_app::SURVIVOR_LEVEL_RISES_ONLY.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Ok(mut rates) = benilla_app::SURVIVOR_FX_RATES.lock() {
            rates.push(("levelup".to_owned(), DING_RATE));
        }
        app.init_resource::<Screen>()
            .add_systems(Update, (show_offer, take_card, hover_cards).chain())
            .add_systems(Update, follow_aim)
            .add_systems(Update, preload_library)
            .add_plugins((queue::plugin, barrier::plugin, haven::plugin, countdown::plugin, run_clock::plugin, mob_bars::plugin, menu_pause::plugin, quest_frame::plugin, quest_panel::plugin, chest_glow::plugin, armory::plugin, orbs::plugin, trails::plugin))
            .add_plugins((talents::plugin, hearth::plugin, spirit::plugin, big_bars::plugin, focus_frame::plugin, trade_icons::plugin, run_summary::plugin, trainer::plugin, loot_feed::plugin, shop::plugin, action_bar::plugin, bag_window::plugin))
            .add_systems(Update, shrink_cards);
    }
}

/// Select the server's auto-target in the client whenever it changes, so the target frame shows
/// it and the action bar's spells have something to cast at.
fn follow_aim(mut last: Local<Option<u64>>, mut select: MessageWriter<benilla_app::SurvivorSelect>) {
    let aim = survivor_server::aim::current();
    if aim != *last {
        *last = aim;
        if let Some(guid) = aim {
            select.write(benilla_app::SurvivorSelect { guid });
        }
    }
}

/// The level-up flash (`Spells\LevelUp\LevelUp.mdx`, a 1.87 s clip) plays this many times
/// faster; the server deals the cards as it ends, the world playing on until then.
const DING_RATE: f32 = 2.5;

/// Above the world and the game's interface; below benilla's loading cover (1000).
const SCREEN_Z: i32 = 950;
/// The survivor's big windows (the level-up cards, the quest window) are drawn this much
/// smaller on a computer's screen than on a phone's.
const SHRINK: f32 = if cfg!(target_os = "android") { 1.0 } else { 0.72 };
/// The purse's top edge and its right edge's distance from the screen's, in screen heights:
/// under the minimap, which is bigger on a phone.
const PURSE_TOP: f32 = if cfg!(target_os = "android") { 27.0 } else { 20.0 };
const PURSE_RIGHT: f32 = if cfg!(target_os = "android") { 8.0 } else { 6.5 };
const GOLD: Color = Color::srgb(1.0, 0.82, 0.0);
const CARD_BG: Color = Color::srgba(0.07, 0.06, 0.05, 0.96);
const CARD_BG_HOVER: Color = Color::srgba(0.16, 0.13, 0.08, 0.98);
const CARD_EDGE: Color = Color::srgb(0.55, 0.43, 0.18);
const NEW_GREEN: Color = Color::srgb(0.35, 1.0, 0.35);
const UPGRADE_BLUE: Color = Color::srgb(0.45, 0.75, 1.0);
const TEXT: Color = Color::srgb(0.93, 0.89, 0.78);
const MUTED: Color = Color::srgb(0.6, 0.6, 0.6);
/// The rarities' colours, WoW's item quality colours: common, uncommon, rare, epic, legendary.
const RARITY: [Color; 5] = [
    Color::srgb(0.85, 0.85, 0.85),
    Color::srgb(0.12, 1.0, 0.0),
    Color::srgb(0.0, 0.44, 0.87),
    Color::srgb(0.64, 0.21, 0.93),
    Color::srgb(1.0, 0.5, 0.0),
];
const RARITY_NAMES: [&str; 5] = ["COMMON", "UNCOMMON", "RARE", "EPIC", "LEGENDARY"];

/// The card's colour for its rarity; past legendary, the collection chests' gold.
fn rarity_color(rarity: u8) -> Color {
    RARITY.get(usize::from(rarity)).copied().unwrap_or(GOLD)
}

/// A card's background: the dark card tinted toward its rarity, the more for a rarer one.
fn card_bg(rarity: u8, lit: bool) -> Color {
    let base = if lit { CARD_BG_HOVER } else { CARD_BG }.to_srgba();
    let tint = rarity_color(rarity).to_srgba();
    let k = [0.0, 0.08, 0.12, 0.16, 0.2][usize::from(rarity.min(4))];
    Color::srgba(
        base.red + (tint.red - base.red) * k,
        base.green + (tint.green - base.green) * k,
        base.blue + (tint.blue - base.blue) * k,
        base.alpha,
    )
}

/// What the screen is showing, and the data it draws from, read once on the first level-up.
#[derive(Resource, Default)]
struct Screen {
    /// The offer on screen, the window height it was laid out for, and its root node.
    shown: Option<(u64, f32, Entity)>,
    /// The offer already answered: it stays hidden while the server takes the answer.
    answered: Option<u64>,
    /// The offer last put on screen, and when (real seconds): cards take no answer for
    /// [`CARD_GRACE`] after they appear, so a click meant for the world is not taken as a pick.
    appeared: (u64, f32),
    library: Option<Mutex<Library>>,
    /// The library failed to load; said once, then the screen stays off.
    broken: bool,
}

impl Screen {
    /// An icon from the game data, once the library has loaded.
    fn icon(&self, path: &str) -> Option<Image> {
        let library = self.library.as_ref()?;
        let library = library.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        library.blp(path)
    }

    /// Whether the library has loaded and knows `spell`: its icon can be had.
    fn spell_icon_ready(&self, spell: u32) -> Option<()> {
        let library = self.library.as_ref()?;
        let library = library.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        library.spells.get(spell).map(|_| ())
    }

    /// A spell's icon, once the library has loaded.
    fn spell_icon(&self, spell: u32) -> Option<Image> {
        let library = self.library.as_ref()?;
        let library = library.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let display = library.spells.get(spell)?;
        library.icon(display)
    }

    /// The game font, once the library has loaded.
    fn font(&self) -> Option<Handle<Font>> {
        let library = self.library.as_ref()?;
        let library = library.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        Some(library.font.clone())
    }
}

/// The cards on screen, in order, for the keys and the clicks.
#[derive(Component)]
struct CardButton {
    offer: u64,
    /// Its place on screen, 0-2: the key that takes it, and what the server is told.
    index: usize,
    rarity: u8,
}

#[derive(Component)]
struct ScreenRoot;

/// The install's spell tables and the game font.
struct Library {
    chain: Chain,
    spells: SpellCatalog,
    durations: SpellDurationCatalog,
    radii: SpellRadiusCatalog,
    ranges: SpellRangeCatalog,
    /// `GlobalStrings.lua`'s templates, which fill a tooltip's numbers ("%d to %d", "%d sec").
    globals: HashMap<String, String>,
    font: Handle<Font>,
    /// WoW's title face (`MORPHEUS.TTF`), for the quest window's headings.
    title_font: Handle<Font>,
    /// Items' looks, for a chest card's icon.
    item_displays: Option<benilla_formats::ItemDisplayCatalog>,
}

impl Library {
    fn load(fonts: &mut Assets<Font>) -> Result<Self> {
        let data = benilla_formats::wow_data().context("no WoW Data folder found")?;
        let mut chain = Chain::open(&data)?;
        let spells = load_spell_catalog(&mut chain)?;
        let durations = load_spell_durations(&mut chain)?;
        let radii = load_spell_radii(&mut chain)?;
        let ranges = load_spell_ranges(&mut chain)?;
        let globals = global_strings(&String::from_utf8_lossy(
            &chain.read("Interface\\FrameXML\\GlobalStrings.lua")?,
        ));
        let font = Font::try_from_bytes(chain.read("Fonts\\FRIZQT__.TTF")?)
            .map_err(|e| anyhow::anyhow!("FRIZQT__.TTF: {e}"))?;
        let item_displays = benilla_formats::load_item_display_catalog(&mut chain).ok();
        let title_font = Font::try_from_bytes(chain.read("Fonts\\MORPHEUS.TTF")?)
            .map_err(|e| anyhow::anyhow!("MORPHEUS.TTF: {e}"))?;
        Ok(Self {
            chain,
            spells,
            durations,
            radii,
            ranges,
            globals,
            font: fonts.add(font),
            title_font: fonts.add(title_font),
            item_displays,
        })
    }

    /// The spell's tooltip text with its `$` numbers filled in.
    fn description(&self, spell: &SpellDisplay) -> String {
        let Some(text) = spell.description.as_deref().filter(|t| !t.is_empty()) else {
            return String::new();
        };
        let skill = |_: u32| 0;
        let lookup = |id: u32| self.spells.get(id);
        let gender = || 0u8;
        let home_area = || None;
        let global = |key: &str| self.globals.get(key).cloned();
        let printf = |t: &str, n: &[TokenNumber]| printf(t, n);
        let ctx = TokenContext {
            durations: &self.durations,
            radii: &self.radii,
            ranges: Some(&self.ranges),
            skill: &skill,
            lookup: &lookup,
            mods: None,
            unmodified_points: false,
            gender: &gender,
            home_area: &home_area,
            global: &global,
            printf: &printf,
        };
        substitute(text, spell, &ctx)
    }

    /// The spell's icon as an image, or `None` when it has none or it does not decode.
    fn icon(&self, spell: &SpellDisplay) -> Option<Image> {
        self.blp(spell.icon.as_deref()?)
    }

    /// An item's inventory icon.
    fn item_icon(&self, display: u32) -> Option<Image> {
        let icon = self.item_displays.as_ref()?.get(display)?.icon.clone()?;
        self.blp(&icon)
    }

    /// An interface texture (`path` without `.blp`) as an image.
    fn blp(&self, path: &str) -> Option<Image> {
        let bytes = self.chain.read(&format!("{path}.blp")).ok()?;
        let (width, height, pixels) = benilla_formats::blp_to_rgba(&bytes).ok()?;
        Some(Image::new(
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            pixels,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        ))
    }
}

/// `KEY = "value";` lines of `GlobalStrings.lua`.
fn global_strings(lua: &str) -> HashMap<String, String> {
    lua.lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            let value = value.trim().trim_end_matches(';').trim();
            let value = value.strip_prefix('"')?.strip_suffix('"')?;
            Some((key.trim().to_owned(), value.replace("\\\"", "\"")))
        })
        .collect()
}

/// A `GlobalStrings` template's `%` holes filled in order: `%d`, `%.Nf` and `%%`.
fn printf(template: &str, numbers: &[TokenNumber]) -> String {
    let mut out = String::new();
    let mut numbers = numbers.iter();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        // Flags and width are skipped; the precision is kept for floats.
        let mut spec = String::new();
        let mut precision = String::new();
        let mut after_dot = false;
        while let Some(&n) = chars.peek() {
            chars.next();
            if n.is_ascii_alphabetic() || n == '%' {
                spec.push(n);
                break;
            }
            if n == '.' {
                after_dot = true;
            } else if after_dot && n.is_ascii_digit() {
                precision.push(n);
            }
        }
        if spec == "%" {
            out.push('%');
            continue;
        }
        let places = precision.parse().unwrap_or(6);
        match numbers.next() {
            Some(TokenNumber::Int(i)) => out.push_str(&i.to_string()),
            Some(TokenNumber::Float(f)) => out.push_str(&format!("{f:.places$}")),
            None => {}
        }
    }
    out
}

/// The number in "Rank N".
fn rank_number(spell: &SpellDisplay) -> Option<u32> {
    spell.rank.as_deref()?.strip_prefix("Rank ")?.trim().parse().ok()
}

/// On a computer the cards are drawn smaller: everything in the screen but its dim.
fn shrink_cards(mut commands: Commands, screens: Query<&Children, Added<ScreenRoot>>) {
    if SHRINK == 1.0 {
        return;
    }
    for children in &screens {
        for &c in children {
            commands.entity(c).insert(UiTransform::from_scale(Vec2::splat(SHRINK)));
        }
    }
}

/// Tool, not game: every talent of the Forsaken classes with its tooltip text, to `out/talents.json`,
/// and each icon's pixels to `out/icons/<name>.rgba` (`width height` then RGBA bytes), for the
/// talent-tagging page (`cargo run -p survivor-ui --example talents -- <out>`).
#[doc(hidden)]
pub fn export_talents(out: &std::path::Path) -> Result<()> {
    use std::fmt::Write as _;
    let mut fonts = Assets::<Font>::default();
    let mut library = Library::load(&mut fonts)?;
    let talents = benilla_formats::load_talent_catalog(&mut library.chain)?;
    std::fs::create_dir_all(out.join("icons"))?;
    let esc = |s: &str| {
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "")
    };
    let classes = [(1u8, "Warrior"), (4, "Rogue"), (5, "Priest"), (8, "Mage"), (9, "Warlock")];
    let mut j = String::from("[");
    let mut first_tree = true;
    for (class, class_name) in classes {
        let mut tabs = talents.tabs_for_class(survivor_server::UNDEAD, class);
        tabs.sort_by_key(|t| t.id);
        for tab in tabs {
            if !first_tree {
                j.push(',');
            }
            first_tree = false;
            write!(j, "{{\"class\":\"{class_name}\",\"tree\":\"{}\",\"talents\":[", esc(&tab.name))?;
            let mut list: Vec<_> = talents.talents_in_tab(tab.id).to_vec();
            list.sort_by_key(|t| (t.row, t.col));
            for (k, t) in list.iter().enumerate() {
                let max = t.max_rank().max(1) as usize;
                let lo = library.spells.get(t.ranks[0]);
                let hi = library.spells.get(t.ranks[max - 1]);
                let name = lo.map_or_else(|| format!("Talent {}", t.id), |s| s.name.clone());
                let d1 = lo.map(|s| library.description(s)).unwrap_or_default();
                let dn = hi.map(|s| library.description(s)).unwrap_or_default();
                let icon = lo.and_then(|s| s.icon.clone()).unwrap_or_default();
                let icon_key = icon.rsplit('\\').next().unwrap_or("").to_ascii_lowercase();
                if !icon_key.is_empty() && !out.join("icons").join(format!("{icon_key}.rgba")).exists() {
                    if let Ok(bytes) = library.chain.read(&format!("{icon}.blp")) {
                        if let Ok((w, h, px)) = benilla_formats::blp_to_rgba(&bytes) {
                            let mut f = format!("{w} {h}\n").into_bytes();
                            f.extend_from_slice(&px);
                            std::fs::write(out.join("icons").join(format!("{icon_key}.rgba")), f)?;
                        }
                    }
                }
                let prereq = list
                    .iter()
                    .find(|p| p.id == t.prereq_talent && t.prereq_talent != 0)
                    .and_then(|p| library.spells.get(p.ranks[0]))
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                if k > 0 {
                    j.push(',');
                }
                write!(
                    j,
                    "{{\"id\":{},\"name\":\"{}\",\"row\":{},\"col\":{},\"ranks\":{max},\"rank1\":\"{}\",\"rankMax\":\"{}\",\"icon\":\"{icon_key}\",\"requires\":\"{}\"}}",
                    t.id,
                    esc(&name),
                    t.row,
                    t.col,
                    esc(&d1),
                    esc(&dn),
                    esc(&prereq)
                )?;
            }
            j.push_str("]}");
        }
    }
    j.push(']');
    std::fs::write(out.join("talents.json"), j)?;
    Ok(())
}

/// Read the cards' game data (spells, icons, the font) once the player is in the world, so the
/// first level-up's cards come up without the wait.
fn preload_library(
    objects: benilla_app::Objects,
    mut screen: ResMut<Screen>,
    mut fonts: ResMut<Assets<Font>>,
) {
    if screen.library.is_some() || screen.broken || objects.player().is_none() {
        return;
    }
    match Library::load(&mut fonts) {
        Ok(l) => screen.library = Some(Mutex::new(l)),
        Err(e) => {
            error!("survivor-ui: the level-up screen cannot read the game data: {e:#}");
            screen.broken = true;
        }
    }
}

/// Put the server's offer on screen, or take it off when there is none.
fn show_offer(
    mut commands: Commands,
    mut screen: ResMut<Screen>,
    mut fonts: ResMut<Assets<Font>>,
    mut images: ResMut<Assets<Image>>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    time: Res<Time<Real>>,
) {
    let offer = picks::offer().filter(|o| screen.answered != Some(o.id));
    if let Some(o) = &offer {
        if screen.appeared.0 != o.id {
            screen.appeared = (o.id, time.elapsed_secs());
        }
    }
    // Text sizes are pixels, so a resized window lays the screen out again.
    let height = window.single().map_or(720.0, Window::height);
    let wanted = offer.as_ref().map(|o| (o.id, height));
    if wanted == screen.shown.map(|s| (s.0, s.1)) {
        return;
    }
    if let Some((_, _, root)) = screen.shown.take() {
        commands.entity(root).despawn();
    }
    let Some(offer) = offer else { return };
    if screen.library.is_none() && !screen.broken {
        match Library::load(&mut fonts) {
            Ok(l) => screen.library = Some(Mutex::new(l)),
            Err(e) => {
                error!("survivor-ui: the level-up screen cannot read the game data: {e:#}");
                screen.broken = true;
            }
        }
    }
    let Some(library) = &screen.library else { return };
    let library = library.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let vh = height / 100.0;
    let root = spawn_screen(&mut commands, &library, &mut images, &offer, vh);
    drop(library);
    screen.shown = Some((offer.id, height, root));
}

fn text(font: &Handle<Font>, s: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(s),
        TextFont {
            font: font.clone(),
            font_size: size,
            ..default()
        },
        TextColor(color),
        TextLayout::new_with_justify(Justify::Center),
    )
}

/// The dimmed backdrop, the title and the cards.
fn spawn_screen(
    commands: &mut Commands,
    library: &Library,
    images: &mut Assets<Image>,
    offer: &Offer,
    vh: f32,
) -> Entity {
    let font = &library.font;
    commands
        .spawn((
            ScreenRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: Val::Vh(1.5),
                // Raised off centre: the cast order strip shows below, to be changed meanwhile.
                padding: UiRect::bottom(Val::Vh(20.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.62)),
            GlobalZIndex(SCREEN_Z),
        ))
        .with_children(|root| {
            root.spawn(text(
                font,
                offer.title.clone().unwrap_or_else(|| {
                    format!("LEVEL UP!  {} -> {}", offer.level.saturating_sub(1), offer.level)
                }),
                9.0 * vh,
                GOLD,
            ));
            root.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Vh(3.0),
                margin: UiRect::top(Val::Vh(2.0)),
                ..default()
            })
            .with_children(|row| {
                for (i, card) in offer.cards.iter().enumerate() {
                    // Each card, with its banish under it while the run has banishes left.
                    row.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        row_gap: Val::Vh(1.2),
                        ..default()
                    })
                    .with_children(|col| {
                        let compact = offer.banishes > 0 || offer.rerolls > 0;
                        spawn_card(col, library, images, offer.id, i, card, vh, compact);
                        if offer.banishes > 0 && offer.title.is_none() {
                            col.spawn(small_button(offer.id, picks::BANISH + i)).with_children(|b| {
                                b.spawn(text(font, format!("Banish   [ Shift {} ]", i + 1), 2.8 * vh, GOLD));
                            });
                        }
                        // An item: kept for the collection instead of worn.
                        if card.item.is_some() {
                            col.spawn(small_button(offer.id, picks::KEEP + i)).with_children(|b| {
                                b.spawn(text(font, "Keep, don't wear", 2.8 * vh, GOLD));
                            });
                        }
                    });
                }
            });
            // The run's rerolls (the cards dealt again) and how many banishes are left.
            if (offer.rerolls > 0 || offer.banishes > 0) && offer.title.is_none() {
                root.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Vh(3.0),
                    ..default()
                })
                .with_children(|line| {
                    if offer.rerolls > 0 {
                        line.spawn(small_button(offer.id, picks::REROLL)).with_children(|b| {
                            b.spawn(text(
                                font,
                                format!("Reroll ({} left)   [ T ]", offer.rerolls),
                                3.2 * vh,
                                GOLD,
                            ));
                        });
                    }
                    if offer.banishes > 0 {
                        line.spawn(text(
                            font,
                            format!("{} banishes left: a banished skill never comes again this run", offer.banishes),
                            2.4 * vh,
                            Color::srgb(0.75, 0.72, 0.65),
                        ));
                    }
                });
            }
            // A chest's way out: none of its things, a part of its price back.
            if let Some(words) = &offer.recycle {
                root.spawn((
                    CardButton {
                        offer: offer.id,
                        index: offer.cards.len(),
                        rarity: 0,
                    },
                    Button,
                    Node {
                        margin: UiRect::top(Val::Vh(2.0)),
                        padding: UiRect::axes(Val::Vh(3.0), Val::Vh(1.2)),
                        border: UiRect::all(Val::Vh(0.3)),
                        border_radius: BorderRadius::all(Val::Vh(1.0)),
                        ..default()
                    },
                    BackgroundColor(card_bg(0, false)),
                    BorderColor::all(CARD_EDGE),
                ))
                .with_children(|b| {
                    b.spawn(text(font, format!("{words}   [ R ]"), 3.6 * vh, GOLD));
                });
            }
        })
        .id()
}

/// A small button under the cards answering `index`.
fn small_button(offer: u64, index: usize) -> impl Bundle {
    (
        CardButton {
            offer,
            index,
            rarity: 0,
        },
        Button,
        Node {
            margin: UiRect::top(Val::Vh(1.0)),
            padding: UiRect::axes(Val::Vh(2.5), Val::Vh(0.9)),
            border: UiRect::all(Val::Vh(0.25)),
            border_radius: BorderRadius::all(Val::Vh(1.0)),
            ..default()
        },
        BackgroundColor(card_bg(0, false)),
        BorderColor::all(CARD_EDGE),
    )
}

fn spawn_card(
    row: &mut ChildSpawnerCommands,
    library: &Library,
    images: &mut Assets<Image>,
    offer: u64,
    index: usize,
    card: &Card,
    vh: f32,
    compact: bool,
) {
    let font = &library.font;
    let spell = library.spells.get(card.spell);
    let mut name = spell.map_or_else(|| format!("Spell {}", card.spell), |s| s.name.clone());
    let edge = if card.rarity == 0 { CARD_EDGE } else { rarity_color(card.rarity) };
    let (tag, tag_color) = match (card.replaces, spell.and_then(rank_number)) {
        _ if !card.boost.is_empty() => (
            format!("{} UPGRADE", RARITY_NAMES[usize::from(card.rarity.min(4))]),
            rarity_color(card.rarity),
        ),
        (Some(_), Some(rank)) => (format!("UPGRADE  Rank {} > {}", rank - 1, rank), UPGRADE_BLUE),
        (Some(_), None) => ("UPGRADE".to_owned(), UPGRADE_BLUE),
        (None, _) => ("NEW!".to_owned(), NEW_GREEN),
    };
    let mut kind = if card.defensive { "Defensive" } else { "Damage" }.to_owned();
    let mut description = spell.map(|s| library.description(s)).unwrap_or_default();
    let mut icon = spell.and_then(|s| library.icon(s)).map(|i| images.add(i));
    let mut tag = tag;
    let mut tag_color = tag_color;
    let mut lines = card.boost.clone();
    // A chest's item: its own name, icon, slot and stats.
    if let Some(item) = &card.item {
        name = item.name.clone();
        icon = library.item_icon(item.display).map(|i| images.add(i));
        tag = if item.upgrade {
            "UPGRADE".to_owned()
        } else {
            RARITY_NAMES[usize::from(card.rarity.min(4))].to_owned()
        };
        tag_color = rarity_color(card.rarity);
        kind = item.slot.clone();
        description = String::new();
        lines = item.lines.clone();
    }
    row.spawn((
        CardButton {
            offer,
            index,
            rarity: card.rarity,
        },
        Button,
        Node {
            // Half the height wide and 70% of it tall; a narrow window caps the width so the
            // three still fit side by side.
            width: Val::Vh(52.0),
            max_width: Val::Vw(31.0),
            min_height: Val::Vh(if compact { 60.0 } else { 70.0 }),
            // As tall as the tallest beside it, so the banishes under them line up.
            flex_grow: 1.0,
            padding: UiRect::all(Val::Vh(2.5)),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Vh(1.0),
            border: UiRect::all(Val::Vh(0.4)),
            border_radius: BorderRadius::all(Val::Vh(1.4)),
            ..default()
        },
        BackgroundColor(card_bg(card.rarity, false)),
        BorderColor::all(edge),
    ))
    .with_children(|c| {
        c.spawn(Node {
            width: Val::Vh(15.0),
            height: Val::Vh(15.0),
            margin: UiRect::vertical(Val::Vh(1.5)),
            border: UiRect::all(Val::Vh(0.3)),
            ..default()
        })
        .insert(BorderColor::all(edge))
        .with_children(|frame| {
            if let Some(icon) = icon {
                frame.spawn((
                    ImageNode::new(icon),
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                ));
            }
        });
        let name_color = if card.rarity == 0 { Color::WHITE } else { rarity_color(card.rarity) };
        c.spawn(text(font, name, 4.0 * vh, name_color));
        c.spawn(text(font, tag, 2.5 * vh, tag_color));
        // An upgrade's lines, big, in its colour: what the card does.
        for line in &lines {
            c.spawn(text(font, line.clone(), 3.0 * vh, rarity_color(card.rarity)));
        }
        c.spawn(text(font, kind, 2.2 * vh, MUTED));
        c.spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Vh(0.15),
                margin: UiRect::vertical(Val::Vh(1.0)),
                ..default()
            },
            BackgroundColor(CARD_EDGE),
        ));
        c.spawn((
            text(font, description, 2.7 * vh, TEXT),
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                ..default()
            },
        ));
        c.spawn(text(font, format!("[ {} ]", index + 1), 3.2 * vh, GOLD));
    });
}

/// How long new cards stand on screen before they take an answer (s).
const CARD_GRACE: f32 = 0.7;

/// A click on a card, or its number key, answers the offer, once the cards have stood a moment.
fn take_card(
    mut screen: ResMut<Screen>,
    keys: Res<ButtonInput<KeyCode>>,
    cards: Query<(&CardButton, &Interaction)>,
    time: Res<Time<Real>>,
) {
    if screen.shown.is_none() || time.elapsed_secs() - screen.appeared.1 < CARD_GRACE {
        return;
    }
    const KEYS: [[KeyCode; 2]; 3] = [
        [KeyCode::Digit1, KeyCode::Numpad1],
        [KeyCode::Digit2, KeyCode::Numpad2],
        [KeyCode::Digit3, KeyCode::Numpad3],
    ];
    // R is the recycle button's, placed after the cards; T rerolls; Shift with a number banishes
    // that card.
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let number = KEYS.iter().position(|k| keys.any_just_pressed(*k));
    let pressed_key = match number {
        Some(n) if shift => Some(picks::BANISH + n),
        Some(n) => Some(n),
        None => None,
    }
    .or_else(|| {
        let recycle = picks::offer().and_then(|o| o.recycle.map(|_| o.cards.len()));
        recycle.filter(|_| keys.just_pressed(KeyCode::KeyR))
    })
    .or_else(|| keys.just_pressed(KeyCode::KeyT).then_some(picks::REROLL));
    // By each card's own index: a query's order is not the order on screen.
    let picked = cards
        .iter()
        .find(|(card, interaction)| {
            **interaction == Interaction::Pressed || pressed_key == Some(card.index)
        })
        .map(|(card, _)| (card.offer, card.index));
    if let Some((offer, index)) = picked {
        info!("survivor-ui: took card {index} from offer {offer}");
        picks::choose(offer, index);
        screen.answered = Some(offer);
    }
}

/// The card under the cursor lights up.
fn hover_cards(
    mut cards: Query<
        (&CardButton, &Interaction, &mut BackgroundColor, &mut BorderColor),
        Changed<Interaction>,
    >,
) {
    for (card, interaction, mut bg, mut edge) in &mut cards {
        let lit = *interaction != Interaction::None;
        bg.0 = card_bg(card.rarity, lit);
        let rest = if card.rarity == 0 { CARD_EDGE } else { rarity_color(card.rarity) };
        *edge = BorderColor::all(if lit { GOLD } else { rest });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printf_fills_holes_in_order() {
        assert_eq!(
            printf("%d sec, %.1f yd, 100%%", &[TokenNumber::Int(30), TokenNumber::Float(2.5)]),
            "30 sec, 2.5 yd, 100%"
        );
    }

    #[test]
    fn global_strings_read_key_and_value() {
        let g = global_strings(
            "INT_SPELL_POINTS_SPREAD_TEMPLATE = \"%d to %d\";\nSAY = \"say \\\"hi\\\"\";",
        );
        assert_eq!(g["INT_SPELL_POINTS_SPREAD_TEMPLATE"], "%d to %d");
        assert_eq!(g["SAY"], "say \"hi\"");
    }
}
