//! The run's clock, top centre while out of town ([`survivor_server::run_clock`]): how long the
//! run has lasted, big, and under it the next boss and how long until it comes. The last
//! seconds before a boss turn red and pulse.

use bevy::prelude::*;
use survivor_server::run_clock::Clock;

/// Text heights, in screen heights, and the gap from the top.
const BIG: f32 = 0.045;
const SMALL: f32 = 0.022;
const TOP: f32 = 1.5;
/// A boss this close turns the line red.
const WARN: f32 = 10.0;
const WHITE: Color = Color::srgb(1.0, 0.96, 0.88);
const GOLD: Color = Color::srgb(1.0, 0.82, 0.0);
const RED: Color = Color::srgb(1.0, 0.25, 0.2);

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, show_clock);
}

#[derive(Component)]
struct Survived;

#[derive(Component)]
struct NextBoss;

/// `m:ss`, or `h:mm:ss` past the hour.
fn clock(secs: f32) -> String {
    let s = secs.max(0.0) as u32;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn show_clock(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    time: Res<Time<Real>>,
    mut survived: Query<&mut Text, (With<Survived>, Without<NextBoss>)>,
    mut next: Query<(&mut Text, &mut TextColor), (With<NextBoss>, Without<Survived>)>,
    mut shown: Local<Option<(Entity, f32)>>,
) {
    let height = window.single().map_or(720.0, Window::height);
    let run: Option<Clock> = survivor_server::run_clock::current();
    // Built for this window height; gone in town, rebuilt when the window changes.
    if let Some((root, built)) = *shown {
        if run.is_none() || built != height {
            commands.entity(root).despawn();
            *shown = None;
        }
    }
    let Some(run) = run else { return };
    if shown.is_none() {
        let Some(library) = &screen.library else { return };
        let font = library
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .font
            .clone();
        let text = |size: f32| TextFont {
            font: font.clone(),
            font_size: height * size,
            ..default()
        };
        let root = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    top: Val::Vh(TOP),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    ..default()
                },
                GlobalZIndex(crate::SCREEN_Z - 30),
                Pickable::IGNORE,
            ))
            .with_children(|r| {
                r.spawn((Survived, Text::new(""), text(BIG), TextColor(WHITE), TextShadow::default()));
                r.spawn((NextBoss, Text::new(""), text(SMALL), TextColor(GOLD), TextShadow::default()));
            })
            .id();
        *shown = Some((root, height));
        return;
    }
    let big = clock(run.survived);
    for mut t in &mut survived {
        if t.0 != big {
            t.0 = big.clone();
        }
    }
    let line = if run.boss.is_empty() {
        format!("Next boss {}", clock(run.boss_in.ceil()))
    } else {
        format!("Next boss: {}  {}", run.boss, clock(run.boss_in.ceil()))
    };
    let colour = if run.boss_in <= WARN {
        let pulse = 0.5 + 0.5 * (time.elapsed_secs() * 6.0).sin();
        RED.mix(&WHITE, pulse * 0.4)
    } else {
        GOLD
    };
    for (mut t, mut c) in &mut next {
        if t.0 != line {
            t.0 = line.clone();
        }
        c.0 = colour;
    }
}
