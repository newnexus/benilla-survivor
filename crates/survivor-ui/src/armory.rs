//! The collection chest's window: a tab for every equipment slot, and in it every item the
//! player owns for that slot (favourites first, then what is worn, then the rarest), each with
//! its upgrades. Items only seen in chests stand in shadow below. A tap on an item unfolds it:
//! its survivor stats on one line, its armour and own bonuses on the next, and a button to wear
//! it or take it off. The star marks a favourite. The pointer over an icon shows the item's
//! tooltip as the character window draws it (the game's own tooltip lies under these windows).
//! The server holds it all ([`survivor_server::armory`]).

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::input::touch::Touches;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use survivor_server::armory::{self, Action, Armory, Row};

const INK: Color = Color::srgb(0.2, 0.12, 0.04);
const HEADING: Color = Color::srgb(0.1, 0.05, 0.0);
const SHADOW: Color = Color::srgb(0.35, 0.3, 0.25);
const TAB: Color = Color::srgba(0.25, 0.15, 0.06, 0.25);
const TAB_ON: Color = Color::srgba(0.45, 0.25, 0.05, 0.6);
const ROW: Color = Color::srgba(0.3, 0.2, 0.08, 0.12);
const ROW_WORN: Color = Color::srgba(0.75, 0.55, 0.1, 0.35);
const BUTTON_RED: Color = Color::srgb(0.5, 0.04, 0.02);
const BUTTON_EDGE: Color = Color::srgb(0.75, 0.6, 0.25);
/// The survivor stats' ink on parchment: a deep gold.
const OWN_INK: Color = Color::srgb(0.45, 0.28, 0.0);
const PANEL_Z: i32 = crate::SCREEN_Z - 12;
const WHEEL_LINE: f32 = 40.0;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (
            show_armory,
            press,
            scroll_list,
            item_tooltip,
            block_touch.after(crate::quest_panel::block_touch),
        ),
    )
    .init_resource::<Unfolded>();
}

/// An item's icon in the list, with the item, for its tooltip.
#[derive(Component)]
struct ItemIcon(Row);

/// The tooltip on screen.
#[derive(Component)]
struct ItemTooltip;

/// The game's tooltip look: its backdrop, its edge, and the white of its lines.
const TIP_BACK: Color = Color::srgba(0.035, 0.035, 0.075, 0.92);
const TIP_EDGE: Color = Color::srgb(0.75, 0.75, 0.75);
const TIP_TEXT: Color = Color::WHITE;
/// The tooltip's name colours, by rarity: the game's item quality colours, common white.
const TIP_NAME: [Color; 5] = [
    Color::WHITE,
    Color::srgb(0.12, 1.0, 0.0),
    Color::srgb(0.0, 0.44, 0.87),
    Color::srgb(0.64, 0.21, 0.93),
    Color::srgb(1.0, 0.5, 0.0),
];

/// The item unfolded in the list, by its key.
#[derive(Resource, Default)]
struct Unfolded(Option<u64>);

#[derive(Component)]
struct ArmoryRoot;

#[derive(Component)]
struct ArmoryList;

#[derive(Component)]
struct ArmorySheet;

#[derive(Component, Clone, Copy)]
enum Press {
    Tab(u16),
    Item(u64),
    Wear(u64),
    Favourite(u64),
    Close,
}

#[derive(Default)]
struct Shown {
    root: Option<Entity>,
    key: Option<(u64, u16, u32, Option<u64>)>,
    parchment: Option<Handle<Image>>,
    star: Option<Handle<Image>>,
}

/// The tab the player looks at: kept while the window is posted again after a tap.
#[derive(Resource, Default)]
struct Selected(Option<u16>);

fn show_armory(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    mut images: ResMut<Assets<Image>>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut selected: Local<Selected>,
    mut pressed_tab: Local<Option<u16>>,
    tabs: Query<(&Press, &Interaction), Changed<Interaction>>,
    unfolded: Res<Unfolded>,
    lists: Query<&ScrollPosition, With<ArmoryList>>,
    mut shown: Local<Shown>,
) {
    for (p, i) in &tabs {
        if let (Press::Tab(slot), Interaction::Pressed) = (p, i) {
            *pressed_tab = Some(*slot);
        }
    }
    if let Some(slot) = pressed_tab.take() {
        selected.0 = Some(slot);
    }
    let armory = armory::current();
    let height = window.single().map_or(720.0, Window::height);
    let Some(a) = armory else {
        if let Some(root) = shown.root.take() {
            commands.entity(root).despawn();
        }
        shown.key = None;
        return;
    };
    // The first tab with something in it, unless one was chosen.
    let slot = selected
        .0
        .filter(|s| a.tabs.iter().any(|t| t.slot == *s))
        .or_else(|| a.tabs.iter().find(|t| !t.rows.is_empty()).map(|t| t.slot))
        .unwrap_or(0);
    selected.0 = Some(slot);
    let key = (a.id, slot, height as u32, unfolded.0);
    if shown.key == Some(key) {
        return;
    }
    // Unfolding an item or posting the tab again keeps the list where it was scrolled.
    let scroll = match shown.key {
        Some(k) if k.1 == slot => lists.iter().next().map_or(0.0, |p| p.0.y),
        _ => 0.0,
    };
    if let Some(root) = shown.root.take() {
        commands.entity(root).despawn();
    }
    let Some(library) = &screen.library else { return };
    let library = library.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let parchment = shown
        .parchment
        .get_or_insert_with(|| images.add(crate::quest_panel::parchment()))
        .clone();
    if shown.star.is_none() {
        shown.star = library
            .blp("Interface\\TargetingFrame\\UI-RaidTargetingIcons")
            .map(|i| images.add(i));
    }
    let star = shown.star.clone();
    let icons: Vec<(u32, Option<Handle<Image>>)> = a
        .tabs
        .iter()
        .find(|t| t.slot == slot)
        .map(|t| {
            t.rows
                .iter()
                .map(|r| (r.display, library.item_icon(r.display).map(|i| images.add(i))))
                .collect()
        })
        .unwrap_or_default();
    let fonts = (library.font.clone(), library.title_font.clone());
    drop(library);
    let vh = height / 100.0;
    let open = unfolded.0;
    shown.root =
        Some(spawn(&mut commands, &a, (slot, open, scroll), &parchment, star, &icons, &fonts, vh));
    shown.key = Some(key);
}

#[allow(clippy::too_many_arguments)]
fn spawn(
    commands: &mut Commands,
    a: &Armory,
    (slot, open, scroll): (u16, Option<u64>, f32),
    parchment: &Handle<Image>,
    star: Option<Handle<Image>>,
    icons: &[(u32, Option<Handle<Image>>)],
    (font, title_font): &(Handle<Font>, Handle<Font>),
    vh: f32,
) -> Entity {
    let text = |s: &str, f: &Handle<Font>, size: f32, color: Color| {
        (
            Text::new(s.to_owned()),
            TextFont {
                font: f.clone(),
                font_size: size * vh,
                ..default()
            },
            TextColor(color),
        )
    };
    let id = a.id;
    let rows: &[Row] = a.tabs.iter().find(|t| t.slot == slot).map_or(&[], |t| &t.rows);
    commands
        .spawn((
            ArmoryRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            GlobalZIndex(PANEL_Z),
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn((
                ArmorySheet,
                ImageNode::new(parchment.clone()).with_mode(NodeImageMode::Stretch),
                Node {
                    width: Val::Vh(150.0),
                    max_width: Val::Vw(94.0),
                    height: Val::Vh(86.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Vh(2.5)),
                    row_gap: Val::Vh(1.5),
                    ..default()
                },
            ))
            .with_children(|sheet| {
                // The heading and the X.
                sheet
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|h| {
                        h.spawn(text("Collection", title_font, 6.0, HEADING));
                        h.spawn((
                            Press::Close,
                            Button,
                            Node {
                                width: Val::Vh(7.0),
                                height: Val::Vh(7.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(Val::Vh(0.35)),
                                border_radius: BorderRadius::all(Val::Vh(1.0)),
                                ..default()
                            },
                            BackgroundColor(BUTTON_RED),
                            BorderColor::all(BUTTON_EDGE),
                        ))
                        .with_children(|b| {
                            b.spawn(text("X", font, 4.5, crate::GOLD));
                        });
                    });
                sheet
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        flex_grow: 1.0,
                        flex_basis: Val::Px(0.0),
                        column_gap: Val::Vh(2.0),
                        ..default()
                    })
                    .with_children(|body| {
                        // The slot tabs, down the left.
                        body.spawn(Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Vh(0.6),
                            width: Val::Vh(26.0),
                            flex_shrink: 0.0,
                            ..default()
                        })
                        .with_children(|tabs| {
                            for t in &a.tabs {
                                let owned = t.rows.iter().filter(|r| !r.seen_only).count();
                                let on = t.slot == slot;
                                tabs.spawn((
                                    Press::Tab(t.slot),
                                    Button,
                                    Node {
                                        padding: UiRect::axes(Val::Vh(1.4), Val::Vh(0.7)),
                                        justify_content: JustifyContent::SpaceBetween,
                                        border_radius: BorderRadius::all(Val::Vh(0.8)),
                                        ..default()
                                    },
                                    BackgroundColor(if on { TAB_ON } else { TAB }),
                                ))
                                .with_children(|b| {
                                    b.spawn(text(&t.name, font, 3.0, if on { HEADING } else { INK }));
                                    b.spawn(text(&owned.to_string(), font, 3.0, INK));
                                });
                            }
                        });
                        // The slot's items.
                        body.spawn((
                            ArmoryList,
                            RelativeCursorPosition::default(),
                            ScrollPosition(Vec2::new(0.0, scroll)),
                            Node {
                                flex_grow: 1.0,
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Vh(0.8),
                                overflow: Overflow::scroll_y(),
                                ..default()
                            },
                        ))
                        .with_children(|list| {
                            if rows.is_empty() {
                                list.spawn(text(
                                    "Nothing yet: chests out on a run hold items for this slot.",
                                    font,
                                    3.0,
                                    INK,
                                ));
                            }
                            for (r, (_, icon)) in rows.iter().zip(icons) {
                                let unfolded = !r.seen_only && open == Some(r.key);
                                spawn_row(list, r, unfolded, icon.clone(), star.clone(), font, vh);
                            }
                        });
                    });
            });
        })
        .insert(ArmoryId(id))
        .id()
}

#[derive(Component)]
struct ArmoryId(u64);

fn spawn_row(
    list: &mut ChildSpawnerCommands,
    r: &Row,
    unfolded: bool,
    icon: Option<Handle<Image>>,
    star: Option<Handle<Image>>,
    font: &Handle<Font>,
    vh: f32,
) {
    let text = |s: &str, size: f32, color: Color| {
        (
            Text::new(s.to_owned()),
            TextFont {
                font: font.clone(),
                font_size: size * vh,
                ..default()
            },
            TextColor(color),
        )
    };
    // On parchment: common in ink, the rarer in their colour, deepened a little to read.
    let name_color = match r.rarity {
        _ if r.seen_only => SHADOW,
        0 => INK,
        1 => Color::srgb(0.05, 0.55, 0.0),
        r => crate::rarity_color(r),
    };
    let mut item = list.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Vh(0.8)),
            row_gap: Val::Vh(0.8),
            border_radius: BorderRadius::all(Val::Vh(0.8)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(if r.worn { ROW_WORN } else { ROW }),
    ));
    if !r.seen_only {
        item.insert((Press::Item(r.key), Button));
    }
    item.with_children(|item| {
        let mut row = item.spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Vh(1.5),
            ..default()
        });
        row.with_children(|row| {
            // The icon, in shadow for an item only seen.
            let mut img = ImageNode::default();
            if let Some(icon) = icon {
                img = ImageNode::new(icon);
            }
            if r.seen_only {
                img.color = Color::srgb(0.15, 0.15, 0.15);
            }
            row.spawn((
                img,
                ItemIcon(r.clone()),
                RelativeCursorPosition::default(),
                Node {
                    width: Val::Vh(8.0),
                    height: Val::Vh(8.0),
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
            row.spawn(Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                ..default()
            })
            .with_children(|c| {
                let mut title = r.name.clone();
                if r.upgrades > 0 {
                    title.push_str(&format!("  +{}", r.upgrades));
                }
                c.spawn(text(&title, 3.4, name_color));
                let sub = if r.seen_only {
                    "Seen in a chest, never brought home".to_owned()
                } else {
                    r.base.join("   ")
                };
                c.spawn(text(&sub, 2.5, if r.seen_only { SHADOW } else { INK }));
            });
            if r.worn {
                row.spawn(text("WORN", 2.8, HEADING));
            }
            if !r.seen_only {
                let mut s = row.spawn((
                    Press::Favourite(r.key),
                    Button,
                    Node {
                        width: Val::Vh(6.0),
                        height: Val::Vh(6.0),
                        flex_shrink: 0.0,
                        ..default()
                    },
                ));
                // The raid-target star, a quarter of the first row of its texture.
                if let Some(star) = star {
                    s.insert(ImageNode {
                        image: star,
                        rect: Some(Rect::new(0.0, 0.0, 64.0, 64.0)),
                        color: if r.favourite {
                            Color::WHITE
                        } else {
                            Color::srgba(0.2, 0.2, 0.2, 0.45)
                        },
                        ..default()
                    });
                }
            }
        });
        if !unfolded {
            return;
        }
        // Unfolded: the survivor stats, then the item's own bonuses, each spaced out, under the
        // name; the wear button at the right.
        item.spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Vh(2.0),
            padding: UiRect::left(Val::Vh(9.5)),
            ..default()
        })
        .with_children(|more| {
            more.spawn(Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                row_gap: Val::Vh(0.6),
                ..default()
            })
            .with_children(|lines| {
                for (stats, ink) in [(&r.own, OWN_INK), (&r.bonus, INK)] {
                    lines
                        .spawn(Node {
                            flex_direction: FlexDirection::Row,
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: Val::Vh(4.0),
                            min_height: Val::Vh(3.4),
                            ..default()
                        })
                        .with_children(|l| {
                            if stats.is_empty() {
                                l.spawn(text("-", 2.9, SHADOW));
                            }
                            for s in stats {
                                l.spawn(text(s, 2.9, ink));
                            }
                        });
                }
            });
            more.spawn((
                Press::Wear(r.key),
                Button,
                Node {
                    padding: UiRect::axes(Val::Vh(2.0), Val::Vh(0.8)),
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(Val::Vh(0.3)),
                    border_radius: BorderRadius::all(Val::Vh(0.8)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(BUTTON_RED),
                BorderColor::all(BUTTON_EDGE),
            ))
            .with_children(|b| {
                b.spawn(text(if r.worn { "Take off" } else { "Wear" }, 3.0, crate::GOLD));
            });
        });
    });
}

/// The tooltip of the item whose icon is under the pointer, beside the pointer, as the game
/// draws one: the name in its rarity's colour, where it is worn and what it is, armour or damage,
/// its stats, its durability.
fn item_tooltip(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    icons: Query<(&ItemIcon, &RelativeCursorPosition)>,
    mut tips: Query<(Entity, &mut Node, &ComputedNode), With<ItemTooltip>>,
    mut shown: Local<Option<(u64, u32)>>,
) {
    let Ok(window) = window.single() else { return };
    let hovered = icons
        .iter()
        .find(|(i, c)| c.cursor_over && !i.0.seen_only)
        .map(|(i, _)| &i.0);
    let now = hovered.map(|r| (r.key, r.entry));
    if now != *shown {
        for (e, ..) in &tips {
            commands.entity(e).despawn();
        }
        *shown = now;
        if let (Some(r), Some(font)) = (hovered, screen.font()) {
            spawn_tooltip(&mut commands, r, font, window.height() / 100.0);
        }
        return;
    }
    // Beside the pointer, kept on the screen.
    let Some(cursor) = window.cursor_position() else { return };
    for (_, mut node, computed) in &mut tips {
        let size = computed.size() * computed.inverse_scale_factor();
        let x = (cursor.x + 18.0).min(window.width() - size.x - 4.0);
        let y = (cursor.y - size.y - 8.0).max(4.0);
        node.left = Val::Px(x);
        node.top = Val::Px(y);
    }
}

fn spawn_tooltip(commands: &mut Commands, r: &Row, font: Handle<Font>, vh: f32) {
    let text = |s: &str, size: f32, color: Color| {
        (
            Text::new(s.to_owned()),
            TextFont {
                font: font.clone(),
                font_size: size * vh,
                ..default()
            },
            TextColor(color),
        )
    };
    let pair = |left: &str, right: &str| (left.to_owned(), right.to_owned());
    let mut lines: Vec<(String, String)> = vec![pair(&r.slot_text, &r.kind)];
    match r.base.as_slice() {
        [damage, speed] => lines.push(pair(damage, speed)),
        [armor] => lines.push(pair(armor, "")),
        _ => {}
    }
    for s in r.own.iter().chain(&r.bonus) {
        lines.push(pair(s, ""));
    }
    if r.durability > 0 {
        lines.push(pair(&format!("Durability {0} / {0}", r.durability), ""));
    }
    let mut name = r.name.clone();
    if r.upgrades > 0 {
        name.push_str(&format!(" +{}", r.upgrades));
    }
    commands
        .spawn((
            ItemTooltip,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(-1000.0),
                top: Val::Px(-1000.0),
                flex_direction: FlexDirection::Column,
                min_width: Val::Vh(22.0),
                padding: UiRect::axes(Val::Vh(1.2), Val::Vh(0.9)),
                row_gap: Val::Vh(0.15),
                border: UiRect::all(Val::Vh(0.25)),
                border_radius: BorderRadius::all(Val::Vh(0.7)),
                ..default()
            },
            BackgroundColor(TIP_BACK),
            BorderColor::all(TIP_EDGE),
            GlobalZIndex(PANEL_Z + 20),
            Pickable::IGNORE,
        ))
        .with_children(|tip| {
            let colour = TIP_NAME[usize::from(r.rarity.min(4))];
            tip.spawn((text(&name, 2.6, colour), Pickable::IGNORE));
            for (left, right) in &lines {
                tip.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        justify_content: JustifyContent::SpaceBetween,
                        column_gap: Val::Vh(3.0),
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|l| {
                    l.spawn((text(left, 2.2, TIP_TEXT), Pickable::IGNORE));
                    if !right.is_empty() {
                        l.spawn((text(right, 2.2, TIP_TEXT), Pickable::IGNORE));
                    }
                });
            }
        });
}

fn press(
    buttons: Query<(&Press, &Interaction), Changed<Interaction>>,
    root: Query<&ArmoryId>,
    mut unfolded: ResMut<Unfolded>,
) {
    let Ok(id) = root.single() else { return };
    for (p, i) in &buttons {
        if *i != Interaction::Pressed {
            continue;
        }
        match *p {
            Press::Item(key) => {
                unfolded.0 = if unfolded.0 == Some(key) { None } else { Some(key) };
            }
            Press::Wear(key) => armory::act(id.0, Action::Toggle(key)),
            Press::Favourite(key) => armory::act(id.0, Action::Favourite(key)),
            Press::Close => armory::act(id.0, Action::Close),
            Press::Tab(_) => {}
        }
    }
}

/// The list scrolls with the wheel over it, and with a thumb dragged on it.
fn scroll_list(
    mut wheel: MessageReader<MouseWheel>,
    touches: Res<Touches>,
    mut lists: Query<
        (&RelativeCursorPosition, &mut ScrollPosition, &ComputedNode, &UiGlobalTransform),
        With<ArmoryList>,
    >,
) {
    let notches: f32 = wheel
        .read()
        .map(|w| match w.unit {
            MouseScrollUnit::Line => w.y * WHEEL_LINE,
            MouseScrollUnit::Pixel => w.y,
        })
        .sum();
    for (cursor, mut pos, node, at) in &mut lists {
        let mut dy = 0.0;
        if cursor.cursor_over {
            dy -= notches;
        }
        let k = node.inverse_scale_factor();
        for t in touches.iter() {
            if node.contains_point(*at, t.start_position() / k) {
                dy -= t.delta().y;
            }
        }
        if dy != 0.0 {
            let top = ((node.content_size.y - node.size.y) * k).max(0.0);
            pos.0.y = (pos.0.y + dy).clamp(0.0, top);
        }
    }
}

/// A thumb on the window moves no one.
fn block_touch(
    sheets: Query<(&ComputedNode, &UiGlobalTransform), With<ArmorySheet>>,
    mut frames: ResMut<benilla_app::SurvivorFrames>,
) {
    for (node, at) in &sheets {
        let k = node.inverse_scale_factor();
        frames
            .blockers
            .push(Rect::from_center_half_size(at.affine().translation * k, node.size * k / 2.0));
    }
}
