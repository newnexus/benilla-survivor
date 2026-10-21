//! The cast queue's animation, drawn over the main action bar. The bar's front is the queue
//! ([`survivor_server::bar`]): when a spell goes off it moves to the back. The client's buttons
//! change at once, so for a moment this draws the step over them: the spell that went off rises,
//! grows and fades from its old button, the ones behind it slide forward a button, and it comes
//! back in at the end, sliding down into its new button as it fades in.

use std::collections::HashMap;
use std::sync::Mutex;

use bevy::prelude::*;
use survivor_server::bar;

use super::{Library, Screen};

/// How long one step takes (s).
const STEP_SECS: f32 = 0.35;
/// Above the game's interface, below the level-up screen.
const QUEUE_Z: i32 = 900;
/// How far the spell that went off rises, and how much it grows, as shares of a button.
const RISE: f32 = 1.2;
const GROW: f32 = 0.6;
/// An empty button, under the moving icons, so the client's already-changed icons stay hidden.
const SLOT_BG: Color = Color::srgb(0.05, 0.05, 0.06);

/// The main bar's buttons, button 1 first.
fn button_name(i: usize) -> String {
    format!("ActionButton{}", i + 1)
}

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, watch_buttons)
        .add_systems(Update, animate_queue);
}

fn watch_buttons(mut frames: ResMut<benilla_app::SurvivorFrames>) {
    frames.watch.extend((0..12).map(button_name));
}

/// The step on screen.
#[derive(Default)]
struct Step {
    from: Vec<u32>,
    to: Vec<u32>,
    fired: u32,
    started: f32,
    root: Option<Entity>,
}

#[derive(Default)]
struct Seen {
    version: u64,
    spells: Vec<u32>,
    step: Option<Step>,
    icons: HashMap<u32, Option<Handle<Image>>>,
}

#[allow(clippy::too_many_arguments)]
fn animate_queue(
    mut commands: Commands,
    mut screen: ResMut<Screen>,
    mut fonts: ResMut<Assets<Font>>,
    mut images: ResMut<Assets<Image>>,
    frames: Res<benilla_app::SurvivorFrames>,
    time: Res<Time<Real>>,
    mut seen: Local<Seen>,
) {
    let now = time.elapsed_secs();
    if let Some(q) = bar::current() {
        if q.version != seen.version {
            let before = std::mem::replace(&mut seen.spells, q.spells.clone());
            seen.version = q.version;
            if let Some(fired) = q.fired.filter(|_| !before.is_empty()) {
                if let Some(root) = seen.step.take().and_then(|s| s.root) {
                    commands.entity(root).despawn();
                }
                seen.step = Some(Step {
                    from: before,
                    to: q.spells,
                    fired,
                    started: now,
                    root: None,
                });
            }
        }
    }
    // Two fields borrowed apart: the step drawn and the icon cache.
    let Seen { step, icons, .. } = &mut *seen;
    let Some(step) = step.as_mut() else { return };
    if let Some(root) = step.root.take() {
        commands.entity(root).despawn();
    }
    let t = (now - step.started) / STEP_SECS;
    if t >= 1.0 {
        seen.step = None;
        return;
    }
    let rect = |i: usize| frames.rects.get(&button_name(i)).copied();
    let Some(last) = rect(step.to.len().saturating_sub(1)) else {
        return;
    };
    if screen.library.is_none() && !screen.broken {
        match Library::load(&mut fonts) {
            Ok(l) => screen.library = Some(Mutex::new(l)),
            Err(_) => screen.broken = true,
        }
    }
    let Some(library) = &screen.library else { return };
    let library = library.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut icon = |spell: u32| -> Option<Handle<Image>> {
        icons
            .entry(spell)
            .or_insert_with(|| {
                let s = library.spells.get(spell)?;
                library.icon(s).map(|i| images.add(i))
            })
            .clone()
    };
    let e = t * t * (3.0 - 2.0 * t);
    let mut nodes: Vec<(Rect, Handle<Image>, f32)> = Vec::new();
    // The ones behind slide forward.
    for (to_i, &spell) in step.to.iter().enumerate() {
        if spell == step.fired {
            continue;
        }
        let from_i = step.from.iter().position(|&s| s == spell).unwrap_or(to_i);
        let (Some(a), Some(b)) = (rect(from_i), rect(to_i)) else { continue };
        let at = Rect::from_center_size(a.center().lerp(b.center(), e), b.size());
        if let Some(img) = icon(spell) {
            nodes.push((at, img, 1.0));
        }
    }
    if let Some(img) = icon(step.fired) {
        // The one that went off rises, grows and fades from where it was.
        if let Some(a) = step.from.iter().position(|&s| s == step.fired).and_then(rect) {
            let c = a.center() - Vec2::Y * a.height() * RISE * e;
            nodes.push((Rect::from_center_size(c, a.size() * (1.0 + GROW * e)), img.clone(), 1.0 - e));
        }
        // ... and comes in at the back from above.
        let c = last.center() - Vec2::Y * last.height() * (1.0 - e);
        nodes.push((Rect::from_center_size(c, last.size()), img, e));
    }
    let px = |r: Rect| Node {
        position_type: PositionType::Absolute,
        left: Val::Px(r.min.x),
        top: Val::Px(r.min.y),
        width: Val::Px(r.width()),
        height: Val::Px(r.height()),
        ..default()
    };
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            GlobalZIndex(QUEUE_Z),
            Pickable::IGNORE,
        ))
        .with_children(|p| {
            for i in 0..step.to.len() {
                if let Some(r) = rect(i) {
                    p.spawn((px(r), BackgroundColor(SLOT_BG), Pickable::IGNORE));
                }
            }
            for (r, img, alpha) in nodes {
                p.spawn((
                    px(r),
                    ImageNode::new(img).with_color(Color::srgba(1.0, 1.0, 1.0, alpha)),
                    Pickable::IGNORE,
                ));
            }
        })
        .id();
    step.root = Some(root);
}
