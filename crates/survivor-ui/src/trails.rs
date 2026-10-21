//! In town, a yellow trail of light from the player to every quest giver with something for them
//! ([`survivor_server::trails`]): a line of small glowing motes along the ground, flowing toward
//! the giver, fading out near both ends.

use bevy::prelude::*;

/// A mote every this many yards, flowing this many yards a second, this high off the line.
const SPACING: f32 = 1.6;
const FLOW: f32 = 2.2;
const LIFT: f32 = 0.35;
const MOTE: f32 = 0.11;
/// The motes thin out this far from each end.
const FADE: f32 = 3.0;
/// Motes drawn at most per trail.
const MOTES: usize = 60;
const YELLOW: Color = Color::srgb(1.0, 0.85, 0.2);

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, make_mote)
        .add_systems(Update, draw_trails);
}

#[derive(Resource)]
struct MoteLook {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

#[derive(Component)]
struct Mote;

fn make_mote(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(MoteLook {
        mesh: meshes.add(Sphere::new(MOTE).mesh().uv(8, 6)),
        material: materials.add(StandardMaterial {
            base_color: YELLOW,
            emissive: LinearRgba::rgb(2.0, 1.6, 0.3),
            unlit: true,
            alpha_mode: AlphaMode::Add,
            fog_enabled: false,
            ..default()
        }),
    });
}

/// Lay the motes out along each trail, every frame: a pool reused, spare ones hidden.
fn draw_trails(
    mut commands: Commands,
    look: Option<Res<MoteLook>>,
    time: Res<Time>,
    objects: benilla_app::Objects,
    transforms: Query<&GlobalTransform, Without<Mote>>,
    mut motes: Query<(&mut Transform, &mut Visibility), With<Mote>>,
    pool: Query<Entity, With<Mote>>,
) {
    let Some(look) = look else { return };
    let ends = survivor_server::trails::current();
    let from = objects
        .player()
        .and_then(|p| transforms.get(p).ok())
        .map(GlobalTransform::translation);
    let mut places: Vec<(Vec3, f32)> = Vec::new();
    if let Some(from) = from {
        let shift = (time.elapsed_secs() * FLOW) % SPACING;
        for end in ends {
            let to = benilla_assets::coords::wow_to_bevy(end);
            let len = from.distance(to);
            if len < 2.0 {
                continue;
            }
            let dir = (to - from) / len;
            let mut d = shift;
            let mut n = 0;
            while d < len && n < MOTES {
                let fade = (d / FADE).min((len - d) / FADE).min(1.0);
                places.push((from + dir * d + Vec3::Y * LIFT, fade));
                d += SPACING;
                n += 1;
            }
        }
    }
    let have = pool.iter().count();
    for _ in have..places.len() {
        commands.spawn((
            Mote,
            Mesh3d(look.mesh.clone()),
            MeshMaterial3d(look.material.clone()),
            Transform::default(),
            Visibility::Hidden,
        ));
    }
    let mut it = places.into_iter();
    for (mut tf, mut vis) in &mut motes {
        match it.next() {
            Some((at, fade)) => {
                tf.translation = at;
                tf.scale = Vec3::splat(0.35 + 0.65 * fade);
                vis.set_if_neq(Visibility::Inherited);
            }
            None => {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
}
