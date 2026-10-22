//! Escape's game menu pauses the world: while the stock `GameMenuFrame` is on screen the server
//! is told so ([`survivor_server::menu_pause`]) and stops as it does for a level-up pick.

use bevy::prelude::*;

/// The stock frame Escape shows.
pub(crate) const FRAME: &str = "GameMenuFrame";

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, tell_server);
}

fn tell_server(frames: Res<benilla_app::SurvivorFrames>, mut was: Local<bool>) {
    let open = frames.shown.contains(FRAME);
    if open != *was {
        *was = open;
        survivor_server::menu_pause::set(open);
    }
}
