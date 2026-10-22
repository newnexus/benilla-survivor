//! The spirit healer's window ([`survivor_server::spirit`]): the souls to spend at the top, then a
//! row per upgrade: [-] (a level off, its souls back in full), the icon, name and what the next
//! level gives, its level, and [+] with the next level's price.
//! Rebuilt whenever the server posts it again (a purchase), closed with its X.

use bevy::prelude::*;
use survivor_server::spirit::{self, Action, Window as Healer};

const PAGE: Color = Color::srgba(0.035, 0.03, 0.025, 0.92);
const PAGE_EDGE: Color = Color::srgb(0.55, 0.45, 0.25);
const WHITE: Color = Color::srgb(0.95, 0.93, 0.88);
const DIM: Color = Color::srgb(0.6, 0.58, 0.54);
const SOUL_BLUE: Color = Color::srgb(0.45, 0.85, 1.0);
const BUY: Color = Color::srgb(0.12, 0.32, 0.45);
const BUY_LIT: Color = Color::srgb(0.18, 0.45, 0.62);
const BUY_OFF: Color = Color::srgb(0.12, 0.12, 0.12);

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, (show_window, press_buttons));
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

#[allow(clippy::too_many_arguments)]
fn show_window(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    mut images: ResMut<Assets<Image>>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    roots: Query<Entity, With<WindowRoot>>,
    mut shown: Local<Option<(u64, f32)>>,
) {
    let current = spirit::current();
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

fn spawn_window(
    commands: &mut Commands,
    screen: &crate::Screen,
    images: &mut Assets<Image>,
    font: &Handle<Font>,
    w: &Healer,
    height: f32,
) {
    let vh = height / 100.0;
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
            UiTransform {
                scale: Vec2::splat(crate::SHRINK),
                ..default()
            },
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: Val::Vh(80.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Vh(1.2),
                    padding: UiRect::all(Val::Vh(2.5)),
                    border: UiRect::all(Val::Px(2.0)),
                    border_radius: BorderRadius::all(Val::Vh(1.2)),
                    ..default()
                },
                BackgroundColor(PAGE),
                BorderColor::all(PAGE_EDGE),
            ))
            .with_children(|page| {
                // The title, the souls, the X.
                page.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    ..default()
                })
                .with_children(|h| {
                    h.spawn(text(font, "Spirit Healer", 5.0 * vh, crate::GOLD));
                    h.spawn(text(font, &format!("{} souls", w.souls), 3.6 * vh, SOUL_BLUE));
                    h.spawn((
                        Press(id, Action::Close),
                        Shade(Some((Color::srgb(0.45, 0.08, 0.06), Color::srgb(0.65, 0.12, 0.08)))),
                        Button,
                        Node {
                            width: Val::Vh(5.5),
                            height: Val::Vh(5.5),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border_radius: BorderRadius::all(Val::Vh(0.8)),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.45, 0.08, 0.06)),
                    ))
                    .with_children(|b| {
                        b.spawn(text(font, "X", 3.4 * vh, crate::GOLD));
                    });
                });
                page.spawn((
                    Node {
                        height: Val::Px(1.5),
                        width: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(PAGE_EDGE),
                ));
                for row in &w.rows {
                    page.spawn(Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Vh(1.5),
                        ..default()
                    })
                    .with_children(|r| {
                        // [-]: the last level off, its souls back.
                        let back = row.refund.is_some();
                        r.spawn(step_button(id, Action::Refund(row.upgrade), back, 5.0))
                            .with_children(|b| {
                                b.spawn(text(font, "-", 4.0 * vh, if back { WHITE } else { DIM }));
                            });
                        let mut icon = r.spawn((
                            Node {
                                width: Val::Vh(6.0),
                                height: Val::Vh(6.0),
                                flex_shrink: 0.0,
                                border: UiRect::all(Val::Px(1.5)),
                                ..default()
                            },
                            BorderColor::all(PAGE_EDGE),
                        ));
                        if let Some(i) = screen.icon(row.icon) {
                            icon.insert(ImageNode::new(images.add(i)));
                        }
                        r.spawn(Node {
                            flex_direction: FlexDirection::Column,
                            flex_grow: 1.0,
                            ..default()
                        })
                        .with_children(|c| {
                            c.spawn(text(font, &row.name, 3.2 * vh, WHITE));
                            c.spawn(text(font, &row.text, 2.5 * vh, DIM));
                        });
                        r.spawn((
                            text(font, &format!("{} / {}", row.level, row.max), 2.8 * vh, DIM),
                            Node {
                                flex_shrink: 0.0,
                                ..default()
                            },
                        ));
                        // [+]: the next level, for its souls.
                        let (label, can) = match row.cost {
                            Some(c) => (format!("+  {c} souls"), w.souls >= c),
                            None => ("Max".to_owned(), false),
                        };
                        r.spawn(step_button(id, Action::Buy(row.upgrade), can, 16.0))
                            .with_children(|b| {
                                b.spawn(text(font, &label, 2.8 * vh, if can { WHITE } else { DIM }));
                            });
                    });
                }
                // Talents: every point back, free.
                page.spawn(Node {
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::Center,
                    margin: UiRect::top(Val::Vh(1.0)),
                    ..default()
                })
                .with_children(|r| {
                    r.spawn(step_button(id, Action::ResetTalents, true, 30.0))
                        .with_children(|b| {
                            b.spawn(text(font, "Reset talents (free)", 2.8 * vh, WHITE));
                        });
                });
            });
        });
}

/// A row's [-] or [+], `wide` vh wide, lit only when it can be pressed.
fn step_button(id: u64, action: Action, can: bool, wide: f32) -> impl Bundle {
    (
        Press(id, action),
        Shade(can.then_some((BUY, BUY_LIT))),
        Button,
        Node {
            width: Val::Vh(wide),
            height: Val::Vh(5.0),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.5)),
            border_radius: BorderRadius::all(Val::Vh(0.8)),
            ..default()
        },
        BackgroundColor(if can { BUY } else { BUY_OFF }),
        BorderColor::all(PAGE_EDGE),
    )
}

fn press_buttons(
    mut buttons: Query<(&Interaction, &Press, &Shade, &mut BackgroundColor), Changed<Interaction>>,
) {
    for (i, press, shade, mut bg) in &mut buttons {
        let Some((rest, lit)) = shade.0 else { continue };
        match i {
            Interaction::Pressed => spirit::act(press.0, press.1),
            Interaction::Hovered => bg.0 = lit,
            Interaction::None => bg.0 = rest,
        }
    }
}
