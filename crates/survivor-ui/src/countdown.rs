//! The count before a run: stepping out through a town's barrier shows 3, 2, 1 big in the middle
//! of the screen, each popping in large and settling, then "GO!" as the run starts. Stepping back
//! in calls it off and the number goes.

use bevy::prelude::*;

/// The numbers' height at rest, in screen heights, and how much bigger each pops in.
const SIZE: f32 = 0.16;
const POP: f32 = 0.6;
/// How long "GO!" stays.
const GO_FOR: f32 = 0.7;
const GOLD: Color = Color::srgb(1.0, 0.82, 0.0);
const GREEN: Color = Color::srgb(0.45, 1.0, 0.45);

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, show_countdown);
}

#[derive(Component)]
struct CountText;

#[derive(Default)]
struct Shown {
    root: Option<Entity>,
    counting: bool,
    go_since: Option<f32>,
}

fn show_countdown(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    time: Res<Time<Real>>,
    mut texts: Query<(&mut Text, &mut TextFont, &mut TextColor), With<CountText>>,
    mut shown: Local<Shown>,
) {
    let now = time.elapsed_secs();
    let left = survivor_server::town::countdown().filter(|&s| s > 0.0);
    if left.is_none() && shown.counting {
        // Out, not back in: the run has begun.
        if !survivor_server::town::in_town() {
            shown.go_since = Some(now);
        }
    }
    shown.counting = left.is_some();
    let go = shown.go_since.filter(|&t| now - t < GO_FOR);
    // `p` runs 1 → 0 across each number's second (and across "GO!").
    let (label, p, color) = match (left, go) {
        (Some(s), _) => {
            let n = s.ceil();
            (format!("{}", n as u32), 1.0 - (n - s), GOLD)
        }
        (None, Some(t)) => ("GO!".to_owned(), 1.0 - (now - t) / GO_FOR, GREEN),
        (None, None) => {
            shown.go_since = None;
            if let Some(root) = shown.root.take() {
                commands.entity(root).despawn();
            }
            return;
        }
    };
    let Some(library) = &screen.library else { return };
    let height = window.single().map_or(720.0, Window::height);
    // Pops in big, settles fast: the ease-out of the second's first part.
    let pop = p.powi(4);
    let size = height * SIZE * (1.0 + POP * pop);
    let alpha = (0.35 + 0.65 * p.min(1.0) / 0.35).min(1.0);
    if shown.root.is_none() {
        let font = library
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .font
            .clone();
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
                GlobalZIndex(crate::SCREEN_Z - 1),
                Pickable::IGNORE,
            ))
            .with_children(|r| {
                r.spawn((
                    CountText,
                    Text::new(label.clone()),
                    TextFont {
                        font,
                        font_size: size,
                        ..default()
                    },
                    TextColor(color.with_alpha(alpha)),
                    TextShadow::default(),
                ));
            })
            .id();
        shown.root = Some(root);
        return;
    }
    for (mut text, mut font, mut c) in &mut texts {
        if text.0 != label {
            text.0 = label.clone();
        }
        font.font_size = size;
        c.0 = color.with_alpha(alpha);
    }
}
