//! Charges on screen: an icon beside a bar filling green, gone when done or broken.
//! - The Hearthstone ([`survivor_server::hearth`]): low in the middle of the screen.
//! - A pickup ([`survivor_server::pickup`], a gather object being gathered): over the object,
//!   its spot in the world turned to a spot on the screen every frame.

use bevy::prelude::*;
use benilla_world::view::WorldCamera;

/// The icon's size, the bar's length and thickness, in screen heights (vh), for the Hearthstone
/// and for a pickup.
const ICON: f32 = 6.0;
const BAR: f32 = 30.0;
const THICK: f32 = 1.8;
const PICKUP_ICON: f32 = 4.5;
const PICKUP_BAR: f32 = 16.0;
const PICKUP_THICK: f32 = 1.4;
/// How high off the screen's bottom the Hearthstone's panel sits, vh; how far over a pickup's
/// object its panel floats, yards.
const LIFT: f32 = 24.0;
const OVER: f32 = 1.6;
const GREEN: Color = Color::srgb(0.18, 0.8, 0.22);
const BACK: Color = Color::srgba(0.03, 0.03, 0.03, 0.85);
const EDGE: Color = Color::srgb(0.75, 0.6, 0.3);
const HEARTHSTONE_ICON: &str = r"Interface\Icons\INV_Misc_Rune_01";

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, (show_hearth, show_pickup));
}

/// A charge's panel on screen: its root, its bar's fill, and the icon it was made with.
#[derive(Default)]
struct Shown {
    root: Option<Entity>,
    fill: Option<Entity>,
    icon: Option<&'static str>,
}

impl Shown {
    fn clear(&mut self, commands: &mut Commands) {
        if let Some(root) = self.root.take() {
            commands.entity(root).despawn();
        }
        self.fill = None;
        self.icon = None;
    }
}

/// Sizes of a panel, vh: icon, bar length, bar thickness.
struct Look {
    icon: f32,
    bar: f32,
    thick: f32,
}

/// A panel inside `root`: the icon, then the bar; its fill's entity.
fn panel(
    commands: &mut Commands,
    root: Entity,
    icon: Option<Handle<Image>>,
    look: &Look,
) -> Entity {
    let mut fill = Entity::PLACEHOLDER;
    commands.entity(root).with_children(|r| {
        r.spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Vh(look.icon * 0.2),
                padding: UiRect::all(Val::Vh(look.icon * 0.13)),
                border: UiRect::all(Val::Px(1.5)),
                border_radius: BorderRadius::all(Val::Vh(0.8)),
                ..default()
            },
            BackgroundColor(BACK),
            BorderColor::all(EDGE),
        ))
        .with_children(|p| {
            let mut i = p.spawn((
                Node {
                    width: Val::Vh(look.icon),
                    height: Val::Vh(look.icon),
                    border: UiRect::all(Val::Px(1.5)),
                    ..default()
                },
                BorderColor::all(EDGE),
            ));
            if let Some(h) = icon {
                i.insert(ImageNode::new(h));
            }
            p.spawn((
                Node {
                    width: Val::Vh(look.bar),
                    height: Val::Vh(look.thick),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.06, 0.06, 0.06)),
                BorderColor::all(EDGE),
            ))
            .with_children(|b| {
                fill = b
                    .spawn((
                        Node {
                            width: Val::Percent(0.0),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        BackgroundColor(GREEN),
                    ))
                    .id();
            });
        });
    });
    fill
}

/// The icon at `path` as an image, once the game data is open.
fn icon(screen: &crate::Screen, images: &mut Assets<Image>, path: &str) -> Option<Handle<Image>> {
    screen.icon(path).map(|i| images.add(i))
}

fn set_fill(nodes: &mut Query<&mut Node>, fill: Option<Entity>, part: f32) {
    if let Some(mut node) = fill.and_then(|f| nodes.get_mut(f).ok()) {
        node.width = Val::Percent(part.clamp(0.0, 1.0) * 100.0);
    }
}

fn show_hearth(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    mut images: ResMut<Assets<Image>>,
    mut nodes: Query<&mut Node>,
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
        let look = Look {
            icon: ICON,
            bar: BAR,
            thick: THICK,
        };
        let h = icon(&screen, &mut images, HEARTHSTONE_ICON);
        shown.fill = Some(panel(&mut commands, root, h, &look));
        shown.root = Some(root);
    }
    set_fill(&mut nodes, shown.fill, part);
}

#[allow(clippy::too_many_arguments)]
fn show_pickup(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    mut images: ResMut<Assets<Image>>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<WorldCamera>>,
    mut nodes: Query<&mut Node>,
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
        let look = Look {
            icon: PICKUP_ICON,
            bar: PICKUP_BAR,
            thick: PICKUP_THICK,
        };
        let h = icon(&screen, &mut images, p.icon);
        shown.fill = Some(panel(&mut commands, root, h, &look));
        shown.root = Some(root);
        shown.icon = Some(p.icon);
    }
    set_fill(&mut nodes, shown.fill, p.part);
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
