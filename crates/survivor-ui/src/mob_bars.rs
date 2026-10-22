//! Health bars over hurt creatures, as Elden Ring draws them: a short, thin red line on a dark
//! strip, its red streaked by a small texture of its own, and the part just lost left showing in
//! pale gold for a moment before it drains away. Only a creature that has lost health shows one.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use benilla_app::ObjectType;
use benilla_world::view::WorldCamera;

/// A bar's size, in screen heights, and how far above a creature's feet it floats, yards (by
/// the creature's scale).
const WIDTH: f32 = 0.075;
const HEIGHT: f32 = 0.0055;
const HEAD: f32 = 2.3;
/// Bars show out to this many yards.
const RANGE: f32 = 45.0;
/// The part just lost holds this long, then drains at this much of the bar a second.
const HOLD: f32 = 0.6;
const DRAIN: f32 = 1.2;
const BACK: Color = Color::srgba(0.02, 0.01, 0.01, 0.7);
const EDGE: Color = Color::srgba(0.32, 0.27, 0.2, 0.85);
const LOST: Color = Color::srgba(0.95, 0.82, 0.5, 0.9);

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, setup).add_systems(Update, draw);
}

#[derive(Resource)]
struct BarLook {
    red: Handle<Image>,
}

/// A creature's bar on screen: its root, the drain and fill inside it, and what it shows.
struct Bar {
    root: Entity,
    lost: Entity,
    fill: Entity,
    /// Health shown, and the trailing part just lost, as parts of the full bar.
    health: f32,
    trail: f32,
    hold_until: f32,
}

#[derive(Default)]
struct Bars(HashMap<u64, Bar>);

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.insert_resource(BarLook {
        red: images.add(red_image()),
    });
}

/// The fill's texture: deep red, brighter along the middle, darker at the edges, with faint
/// streaks along its length.
fn red_image() -> Image {
    const W: usize = 128;
    const H: usize = 8;
    let mut data = Vec::with_capacity(W * H * 4);
    for y in 0..H {
        let v = (y as f32 + 0.5) / H as f32;
        // Brightest a little above the middle, as if lit from above.
        let band = 1.0 - ((v - 0.42) * 2.2).abs().min(1.0);
        for x in 0..W {
            let u = x as f32 / W as f32;
            let streak = 0.5 + 0.5 * ((u * 37.0 + v * 3.0).sin() * (u * 11.0).cos());
            let k = 0.55 + 0.45 * band + 0.08 * (streak - 0.5);
            let (r, g, b) = (0.72 * k + 0.2 * band * band, 0.06 * k, 0.05 * k);
            data.extend([r, g, b, 1.0].map(|c: f32| (c.clamp(0.0, 1.0) * 255.0) as u8));
        }
    }
    Image::new(
        Extent3d {
            width: W as u32,
            height: H as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    look: Option<Res<BarLook>>,
    objects: benilla_app::Objects,
    time: Res<Time<Real>>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<WorldCamera>>,
    transforms: Query<&GlobalTransform>,
    mut nodes: Query<&mut Node>,
    mut visibility: Query<&mut Visibility>,
    mut bars: Local<Bars>,
) {
    let Some(look) = look else { return };
    let (Ok(window), Ok((camera, eye))) = (window.single(), camera.single()) else {
        return;
    };
    let me = objects.player().and_then(|p| transforms.get(p).ok()).map(|t| t.translation());
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let h = window.height();
    let (w, tall) = (WIDTH * h, (HEIGHT * h).max(3.0));
    let mut seen = Vec::new();
    for (guid, entity, fields) in objects.iter() {
        if fields.object_type() != Some(ObjectType::Unit) {
            continue;
        }
        let (Some(health), Some(max)) = (fields.unit_health(), fields.unit_max_health()) else {
            continue;
        };
        if health == 0 || max == 0 || health >= max {
            continue;
        }
        let Ok(tf) = transforms.get(entity) else { continue };
        let feet = tf.translation();
        if me.is_some_and(|m| m.distance(feet) > RANGE) {
            continue;
        }
        let scale = fields.object_scale_x().filter(|s| *s > 0.0).unwrap_or(1.0);
        let head = feet + Vec3::Y * HEAD * scale;
        let Ok(at) = camera.world_to_viewport(eye, head) else { continue };
        seen.push(guid);
        let part = health as f32 / max as f32;
        let bar = bars.0.entry(guid).or_insert_with(|| {
            let mut lost = Entity::PLACEHOLDER;
            let mut fill = Entity::PLACEHOLDER;
            let root = commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(BACK),
                    BorderColor::all(EDGE),
                    GlobalZIndex(crate::SCREEN_Z - 40),
                    Pickable::IGNORE,
                ))
                .with_children(|r| {
                    let inner = |c: Color| {
                        (
                            Node {
                                position_type: PositionType::Absolute,
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            BackgroundColor(c),
                            Pickable::IGNORE,
                        )
                    };
                    lost = r.spawn(inner(LOST)).id();
                    fill = r
                        .spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            ImageNode::new(look.red.clone()),
                            Pickable::IGNORE,
                        ))
                        .id();
                })
                .id();
            Bar {
                root,
                lost,
                fill,
                health: part,
                trail: part,
                hold_until: 0.0,
            }
        });
        if part < bar.health {
            bar.hold_until = now + HOLD;
        }
        bar.health = part;
        if now >= bar.hold_until {
            bar.trail = (bar.trail - DRAIN * dt).max(part);
        }
        bar.trail = bar.trail.max(part);
        if let Ok(mut n) = nodes.get_mut(bar.root) {
            n.left = Val::Px((at.x - w / 2.0).round());
            n.top = Val::Px((at.y - tall).round());
            n.width = Val::Px(w.round());
            n.height = Val::Px(tall.round());
        }
        if let Ok(mut n) = nodes.get_mut(bar.fill) {
            n.width = Val::Percent(part * 100.0);
        }
        if let Ok(mut n) = nodes.get_mut(bar.lost) {
            n.width = Val::Percent(bar.trail * 100.0);
        }
        if let Ok(mut v) = visibility.get_mut(bar.root) {
            v.set_if_neq(Visibility::Inherited);
        }
    }
    // Healed, dead, gone or out of sight: no bar.
    bars.0.retain(|guid, bar| {
        let keep = seen.contains(guid);
        if !keep {
            commands.entity(bar.root).despawn();
        }
        keep
    });
}
