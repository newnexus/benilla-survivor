//! In town, a trail of small burning footsteps on the ground from the player to every quest giver
//! of that town with something for them ([`survivor_server::trails`]): footprints of the
//! barrier's golden flame (`shaders/footstep.wgsl`), with a short ring of the barrier's tongues
//! rising round each, tallest at the toe (`shaders/footflame.wgsl`). The steps keep to the roads
//! ([`crate::roads`]) and lie on the terrain, and a glow walks along them toward the giver.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;
use survivor_server::town::{town_at, TOWNS};

use crate::ground::Ground;
use crate::roads::Roads;

/// A step every this many yards, each foot this far to its side of the way.
const STRIDE: f32 = 0.75;
const SIDE: f32 = 0.14;
/// A footprint's size, in yards, and how far above the ground it lies.
const LENGTH: f32 = 0.5;
const WIDTH: f32 = 0.25;
const LIFT: f32 = 0.04;
/// No step this close to the player or the giver.
const CLEAR_START: f32 = 1.2;
const CLEAR_END: f32 = 1.5;
/// The glow walks this many yards a second, one every this many yards, lighting this long a
/// stretch behind it.
const FLOW: f32 = 6.0;
const PULSE: f32 = 16.0;
const TAIL: f32 = 7.0;
/// A step's strength between glows, and how the steps thin out near both ends.
const DIM: f32 = 0.35;
const FADE: f32 = 2.5;
/// Strengths the steps are drawn at: one material each.
const LEVELS: usize = 8;
/// The way is found again once the player has moved this far.
const REPLAN: f32 = 0.75;
/// The flame ring round a footprint: its radii as parts of the print's, its height, how many
/// tongues round it, and the points it is drawn through.
const RING_ACROSS: f32 = 0.42;
const RING_ALONG: f32 = 0.44;
const RING_HEIGHT: f32 = 0.6;
const RING_TONGUES: f32 = 4.0;
const RING_POINTS: usize = 24;
/// Where round the print the fire is strongest: the toe (`true`) or the heel.
const FIRE_AT_TOE: bool = true;

/// A footstep's flame: a fragment stage of our own over the stock mesh pipeline.
type StepMaterial = ExtendedMaterial<StandardMaterial, Ember>;

/// The flame's colour (alpha: the step's strength) and the footprint's shape.
#[derive(Asset, AsBindGroup, Clone, TypePath)]
struct Ember {
    #[uniform(100)]
    tint: LinearRgba,
    #[texture(101)]
    #[sampler(102)]
    mask: Handle<Image>,
}

impl MaterialExtension for Ember {
    fn fragment_shader() -> ShaderRef {
        "embedded://survivor_ui/shaders/footstep.wgsl".into()
    }
}

/// The flames rising from a footstep.
type TongueMaterial = ExtendedMaterial<StandardMaterial, Tongues>;

/// The flame's colour; alpha, the step's strength.
#[derive(Asset, AsBindGroup, Clone, TypePath)]
struct Tongues {
    #[uniform(100)]
    tint: LinearRgba,
}

impl MaterialExtension for Tongues {
    fn fragment_shader() -> ShaderRef {
        "embedded://survivor_ui/shaders/footflame.wgsl".into()
    }
}

pub(crate) fn plugin(app: &mut App) {
    bevy::asset::embedded_asset!(app, "shaders/footstep.wgsl");
    bevy::asset::embedded_asset!(app, "shaders/footflame.wgsl");
    app.add_plugins((
        MaterialPlugin::<StepMaterial>::default(),
        MaterialPlugin::<TongueMaterial>::default(),
    ))
        .add_systems(Startup, setup)
        .add_systems(Update, (plan, draw).chain());
}

#[derive(Resource)]
struct StepLook {
    mesh: Handle<Mesh>,
    ring: Handle<Mesh>,
    /// From faint to full.
    materials: Vec<Handle<StepMaterial>>,
    tongues: Vec<Handle<TongueMaterial>>,
}

/// The ground reader and each town's road grid, made when first needed. Not `Send`: the
/// install's archives stay on the main thread.
#[derive(Default)]
struct Planner {
    ground: Option<Ground>,
    roads: Vec<Option<Roads>>,
    /// What the steps were laid for: the player's spot and the givers.
    from: Option<[f32; 2]>,
    ends: Vec<[f32; 3]>,
}

/// The steps laid out, in world space.
#[derive(Resource, Default)]
struct Steps(Vec<Step>);

struct Step {
    at: Vec3,
    turn: Quat,
    left: bool,
    /// Yards from the player along its way, and how far from the way's nearer end.
    along: f32,
    edge: f32,
}

#[derive(Component)]
struct Footprint;

/// A footprint's flame ring, its child.
#[derive(Component)]
struct Ring;

/// The see-through glow both parts are drawn with: unlit and added onto what is behind,
/// depth-tested, writing no depth.
fn glow_base() -> StandardMaterial {
    StandardMaterial {
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        fog_enabled: false,
        depth_bias: 20.0,
        ..default()
    }
}

/// Gold at strength `k` of [`LEVELS`].
fn gold(k: usize) -> LinearRgba {
    crate::barrier::GOLD.to_linear().with_alpha(k as f32 / LEVELS as f32)
}

fn setup(
    world: &mut World,
) {
    world.insert_non_send_resource(Planner::default());
    world.init_resource::<Steps>();
    let image = world.resource_mut::<Assets<Image>>().add(footprint_image());
    let mut meshes = world.resource_mut::<Assets<Mesh>>();
    let (mesh, ring) = (meshes.add(footprint_mesh()), meshes.add(ring_mesh()));
    let mut materials = world.resource_mut::<Assets<StepMaterial>>();
    let materials = (1..=LEVELS)
        .map(|k| {
            materials.add(StepMaterial {
                base: glow_base(),
                extension: Ember {
                    tint: gold(k),
                    mask: image.clone(),
                },
            })
        })
        .collect();
    let mut tongue_materials = world.resource_mut::<Assets<TongueMaterial>>();
    let tongues = (1..=LEVELS)
        .map(|k| {
            tongue_materials.add(TongueMaterial {
                base: glow_base(),
                extension: Tongues { tint: gold(k) },
            })
        })
        .collect();
    world.insert_resource(StepLook {
        mesh,
        ring,
        materials,
        tongues,
    });
}

/// A bare left footprint, toe toward v = 0 and the big toe on the right (+u): five toes, then
/// the sole, wide at the ball, pinched at the arch on the inner side, round at the heel.
fn footprint_image() -> Image {
    const W: usize = 40;
    const H: usize = 80;
    let mut rgba = vec![0u8; W * H * 4];
    // Toes (centre, radius), big toe first, in the image's 0-1 square (v scaled by H/W).
    let toes: [([f32; 2], f32); 5] = [
        ([0.66, 0.1], 0.13),
        ([0.45, 0.07], 0.085),
        ([0.32, 0.1], 0.075),
        ([0.21, 0.15], 0.065),
        ([0.13, 0.21], 0.055),
    ];
    // The sole's edges across its length, from the ball (v 0.25) to the heel (v 0.97).
    let sole = |v: f32| -> Option<(f32, f32)> {
        if !(0.25..=0.97).contains(&v) {
            return None;
        }
        let t = (v - 0.25) / 0.72;
        // Outer edge straight-ish; inner edge pulled in at the arch.
        let outer = 0.1 + 0.12 * t;
        let arch = (-((t - 0.5) / 0.18).powi(2)).exp();
        let inner = 0.86 - 0.08 * t - 0.26 * arch;
        // The ball and the heel round off.
        let round = |e: f32| (e.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2).sin();
        let ends = round((v - 0.25) / 0.1).min(round((0.97 - v) / 0.18));
        let mid = (outer + inner) / 2.0;
        let half = (inner - outer) / 2.0 * ends;
        Some((mid - half, mid + half))
    };
    let aspect = H as f32 / W as f32;
    for y in 0..H {
        for x in 0..W {
            let p = [(x as f32 + 0.5) / W as f32, (y as f32 + 0.5) / H as f32];
            // Distance inside the shape, in image widths: soft over the last few texels.
            let mut inside: f32 = -1.0;
            for (c, r) in toes {
                let d = (p[0] - c[0]).hypot((p[1] - c[1]) * aspect);
                inside = inside.max(r - d);
            }
            if let Some((lo, hi)) = sole(p[1]) {
                inside = inside.max((p[0] - lo).min(hi - p[0]));
            }
            // A wide soft edge: the shader burns brightest there.
            let a = (inside / 0.09 + 0.35).clamp(0.0, 1.0);
            let px = &mut rgba[(y * W + x) * 4..][..4];
            px.copy_from_slice(&[255, 255, 255, (a * 255.0) as u8]);
        }
    }
    Image::new(
        Extent3d {
            width: W as u32,
            height: H as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// A short wall standing round the footprint, an ellipse about its sole. uv.x runs round it in
/// tongues, uv.y from the ground (0) to the top (1); the second uv's x is how near the fire's
/// strong end (the toe, -Z, or the heel) each point is, 0 to 1.
fn ring_mesh() -> Mesh {
    let (a, b) = (WIDTH * RING_ACROSS, LENGTH * RING_ALONG);
    let (mut positions, mut normals, mut uvs, mut uvs_b) = (vec![], vec![], vec![], vec![]);
    for k in 0..=RING_POINTS {
        let t = k as f32 / RING_POINTS as f32;
        let angle = t * std::f32::consts::TAU;
        let (x, z) = (a * angle.sin(), -b * angle.cos());
        // 1 at the toe (angle 0, -Z), 0 at the heel.
        let toe = 0.5 + 0.5 * angle.cos();
        let strong = if FIRE_AT_TOE { toe } else { 1.0 - toe };
        let out = Vec3::new(angle.sin() / a, 0.0, -angle.cos() / b).normalize();
        for (y, v) in [(0.0, 0.0), (RING_HEIGHT, 1.0)] {
            positions.push([x, y, z]);
            normals.push(out.to_array());
            uvs.push([t * RING_TONGUES, v]);
            uvs_b.push([strong, 0.0]);
        }
    }
    let indices = (0..RING_POINTS as u32)
        .flat_map(|k| {
            let (a, b, c, d) = (2 * k, 2 * k + 1, 2 * k + 2, 2 * k + 3);
            [a, c, b, b, c, d]
        })
        .collect();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, uvs_b)
        .with_inserted_indices(Indices::U32(indices))
}

/// A flat quad facing up, its toe toward -Z (Bevy's forward).
fn footprint_mesh() -> Mesh {
    let (w, l) = (WIDTH / 2.0, LENGTH / 2.0);
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[-w, 0.0, -l], [w, 0.0, -l], [w, 0.0, l], [-w, 0.0, l]],
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 4])
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_UV_0,
            vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        )
        .with_inserted_indices(Indices::U32(vec![0, 2, 1, 0, 3, 2]))
}

/// Lay the steps out again when the player has moved or the givers have changed.
fn plan(
    mut planner: NonSendMut<Planner>,
    mut steps: ResMut<Steps>,
    objects: benilla_app::Objects,
    transforms: Query<&GlobalTransform, Without<Footprint>>,
) {
    let ends = survivor_server::trails::current();
    let from = objects
        .player()
        .and_then(|p| transforms.get(p).ok())
        .map(|t| benilla_assets::coords::bevy_to_wow(t.translation()));
    let (Some(from), false) = (from, ends.is_empty()) else {
        planner.from = None;
        steps.0.clear();
        return;
    };
    let start = [from[0], from[1]];
    let moved = planner
        .from
        .is_none_or(|p| (p[0] - start[0]).hypot(p[1] - start[1]) > REPLAN);
    if !moved && planner.ends == ends {
        return;
    }
    let Some(town) = town_at(from) else {
        steps.0.clear();
        return;
    };
    let planner = &mut *planner;
    planner.from = Some(start);
    planner.ends = ends.clone();
    let ground = planner.ground.get_or_insert_with(Ground::open);
    planner.roads.resize_with(TOWNS.len(), || None);
    let roads = planner.roads[town].get_or_insert_with(|| Roads::of(&TOWNS[town], ground));
    let ways = roads.search(start);
    steps.0.clear();
    for end in ends {
        let way = roads.way(&ways, start, [end[0], end[1]]);
        lay(&way, ground, &mut steps.0);
    }
}

/// Steps along `way`, left foot first, skipping any already laid there by an earlier way (two
/// givers' ways share the road until they part).
fn lay(way: &[[f32; 2]], ground: &mut Ground, steps: &mut Vec<Step>) {
    let total: f32 = way.windows(2).map(|s| dist(s[0], s[1])).sum();
    let first = steps.len();
    let mut seg = 0;
    let mut seg_start = 0.0;
    let mut k = 0;
    loop {
        let along = CLEAR_START + k as f32 * STRIDE;
        if along > total - CLEAR_END {
            break;
        }
        while seg + 1 < way.len() - 1 && seg_start + dist(way[seg], way[seg + 1]) < along {
            seg_start += dist(way[seg], way[seg + 1]);
            seg += 1;
        }
        let (a, b) = (way[seg], way[seg + 1]);
        let len = dist(a, b).max(1e-3);
        let t = ((along - seg_start) / len).clamp(0.0, 1.0);
        let dir = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
        let left = k % 2 == 0;
        // WoW's left of `dir` is +90 degrees: x and y as on the map, z up.
        let side = if left { SIDE } else { -SIDE };
        let p = [
            a[0] + (b[0] - a[0]) * t - dir[1] * side,
            a[1] + (b[1] - a[1]) * t + dir[0] * side,
        ];
        k += 1;
        let laid = steps[..first].iter().any(|s| {
            let w = benilla_assets::coords::bevy_to_wow(s.at);
            dist([w[0], w[1]], p) < STRIDE * 0.5
        });
        if laid {
            continue;
        }
        let Some(step) = place(p, dir, ground) else {
            continue;
        };
        steps.push(Step {
            left,
            along,
            edge: (along - CLEAR_START).min(total - CLEAR_END - along),
            ..step
        });
    }
}

/// A footprint at WoW `p` heading `dir`, lying on the slope of the ground under it.
fn place(p: [f32; 2], dir: [f32; 2], ground: &mut Ground) -> Option<Step> {
    let h = |ground: &mut Ground, x: f32, y: f32| ground.height(x, y);
    let z = h(ground, p[0], p[1])?;
    let r = LENGTH / 2.0;
    let ahead = h(ground, p[0] + dir[0] * r, p[1] + dir[1] * r).unwrap_or(z);
    let behind = h(ground, p[0] - dir[0] * r, p[1] - dir[1] * r).unwrap_or(z);
    let to = |x: f32, y: f32, z: f32| benilla_assets::coords::wow_to_bevy([x, y, z]);
    let at = to(p[0], p[1], z);
    let forward = (to(p[0] + dir[0] * r, p[1] + dir[1] * r, ahead)
        - to(p[0] - dir[0] * r, p[1] - dir[1] * r, behind))
    .normalize_or(Vec3::NEG_Z);
    let up = Vec3::Y.reject_from_normalized(forward).normalize_or(Vec3::Y);
    let back = -forward;
    let turn = Quat::from_mat3(&Mat3::from_cols(up.cross(back), up, back));
    Some(Step {
        at: at + up * LIFT,
        turn,
        left: true,
        along: 0.0,
        edge: 0.0,
    })
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

/// Draw the steps every frame from a pool of footprints, the glow walking along them; spare
/// footprints hidden.
fn draw(
    mut commands: Commands,
    look: Option<Res<StepLook>>,
    time: Res<Time>,
    steps: Res<Steps>,
    mut prints: Query<
        (
            &mut Transform,
            &mut Visibility,
            &mut MeshMaterial3d<StepMaterial>,
            &Children,
        ),
        With<Footprint>,
    >,
    mut rings: Query<&mut MeshMaterial3d<TongueMaterial>, With<Ring>>,
) {
    let Some(look) = look else { return };
    let have = prints.iter().count();
    for _ in have..steps.0.len() {
        commands
            .spawn((
                Footprint,
                Mesh3d(look.mesh.clone()),
                MeshMaterial3d(look.materials[0].clone()),
                Transform::default(),
                Visibility::Hidden,
            ))
            .with_child((
                Ring,
                Mesh3d(look.ring.clone()),
                MeshMaterial3d(look.tongues[0].clone()),
                Transform::default(),
            ));
    }
    let head = time.elapsed_secs() * FLOW;
    let mut it = steps.0.iter();
    for (mut tf, mut vis, mut material, children) in &mut prints {
        let Some(s) = it.next() else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        // How far behind the nearest glow ahead of it this step is.
        let behind = (head - s.along).rem_euclid(PULSE);
        let glow = (1.0 - behind / TAIL).max(0.0);
        let fade = (s.edge / FADE).clamp(0.0, 1.0);
        let strength = (DIM + (1.0 - DIM) * glow) * (0.25 + 0.75 * fade);
        let level = ((strength * LEVELS as f32).ceil() as usize).clamp(1, LEVELS) - 1;
        if material.0 != look.materials[level] {
            material.0 = look.materials[level].clone();
        }
        for &child in children {
            if let Ok(mut ring) = rings.get_mut(child) {
                if ring.0 != look.tongues[level] {
                    ring.0 = look.tongues[level].clone();
                }
            }
        }
        tf.translation = s.at;
        tf.rotation = s.turn;
        // The right foot is the left one mirrored.
        tf.scale = Vec3::new(if s.left { 1.0 } else { -1.0 }, 1.0, 1.0);
        vis.set_if_neq(Visibility::Inherited);
    }
}
