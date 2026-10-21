//! The quest giver's window, laid out for the survivor: in the middle of the screen and bigger,
//! its text in at once instead of fading in letter by letter, no scrollbar (a thumb drags the
//! text, see `survivor-touch`), and big Accept and Decline buttons.
//!
//! The stock frames are moved and sized through their own Lua API; the interface places the
//! window again each time it opens, so the place is said again while it is open.

use bevy::prelude::*;

/// How often the layout is said again while the window is open, seconds.
const EVERY: f32 = 0.15;

const LAYOUT: &str = r#"
QUEST_DESCRIPTION_GRADIENT_CPS = 100000
if QuestFrame and QuestFrame:IsVisible() then
    if not SURVIVOR_QUEST_LAYOUT then
        SURVIVOR_QUEST_LAYOUT = true
        -- As big as fits: the window's drawn part is about 470 tall, and the interface may be
        -- scaled up already (a phone's is).
        local fit = UIParent:GetHeight() * 0.96 / 470
        if fit > 1.3 then fit = 1.3 end
        QuestFrame:SetScale(fit)
        for _, n in ipairs({ "QuestFrameAcceptButton", "QuestFrameDeclineButton",
                             "QuestFrameCompleteQuestButton", "QuestFrameCompleteButton",
                             "QuestFrameGoodbyeButton", "QuestFrameCancelButton",
                             "QuestFrameGreetingGoodbyeButton" }) do
            local b = getglobal(n)
            if b then
                b:SetWidth(150)
                b:SetHeight(34)
                if GameFontNormalLarge then b:SetTextFontObject(GameFontNormalLarge) end
            end
        end
    end
    QuestFrame:ClearAllPoints()
    -- The frame's lower part is empty: its drawn part sits centred a little higher.
    QuestFrame:SetPoint("CENTER", UIParent, "CENTER", 30, -30)
    for _, n in ipairs({ "QuestDetailScrollFrameScrollBar", "QuestRewardScrollFrameScrollBar",
                         "QuestProgressScrollFrameScrollBar", "QuestGreetingScrollFrameScrollBar" }) do
        local s = getglobal(n)
        if s and s:IsShown() then s:Hide() end
    end
end
"#;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, lay_out_quest_frame);
}

fn lay_out_quest_frame(
    time: Res<Time>,
    objects: benilla_app::Objects,
    mut frames: ResMut<benilla_app::SurvivorFrames>,
    mut since: Local<f32>,
) {
    if objects.player().is_none() {
        return;
    }
    *since += time.delta_secs();
    if *since < EVERY {
        return;
    }
    *since = 0.0;
    frames.run.push(LAYOUT.to_owned());
}
