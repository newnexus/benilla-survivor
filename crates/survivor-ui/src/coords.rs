//! While the game is being made: where the player stands, under their portrait, as the server
//! has it (x, y, z and facing), for placing things in the world. `$SURVIVOR_COORDS=off` hides it.

use bevy::prelude::*;

/// Text height in screen heights; where it sits, in screen heights from the top-left corner.
const SIZE: f32 = 0.02;
const TOP: f32 = 9.5;
const LEFT: f32 = 1.2;
const COLOR: Color = Color::srgb(0.85, 0.82, 0.75);

pub(crate) fn plugin(app: &mut App) {
    if std::env::var("SURVIVOR_COORDS").is_ok_and(|v| v.eq_ignore_ascii_case("off")) {
        return;
    }
    app.add_systems(Update, show_coords);
}

#[derive(Component)]
struct Coords;

fn show_coords(
    mut commands: Commands,
    screen: Res<crate::Screen>,
    window: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut texts: Query<&mut Text, With<Coords>>,
    mut shown: Local<Option<(Entity, f32)>>,
) {
    let height = window.single().map_or(720.0, Window::height);
    let here = survivor_server::coords::get();
    if let Some((root, built)) = *shown {
        if here.is_none() || built != height {
            commands.entity(root).despawn();
            *shown = None;
        }
    }
    let Some(([x, y, z], facing)) = here else { return };
    let line = format!("{x:.1}, {y:.1}, {z:.1}   facing {facing:.2}");
    if shown.is_none() {
        let Some(library) = &screen.library else { return };
        let font = library
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .font
            .clone();
        let root = commands
            .spawn((
                Coords,
                Text::new(line),
                TextFont {
                    font,
                    font_size: height * SIZE,
                    ..default()
                },
                TextColor(COLOR),
                TextShadow::default(),
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Vh(TOP),
                    left: Val::Vh(LEFT),
                    ..default()
                },
                GlobalZIndex(crate::SCREEN_Z - 30),
                Pickable::IGNORE,
            ))
            .id();
        *shown = Some((root, height));
        return;
    }
    for mut t in &mut texts {
        if t.0 != line {
            t.0 = line.clone();
        }
    }
}
