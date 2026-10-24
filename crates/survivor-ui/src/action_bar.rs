//! The main bar cut down to its twelve ability buttons between the two gryphons; everything
//! else on it (page arrows, micro buttons, the performance meter, the bag slots) hidden, and the
//! backpack on its own in the screen's bottom-right corner, larger, showing how many of its slots
//! are free. A bigger bag replaces the backpack
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
-- The backpack alone in the corner, larger, its free slots counted on it (the stock button's
-- own count, as on a stack) whenever the bag changes.
local bag = MainMenuBarBackpackButton
bag:SetScale(%SCALE%)
bag:ClearAllPoints()
bag:SetPoint("BOTTOMRIGHT", UIParent, "BOTTOMRIGHT", -8 / %SCALE%, 8 / %SCALE%)
local function count_free()
    local free = 0
    for i = 1, GetContainerNumSlots(0) do
        if not GetContainerItemInfo(0, i) then free = free + 1 end
    end
    MainMenuBarBackpackButtonCount:SetText(free)
    MainMenuBarBackpackButtonCount:Show()
end
if not SurvivorBagCounter then
    local f = CreateFrame("Frame", "SurvivorBagCounter")
    f:RegisterEvent("BAG_UPDATE")
    f:RegisterEvent("PLAYER_ENTERING_WORLD")
    f:SetScript("OnEvent", function() SurvivorBagCounter.count() end)
end
SurvivorBagCounter.count = count_free
count_free()
"#;

/// The backpack's size against the stock button's (37).
const BAG_SCALE: f32 = 1.6;

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
        frames.run.push(BAR.replace("%SCALE%", &BAG_SCALE.to_string()));
    }
}
