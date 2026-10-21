//! In a safe town the action bars slide away: the main bar down off the screen's bottom edge,
//! the right-hand bar off its right edge. Nothing is fought in town, and the main bar is the
//! run's cast queue. They slide back as the player steps out through the barrier.
//!
//! The slide moves the stock frames' own anchors (`SetPoint`), each frame of the animation,
//! from the anchors they had when first seen. While hidden the place is said again every half
//! second, in case the stock interface re-anchors a bar.

use bevy::prelude::*;

/// The slide's length, seconds.
const SLIDE: f32 = 0.45;
/// How far each bar travels, in the interface's units (768 tall): past its own size.
const DOWN: f32 = 90.0;
const RIGHT: f32 = 70.0;
/// How often the hidden place is said again, seconds.
const EVERY: f32 = 0.5;

/// Remembers each bar's first anchor, then sets it moved by `%DX%`, `%DY%`.
const PLACE: &str = r#"
if not SURVIVOR_HAVEN then
    SURVIVOR_HAVEN = {}
    for _, n in ipairs({ "MainMenuBar", "MultiBarRight" }) do
        local f = getglobal(n)
        if f then
            local p, rel, rp, x, y = f:GetPoint(1)
            if p then SURVIVOR_HAVEN[n] = { p, rel, rp, x or 0, y or 0 } end
        end
    end
end
local function place(n, dx, dy)
    local f, a = getglobal(n), SURVIVOR_HAVEN[n]
    if f and a then
        f:ClearAllPoints()
        f:SetPoint(a[1], a[2], a[3], a[4] + dx, a[5] + dy)
    end
end
place("MainMenuBar", 0, -%DOWN%)
place("MultiBarRight", %RIGHT%, 0)
"#;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, slide_in_town);
}

#[derive(Default)]
struct Slide {
    /// 0 shown, 1 away.
    away: f32,
    /// Seconds since the place was last said.
    since: f32,
    /// The bars have been placed at least once (their anchors are remembered).
    placed: bool,
}

fn slide_in_town(
    time: Res<Time>,
    objects: benilla_app::Objects,
    mut frames: ResMut<benilla_app::SurvivorFrames>,
    mut s: Local<Slide>,
) {
    if objects.player().is_none() {
        return;
    }
    let target = if survivor_server::town::in_town() { 1.0 } else { 0.0 };
    let before = s.away;
    let step = time.delta_secs() / SLIDE;
    s.away = if target > s.away {
        (s.away + step).min(1.0)
    } else {
        (s.away - step).max(0.0)
    };
    s.since += time.delta_secs();
    let moving = s.away != before;
    let resting_away = s.away == 1.0 && s.since >= EVERY;
    if !moving && !resting_away && s.placed {
        return;
    }
    // Ease in and out.
    let t = s.away * s.away * (3.0 - 2.0 * s.away);
    let lua = PLACE
        .replace("%DOWN%", &format!("{:.1}", DOWN * t))
        .replace("%RIGHT%", &format!("{:.1}", RIGHT * t));
    frames.run.push(lua);
    s.since = 0.0;
    s.placed = true;
}
