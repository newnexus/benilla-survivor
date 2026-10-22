//! The Hearthstone charging ([`survivor_server::hearth`]): a small panel low in the middle of the
//! screen, the Hearthstone's icon beside a bar filling green, gone when it is done or broken.

use bevy::prelude::*;

/// The icon's size, the bar's length and thickness, and how high off the screen's bottom the
/// panel sits, in screen heights (vh).
const ICON: f32 = 6.0;
const BAR: f32 = 30.0;
const THICK: f32 = 1.8;
const LIFT: f32 = 24.0;
const GREEN: Color = Color::srgb(0.18, 0.8, 0.22);
const BACK: Color = Color::srgba(0.03, 0.03, 0.03, 0.85);
const EDGE: Color = Color::srgb(0.75, 0.6, 0.3);

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, show_charge);
}

/// The panel on screen, and its bar's fill.
#[derive(Default)]
struct Shown {
    root: Option<Entity>,
    fill: Option<Entity>,
    icon: Option<Handle<Image>>,
}

fn show_charge(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    mut images: ResMut<Assets<Image>>,
    mut nodes: Query<&mut Node>,
    mut shown: Local<Shown>,
) {
    let charge = survivor_server::hearth::charge();
    let Some(part) = charge else {
        if let Some(root) = shown.root.take() {
            commands.entity(root).despawn();
            shown.fill = None;
        }
        return;
    };
    if shown.root.is_none() {
        if shown.icon.is_none() {
            shown.icon = screen
                .icon("Interface\\Icons\\INV_Misc_Rune_01")
                .map(|i| images.add(i));
        }
        let mut fill = Entity::PLACEHOLDER;
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
            .with_children(|r| {
                r.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Vh(1.2),
                        padding: UiRect::all(Val::Vh(0.8)),
                        border: UiRect::all(Val::Px(1.5)),
                        border_radius: BorderRadius::all(Val::Vh(0.8)),
                        ..default()
                    },
                    BackgroundColor(BACK),
                    BorderColor::all(EDGE),
                ))
                .with_children(|p| {
                    let mut icon = p.spawn((
                        Node {
                            width: Val::Vh(ICON),
                            height: Val::Vh(ICON),
                            border: UiRect::all(Val::Px(1.5)),
                            ..default()
                        },
                        BorderColor::all(EDGE),
                    ));
                    if let Some(h) = &shown.icon {
                        icon.insert(ImageNode::new(h.clone()));
                    }
                    p.spawn((
                        Node {
                            width: Val::Vh(BAR),
                            height: Val::Vh(THICK),
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
            })
            .id();
        shown.root = Some(root);
        shown.fill = Some(fill);
    }
    if let Some(mut node) = shown.fill.and_then(|f| nodes.get_mut(f).ok()) {
        node.width = Val::Percent(part.clamp(0.0, 1.0) * 100.0);
    }
}
