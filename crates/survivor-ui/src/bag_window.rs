//! The bag as one window, as the Bagnon addon draws it: a plain panel with every slot of the
//! backpack in a single grid, no gold strip (the purse in the corner shows the gold). It takes
//! the stock backpack window's place: B, a click on the backpack and a vendor all open this one.
//! The grid grows with the bag (a bigger bag replaces the backpack), past the stock window's 20.
//!
//! The slots are the stock `ContainerFrameItemButtonTemplate` buttons in a frame whose id is the
//! backpack's (0), so a click, a drag and the tooltip work as on the stock window.

use bevy::prelude::*;

const WINDOW: &str = r#"
local COLS, SLOT, GAP, PAD, TOP = %COLS%, 37, 4, 10, 30
-- Installed again after every resize: a backdrop's pieces keep the size the frame had then.
local BACKDROP = {
    bgFile = "Interface\\Tooltips\\UI-Tooltip-Background",
    edgeFile = "Interface\\Tooltips\\UI-Tooltip-Border",
    tile = true, tileSize = 16, edgeSize = 16,
    insets = { left = 4, right = 4, top = 4, bottom = 4 },
}
if not SurvivorBag then
    local f = CreateFrame("Frame", "SurvivorBag", UIParent)
    f:SetFrameStrata("HIGH")
    f:EnableMouse(true)
    f:SetPoint("BOTTOMRIGHT", MainMenuBarBackpackButton, "TOPRIGHT", 0, 6)
    f:Hide()
    local title = f:CreateFontString("SurvivorBagTitle", "OVERLAY", "GameFontNormal")
    title:SetPoint("TOPLEFT", f, "TOPLEFT", PAD + 2, -10)
    title:SetText("Bag")
    local close = CreateFrame("Button", "SurvivorBagClose", f, "UIPanelCloseButton")
    close:SetPoint("TOPRIGHT", f, "TOPRIGHT", 3, 3)
    -- The buttons' parent: its id is the bag the stock button code reads.
    local slots = CreateFrame("Frame", "SurvivorBagSlots", f)
    slots:SetID(0)
    slots:SetPoint("TOPLEFT", f, "TOPLEFT", PAD, -TOP)
    slots:SetWidth(1)
    slots:SetHeight(1)
    f:RegisterEvent("BAG_UPDATE")
    f:RegisterEvent("ITEM_LOCK_CHANGED")
    f:RegisterEvent("BAG_UPDATE_COOLDOWN")
    f:SetScript("OnEvent", function() if SurvivorBag:IsShown() then SurvivorBag_Update() end end)
    f:SetScript("OnShow", function()
        SurvivorBag_Update()
        MainMenuBarBackpackButton:SetChecked(1)
        PlaySound("igBackPackOpen")
    end)
    f:SetScript("OnHide", function()
        MainMenuBarBackpackButton:SetChecked(nil)
        PlaySound("igBackPackClose")
    end)
    tinsert(UISpecialFrames, "SurvivorBag")
end

function SurvivorBag_Update()
    local size = GetContainerNumSlots(0)
    for i = 1, size do
        local name = "SurvivorBagItem" .. i
        local b = getglobal(name)
        if not b then
            b = CreateFrame("Button", name, SurvivorBagSlots, "ContainerFrameItemButtonTemplate")
            -- The stock window paints the empty slots into its bag art; here each has its own.
            local empty = b:CreateTexture(name .. "Empty", "BACKGROUND")
            empty:SetTexture("Interface\\PaperDoll\\UI-Backpack-EmptySlot")
            empty:SetPoint("TOPLEFT", b, "TOPLEFT", 0, 0)
            empty:SetWidth(SLOT)
            empty:SetHeight(SLOT)
        end
        b:SetID(i)
        local row = math.floor((i - 1) / COLS)
        local col = (i - 1) - row * COLS
        b:ClearAllPoints()
        b:SetPoint("TOPLEFT", SurvivorBagSlots, "TOPLEFT", col * (SLOT + GAP), -row * (SLOT + GAP))
        local texture, count, locked = GetContainerItemInfo(0, i)
        SetItemButtonTexture(b, texture)
        SetItemButtonCount(b, count)
        SetItemButtonDesaturated(b, locked, 0.5, 0.5, 0.5)
        local start, duration, enable = GetContainerItemCooldown(0, i)
        CooldownFrame_SetTimer(getglobal(name .. "Cooldown"), start, duration, enable)
        b:Show()
    end
    local i = size + 1
    while getglobal("SurvivorBagItem" .. i) do
        getglobal("SurvivorBagItem" .. i):Hide()
        i = i + 1
    end
    local cols = math.min(size, COLS)
    local rows = math.ceil(size / COLS)
    SurvivorBag:SetWidth(PAD * 2 + cols * SLOT + (cols - 1) * GAP)
    SurvivorBag:SetHeight(TOP + PAD + rows * SLOT + (rows - 1) * GAP)
    SurvivorBag:SetBackdrop(BACKDROP)
    -- The page and edge of our other windows (the trainer's, the spirit healer's).
    SurvivorBag:SetBackdropColor(0.035, 0.03, 0.025, 0.92)
    SurvivorBag:SetBackdropBorderColor(0.55, 0.45, 0.25)
end

-- The stock ways in to the backpack's window open this one; there is no other bag.
function ToggleBackpack()
    if SurvivorBag:IsShown() then SurvivorBag:Hide() else SurvivorBag:Show() end
end
function OpenBackpack() SurvivorBag:Show() end
function CloseBackpack() SurvivorBag:Hide() end
OpenAllBags = ToggleBackpack
CloseAllBags = CloseBackpack
function ToggleBag(id) if id == 0 then ToggleBackpack() end end
function OpenBag(id) if id == 0 then OpenBackpack() end end
function CloseBag(id) if id == 0 then CloseBackpack() end end
if ContainerFrame1 then ContainerFrame1:Hide() end
"#;

/// Slots in a row.
const COLUMNS: u32 = 8;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, build_window);
}

/// Said at every world entry (the interface is built anew then).
fn build_window(
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
        frames.run.push(WINDOW.replace("%COLS%", &COLUMNS.to_string()));
    }
}
