//! The safe towns' barriers ([`survivor_server::town::TOWNS`]): a wall of golden flame along
//! each, strongest where it meets the ground and fading as it rises, its tongues warping slowly
//! (`shaders/barrier.wgsl`). It glows (additive) rather than tints, so the world behind it stays
//! clear. Shown while the player is in the world. The level gates
//! ([`survivor_server::town::GATES`]) are the same flame in blue, shown while closed.
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
        .add_systems(Update, show_in_world);
}

#[derive(Component)]
struct Barrier;

/// A level gate's wall, by its place in [`GATES`].
#[derive(Component)]
struct GateWall(usize);

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
            Mesh3d(meshes.add(wall(&round, town.centre, town.ground, &mut ground))),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(at),
            Visibility::Hidden,
        ));
    }
    let blue = materials.add(flame(BLUE));
    for (i, gate) in GATES.iter().enumerate() {
        let [a, b] = gate.ends;
        let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        let at = benilla_assets::coords::wow_to_bevy([mid[0], mid[1], 0.0]);
        commands.spawn((
            Barrier,
            GateWall(i),
            Name::new(format!("survivor level gate: {}", gate.name)),
            Mesh3d(meshes.add(wall(&gate.ends, mid, gate.ground, &mut ground))),
            MeshMaterial3d(blue.clone()),
            Transform::from_translation(at),
            Visibility::Hidden,
        ));
    }
}

/// A wall along `line` (a town's outline closed round, or a gate's two ends), standing on the
/// ground under each point, placed from `centre`; `fallback` is the ground where the map has
/// none. Its uv runs along the wall in flame widths, and from the foot (0) to the top (1).
fn wall(line: &[[f32; 2]], centre: [f32; 2], fallback: f32, ground: &mut Ground) -> Mesh {
    // The line walked, a point every [`STEP`] yards.
    let mut path: Vec<([f32; 2], f32)> = Vec::new();
    let mut along = 0.0;
    for i in 0..line.len() - 1 {
        let (p, q) = (line[i], line[i + 1]);
        let len = (q[0] - p[0]).hypot(q[1] - p[1]);
        let steps = (len / STEP).ceil().max(1.0) as usize;
        for k in 0..steps {
            let t = k as f32 / steps as f32;
            path.push(([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t], along + len * t));
        }
        along += len;
    }
    path.push((line[line.len() - 1], along));
    // Flame widths that fit the whole way round evenly.
    let widths = (along / FLAME_WIDTH).round().max(1.0);
    let floor: Vec<f32> = path
        .iter()
        .map(|&([x, y], _)| ground.height(x, y).unwrap_or(fallback))
        .collect();
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    for ring in 0..=RINGS {
        let up = ring as f32 / RINGS as f32;
        let height = -SINK + up * (TOP + SINK);
        for (&([x, y], d), &h) in path.iter().zip(&floor) {
            // Bevy's (x, z) is WoW's (-y, -x), from the centre.
            let (bx, bz) = (-(y - centre[1]), -(x - centre[0]));
            positions.push([bx, h + height, bz]);
            let out = Vec2::new(bx, bz).normalize_or_zero();
            normals.push([out.x, 0.0, out.y]);
            // The ground line is v = 0: the sunk part repeats the seam.
            uvs.push([d / along * widths, (height / TOP).max(0.0)]);
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
