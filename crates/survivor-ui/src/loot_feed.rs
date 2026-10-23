//! The loot feed ([`survivor_server::loot_feed`]): each pickup's icon in a column over the
//! backpack button, newest at the bottom, a stack's count on it (`x3`). The same item picked up
//! again while still shown adds to its count. Each fades after a few seconds; one that did not
//! fit the backpack stands in grey with "Bags full".

use std::collections::HashMap;

use bevy::prelude::*;
use survivor_server::loot_feed;

/// The bag button the column stands on.
const BAG: &str = "MainMenuBarBackpackButton";
/// How long an entry shows, the last part of it fading (s); how many show at once.
const SHOW: f32 = 4.0;
const FADE: f32 = 1.0;
const MOST: usize = 6;
/// An icon's size and the gap between entries, in screen heights (vh).
const ICON: f32 = 4.6;
const GAP: f32 = 0.6;
const FULL: Color = Color::srgb(1.0, 0.25, 0.2);

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Feed>()
        .add_systems(Startup, watch_bag)
        .add_systems(Update, (take_pickups, draw_feed, fade_feed).chain());
}

fn watch_bag(mut frames: ResMut<benilla_app::SurvivorFrames>) {
    frames.watch.push(BAG.to_owned());
}

struct Entry {
    entry: u32,
    count: u32,
    display: u32,
    full: bool,
    /// When it was last added to.
    at: f32,
}

#[derive(Resource, Default)]
struct Feed {
    entries: Vec<Entry>,
    /// Bumped by every change, for a fresh draw.
    version: u64,
}

#[derive(Component)]
struct FeedRoot;

/// An entry's parts that fade: by its index in the feed.
#[derive(Component)]
struct Fades(usize);

fn take_pickups(mut feed: ResMut<Feed>, time: Res<Time<Real>>) {
    let now = time.elapsed_secs();
    let before = feed.entries.len();
    feed.entries.retain(|e| now - e.at < SHOW);
    let mut changed = feed.entries.len() != before;
    for (p, full) in loot_feed::take() {
        changed = true;
        match feed.entries.iter_mut().find(|e| e.entry == p.entry && e.full == full) {
            Some(e) => {
                e.count += p.count;
                e.at = now;
            }
            None => feed.entries.push(Entry {
                entry: p.entry,
                count: p.count,
                display: p.display,
                full,
                at: now,
            }),
        }
    }
    let over = feed.entries.len().saturating_sub(MOST);
    feed.entries.drain(..over);
    if changed {
        feed.version += 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_feed(
    mut commands: Commands,
    feed: Res<Feed>,
    screen: Res<crate::Screen>,
    frames: Res<benilla_app::SurvivorFrames>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    roots: Query<Entity, With<FeedRoot>>,
    mut images: ResMut<Assets<Image>>,
    mut icons: Local<HashMap<u32, Handle<Image>>>,
    mut drawn: Local<u64>,
) {
    if feed.version == *drawn {
        return;
    }
    let Ok(window) = window.single() else { return };
    let Some(bag) = frames.rects.get(BAG).copied() else { return };
    let Some(font) = screen.font() else { return };
    *drawn = feed.version;
    for e in &roots {
        commands.entity(e).despawn();
    }
    if feed.entries.is_empty() {
        return;
    }
    let vh = window.height() / 100.0;
    if let Some(library) = &screen.library {
        let library = library.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for e in &feed.entries {
            if !icons.contains_key(&e.display) {
                if let Some(i) = library.item_icon(e.display) {
                    icons.insert(e.display, images.add(i));
                }
            }
        }
    }
    let text = |s: &str, size: f32, color: Color| {
        (
            Text::new(s.to_owned()),
            TextFont {
                font: font.clone(),
                font_size: size * vh,
                ..default()
            },
            TextColor(color),
            TextShadow::default(),
        )
    };
    commands
        .spawn((
            FeedRoot,
            Node {
                position_type: PositionType::Absolute,
                // Right-aligned on the bag button, standing on its top.
                right: Val::Px(window.width() - bag.max.x),
                bottom: Val::Px(window.height() - bag.min.y + GAP * vh),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                row_gap: Val::Vh(GAP),
                ..default()
            },
            GlobalZIndex(crate::SCREEN_Z - 40),
            Pickable::IGNORE,
        ))
        .with_children(|col| {
            for (i, e) in feed.entries.iter().enumerate() {
                col.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Vh(0.8),
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|row| {
                    if e.full {
                        row.spawn((text("Bags full", 2.2, FULL), Fades(i), Pickable::IGNORE));
                    }
                    let mut icon = row.spawn((
                        Node {
                            width: Val::Vh(ICON),
                            height: Val::Vh(ICON),
                            justify_content: JustifyContent::FlexEnd,
                            align_items: AlignItems::FlexEnd,
                            border: UiRect::all(Val::Vh(0.15)),
                            ..default()
                        },
                        BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.8)),
                        Fades(i),
                        Pickable::IGNORE,
                    ));
                    if let Some(h) = icons.get(&e.display) {
                        let mut img = ImageNode::new(h.clone());
                        if e.full {
                            img.color = Color::srgb(0.4, 0.4, 0.4);
                        }
                        icon.insert(img);
                    }
                    icon.with_children(|c| {
                        if e.count > 1 {
                            c.spawn((
                                text(&format!("x{}", e.count), 2.0, Color::WHITE),
                                Fades(i),
                                Pickable::IGNORE,
                                Node {
                                    margin: UiRect::all(Val::Vh(0.2)),
                                    ..default()
                                },
                            ));
                        }
                    });
                });
            }
        });
}

/// The last part of an entry's time: its icon and words fade out.
fn fade_feed(
    feed: Res<Feed>,
    time: Res<Time<Real>>,
    mut parts: Query<(&Fades, Option<&mut ImageNode>, Option<&mut TextColor>)>,
) {
    let now = time.elapsed_secs();
    for (f, img, color) in &mut parts {
        let Some(e) = feed.entries.get(f.0) else { continue };
        let alpha = ((SHOW - (now - e.at)) / FADE).clamp(0.0, 1.0);
        if let Some(mut img) = img {
            img.color.set_alpha(alpha);
        }
        if let Some(mut c) = color {
            c.0.set_alpha(alpha);
        }
    }
}
