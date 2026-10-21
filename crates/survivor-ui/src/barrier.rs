//! The safe towns' barriers ([`survivor_server::town::TOWNS`]): a wall of pale green flame around
//! each, strongest where it meets the ground and fading as it rises, its tongues warping slowly
//! (`shaders/barrier.wgsl`). It glows (additive) rather than tints, so the world behind it stays
//! clear. Shown while the player is in the world.
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
use survivor_server::town::{Town, TOWNS};

/// Segments around a wall.
const SEGMENTS: usize = 160;
/// The wall's height above the ground, and how far below it the foot reaches (a slope between
/// two samples must not show a gap).
const TOP: f32 = 12.0;
const SINK: f32 = 1.5;
/// Rings between the foot and the top, for the terrain-following and the fade.
const RINGS: usize = 6;
/// The flame's pattern repeats every this many yards around the wall.
const FLAME_WIDTH: f32 = 4.0;
/// A map tile's edge and a terrain chunk's, in yards.
const TILE: f32 = 533.333_3;
const CHUNK: f32 = TILE / 16.0;

/// The flame: a fragment stage of our own over the stock mesh pipeline.
type FlameMaterial = ExtendedMaterial<StandardMaterial, Flame>;

#[derive(Asset, AsBindGroup, Clone, TypePath)]
struct Flame {}

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

fn spawn_walls(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FlameMaterial>>,
) {
    let mut ground = Ground::open();
    let material = materials.add(FlameMaterial {
        base: StandardMaterial {
            // Unlit and added onto what is behind: a glow, depth-tested, writing no depth.
            unlit: true,
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            fog_enabled: false,
            ..default()
        },
        extension: Flame {},
    });
    for town in TOWNS {
        let at = benilla_assets::coords::wow_to_bevy([town.centre[0], town.centre[1], 0.0]);
        commands.spawn((
            Barrier,
            Name::new(format!("survivor barrier: {}", town.name)),
            Mesh3d(meshes.add(wall(town, &mut ground))),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(at),
            Visibility::Hidden,
        ));
    }
}

/// An open cylinder round the town whose rings stand on the ground under each segment. Its uv
/// runs around the wall in flame widths, and from the foot (0) to the top (1).
fn wall(town: &Town, ground: &mut Ground) -> Mesh {
    let around = (std::f32::consts::TAU * town.radius / FLAME_WIDTH).round();
    // Bevy's (x, z) is WoW's (-y, -x): a segment at angle `a` stands at WoW
    // (cx - sin a * r, cy - cos a * r).
    let floor: Vec<f32> = (0..=SEGMENTS)
        .map(|s| {
            let a = s as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            let (sin, cos) = a.sin_cos();
            ground
                .height(town.centre[0] - sin * town.radius, town.centre[1] - cos * town.radius)
                .unwrap_or(town.ground)
        })
        .collect();
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    for ring in 0..=RINGS {
        let up = ring as f32 / RINGS as f32;
        let height = -SINK + up * (TOP + SINK);
        for (s, &h) in floor.iter().enumerate() {
            let a = s as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            let (sin, cos) = a.sin_cos();
            positions.push([cos * town.radius, h + height, sin * town.radius]);
            normals.push([cos, 0.0, sin]);
            // The ground line is v = 0: the sunk part repeats the seam.
            uvs.push([s as f32 / SEGMENTS as f32 * around, (height / TOP).max(0.0)]);
        }
    }
    let row = SEGMENTS as u32 + 1;
    let mut indices = Vec::new();
    for ring in 0..RINGS as u32 {
        for s in 0..SEGMENTS as u32 {
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

/// Terrain heights from the Eastern Kingdoms' map tiles, read as asked.
struct Ground {
    chain: Option<benilla_formats::Chain>,
    tiles: std::collections::HashMap<(i32, i32), Option<Vec<benilla_adt::McnkChunk>>>,
}

impl Ground {
    fn open() -> Self {
        let chain = benilla_formats::wow_data()
            .and_then(|d| benilla_formats::Chain::open(&d).ok());
        Self {
            chain,
            tiles: std::collections::HashMap::new(),
        }
    }

    /// The ground at WoW `(x, y)`: the nearest of the chunk's outer height grid's corners,
    /// blended across the cell.
    fn height(&mut self, x: f32, y: f32) -> Option<f32> {
        let (col, row) = ((32.0 - y / TILE).floor() as i32, (32.0 - x / TILE).floor() as i32);
        let chain = self.chain.as_mut()?;
        let chunks = self.tiles.entry((col, row)).or_insert_with(|| {
            let bytes = chain
                .read(&format!("World\\Maps\\Azeroth\\Azeroth_{col}_{row}.adt"))
                .ok()?;
            let benilla_adt::ParsedAdt::Root(adt) =
                benilla_adt::parse_adt(&mut std::io::Cursor::new(&bytes[..])).ok()?;
            Some(adt.mcnk_chunks)
        });
        // A chunk's `position` is its corner at the largest x and y.
        let c = chunks.as_ref()?.iter().find(|c| {
            let p = c.header.position;
            x <= p[0] && x > p[0] - CHUNK && y <= p[1] && y > p[1] - CHUNK
        })?;
        let heights = &c.heights.as_ref()?.heights;
        let unit = CHUNK / 8.0;
        let p = c.header.position;
        // Outer vertex (i, j) is row i of 17 (nine outer, eight inner), column j.
        let (fi, fj) = ((p[0] - x) / unit, (p[1] - y) / unit);
        let (i, j) = ((fi.floor() as usize).min(7), (fj.floor() as usize).min(7));
        let (ti, tj) = (fi - i as f32, fj - j as f32);
        let at = |i: usize, j: usize| heights.get(i * 17 + j).copied();
        let top = at(i, j)? * (1.0 - tj) + at(i, j + 1)? * tj;
        let bottom = at(i + 1, j)? * (1.0 - tj) + at(i + 1, j + 1)? * tj;
        Some(p[2] + top * (1.0 - ti) + bottom * ti)
    }
}

/// Walls only in the world, never over the login and character screens.
fn show_in_world(objects: benilla_app::Objects, mut walls: Query<&mut Visibility, With<Barrier>>) {
    let want = if objects.player().is_some() {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut v in &mut walls {
        v.set_if_neq(want);
    }
}
