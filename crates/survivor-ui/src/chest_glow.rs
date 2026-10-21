//! A glow round every closed chest during a run, in the colour of the best item inside it:
//! a small ring of the barrier's flame, grey for common up to orange for legendary.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::barrier::{flame, FlameMaterial};

/// The ring's radius and height, yards, and how far it sinks into the ground.
const RADIUS: f32 = 1.4;
const HEIGHT: f32 = 2.4;
const SINK: f32 = 0.6;
const SEGMENTS: usize = 32;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, show_glows);
}

#[derive(Component)]
struct ChestGlow;

#[derive(Default)]
struct Shown {
    version: u64,
    mesh: Option<Handle<Mesh>>,
    materials: Vec<Handle<FlameMaterial>>,
}

fn show_glows(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FlameMaterial>>,
    glows: Query<Entity, With<ChestGlow>>,
    mut shown: Local<Shown>,
) {
    let (version, list) = survivor_server::chest_glow::current();
    if version == shown.version {
        return;
    }
    shown.version = version;
    for e in &glows {
        commands.entity(e).despawn();
    }
    let mesh = shown.mesh.get_or_insert_with(|| meshes.add(ring())).clone();
    if shown.materials.is_empty() {
        shown.materials = (0..=survivor_server::chest_glow::COLLECTION)
            .map(|r| materials.add(flame(crate::rarity_color(r))))
            .collect();
    }
    for (pos, rarity) in list {
        commands.spawn((
            ChestGlow,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(shown.materials[usize::from(rarity).min(shown.materials.len() - 1)].clone()),
            Transform::from_translation(benilla_assets::coords::wow_to_bevy(pos)),
        ));
    }
}

/// An open ring standing on its centre, uv round it and up it.
fn ring() -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    for (row, h) in [-SINK, 0.0, HEIGHT * 0.5, HEIGHT].into_iter().enumerate() {
        let _ = row;
        for s in 0..=SEGMENTS {
            let a = s as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            let (sin, cos) = a.sin_cos();
            positions.push([cos * RADIUS, h, sin * RADIUS]);
            normals.push([cos, 0.0, sin]);
            uvs.push([s as f32 / SEGMENTS as f32 * 4.0, (h / HEIGHT).max(0.0)]);
        }
    }
    let row = SEGMENTS as u32 + 1;
    let mut indices = Vec::new();
    for ring in 0..3u32 {
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
