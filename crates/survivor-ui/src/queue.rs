//! The cast queue strip ([`survivor_server::bar`]), in a row between the character and the action
//! bars. On a run it is the live queue: the spell the auto-cast tries next first, in a gold frame,
//! then the ones after it, the line moving on as each is cast. In town, and on a run paused with
//! the game menu, it is the player's order to set: click a spell, then the place to move it to.
//! In town it folds away into a small tab, opened and closed with Alt.
//!
//! Each step of the live queue is animated: the spell just cast rises, grows and fades from the
//! front, the ones behind it slide forward, and it drops back in at its new place as it fades in.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use survivor_server::bar;

/// An icon's size, the gap between icons, and how high off the screen's bottom the strip sits,
/// in screen heights (vh): over the action bars.
const ICON: f32 = 5.2;
const GAP: f32 = 0.8;
const LIFT: f32 = 14.5;
/// How long one step of the live queue takes (s); how far the spell cast rises, and how much it
/// grows, as shares of an icon.
const STEP_SECS: f32 = 0.35;
const RISE: f32 = 1.2;
const GROW: f32 = 0.6;
const FRAME: Color = Color::srgba(0.35, 0.3, 0.2, 0.9);
const NEXT: Color = Color::srgb(1.0, 0.82, 0.0);
const BACK: Color = Color::srgba(0.0, 0.0, 0.0, 0.45);

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Slide>()
        .add_systems(Startup, watch_frames)
        .add_systems(Update, (show_strip, drag_strip, slide_icons).chain());
}

fn watch_frames(mut frames: ResMut<benilla_app::SurvivorFrames>) {
    // The Escape menu, whose showing pauses the world (`crate::menu_pause`).
    frames.watch.push(crate::menu_pause::FRAME.to_owned());
}

#[derive(Component)]
struct StripRoot;

/// A place on the strip, by its index in the queue.
#[derive(Component)]
struct Slot(usize);

/// The icon at a place, which flashes when its spell is cast.
#[derive(Component)]
struct SlotIcon(usize);

#[derive(Default)]
struct Shown {
    version: u64,
    height: f32,
    /// Whether the player opened the strip in town (Alt), and whether it is drawn folded now.
    open: bool,
    folded: bool,
    icons: HashMap<u32, Handle<Image>>,
    /// The spells as last drawn on a run, left to right: where each step's icons come from.
    order: Vec<u32>,
}

#[allow(clippy::too_many_arguments)]
fn show_strip(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    objects: benilla_app::Objects,
    mut images: ResMut<Assets<Image>>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    roots: Query<Entity, With<StripRoot>>,
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut shown: Local<Shown>,
    mut slide: ResMut<Slide>,
) {
    let height = window.single().map_or(720.0, Window::height);
    let q = bar::current().filter(|_| objects.player().is_some());
    let in_town = q.as_ref().is_some_and(|q| q.in_town);
    if in_town && keys.any_just_pressed([KeyCode::AltLeft, KeyCode::AltRight]) {
        shown.open = !shown.open;
    }
    let folded = in_town && !shown.open;
    let version = q.as_ref().map_or(0, |q| q.version);
    if version == shown.version && height == shown.height && folded == shown.folded {
        return;
    }
    shown.version = version;
    shown.height = height;
    shown.folded = folded;
    for e in &roots {
        commands.entity(e).despawn();
    }
    let Some(q) = q else { return };
    if q.spells.is_empty() {
        return;
    }
    // On a run, from the next spell round; setting the order, as it stands.
    let n = q.spells.len();
    let start = if q.editable { 0 } else { q.next % n };
    let order: Vec<u32> = (0..n).map(|k| q.spells[(start + k) % n]).collect();
    let before = std::mem::replace(&mut shown.order, if q.editable { Vec::new() } else { order.clone() });
    // A cast on a run: each icon slides from where it stood, the cast one from the front.
    slide.moves.clear();
    if let Some(fired) = q.fired.filter(|_| !q.editable && before.len() == n) {
        slide.at = time.elapsed_secs();
        for (j, &spell) in order.iter().enumerate() {
            let Some(i) = before.iter().position(|&s| s == spell) else { continue };
            if i != j || spell == fired {
                slide.moves.push(((start + j) % n, i as f32 - j as f32, spell == fired));
            }
        }
    }
    // Before the game data has loaded there are no icons yet: the strip is built again until
    // every icon is there.
    if q.spells.iter().any(|&s| screen.spell_icon_ready(s).is_none()) {
        shown.version = 0;
    }
    let Shown { icons, .. } = &mut *shown;
    let mut icon = |spell: u32| -> Option<Handle<Image>> {
        if let Some(h) = icons.get(&spell) {
            return Some(h.clone());
        }
        let h = images.add(screen.spell_icon(spell)?);
        icons.insert(spell, h.clone());
        Some(h)
    };
    let font = screen.font();
    if folded {
        // The folded tab: where the strip would be, its name and the key that opens it.
        let Some(font) = font else {
            shown.version = 0;
            return;
        };
        commands
            .spawn((
                StripRoot,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    bottom: Val::Vh(LIFT),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                GlobalZIndex(crate::SCREEN_Z - 60),
                Pickable::IGNORE,
            ))
            .with_children(|root| {
                root.spawn((
                    Node {
                        padding: UiRect::axes(Val::Vh(1.2), Val::Vh(0.5)),
                        border: UiRect::all(Val::Vh(0.2)),
                        border_radius: BorderRadius::all(Val::Vh(0.8)),
                        ..default()
                    },
                    BackgroundColor(BACK),
                    BorderColor::all(FRAME),
                    Pickable::IGNORE,
                ))
                .with_children(|tab| {
                    tab.spawn((
                        Text::new("Cast order  (Alt)"),
                        TextFont {
                            font,
                            font_size: 2.0 * height / 100.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.85, 0.82, 0.75)),
                        TextShadow::default(),
                        Pickable::IGNORE,
                    ));
                });
            });
        return;
    }
    commands
        .spawn((
            StripRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                bottom: Val::Vh(LIFT),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Vh(0.5),
                ..default()
            },
            // Over the level-up and chest cards while the order may be changed, so it can be
            // clicked there; else under every window.
            GlobalZIndex(if q.editable { crate::SCREEN_Z + 5 } else { crate::SCREEN_Z - 60 }),
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            if q.editable {
                if let Some(font) = &font {
                    root.spawn((
                        Text::new("Cast order: click a spell, then the place to move it to"),
                        TextFont {
                            font: font.clone(),
                            font_size: 2.0 * height / 100.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.85, 0.82, 0.75)),
                        TextShadow::default(),
                        Pickable::IGNORE,
                    ));
                }
            }
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Vh(GAP),
                    padding: UiRect::all(Val::Vh(0.6)),
                    border_radius: BorderRadius::all(Val::Vh(0.8)),
                    ..default()
                },
                BackgroundColor(BACK),
                Pickable::IGNORE,
            ))
            .with_children(|row| {
                for k in 0..n {
                    let i = (start + k) % n;
                    let spell = q.spells[i];
                    let next = !q.editable && k == 0;
                    let mut slot = row.spawn((
                        Slot(i),
                        RelativeCursorPosition::default(),
                        Node {
                            width: Val::Vh(ICON),
                            height: Val::Vh(ICON),
                            border: UiRect::all(Val::Vh(if next { 0.45 } else { 0.2 })),
                            border_radius: BorderRadius::all(Val::Vh(0.5)),
                            ..default()
                        },
                        BorderColor::all(if next { NEXT } else { FRAME }),
                        BackgroundColor(Color::NONE),
                    ));
                    if q.editable {
                        slot.insert(Button);
                    } else {
                        slot.insert(Pickable::IGNORE);
                    }
                    slot.with_children(|s| {
                        let mut img = s.spawn((
                            SlotIcon(i),
                            UiTransform::default(),
                            Node {
                                width: Val::Percent(100.0),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            Pickable::IGNORE,
                        ));
                        if let Some(h) = icon(spell) {
                            img.insert(ImageNode::new(h));
                        }
                    });
                }
            });
        });
}

/// The step under way: when it began, and each moving icon (its place in the queue, how many
/// places to its right it set off from, and whether it is the spell cast).
#[derive(Resource, Default)]
struct Slide {
    at: f32,
    moves: Vec<(usize, f32, bool)>,
}

/// The step: the icons behind slide forward a place; the spell cast rises, grows and fades from
/// the front in the first half, then drops into its new place, fading in.
fn slide_icons(
    slide: Res<Slide>,
    time: Res<Time<Real>>,
    mut icons: Query<(&SlotIcon, &mut UiTransform, Option<&mut ImageNode>)>,
) {
    let t = ((time.elapsed_secs() - slide.at) / STEP_SECS).clamp(0.0, 1.0);
    let ease = |x: f32| 1.0 - (1.0 - x) * (1.0 - x);
    let place = ICON + GAP;
    for (s, mut tf, img) in &mut icons {
        let (x, y, scale, alpha) = match slide.moves.iter().find(|m| m.0 == s.0) {
            Some(&(_, from, false)) if t < 1.0 => (from * place * (1.0 - ease(t)), 0.0, 1.0, 1.0),
            Some(&(_, from, true)) if t < 1.0 => {
                if t < 0.5 {
                    let u = ease(t * 2.0);
                    (from * place, -RISE * ICON * u, 1.0 + GROW * u, 1.0 - u)
                } else {
                    let u = ease(t * 2.0 - 1.0);
                    (0.0, -ICON * (1.0 - u), 1.0, u)
                }
            }
            _ => (0.0, 0.0, 1.0, 1.0),
        };
        let want = Val2::new(Val::Vh(x), Val::Vh(y));
        if tf.translation != want {
            tf.translation = want;
        }
        if tf.scale != Vec2::splat(scale) {
            tf.scale = Vec2::splat(scale);
        }
        if let Some(mut img) = img {
            if img.color.alpha() != alpha {
                img.color.set_alpha(alpha);
            }
        }
    }
}

/// In town: click a spell to pick it up, then click the place to put it (clicking it again puts
/// it back). A click, not a drag: holding the mouse button over the world turns the camera.
fn drag_strip(
    slots: Query<(&Slot, &Interaction), Changed<Interaction>>,
    mut held: Local<Option<(u64, usize)>>,
    mut backs: Query<(&Slot, &mut BackgroundColor)>,
) {
    let version = bar::current().map_or(0, |q| q.version);
    if held.is_some_and(|h| h.0 != version) {
        *held = None;
    }
    let Some(clicked) = slots
        .iter()
        .find(|(_, i)| **i == Interaction::Pressed)
        .map(|(s, _)| s.0)
    else {
        return;
    };
    match *held {
        None => *held = Some((version, clicked)),
        Some((_, from)) => {
            *held = None;
            if from != clicked {
                bar::move_spell(from, clicked);
            }
        }
    }
    let lifted = held.map(|h| h.1);
    for (s, mut bg) in &mut backs {
        bg.0 = if Some(s.0) == lifted { Color::srgba(1.0, 0.82, 0.0, 0.45) } else { Color::NONE };
    }
}
