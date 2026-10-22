//! The talent tree opens in a safe town from level 2 (the stock interface waits for level 10, in
//! `UIParent.lua`'s `ToggleTalentFrame` and `MainMenuBarMicroButtons.lua`'s
//! `UpdateTalentButton`). Out on a run it stays shut, and closes if it was open.

use bevy::prelude::*;

/// The lowest level the tree opens at.
const FROM_LEVEL: u32 = 2;

/// The stock functions again, with our level and the town in their test.
const GATE: &str = r#"
SURVIVOR_IN_TOWN = %TOWN%
local function open() return SURVIVOR_IN_TOWN and UnitLevel("player") >= %LEVEL% end
function ToggleTalentFrame()
    if not open() then
        if TalentFrame and TalentFrame:IsVisible() then HideUIPanel(TalentFrame) end
        return
    end
    TalentFrame_LoadUI()
    if TalentFrame_Toggle then TalentFrame_Toggle() end
end
function UpdateTalentButton()
    if not open() then
        TalentMicroButton:Hide()
        QuestLogMicroButton:SetPoint("BOTTOMLEFT", "TalentMicroButton", "BOTTOMLEFT", 0, 0)
    else
        TalentMicroButton:Show()
        QuestLogMicroButton:SetPoint("BOTTOMLEFT", "TalentMicroButton", "BOTTOMRIGHT", -2, 0)
    end
end
UpdateTalentButton()
if not open() and TalentFrame and TalentFrame:IsVisible() then HideUIPanel(TalentFrame) end
"#;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, gate_talents);
}

/// Say the gate again whenever the player enters the world or crosses the barrier.
fn gate_talents(
    objects: benilla_app::Objects,
    mut frames: ResMut<benilla_app::SurvivorFrames>,
    mut said: Local<Option<bool>>,
) {
    if objects.player().is_none() {
        *said = None;
        return;
    }
    let town = survivor_server::town::in_town();
    if *said == Some(town) {
        return;
    }
    *said = Some(town);
    frames.run.push(
        GATE.replace("%TOWN%", if town { "true" } else { "false" })
            .replace("%LEVEL%", &FROM_LEVEL.to_string()),
    );
}
