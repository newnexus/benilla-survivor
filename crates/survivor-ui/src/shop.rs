//! The innkeeper's window ([`survivor_server::shop`]), laid out like the game's merchant window
//! in the quest window's dark style: the town purse at the top, then the wares in two columns of
//! slots (icon, name, price beside a coin), a click on one buying one. Above the camp level a
//! ware's icon shows red with its level; a price the purse cannot pay is red. Below, what the
//! backpack holds, a click selling every one of it. Rebuilt whenever the server posts it again,
//! closed with its X.

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use survivor_server::shop::{self, Action, Ware, Window as Shop};

const WHITE: Color = Color::srgb(0.95, 0.93, 0.88);
const DIM: Color = Color::srgb(0.6, 0.58, 0.54);
/// A slot at rest and under the pointer; a price or level out of reach.
const SLOT: Color = Color::srgba(1.0, 1.0, 1.0, 0.04);
const SLOT_LIT: Color = Color::srgba(0.85, 0.65, 0.25, 0.22);
const SHORT: Color = Color::srgb(1.0, 0.25, 0.2);
const LOCKED: Color = Color::srgb(1.0, 0.35, 0.35);
/// What a food does, in the game's green for an item's use line.
const USE: Color = Color::srgb(0.12, 1.0, 0.0);
/// Pixels a wheel notch scrolls.
const WHEEL_LINE: f32 = 40.0;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, (show_window, show_keeper, press_buttons, scroll_list));
}

/// The shopkeeper's face in the window's top left corner, once the client has baked it.
#[derive(Component)]
struct KeeperPortrait;

/// While the window is open the keeper is the client's "npc" unit, so their portrait bakes.
fn show_keeper(
    portrait: Res<benilla_app::SurvivorPortrait>,
    mut nodes: Query<(&mut ImageNode, &mut Visibility), With<KeeperPortrait>>,
    mut talker: Local<u64>,
) {
    let keeper = shop::current().map_or(0, |w| w.keeper_guid);
    if keeper != *talker {
        // Only what this window set is taken back: the quest window sets its own giver.
        let now = benilla_app::SURVIVOR_TALKER.load(std::sync::atomic::Ordering::Relaxed);
        if keeper != 0 || now == *talker {
            benilla_app::SURVIVOR_TALKER.store(keeper, std::sync::atomic::Ordering::Relaxed);
        }
        *talker = keeper;
    }
    for (mut image, mut vis) in &mut nodes {
        match &portrait.0 {
            Some(h) => {
                if image.image != *h {
                    image.image = h.clone();
                }
                vis.set_if_neq(Visibility::Inherited);
            }
            None => {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
}

#[derive(Component)]
struct WindowRoot;

/// The wares' list, which scrolls.
#[derive(Component)]
struct ShopList;

/// A button's answer, for window `.0`.
#[derive(Component)]
struct Press(u64, Action);

/// A button's colours, at rest and under the pointer; none when it cannot be pressed.
#[derive(Component)]
struct Shade(Option<(Color, Color)>);

fn text(font: &Handle<Font>, s: &str, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(s.to_owned()),
        TextFont {
            font: font.clone(),
            font_size: size,
            ..default()
        },
        TextColor(color),
    )
}

fn money(c: u32) -> String {
    format!("{c} gold")
}

fn show_window(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    mut images: ResMut<Assets<Image>>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    roots: Query<Entity, With<WindowRoot>>,
    lists: Query<&ScrollPosition, With<ShopList>>,
    mut shown: Local<Option<(u64, f32)>>,
) {
    let current = shop::current();
    let height = window.single().map_or(720.0, Window::height);
    let wanted = current.as_ref().map(|w| (w.id, height));
    if wanted == *shown {
        return;
    }
    // A purchase posts the window again: the list stays where it was scrolled.
    let scroll = lists.iter().next().map_or(0.0, |p| p.0.y);
    for e in &roots {
        commands.entity(e).despawn();
    }
    *shown = wanted;
    let Some(w) = current else { return };
    let Some(font) = screen.font() else {
        *shown = None;
        return;
    };
    spawn_window(&mut commands, &screen, &mut images, &font, &w, height, scroll);
}

fn spawn_window(
    commands: &mut Commands,
    screen: &crate::Screen,
    images: &mut Assets<Image>,
    font: &Handle<Font>,
    w: &Shop,
    height: f32,
    scroll: f32,
) {
    let vh = height / 100.0 * crate::SHRINK;
    let s = crate::SHRINK;
    let id = w.id;
    let coin = screen.library.as_ref().and_then(|l| {
        let l = l.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let i = l.blp("Interface\\MoneyFrame\\UI-MoneyIcons")?;
        let w = i.size().as_vec2().x / 4.0;
        let h = i.size().as_vec2().y;
        Some((images.add(i), Rect::new(0.0, 0.0, w, h)))
    });
    let mut icon_of = |display: u32| -> Option<Handle<Image>> {
        let library = screen.library.as_ref()?;
        let library = library.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        library.item_icon(display).map(|i| images.add(i))
    };
    // One ware's slot, as the merchant window draws one: the icon (its count in the corner),
    // the name in its quality's colour, the price beside a gold coin. A click on it answers.
    let mut slot = |list: &mut ChildSpawnerCommands, ware: &Ware, action: Action, can: bool, locked: bool| {
        let icon = icon_of(ware.display);
        list.spawn((
            Press(id, action),
            Shade(can.then_some((SLOT, SLOT_LIT))),
            Button,
            Node {
                width: Val::Percent(49.0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Vh(1.2 * s),
                padding: UiRect::all(Val::Vh(0.8 * s)),
                border_radius: BorderRadius::all(Val::Vh(0.8 * s)),
                ..default()
            },
            BackgroundColor(SLOT),
        ))
        .with_children(|r| {
            let mut i = r.spawn((
                Node {
                    width: Val::Vh(7.0 * s),
                    height: Val::Vh(7.0 * s),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::FlexEnd,
                    align_items: AlignItems::FlexEnd,
                    border: UiRect::all(Val::Px(1.5)),
                    border_radius: BorderRadius::all(Val::Vh(0.5 * s)),
                    ..default()
                },
                BorderColor::all(crate::quest_panel::PAGE_EDGE),
                Pickable::IGNORE,
            ));
            if let Some(h) = icon {
                let mut img = ImageNode::new(h);
                if locked {
                    img.color = LOCKED;
                }
                i.insert(img);
            }
            if ware.count > 1 {
                i.with_children(|c| {
                    c.spawn((
                        text(font, &ware.count.to_string(), 2.4 * vh, WHITE),
                        TextShadow::default(),
                        Node {
                            margin: UiRect::all(Val::Vh(0.3 * s)),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ));
                });
            }
            r.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    row_gap: Val::Vh(0.4 * s),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|c| {
                let rarity = ware.quality.saturating_sub(1).min(4) as u8;
                let color = if rarity == 0 { WHITE } else { crate::rarity_color(rarity) };
                c.spawn((text(font, &ware.name, 2.8 * vh, color), Pickable::IGNORE));
                c.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Vh(0.6 * s),
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|p| {
                    let short = matches!(action, Action::Buy(_)) && !can && !locked;
                    let ink = if short { SHORT } else { WHITE };
                    p.spawn((text(font, &ware.price.to_string(), 2.6 * vh, ink), Pickable::IGNORE));
                    if let Some((h, rect)) = &coin {
                        p.spawn((
                            ImageNode {
                                image: h.clone(),
                                rect: Some(*rect),
                                ..default()
                            },
                            Node {
                                width: Val::Vh(2.4 * s),
                                height: Val::Vh(2.4 * s),
                                ..default()
                            },
                            Pickable::IGNORE,
                        ));
                    }
                    if locked {
                        p.spawn((
                            text(font, &format!("  camp level {}", ware.level), 2.3 * vh, LOCKED),
                            Pickable::IGNORE,
                        ));
                    }
                });
                if !ware.about.is_empty() {
                    c.spawn((text(font, &ware.about, 2.1 * vh, USE), Pickable::IGNORE));
                }
            });
        });
    };
    // The slots in two columns.
    let grid = Node {
        flex_direction: FlexDirection::Row,
        flex_wrap: FlexWrap::Wrap,
        justify_content: JustifyContent::SpaceBetween,
        row_gap: Val::Vh(0.8 * s),
        flex_shrink: 0.0,
        ..default()
    };
    commands
        .spawn((
            WindowRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                // On the left of the screen, as the quest giver's talk page.
                justify_content: JustifyContent::FlexStart,
                align_items: AlignItems::Center,
                padding: UiRect::left(Val::Vh(3.0)),
                ..default()
            },
            GlobalZIndex(crate::SCREEN_Z - 20),
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn((
                crate::quest_panel::Sheet,
                Node {
                    width: Val::Vh(90.0 * s),
                    height: Val::Vh(115.0 * s),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Vh(0.8 * s),
                    padding: UiRect::all(Val::Vh(2.5 * s)),
                    border: UiRect::all(Val::Px(1.5)),
                    border_radius: BorderRadius::all(Val::Vh(1.2 * s)),
                    ..default()
                },
                BackgroundColor(crate::quest_panel::PAGE),
                BorderColor::all(crate::quest_panel::PAGE_EDGE),
            ))
            .with_children(|page| {
                page.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    ..default()
                })
                .with_children(|h| {
                    // The keeper, top left: their face in a gold ring, their name beside it.
                    h.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Vh(1.5 * s),
                        ..default()
                    })
                    .with_children(|k| {
                        k.spawn((
                            Node {
                                width: Val::Vh(11.0 * s),
                                height: Val::Vh(11.0 * s),
                                flex_shrink: 0.0,
                                border: UiRect::all(Val::Vh(0.5 * s)),
                                border_radius: BorderRadius::all(Val::Percent(50.0)),
                                padding: UiRect::all(Val::Vh(0.25 * s)),
                                ..default()
                            },
                            BorderColor::all(Color::srgb(0.85, 0.65, 0.2)),
                            BackgroundColor(Color::srgb(0.02, 0.015, 0.01)),
                        ))
                        .with_children(|f| {
                            f.spawn((
                                KeeperPortrait,
                                ImageNode::default(),
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Percent(100.0),
                                    border_radius: BorderRadius::all(Val::Percent(50.0)),
                                    ..default()
                                },
                                Visibility::Hidden,
                            ));
                        });
                        k.spawn(text(font, &w.keeper, 4.6 * vh, crate::GOLD));
                    });
                    h.spawn(text(font, &money(w.money), 3.4 * vh, crate::GOLD));
                    h.spawn((
                        Press(id, Action::Close),
                        Shade(Some((
                            Color::srgb(0.45, 0.08, 0.06),
                            Color::srgb(0.65, 0.12, 0.08),
                        ))),
                        Button,
                        Node {
                            width: Val::Vh(5.5 * s),
                            height: Val::Vh(5.5 * s),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border_radius: BorderRadius::all(Val::Vh(0.8 * s)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.45, 0.08, 0.06)),
                    ))
                    .with_children(|b| {
                        b.spawn(text(font, "X", 3.4 * vh, crate::GOLD));
                    });
                });
                page.spawn(text(
                    font,
                    "Click a ware to buy one. Click a find to sell all of it.",
                    2.4 * vh,
                    DIM,
                ));
                page.spawn((
                    Node {
                        height: Val::Px(1.5),
                        width: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(crate::quest_panel::PAGE_EDGE),
                ));
                page.spawn((
                    ShopList,
                    RelativeCursorPosition::default(),
                    ScrollPosition(Vec2::new(0.0, scroll)),
                    Node {
                        flex_grow: 1.0,
                        flex_basis: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Vh(0.8 * s),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                ))
                .with_children(|list| {
                    list.spawn(text(font, "Food and drink", 3.6 * vh, crate::GOLD));
                    list.spawn(grid.clone()).with_children(|g| {
                        for ware in &w.buy {
                            let locked = ware.level > w.level;
                            let can = !locked && w.money >= ware.price;
                            // What is to buy shows no count: the merchant sells one at a time.
                            let one = Ware { count: 1, ..ware.clone() };
                            slot(g, &one, Action::Buy(ware.entry), can, locked);
                        }
                    });
                    list.spawn((
                        text(font, "Sell", 3.6 * vh, crate::GOLD),
                        Node {
                            margin: UiRect::top(Val::Vh(1.5 * s)),
                            ..default()
                        },
                    ));
                    if w.sell.is_empty() {
                        list.spawn(text(font, "Nothing in your backpack to sell.", 2.6 * vh, DIM));
                    }
                    list.spawn(grid).with_children(|g| {
                        for ware in &w.sell {
                            slot(g, ware, Action::Sell(ware.entry), true, false);
                        }
                    });
                });
            });
        });
}

fn press_buttons(
    mut buttons: Query<(&Interaction, &Press, &Shade, &mut BackgroundColor), Changed<Interaction>>,
) {
    for (i, press, shade, mut bg) in &mut buttons {
        let Some((rest, lit)) = shade.0 else { continue };
        match i {
            Interaction::Pressed => shop::act(press.0, press.1),
            Interaction::Hovered => bg.0 = lit,
            Interaction::None => bg.0 = rest,
        }
    }
}

/// The list scrolls with the wheel over it, and with a thumb dragged on it.
fn scroll_list(
    mut wheel: MessageReader<MouseWheel>,
    touches: Res<Touches>,
    mut lists: Query<
        (&RelativeCursorPosition, &mut ScrollPosition, &ComputedNode, &UiGlobalTransform),
        With<ShopList>,
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
