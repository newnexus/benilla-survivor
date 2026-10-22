//! The class trainer's window ([`survivor_server::trainer`]): the town purse at the top, then a
//! row per skill: [-] (the last rank back, its gold refunded), the icon, the name and the levels
//! its bought ranks come at on a run, the ranks bought, and [+] with the next rank's level and
//! price. Rebuilt whenever the server posts it again, closed with its X.

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use survivor_server::trainer::{self, Action, Window as Trainer};

const PAGE: Color = Color::srgba(0.035, 0.03, 0.025, 0.92);
const PAGE_EDGE: Color = Color::srgb(0.55, 0.45, 0.25);
const WHITE: Color = Color::srgb(0.95, 0.93, 0.88);
const DIM: Color = Color::srgb(0.6, 0.58, 0.54);
const DEFENSIVE: Color = Color::srgb(0.55, 0.75, 1.0);
const BUY: Color = Color::srgb(0.38, 0.28, 0.08);
const BUY_LIT: Color = Color::srgb(0.55, 0.4, 0.1);
const BUY_OFF: Color = Color::srgb(0.12, 0.12, 0.12);

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, (show_window, press_buttons, scroll_list));
}

/// The skills' list, which scrolls.
#[derive(Component)]
struct TrainerList;

/// Pixels a wheel notch scrolls.
const WHEEL_LINE: f32 = 40.0;

/// The list scrolls with the wheel over it, and with a thumb dragged on it.
fn scroll_list(
    mut wheel: MessageReader<MouseWheel>,
    touches: Res<Touches>,
    mut lists: Query<
        (
            &RelativeCursorPosition,
            &mut ScrollPosition,
            &ComputedNode,
            &UiGlobalTransform,
        ),
        With<TrainerList>,
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

#[derive(Component)]
struct WindowRoot;

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

/// An amount of gold.
fn money(c: u32) -> String {
    format!("{c} gold")
}

fn show_window(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    mut images: ResMut<Assets<Image>>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    roots: Query<Entity, With<WindowRoot>>,
    mut shown: Local<Option<(u64, f32)>>,
) {
    let current = trainer::current();
    let height = window.single().map_or(720.0, Window::height);
    let wanted = current.as_ref().map(|w| (w.id, height));
    if wanted == *shown {
        return;
    }
    for e in &roots {
        commands.entity(e).despawn();
    }
    *shown = wanted;
    let Some(w) = current else { return };
    let Some(font) = screen.font() else {
        *shown = None;
        return;
    };
    spawn_window(&mut commands, &screen, &mut images, &font, &w, height);
}

/// A row's [-] or [+], `wide` vh wide, lit only when it can be pressed.
fn step_button(id: u64, action: Action, can: bool, wide: f32) -> impl Bundle {
    (
        Press(id, action),
        Shade(can.then_some((BUY, BUY_LIT))),
        Button,
        Node {
            width: Val::Vh((wide) * crate::SHRINK),
            height: Val::Vh((4.6) * crate::SHRINK),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.5)),
            border_radius: BorderRadius::all(Val::Vh((0.8) * crate::SHRINK)),
            ..default()
        },
        BackgroundColor(if can { BUY } else { BUY_OFF }),
        BorderColor::all(PAGE_EDGE),
    )
}

fn spawn_window(
    commands: &mut Commands,
    screen: &crate::Screen,
    images: &mut Assets<Image>,
    font: &Handle<Font>,
    w: &Trainer,
    height: f32,
) {
    // Sized by the screen, shrunk on the PC like the other windows (a scale transform would
    // stop the list clipping).
    let vh = height / 100.0 * crate::SHRINK;
    let id = w.id;
    commands
        .spawn((
            WindowRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            GlobalZIndex(crate::SCREEN_Z - 20),
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: Val::Vh((96.0) * crate::SHRINK),
                    height: Val::Vh((115.0) * crate::SHRINK),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Vh((0.8) * crate::SHRINK),
                    padding: UiRect::all(Val::Vh((2.5) * crate::SHRINK)),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Vh((1.2) * crate::SHRINK)),
                    ..default()
                },
                BackgroundColor(PAGE),
                BorderColor::all(PAGE_EDGE),
            ))
            .with_children(|page| {
                page.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    ..default()
                })
                .with_children(|h| {
                    h.spawn(text(font, &w.trainer, 4.6 * vh, crate::GOLD));
                    h.spawn(text(font, &money(w.money), 3.4 * vh, crate::GOLD));
                    h.spawn((
                        Press(id, Action::Close),
                        Shade(Some((
                            Color::srgb(0.45, 0.08, 0.06),
                            Color::srgb(0.65, 0.12, 0.08),
                        ))),
                        Button,
                        Node {
                            width: Val::Vh((5.5) * crate::SHRINK),
                            height: Val::Vh((5.5) * crate::SHRINK),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border_radius: BorderRadius::all(Val::Vh((0.8) * crate::SHRINK)),
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
                    "What you buy comes to you on a run as you reach each rank's level.",
                    2.4 * vh,
                    DIM,
                ));
                page.spawn((
                    Node {
                        height: Val::Px(1.5),
                        width: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(PAGE_EDGE),
                ));
                page.spawn((
                    TrainerList,
                    RelativeCursorPosition::default(),
                    ScrollPosition::default(),
                    Node {
                        flex_grow: 1.0,
                        flex_basis: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Vh((0.8) * crate::SHRINK),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                ))
                .with_children(|page| {
                    for row in &w.rows {
                        page.spawn(Node {
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            column_gap: Val::Vh((1.2) * crate::SHRINK),
                            flex_shrink: 0.0,
                            ..default()
                        })
                        .with_children(|r| {
                            let back = row.bought > 0;
                            r.spawn(step_button(id, Action::Refund(row.chain), back, 4.6))
                                .with_children(|b| {
                                    b.spawn(text(
                                        font,
                                        "-",
                                        3.6 * vh,
                                        if back { WHITE } else { DIM },
                                    ));
                                });
                            let mut icon = r.spawn((
                                Node {
                                    width: Val::Vh((5.0) * crate::SHRINK),
                                    height: Val::Vh((5.0) * crate::SHRINK),
                                    flex_shrink: 0.0,
                                    border: UiRect::all(Val::Px(1.5)),
                                    ..default()
                                },
                                BorderColor::all(PAGE_EDGE),
                            ));
                            if let Some(i) = screen.spell_icon(row.spell) {
                                icon.insert(ImageNode::new(images.add(i)));
                            }
                            r.spawn(Node {
                                flex_direction: FlexDirection::Column,
                                flex_grow: 1.0,
                                ..default()
                            })
                            .with_children(|c| {
                                let color = if row.defensive { DEFENSIVE } else { WHITE };
                                c.spawn(text(font, &row.name, 3.0 * vh, color));
                                let comes = if row.levels.is_empty() {
                                    "Not bought".to_owned()
                                } else {
                                    let l: Vec<String> =
                                        row.levels.iter().map(u32::to_string).collect();
                                    format!("Comes at level {}", l.join(", "))
                                };
                                c.spawn(text(font, &comes, 2.3 * vh, DIM));
                            });
                            r.spawn((
                                text(
                                    font,
                                    &format!("{} / {}", row.bought, row.ranks),
                                    2.6 * vh,
                                    DIM,
                                ),
                                Node {
                                    flex_shrink: 0.0,
                                    ..default()
                                },
                            ));
                            let (label, can) = match row.next {
                                Some((level, cost)) => {
                                    (format!("+  Lv {level}  {}", money(cost)), w.money >= cost)
                                }
                                None => ("All ranks".to_owned(), false),
                            };
                            r.spawn(step_button(id, Action::Buy(row.chain), can, 22.0))
                                .with_children(|b| {
                                    b.spawn(text(
                                        font,
                                        &label,
                                        2.6 * vh,
                                        if can { WHITE } else { DIM },
                                    ));
                                });
                        });
                    }
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
            Interaction::Pressed => trainer::act(press.0, press.1),
            Interaction::Hovered => bg.0 = lit,
            Interaction::None => bg.0 = rest,
        }
    }
}
