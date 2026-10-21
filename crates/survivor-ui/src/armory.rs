//! The collection chest's window: a tab for every equipment slot, and in it every item the
//! player owns for that slot (favourites first, then what is worn, then the rarest), each with
//! its upgrades. Items only seen in chests stand in shadow below. A tap wears an item, or takes
//! it off; the star marks a favourite. The server holds it all ([`survivor_server::armory`]).

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
const PANEL_Z: i32 = crate::SCREEN_Z - 12;
const WHEEL_LINE: f32 = 40.0;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (show_armory, press, scroll_list, block_touch.after(crate::quest_panel::block_touch)),
    );
}

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
    Favourite(u64),
    Close,
}

#[derive(Default)]
struct Shown {
    root: Option<Entity>,
    key: Option<(u64, u16, u32)>,
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
    let key = (a.id, slot, height as u32);
    if shown.key == Some(key) {
        return;
    }
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
    shown.root = Some(spawn(&mut commands, &a, slot, &parchment, star, &icons, &fonts, vh));
    shown.key = Some(key);
}

#[allow(clippy::too_many_arguments)]
fn spawn(
    commands: &mut Commands,
    a: &Armory,
    slot: u16,
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
                            ScrollPosition::default(),
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
                                spawn_row(list, r, icon.clone(), star.clone(), font, vh);
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
    let mut row = list.spawn((
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: Val::Vh(1.5),
            padding: UiRect::all(Val::Vh(0.8)),
            border_radius: BorderRadius::all(Val::Vh(0.8)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(if r.worn { ROW_WORN } else { ROW }),
    ));
    if !r.seen_only {
        row.insert((Press::Item(r.key), Button));
    }
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
                r.lines.join("   ")
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
}

fn press(
    buttons: Query<(&Press, &Interaction), Changed<Interaction>>,
    root: Query<&ArmoryId>,
) {
    let Ok(id) = root.single() else { return };
    for (p, i) in &buttons {
        if *i != Interaction::Pressed {
            continue;
        }
        match *p {
            Press::Item(key) => armory::act(id.0, Action::Toggle(key)),
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
