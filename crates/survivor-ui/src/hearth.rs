//! Charges on screen: an icon inside a ring of six parts that light up green in turn (the part
//! filling glows in as it goes), gone when done or broken.
//! - The Hearthstone ([`survivor_server::hearth`]): low in the middle of the screen.
//! - A pickup ([`survivor_server::pickup`], a gather object being gathered): over the object,
//!   its spot in the world turned to a spot on the screen every frame.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use benilla_world::view::WorldCamera;

/// The icon's size in screen heights (vh), for the Hearthstone and for a pickup; the ring is this
/// much wider than the icon.
const ICON: f32 = 6.0;
const PICKUP_ICON: f32 = 4.5;
const RING: f32 = 1.75;
/// The ring's parts, the gap between them (degrees), and its inner and outer radius as parts of
/// its half-width.
const PARTS: usize = 6;
const GAP: f32 = 7.0;
const INNER: f32 = 0.74;
const OUTER: f32 = 0.97;
/// A ring texture's side, pixels.
const RING_PX: usize = 128;
/// How high off the screen's bottom the Hearthstone's panel sits, vh; how far over a pickup's
/// object its panel floats, yards.
const LIFT: f32 = 24.0;
const OVER: f32 = 1.6;
const GREEN: Color = Color::srgb(0.25, 0.95, 0.3);
const UNLIT: Color = Color::srgba(0.05, 0.05, 0.05, 0.8);
const BACK: Color = Color::srgba(0.03, 0.03, 0.03, 0.6);
const EDGE: Color = Color::srgb(0.75, 0.6, 0.3);
const HEARTHSTONE_ICON: &str = r"Interface\Icons\INV_Misc_Rune_01";

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, make_ring)
        .add_systems(Update, (show_hearth, show_pickup));
}

/// A charge's panel on screen: its root, the ring's parts, and the icon it was made with.
#[derive(Default)]
struct Shown {
    root: Option<Entity>,
    parts: Vec<Entity>,
    icon: Option<&'static str>,
}

impl Shown {
    fn clear(&mut self, commands: &mut Commands) {
        if let Some(root) = self.root.take() {
            commands.entity(root).despawn();
        }
        self.parts.clear();
        self.icon = None;
    }
}

/// The ring's masks, made once: every part (the unlit ring), then each part alone.
#[derive(Resource)]
struct RingLook {
    all: Handle<Image>,
    parts: Vec<Handle<Image>>,
}

fn make_ring(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.insert_resource(RingLook {
        all: images.add(ring_image(None)),
        parts: (0..PARTS).map(|k| images.add(ring_image(Some(k)))).collect(),
    });
}

/// The ring's part `k` (clockwise from the top) as a white mask, or every part when `None`.
fn ring_image(k: Option<usize>) -> Image {
    let n = RING_PX;
    let mut rgba = vec![0u8; n * n * 4];
    let span = 360.0 / PARTS as f32;
    for y in 0..n {
        for x in 0..n {
            let dx = (x as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let dy = (y as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let r = dx.hypot(dy);
            // Degrees clockwise from the top.
            let a = dx.atan2(-dy).to_degrees().rem_euclid(360.0);
            let part = ((a / span) as usize).min(PARTS - 1);
            let within = a - part as f32 * span;
            // Soft over about a pixel at every edge.
            let edge = |d: f32| (d * n as f32 * 0.5).clamp(0.0, 1.0);
            let gap = ((within - GAP / 2.0).min(span - GAP / 2.0 - within) / 2.0).clamp(0.0, 1.0);
            let cover = edge(r - INNER).min(edge(OUTER - r)) * gap;
            if k.is_none_or(|k| k == part) {
                rgba[(y * n + x) * 4..][..4].copy_from_slice(&[255, 255, 255, (cover * 255.0) as u8]);
            }
        }
    }
    Image::new(
        Extent3d {
            width: n as u32,
            height: n as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// A panel inside `root`: the unlit ring, its six parts over it, the icon in the middle; the
/// parts' entities, clockwise from the top.
fn panel(
    commands: &mut Commands,
    root: Entity,
    icon: Option<Handle<Image>>,
    ring: &RingLook,
    size: f32,
) -> Vec<Entity> {
    let layer = |h: Handle<Image>, c: Color| {
        (
            ImageNode::new(h).with_color(c),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
        )
    };
    let mut parts = Vec::new();
    commands.entity(root).with_children(|r| {
        r.spawn((
            Node {
                width: Val::Vh(size * RING),
                height: Val::Vh(size * RING),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(Val::Percent(50.0)),
                ..default()
            },
            BackgroundColor(BACK),
        ))
        .with_children(|p| {
            p.spawn(layer(ring.all.clone(), UNLIT));
            for h in &ring.parts {
                parts.push(p.spawn(layer(h.clone(), GREEN.with_alpha(0.0))).id());
            }
            let mut i = p.spawn((
                Node {
                    width: Val::Vh(size),
                    height: Val::Vh(size),
                    border: UiRect::all(Val::Px(1.5)),
                    border_radius: BorderRadius::all(Val::Percent(50.0)),
                    ..default()
                },
                BorderColor::all(EDGE),
            ));
            if let Some(h) = icon {
                i.insert(ImageNode::new(h));
            }
        });
    });
    parts
}

/// The icon at `path` as an image, once the game data is open.
fn icon(screen: &crate::Screen, images: &mut Assets<Image>, path: &str) -> Option<Handle<Image>> {
    screen.icon(path).map(|i| images.add(i))
}

/// The parts lit for `part` of the charge done: whole ones full, the one filling glowing in.
fn set_fill(images: &mut Query<&mut ImageNode>, parts: &[Entity], part: f32) {
    for (k, &e) in parts.iter().enumerate() {
        let lit = (part * PARTS as f32 - k as f32).clamp(0.0, 1.0);
        if let Ok(mut i) = images.get_mut(e) {
            i.color = GREEN.with_alpha(lit);
        }
    }
}

fn show_hearth(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    ring: Res<RingLook>,
    mut images: ResMut<Assets<Image>>,
    mut fills: Query<&mut ImageNode>,
    mut shown: Local<Shown>,
) {
    let Some(part) = survivor_server::hearth::charge() else {
        shown.clear(&mut commands);
        return;
    };
    if shown.root.is_none() {
        let root = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    bottom: Val::Vh(LIFT),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                GlobalZIndex(crate::SCREEN_Z - 30),
                Pickable::IGNORE,
            ))
            .id();
        let h = icon(&screen, &mut images, HEARTHSTONE_ICON);
        shown.parts = panel(&mut commands, root, h, &ring, ICON);
        shown.root = Some(root);
    }
    set_fill(&mut fills, &shown.parts, part);
}

#[allow(clippy::too_many_arguments)]
fn show_pickup(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    ring: Res<RingLook>,
    mut images: ResMut<Assets<Image>>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<WorldCamera>>,
    mut nodes: Query<&mut Node>,
    mut fills: Query<&mut ImageNode>,
    mut visibility: Query<&mut Visibility>,
    mut shown: Local<Shown>,
) {
    let Some(p) = survivor_server::pickup::current() else {
        shown.clear(&mut commands);
        return;
    };
    if shown.icon != Some(p.icon) {
        shown.clear(&mut commands);
    }
    if shown.root.is_none() {
        // A full-screen layer the panel is centred in, moved so its centre is over the object.
        let root = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                GlobalZIndex(crate::SCREEN_Z - 35),
                Pickable::IGNORE,
                Visibility::Hidden,
            ))
            .id();
        let h = icon(&screen, &mut images, p.icon);
        shown.parts = panel(&mut commands, root, h, &ring, PICKUP_ICON);
        shown.root = Some(root);
        shown.icon = Some(p.icon);
    }
    set_fill(&mut fills, &shown.parts, p.part);
    let Some(root) = shown.root else { return };
    let (Ok(window), Ok((camera, eye))) = (window.single(), camera.single()) else {
        return;
    };
    let over = benilla_assets::coords::wow_to_bevy([p.pos[0], p.pos[1], p.pos[2] + OVER]);
    let at = camera.world_to_viewport(eye, over).ok();
    if let Ok(mut vis) = visibility.get_mut(root) {
        vis.set_if_neq(if at.is_some() { Visibility::Inherited } else { Visibility::Hidden });
    }
    if let (Some(at), Ok(mut node)) = (at, nodes.get_mut(root)) {
        // The layer's centre is the screen's: slide it by the object's offset from there.
        node.left = Val::Px(at.x - window.width() / 2.0);
        node.top = Val::Px(at.y - window.height() / 2.0);
    }
}
