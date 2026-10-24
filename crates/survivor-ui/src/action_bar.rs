//! The main bar cut down to its twelve ability buttons between the two gryphons; everything
//! else on it (page arrows, micro buttons, the performance meter, the bag slots) hidden, and the
//! backpack on its own in the screen's bottom-right corner. A bigger bag replaces the backpack
//! when picked up, so there is never a second bag to place.
//!
//! The stock bar (`MainMenuBar.xml`) is 1024 wide, four 256 strips of art, the buttons on the
//! left two (x 8-506). The bar is made 512 wide, still centred; its art hangs from the centre
//! (strips at -384/-128, gryphons at -/+544), so each piece moves 256 in, and the right two strips
//! are hidden. The XP bar over it is cut the same way. The micro buttons' windows still open from their keys (C, P, N, L, Esc...).

use bevy::prelude::*;

/// The stock frames moved and hidden. Said again at every world entry (the interface is built
/// anew then).
const BAR: &str = r#"
local function hide(n)
    local f = getglobal(n)
    if f then f:Hide() end
end
for _, n in ipairs({
    "ActionBarUpButton", "ActionBarDownButton", "MainMenuBarPageNumber",
    "MainMenuBarPerformanceBarFrame", "KeyRingButton",
    "CharacterMicroButton", "SpellbookMicroButton", "TalentMicroButton", "QuestLogMicroButton",
    "SocialsMicroButton", "WorldMapMicroButton", "MainMenuMicroButton", "HelpMicroButton",
    "CharacterBag0Slot", "CharacterBag1Slot", "CharacterBag2Slot", "CharacterBag3Slot",
    "MainMenuBarTexture2", "MainMenuBarTexture3", "MainMenuXPBarTexture2", "MainMenuXPBarTexture3",
}) do hide(n) end
-- The stock code shows these again on a level or a key; here they stay hidden.
MainMenuBar_UpdateKeyRing = function() end
UpdateTalentButton = function() end
MainMenuBar:SetWidth(512)
MainMenuBarArtFrame:SetWidth(512)
MainMenuExpBar:SetWidth(512)
-- The art hangs from the bar's centre: each piece a quarter of the old width further in.
local function at(n, parent, x, y)
    local f = getglobal(n)
    f:ClearAllPoints()
    f:SetPoint("BOTTOM", parent, "BOTTOM", x, y)
end
at("MainMenuBarTexture0", MainMenuBarArtFrame, -128, 0)
at("MainMenuBarTexture1", MainMenuBarArtFrame, 128, 0)
at("MainMenuXPBarTexture0", MainMenuExpBar, -128, 3)
at("MainMenuXPBarTexture1", MainMenuExpBar, 128, 3)
at("MainMenuBarLeftEndCap", MainMenuBarArtFrame, -288, 0)
at("MainMenuBarRightEndCap", MainMenuBarArtFrame, 288, 0)
local bag = MainMenuBarBackpackButton
bag:ClearAllPoints()
bag:SetPoint("BOTTOMRIGHT", UIParent, "BOTTOMRIGHT", -8, 8)
"#;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, cut_bar);
}

fn cut_bar(
    objects: benilla_app::Objects,
    mut frames: ResMut<benilla_app::SurvivorFrames>,
    mut said: Local<bool>,
) {
    if objects.player().is_none() {
        *said = false;
        return;
    }
    if !*said {
        *said = true;
        frames.run.push(BAR.to_owned());
    }
}
