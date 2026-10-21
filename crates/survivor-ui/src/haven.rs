//! In a safe town the main bar's skills are hidden: nothing is fought there, and the bar is the
//! run's cast queue. They come back as the player steps out through the barrier.
//!
//! The stock interface re-shows a button whenever its action changes, so while in town the hide
//! is said again every half second.

use bevy::prelude::*;

/// How often the hide is repeated in town, seconds.
const EVERY: f32 = 0.5;

const HIDE: &str = r#"
for i = 1, 12 do
    local b = getglobal("ActionButton" .. i)
    if b and b:IsShown() then b:Hide() end
end
for _, n in ipairs({ "ActionBarUpButton", "ActionBarDownButton", "MainMenuBarPageNumber" }) do
    local f = getglobal(n)
    if f and f:IsShown() then f:Hide() end
end
"#;

// The stock update decides each button again: an empty one stays hidden, as before.
const SHOW: &str = r#"
for i = 1, 12 do
    local b = getglobal("ActionButton" .. i)
    if b then
        b:Show()
        this = b
        pcall(ActionButton_Update)
    end
end
for _, n in ipairs({ "ActionBarUpButton", "ActionBarDownButton", "MainMenuBarPageNumber" }) do
    local f = getglobal(n)
    if f then f:Show() end
end
"#;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, hide_in_town);
}

fn hide_in_town(
    time: Res<Time>,
    objects: benilla_app::Objects,
    mut frames: ResMut<benilla_app::SurvivorFrames>,
    mut state: Local<(bool, f32)>,
) {
    let in_town = objects.player().is_some() && survivor_server::town::in_town();
    let (was, since) = &mut *state;
    *since += time.delta_secs();
    if in_town && (!*was || *since >= EVERY) {
        frames.run.push(HIDE.to_owned());
        *since = 0.0;
    } else if !in_town && *was {
        frames.run.push(SHOW.to_owned());
    }
    *was = in_town;
}
