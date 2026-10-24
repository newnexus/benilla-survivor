//! One bag: the backpack alone on the main bar, the four bag slots beside it hidden. A bigger
//! bag replaces the backpack when picked up, so there is never a second bag to place.

use bevy::prelude::*;

/// The stock bag slots (`MainMenuBarBagButtons.xml`) hidden, and the four hollows painted for
/// them on the bar's right strip (`MainMenuBarTexture3`, x 40-212 of its 256: the backpack 6 from
/// the bar's right, 37-wide slots 5 apart) covered with the plain dark end of the strip beside it
/// (`MainMenuBarTexture2`, x 232-250, past the micro buttons), stretched. The strips are ARTWORK,
/// the cover OVERLAY. The keyring never comes (it would swap the strip for its own plate).
const ONE_BAG: &str = r#"
for i = 0, 3 do
    local slot = getglobal("CharacterBag" .. i .. "Slot")
    if slot then slot:Hide() end
end
if KeyRingButton then KeyRingButton:Hide() end
MainMenuBar_UpdateKeyRing = function() end
if not SurvivorBagCover then
    local strip, plain = MainMenuBarTexture3, MainMenuBarTexture2
    local cover = MainMenuBarArtFrame:CreateTexture("SurvivorBagCover", "OVERLAY")
    cover:SetTexture(plain:GetTexture())
    local _, top, _, bottom = plain:GetTexCoord()
    cover:SetTexCoord(232 / 256, 250 / 256, top, bottom)
    cover:SetPoint("TOPLEFT", strip, "TOPLEFT", 40, 0)
    cover:SetWidth(172)
    cover:SetHeight(strip:GetHeight())
end
"#;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, one_bag);
}

/// Said each time the player enters the world (the interface is built anew at world entry).
fn one_bag(
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
        frames.run.push(ONE_BAG.to_owned());
    }
}
