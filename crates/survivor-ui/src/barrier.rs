//! The safe towns' barriers ([`survivor_server::town::TOWNS`]): a wall of golden flame along
//! each, strongest where it meets the ground and fading as it rises, its tongues warping slowly
//! (`shaders/barrier.wgsl`). It glows (additive) rather than tints, so the world behind it stays
//! clear. Shown while the player is in the world. The level gates
//! ([`survivor_server::town::GATES`]) are the same flame in blue, taller and denser, shown while
//! closed, with the level they ask for written over them.
//!
//! World coordinates are absolute in benilla (no floating origin), so each wall is placed once,
//! through the client's own WoW-to-Bevy transform. Its foot follows the terrain: the ground
//! under every segment is read from the map tiles (`MCVT`), as the server never knows it.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use survivor_server::town::{GATES, TOWNS};

use crate::ground::Ground;

/// A wall point every this many yards along the outline.
const STEP: f32 = 1.5;
/// The wall's height above the ground, and how far below it the foot reaches (a slope between
/// two samples must not show a gap).
const TOP: f32 = 12.0;
const SINK: f32 = 1.5;
/// Rings between the foot and the top, for the terrain-following and the fade.
const RINGS: usize = 6;
/// The flame's pattern repeats every this many yards around the wall.
const FLAME_WIDTH: f32 = 4.0;
/// A level gate's wall: its height, and its flame's width, a quarter of a town's: four times the
/// tongues.
const GATE_TOP: f32 = 36.0;
const GATE_FLAME_WIDTH: f32 = 1.0;
/// A gate's level is written on its wall this far over the player's feet, where the wall is
/// nearest them, readable out to this many yards, at this part of the screen's height. The camera
/// looks down, so a wall far off would put it over the screen's top: it stays at least this far
/// down the screen, under the run clock.
const LABEL_OVER: f32 = 2.0;
const LABEL_TOP: f32 = 0.15;
const LABEL_RANGE: f32 = 150.0;
const LABEL_SIZE: f32 = 0.045;

/// The flame: a fragment stage of our own over the stock mesh pipeline.
pub(crate) type FlameMaterial = ExtendedMaterial<StandardMaterial, Flame>;

/// The flame's colour: its hottest is this, paling toward white.
#[derive(Asset, AsBindGroup, Clone, TypePath)]
pub(crate) struct Flame {
    #[uniform(100)]
    tint: LinearRgba,
}

/// The barrier's gold, and the level gates' blue.
pub(crate) const GOLD: Color = Color::srgb(1.0, 0.72, 0.18);
const BLUE: Color = Color::srgb(0.22, 0.55, 1.0);

/// A flame material of `color`: glowing, see-through, both faces.
pub(crate) fn flame(color: Color) -> FlameMaterial {
    FlameMaterial {
        base: StandardMaterial {
            // Unlit and added onto what is behind: a glow, depth-tested, writing no depth.
            unlit: true,
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            fog_enabled: false,
            ..default()
        },
        extension: Flame {
            tint: color.to_linear(),
        },
    }
}

impl MaterialExtension for Flame {
    fn fragment_shader() -> ShaderRef {
        "embedded://survivor_ui/shaders/barrier.wgsl".into()
    }
}

pub(crate) fn plugin(app: &mut App) {
    bevy::asset::embedded_asset!(app, "shaders/barrier.wgsl");
    app.add_plugins(MaterialPlugin::<FlameMaterial>::default())
        .add_systems(Startup, spawn_walls)
        .add_systems(Update, (show_in_world, place_labels).chain());
}

#[derive(Component)]
struct Barrier;

/// A level gate's wall, by its place in [`GATES`].
#[derive(Component)]
struct GateWall(usize);

/// The writing over a level gate's wall: its level, by the gate's place in [`GATES`].
#[derive(Component)]
struct GateLabel(usize);

fn spawn_walls(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FlameMaterial>>,
) {
    let mut ground = Ground::open();
    let material = materials.add(flame(GOLD));
    for town in TOWNS {
        let at = benilla_assets::coords::wow_to_bevy([town.centre[0], town.centre[1], 0.0]);
        let mut round = town.outline.to_vec();
        round.push(town.outline[0]);
        commands.spawn((
            Barrier,
            Name::new(format!("survivor barrier: {}", town.name)),
            Mesh3d(meshes.add(wall(
                &round,
                town.centre,
                town.ground,
                TOP,
                FLAME_WIDTH,
                &mut ground,
            ))),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(at),
            Visibility::Hidden,
        ));
    }
    // Taller and with narrower tongues than a town's: the pattern runs finer up the wall by both,
    // so the tongues keep a town barrier's shape (the shader reads it from the tint's alpha).
    let mut gate_flame = flame(BLUE);
    gate_flame.extension.tint.alpha = (GATE_TOP / TOP) * (FLAME_WIDTH / GATE_FLAME_WIDTH);
    let blue = materials.add(gate_flame);
    for (i, gate) in GATES.iter().enumerate() {
        let [a, b] = gate.ends;
        let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        let at = benilla_assets::coords::wow_to_bevy([mid[0], mid[1], 0.0]);
        commands.spawn((
            Barrier,
            GateWall(i),
            Name::new(format!("survivor level gate: {}", gate.name)),
            Mesh3d(meshes.add(wall(
                &gate.ends,
                mid,
                gate.ground,
                GATE_TOP,
                GATE_FLAME_WIDTH,
                &mut ground,
            ))),
            MeshMaterial3d(blue.clone()),
            Transform::from_translation(at),
            Visibility::Hidden,
        ));
    }
}

/// A wall along `line` (a town's outline closed round, or a gate's two ends), standing on the
/// ground under each point, placed from `centre`; `fallback` is the ground where the map has
/// none; `top` is its height. Its uv runs along the wall in `flame_width`s, and from the foot (0)
/// to the top (1).
fn wall(
    line: &[[f32; 2]],
    centre: [f32; 2],
    fallback: f32,
    top: f32,
    flame_width: f32,
    ground: &mut Ground,
) -> Mesh {
    // The line walked, a point every [`STEP`] yards.
    let mut path: Vec<([f32; 2], f32)> = Vec::new();
    let mut along = 0.0;
    for i in 0..line.len() - 1 {
        let (p, q) = (line[i], line[i + 1]);
        let len = (q[0] - p[0]).hypot(q[1] - p[1]);
        let steps = (len / STEP).ceil().max(1.0) as usize;
        for k in 0..steps {
            let t = k as f32 / steps as f32;
            path.push((
                [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t],
                along + len * t,
            ));
        }
        along += len;
    }
    path.push((line[line.len() - 1], along));
    // Flame widths that fit the whole way round evenly.
    let widths = (along / flame_width).round().max(1.0);
    let floor: Vec<f32> = path
        .iter()
        .map(|&([x, y], _)| ground.height(x, y).unwrap_or(fallback))
        .collect();
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    for ring in 0..=RINGS {
        let up = ring as f32 / RINGS as f32;
        let height = -SINK + up * (top + SINK);
        for (&([x, y], d), &h) in path.iter().zip(&floor) {
            // Bevy's (x, z) is WoW's (-y, -x), from the centre.
            let (bx, bz) = (-(y - centre[1]), -(x - centre[0]));
            positions.push([bx, h + height, bz]);
            let out = Vec2::new(bx, bz).normalize_or_zero();
            normals.push([out.x, 0.0, out.y]);
            // The ground line is v = 0: the sunk part repeats the seam.
            uvs.push([d / along * widths, (height / top).max(0.0)]);
        }
    }
    let segments = path.len() - 1;
    let row = segments as u32 + 1;
    let mut indices = Vec::new();
    for ring in 0..RINGS as u32 {
        for s in 0..segments as u32 {
            let (a, b) = (ring * row + s, ring * row + s + 1);
            let (c, d) = (a + row, b + row);
            indices.extend([a, c, b, b, c, d]);
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

/// Walls only in the world, never over the login and character screens; a level gate only
/// while closed.
fn show_in_world(
    objects: benilla_app::Objects,
    mut walls: Query<(&mut Visibility, Option<&GateWall>), With<Barrier>>,
) {
    let in_world = objects.player().is_some();
    for (mut v, gate) in &mut walls {
        let open = gate.is_some_and(|g| survivor_server::town::gate_open(g.0));

        let want = if in_world && !open {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        v.set_if_neq(want);
    }
}

/// Each closed gate's level, written over its wall where the camera sees it, out to
/// [`LABEL_RANGE`]; made once the game font has loaded.
#[allow(clippy::too_many_arguments)]
fn place_labels(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    walls: Query<(&GateWall, &Visibility)>,
    objects: benilla_app::Objects,
    transforms: Query<&GlobalTransform>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<benilla_world::view::WorldCamera>>,
    mut labels: Query<(&GateLabel, &mut Node, &mut Visibility, &mut TextFont), Without<GateWall>>,
    mut made: Local<bool>,
) {
    if !*made {
        let Some(font) = screen.font() else {
                return;
        };
        for (i, gate) in GATES.iter().enumerate() {
            commands.spawn((
                GateLabel(i),
                Text::new(format!("Level {} required", gate.level)),
                TextFont {
                    font: font.clone(),
                    font_size: 24.0,
                    ..default()
                },
                TextColor(Color::srgb(0.65, 0.85, 1.0)),
                TextShadow::default(),
                TextLayout::new_with_justify(Justify::Center),
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                GlobalZIndex(crate::SCREEN_Z - 50),
                Pickable::IGNORE,
                Visibility::Hidden,
            ));
        }
        *made = true;
        return;
    }
    let (Ok(window), Ok((camera, eye))) = (window.single(), camera.single()) else {
        return;
    };
    let me = objects
        .player()
        .and_then(|p| transforms.get(p).ok())
        .map(|t| benilla_assets::coords::bevy_to_wow(t.translation()));
    let size = window.height() * LABEL_SIZE;
    for (label, mut node, mut vis, mut font) in &mut labels {
        let shown = walls.iter().any(|(w, v)| w.0 == label.0 && *v != Visibility::Hidden);
        let at = me.filter(|_| shown).and_then(|me| {
            // The wall's point nearest the player, a little in from its ends.
            let [a, b] = GATES[label.0].ends;
            let d = [b[0] - a[0], b[1] - a[1]];
            let len2 = (d[0] * d[0] + d[1] * d[1]).max(1e-3);
            let t = (((me[0] - a[0]) * d[0] + (me[1] - a[1]) * d[1]) / len2).clamp(0.1, 0.9);
            let p = [a[0] + d[0] * t, a[1] + d[1] * t, me[2] + LABEL_OVER];
            if (p[0] - me[0]).hypot(p[1] - me[1]) > LABEL_RANGE {
                return None;
            }
            camera.world_to_viewport(eye, benilla_assets::coords::wow_to_bevy(p)).ok()
        });
        let Some(at) = at else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        if font.font_size != size {
            font.font_size = size;
        }
        // Centred on the point: the text is about eight letters of half its height wide.
        let width = size * 0.5 * 16.0;
        node.left = Val::Px(at.x - width / 2.0);
        node.top = Val::Px((at.y - size / 2.0).max(window.height() * LABEL_TOP));
        node.width = Val::Px(width);
        vis.set_if_neq(Visibility::Inherited);
    }
}
