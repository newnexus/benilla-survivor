//! The profession trainers' trades over their heads: each trade's icon in a round gold rim,
//! side by side when they teach two ([`survivor_server::events::profession_icons`]), so a
//! trainer is known from afar, held out in the zone or standing in town.

use std::collections::HashMap;

use bevy::prelude::*;
use benilla_app::ObjectType;
use benilla_world::view::WorldCamera;

/// An icon's size and the gap between two, in screen heights; how far above the feet they float,
/// yards (by the creature's scale), and out to how far they show.
const SIZE: f32 = 0.042;
const GAP: f32 = 0.006;
const HEAD: f32 = 2.9;
const RANGE: f32 = 70.0;
/// The icons show at 30%, rim and all: a hint over the head, not a sign.
const OPACITY: f32 = 0.3;
const RIM: Color = Color::srgba(0.85, 0.65, 0.2, OPACITY);

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, draw);
}

/// A trainer's icons on screen: their row.
#[derive(Default)]
struct Shown {
    rows: HashMap<u64, Entity>,
    icons: HashMap<&'static str, Handle<Image>>,
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    objects: benilla_app::Objects,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<WorldCamera>>,
    transforms: Query<&GlobalTransform>,
    mut nodes: Query<&mut Node>,
    mut images: ResMut<Assets<Image>>,
    mut shown: Local<Shown>,
) {
    let (Ok(window), Ok((camera, eye))) = (window.single(), camera.single()) else {
        return;
    };
    let me = objects.player().and_then(|p| transforms.get(p).ok()).map(|t| t.translation());
    let h = window.height();
    let (size, gap) = (SIZE * h, GAP * h);
    let mut seen = Vec::new();
    for (guid, entity, fields) in objects.iter() {
        if fields.object_type() != Some(ObjectType::Unit) {
            continue;
        }
        // A creature's guid carries its entry (bits 24 to 47).
        let trades = survivor_server::events::profession_icons(((guid >> 24) & 0xFF_FFFF) as u32);
        if trades.is_empty() {
            continue;
        }
        let Ok(tf) = transforms.get(entity) else { continue };
        let feet = tf.translation();
        if me.is_some_and(|m| m.distance(feet) > RANGE) {
            continue;
        }
        let scale = fields.object_scale_x().filter(|s| *s > 0.0).unwrap_or(1.0);
        let Ok(at) = camera.world_to_viewport(eye, feet + Vec3::Y * HEAD * scale) else { continue };
        if !shown.rows.contains_key(&guid) {
            for &t in trades {
                if !shown.icons.contains_key(t) {
                    let Some(i) = screen.icon(&format!("Interface\\Icons\\{t}")) else { return };
                    shown.icons.insert(t, images.add(i));
                }
            }
            let icons: Vec<Handle<Image>> = trades.iter().map(|t| shown.icons[t].clone()).collect();
            let row = commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(gap),
                        ..default()
                    },
                    GlobalZIndex(crate::SCREEN_Z - 41),
                    Pickable::IGNORE,
                ))
                .with_children(|r| {
                    for icon in icons {
                        r.spawn((
                            Node {
                                width: Val::Px(size),
                                height: Val::Px(size),
                                border: UiRect::all(Val::Px((size * 0.07).max(1.5))),
                                border_radius: BorderRadius::all(Val::Percent(50.0)),
                                overflow: Overflow::clip(),
                                ..default()
                            },
                            BorderColor::all(RIM),
                            BackgroundColor(Color::BLACK.with_alpha(OPACITY)),
                            Pickable::IGNORE,
                        ))
                        .with_children(|f| {
                            f.spawn((
                                ImageNode::new(icon).with_color(Color::WHITE.with_alpha(OPACITY)),
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Percent(100.0),
                                    border_radius: BorderRadius::all(Val::Percent(50.0)),
                                    ..default()
                                },
                                Pickable::IGNORE,
                            ));
                        });
                    }
                })
                .id();
            shown.rows.insert(guid, row);
        }
        seen.push(guid);
        let n = trades.len() as f32;
        let width = n * size + (n - 1.0) * gap;
        if let Some(mut node) = shown.rows.get(&guid).and_then(|&r| nodes.get_mut(r).ok()) {
            node.left = Val::Px((at.x - width / 2.0).round());
            node.top = Val::Px((at.y - size).round());
        }
    }
    // Out of view, out of range, or gone: their icons go.
    shown.rows.retain(|guid, row| {
        let stays = seen.contains(guid);
        if !stays {
            commands.entity(*row).despawn();
        }
        stays
    });
}
