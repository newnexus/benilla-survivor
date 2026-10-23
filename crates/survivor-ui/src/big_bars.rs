//! Elites' and bosses' health along the top of the screen: the name over a long thin red bar, the
//! part just lost pale for a moment before it drains. They stay up while the creature is in view;
//! several stack downward, the first highest.

use bevy::prelude::*;

/// A bar's length and thickness, the gap between stacked bars, and how far down from the screen's
/// top the first one sits, in screen heights (vh), below the run clock.
const LENGTH: f32 = 70.0;
const THICK: f32 = 1.1;
const GAP: f32 = 1.4;
const TOP: f32 = 9.0;
const NAME: f32 = 2.6;
/// The part just lost holds this long, then drains this much of the bar a second.
const HOLD: f32 = 0.6;
const DRAIN: f32 = 0.6;
const RED: Color = Color::srgb(0.62, 0.06, 0.05);
/// Someone to protect: a trainer defended or escorted.
const GREEN: Color = Color::srgb(0.15, 0.6, 0.1);
const LOST: Color = Color::srgba(0.95, 0.82, 0.5, 0.9);
const BACK: Color = Color::srgba(0.02, 0.01, 0.01, 0.75);
const EDGE: Color = Color::srgba(0.45, 0.38, 0.25, 0.9);
const NAME_COLOR: Color = Color::srgb(0.92, 0.9, 0.85);

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, show_bars);
}

#[derive(Component)]
struct BarsRoot;

/// One creature's bar on screen: its row, fill and drain, and what it shows.
struct Shown {
    row: Entity,
    fill: Entity,
    lost: Entity,
    health: f32,
    trail: f32,
    hold_until: f32,
}

#[derive(Default)]
struct State {
    root: Option<Entity>,
    bars: Vec<(u64, Shown)>,
}

#[allow(clippy::too_many_arguments)]
fn show_bars(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    objects: benilla_app::Objects,
    time: Res<Time<Real>>,
    mut nodes: Query<&mut Node>,
    mut state: Local<State>,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    let list = if objects.player().is_some() {
        survivor_server::big_bars::current()
    } else {
        Vec::new()
    };
    // Gone from view: its row goes.
    let keep: Vec<u64> = list.iter().map(|b| b.guid).collect();
    state.bars.retain(|(guid, s)| {
        let stays = keep.contains(guid);
        if !stays {
            commands.entity(s.row).despawn();
        }
        stays
    });
    if list.is_empty() {
        return;
    }
    let Some(font) = screen.font() else { return };
    let root = *state.root.get_or_insert_with(|| {
        commands
            .spawn((
                BarsRoot,
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    top: Val::Vh(TOP),
                    // Stacked downward: the first bar at the top.
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Vh(GAP),
                    ..default()
                },
                GlobalZIndex(crate::SCREEN_Z - 45),
                Pickable::IGNORE,
            ))
            .id()
    });
    for b in &list {
        let part = if b.max == 0 { 0.0 } else { b.health as f32 / b.max as f32 };
        if !state.bars.iter().any(|(g, _)| *g == b.guid) {
            let (mut fill, mut lost) = (Entity::PLACEHOLDER, Entity::PLACEHOLDER);
            let row = commands
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Vh(0.4),
                    width: Val::Vh(LENGTH),
                    max_width: Val::Vw(70.0),
                    ..default()
                })
                .with_children(|r| {
                    r.spawn((
                        Text::new(b.name.clone()),
                        TextFont {
                            font: font.clone(),
                            font_size: 18.0,
                            ..default()
                        },
                        TextColor(NAME_COLOR),
                        TextShadow::default(),
                        crate::quest_panel::VhText(NAME),
                    ));
                    r.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Vh(THICK),
                            border: UiRect::all(Val::Px(1.0)),
                            ..default()
                        },
                        BackgroundColor(BACK),
                        BorderColor::all(EDGE),
                    ))
                    .with_children(|bar| {
                        let inner = |c: Color| {
                            (
                                Node {
                                    position_type: PositionType::Absolute,
                                    height: Val::Percent(100.0),
                                    width: Val::Percent(100.0),
                                    ..default()
                                },
                                BackgroundColor(c),
                            )
                        };
                        lost = bar.spawn(inner(LOST)).id();
                        fill = bar.spawn(inner(if b.friendly { GREEN } else { RED })).id();
                    });
                })
                .id();
            commands.entity(root).add_child(row);
            state.bars.push((
                b.guid,
                Shown {
                    row,
                    fill,
                    lost,
                    health: part,
                    trail: part,
                    hold_until: 0.0,
                },
            ));
        }
        let Some((_, s)) = state.bars.iter_mut().find(|(g, _)| *g == b.guid) else { continue };
        if part < s.health {
            s.hold_until = now + HOLD;
        }
        s.health = part;
        if now >= s.hold_until {
            s.trail = (s.trail - DRAIN * dt).max(s.health);
        }
        s.trail = s.trail.max(s.health);
        if let Ok(mut n) = nodes.get_mut(s.fill) {
            n.width = Val::Percent(s.health * 100.0);
        }
        if let Ok(mut n) = nodes.get_mut(s.lost) {
            n.width = Val::Percent(s.trail * 100.0);
        }
    }
}
