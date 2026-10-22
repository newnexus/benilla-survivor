//! Escape's game menu pauses the world: while the stock `GameMenuFrame` is on screen the server
//! is told so ([`survivor_server::menu_pause`]) and stops as it does for a level-up pick.
//!
//! The stock Escape (`UIParent.lua`'s `ToggleGameMenu`) first stops a cast and clears the target,
//! and only opens the menu when there is neither. On a run the auto-cast and the auto-aim always
//! have one, so it never would: the survivor's Escape leaves both alone.

use bevy::prelude::*;

/// The stock frame Escape shows.
pub(crate) const FRAME: &str = "GameMenuFrame";

/// `ToggleGameMenu` without the cast stop and the target clear.
const ESCAPE: &str = r#"
function ToggleGameMenu(clicked)
    if clicked then
        if OptionsFrame:IsVisible() then OptionsFrameCancel:Click() end
        if GameMenuFrame:IsVisible() then
            PlaySound("igMainMenuQuit")
            HideUIPanel(GameMenuFrame)
        else
            CloseMenus()
            CloseAllWindows()
            PlaySound("igMainMenuOpen")
            ShowUIPanel(GameMenuFrame)
        end
        return
    end
    if StaticPopup_EscapePressed() then
    elseif OptionsFrame:IsVisible() then
        OptionsFrameCancel:Click()
    elseif GameMenuFrame:IsVisible() then
        PlaySound("igMainMenuQuit")
        HideUIPanel(GameMenuFrame)
    elseif CloseMenus() then
    elseif SpellStopTargeting() then
    elseif CloseAllWindows() then
    else
        PlaySound("igMainMenuOpen")
        ShowUIPanel(GameMenuFrame)
    end
end
"#;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, (tell_server, replace_escape));
}

fn tell_server(frames: Res<benilla_app::SurvivorFrames>, mut was: Local<bool>) {
    let open = frames.shown.contains(FRAME);
    if open != *was {
        *was = open;
        survivor_server::menu_pause::set(open);
    }
}

/// Said once per entry into the world, after the stock interface has loaded.
fn replace_escape(
    objects: benilla_app::Objects,
    mut frames: ResMut<benilla_app::SurvivorFrames>,
    mut said: Local<bool>,
) {
    let in_world = objects.player().is_some();
    if in_world && !*said {
        frames.run.push(ESCAPE.to_owned());
    }
    *said = in_world;
}
