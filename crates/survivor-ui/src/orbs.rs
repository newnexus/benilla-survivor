//! What kills leave: souls (small light blue glowing orbs) and coins (spinning copper, silver or
//! gold pieces, larger for the 5s and 25s), drawn where the
//! server has them ([`survivor_server::orbs`]) and eased toward each new place between its
//! ticks. And the purse: the player's gold in the corner, with WoW's coin icons.

use bevy::prelude::*;
use survivor_server::orbs::Kind;

/// How fast a drawn orb catches up with the server's place for it, per second.
const CATCH_UP: f32 = 14.0;
/// A soul's core and glow radii, a single coin's radius and thickness, yards.
const SOUL: f32 = 0.055;
const SOUL_GLOW: f32 = 0.125;
const COIN: f32 = 0.12;
const COIN_THICK: f32 = 0.025;
/// A 5 and a 25 coin are this much larger than a single one.
const COIN_SIZES: [f32; 3] = [1.0, 1.3, 1.65];
const SOUL_BLUE: Color = Color::srgb(0.45, 0.85, 1.0);
/// Copper, silver and gold, with how much each glows.
const METALS: [(Color, [f32; 3]); 3] = [
    (Color::srgb(0.72, 0.42, 0.2), [0.3, 0.13, 0.04]),
    (Color::srgb(0.85, 0.88, 0.92), [0.3, 0.3, 0.33]),
    (Color::srgb(1.0, 0.8, 0.25), [0.6, 0.45, 0.05]),
];

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, make_looks)
        .add_systems(Update, (sync_orbs, move_orbs, show_purse));
}

#[derive(Resource)]
struct Looks {
    soul: Handle<Mesh>,
    soul_glow: Handle<Mesh>,
    soul_mat: Handle<StandardMaterial>,
    glow_mat: Handle<StandardMaterial>,
    /// Single, 5 and 25.
    coins: [Handle<Mesh>; 3],
    /// Copper, silver and gold.
    metals: [Handle<StandardMaterial>; 3],
}

#[derive(Component)]
struct Drawn {
    id: u64,
    kind: Kind,
    target: Vec3,
    seed: f32,
}

fn make_looks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let glow = |c: Color, a: f32| StandardMaterial {
        base_color: c.with_alpha(a),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        fog_enabled: false,
        ..default()
    };
    commands.insert_resource(Looks {
        soul: meshes.add(Sphere::new(SOUL).mesh().uv(16, 12)),
        soul_glow: meshes.add(Sphere::new(SOUL_GLOW).mesh().uv(16, 12)),
        soul_mat: materials.add(glow(SOUL_BLUE, 1.0)),
        glow_mat: materials.add(glow(SOUL_BLUE, 0.35)),
        coins: COIN_SIZES.map(|k| meshes.add(Cylinder::new(COIN * k, COIN_THICK * k))),
        metals: METALS.map(|(color, [r, g, b])| {
            materials.add(StandardMaterial {
                base_color: color,
                emissive: LinearRgba::rgb(r, g, b),
                metallic: 0.9,
                perceptual_roughness: 0.3,
                ..default()
            })
        }),
    });
}

/// New orbs drawn, taken ones gone, every one's place from the server.
fn sync_orbs(
    mut commands: Commands,
    looks: Option<Res<Looks>>,
    mut drawn: Query<(Entity, &mut Drawn)>,
    mut seen: Local<u64>,
) {
    let Some(looks) = looks else { return };
    let (version, list) = survivor_server::orbs::current();
    if version == *seen {
        return;
    }
    *seen = version;
    let mut have = std::collections::HashMap::new();
    for (e, mut d) in &mut drawn {
        match list.iter().find(|o| o.id == d.id) {
            Some(o) => {
                d.target = benilla_assets::coords::wow_to_bevy(o.pos);
                have.insert(d.id, ());
            }
            None => commands.entity(e).despawn(),
        }
    }
    for o in list.iter().filter(|o| !have.contains_key(&o.id)) {
        let at = benilla_assets::coords::wow_to_bevy(o.pos);
        let mut e = commands.spawn((
            Drawn {
                id: o.id,
                kind: o.kind,
                target: at,
                seed: (o.id % 97) as f32 * 0.37,
            },
            Transform::from_translation(at),
            Visibility::default(),
        ));
        match o.kind {
            Kind::Soul => {
                e.insert((Mesh3d(looks.soul.clone()), MeshMaterial3d(looks.soul_mat.clone())));
                e.with_children(|c| {
                    c.spawn((
                        Mesh3d(looks.soul_glow.clone()),
                        MeshMaterial3d(looks.glow_mat.clone()),
                        Transform::default(),
                    ));
                });
            }
            Kind::Coin => {
                // Copper below 100, silver below 10,000; then single, 5 or 25 of that metal.
                let metal = match o.amount {
                    0..100 => 0,
                    100..10_000 => 1,
                    _ => 2,
                };
                let single = o.amount / [1, 100, 10_000][metal];
                let size = match single {
                    0..5 => 0,
                    5..25 => 1,
                    _ => 2,
                };
                e.insert((
                    Mesh3d(looks.coins[size].clone()),
                    MeshMaterial3d(looks.metals[metal].clone()),
                ));
            }
        }
    }
}

/// Ease each orb toward the server's place; souls bob and breathe, coins spin.
fn move_orbs(time: Res<Time>, mut drawn: Query<(&Drawn, &mut Transform)>) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    let k = (CATCH_UP * dt).min(1.0);
    for (d, mut tf) in &mut drawn {
        let bob = Vec3::Y * (0.06 * (t * 3.0 + d.seed).sin());
        let goal = d.target + bob;
        let here = tf.translation;
        tf.translation = here + (goal - here) * k;
        match d.kind {
            Kind::Soul => {
                tf.scale = Vec3::splat(1.0 + 0.15 * (t * 5.0 + d.seed).sin());
            }
            Kind::Coin => {
                // On edge, turning about the up axis.
                tf.rotation = Quat::from_rotation_y(t * 4.0 + d.seed) * Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
            }
        }
    }
}

#[derive(Component)]
struct Purse;

#[derive(Component)]
struct PurseAmount(usize);

/// The gold in the corner, under the player's portrait: gold, silver and copper with their
/// coins, a part shown only when it has something (copper always).
fn show_purse(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    mut images: ResMut<Assets<Image>>,
    objects: benilla_app::Objects,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut amounts: Query<(&PurseAmount, Option<&mut Text>, &mut Node)>,
    purse: Query<Entity, With<Purse>>,
    mut built: Local<Option<f32>>,
) {
    let height = window.single().map_or(720.0, Window::height);
    let in_world = objects.player().is_some();
    if !in_world || *built != Some(height) {
        for e in &purse {
            commands.entity(e).despawn();
        }
        *built = None;
        if !in_world {
            return;
        }
        let Some(library) = &screen.library else { return };
        let library = library.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(img) = library.blp("Interface\\MoneyFrame\\UI-MoneyIcons") else { return };
        let size = img.size().as_vec2();
        let coins = images.add(img);
        let font = library.font.clone();
        drop(library);
        let vh = height / 100.0;
        commands
            .spawn((
                Purse,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Vh(2.0),
                    top: Val::Vh(crate::PURSE_TOP),
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Vh(0.6),
                    padding: UiRect::axes(Val::Vh(1.0), Val::Vh(0.5)),
                    border_radius: BorderRadius::all(Val::Vh(0.8)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
                GlobalZIndex(crate::SCREEN_Z - 20),
                Pickable::IGNORE,
            ))
            .with_children(|row| {
                for k in 0..3 {
                    row.spawn((
                        PurseAmount(k),
                        Text::new(""),
                        TextFont {
                            font: font.clone(),
                            font_size: 2.6 * vh,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                        TextShadow::default(),
                        Node::default(),
                    ));
                    let w = size.x / 4.0;
                    row.spawn((
                        PurseAmount(k + 3),
                        ImageNode {
                            image: coins.clone(),
                            rect: Some(Rect::new(w * k as f32, 0.0, w * (k + 1) as f32, size.y)),
                            ..default()
                        },
                        Node {
                            width: Val::Vh(2.2),
                            height: Val::Vh(2.2),
                            ..default()
                        },
                    ));
                }
            });
        *built = Some(height);
        return;
    }
    let m = survivor_server::purse::money();
    let parts = [m / 10_000, m / 100 % 100, m % 100];
    let show = [parts[0] > 0, parts[0] > 0 || parts[1] > 0, true];
    for (a, mut text, mut node) in &mut amounts {
        let k = a.0 % 3;
        node.display = if show[k] { Display::Flex } else { Display::None };
        if let Some(text) = text.as_mut() {
            let s = parts[k].to_string();
            if text.0 != s {
                text.0 = s;
            }
        }
    }
}
