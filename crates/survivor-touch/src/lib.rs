//! survivor: touch controls, for the phone build. No sticks are drawn:
//! - a thumb put down on the left half of the screen is the move stick, its direction measured
//!   from where it went down;
//! - a thumb on the right half is the free look: its drag turns the camera;
//! - a quick tap anywhere is a left mouse click at that spot, so the action bar, the level-up
//!   cards and every other button work by touch;
//! - a thumb put down on an open quest text drags it up and down instead, and moves nothing.
//!
//! It drives the client through [`benilla_app::SurvivorTouch`] and plain mouse input.
//!
//! [`SurvivorCameraPlugin`], on the phone and the desktop alike, swings the camera in behind the
//! running character by itself; `$SURVIVOR_CAMERA_FOLLOW=0` turns it off.

use bevy::input::mouse::MouseButtonInput;
use bevy::input::touch::Touches;
use bevy::input::{ButtonState, InputSystems};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use benilla_app::SurvivorTouch;

/// How far (logical px) a thumb moves before the move stick runs: a tap never walks.
const DEAD_ZONE: f32 = 18.0;
/// A touch that ends this soon (s), having moved less than [`TAP_SLOP`], is a click.
const TAP_TIME: f32 = 0.35;
const TAP_SLOP: f32 = 20.0;
/// The look drag is scaled by this before it reaches the camera: a thumb sweeps fewer pixels
/// than a mouse.
const LOOK_GAIN: f32 = 1.5;
/// The quest frames' text areas a thumb scrolls ([`SCROLLERS`]).
const SCROLLERS: [&str; 4] = [
    "QuestDetailScrollFrame",
    "QuestRewardScrollFrame",
    "QuestProgressScrollFrame",
    "QuestGreetingScrollFrame",
];
/// The interface's height in its own units: a thumb's pixels become those.
const UI_HEIGHT: f32 = 768.0;

/// The camera chase: seconds of no look drag before it resumes, and its time constant (a 90°
/// turn is followed to within 10° in about 2.6 s).
const FOLLOW_GRACE: f32 = 0.6;
const FOLLOW_TIME_CONSTANT: f32 = 1.2;

/// The camera eases back behind a moving character once the player stops dragging the view.
pub struct SurvivorCameraPlugin;

impl Plugin for SurvivorCameraPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(benilla_app::SurvivorCameraFollow {
            enabled: std::env::var("SURVIVOR_CAMERA_FOLLOW").as_deref() != Ok("0"),
            grace: FOLLOW_GRACE,
            time_constant: FOLLOW_TIME_CONSTANT,
        });
    }
}

pub struct SurvivorTouchPlugin;

impl Plugin for SurvivorTouchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Thumbs>()
            .add_systems(Startup, watch_scrollers)
            .add_systems(PreUpdate, play_click.before(InputSystems))
            .add_systems(Update, read_touches);
    }
}

/// The touches that are sticks, by touch id, and the click waiting to be played.
#[derive(Resource, Default)]
struct Thumbs {
    run: Option<u64>,
    look: Option<u64>,
    /// A thumb scrolling a quest text, and which.
    scroll: Option<(u64, &'static str)>,
    click: Option<Click>,
}

fn watch_scrollers(mut frames: ResMut<benilla_app::SurvivorFrames>) {
    frames.watch.extend(SCROLLERS.iter().map(|s| (*s).to_owned()));
}

/// A click in flight, one step a frame: the cursor moves there, the button goes down, it comes up.
#[derive(Clone, Copy)]
struct Click {
    at: Vec2,
    step: u8,
}

fn read_touches(
    touches: Res<Touches>,
    time: Res<Time<Real>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut thumbs: ResMut<Thumbs>,
    mut steer: ResMut<SurvivorTouch>,
    mut frames: ResMut<benilla_app::SurvivorFrames>,
    mut started: Local<std::collections::HashMap<u64, f32>>,
) {
    let (width, height) = windows.single().map_or((0.0, 0.0), |w| (w.width(), w.height()));
    let now = time.elapsed_secs();
    for t in touches.iter_just_pressed() {
        started.insert(t.id(), now);
        // On an open quest text: a scroll, not a stick.
        let over = SCROLLERS.iter().find(|name| {
            frames.shown.contains(**name)
                && frames.rects.get(**name).is_some_and(|r| r.contains(t.start_position()))
        });
        let blocked = frames.blockers.iter().any(|r| r.contains(t.start_position()));
        if let Some(name) = over {
            thumbs.scroll.get_or_insert((t.id(), name));
        } else if blocked {
            // Over one of our own windows: it scrolls or clicks itself.
        } else if t.start_position().x < width / 2.0 {
            thumbs.run.get_or_insert(t.id());
        } else {
            thumbs.look.get_or_insert(t.id());
        }
    }
    for t in touches.iter_just_released() {
        let held = started.remove(&t.id()).map_or(f32::MAX, |s| now - s);
        let moved = t.position().distance(t.start_position());
        if held <= TAP_TIME && moved <= TAP_SLOP && thumbs.click.is_none() {
            thumbs.click = Some(Click {
                at: t.start_position(),
                step: 0,
            });
        }
        if thumbs.run == Some(t.id()) {
            thumbs.run = None;
        }
        if thumbs.look == Some(t.id()) {
            thumbs.look = None;
        }
        if thumbs.scroll.is_some_and(|s| s.0 == t.id()) {
            thumbs.scroll = None;
        }
    }
    for t in touches.iter_just_canceled() {
        if thumbs.scroll.is_some_and(|s| s.0 == t.id()) {
            thumbs.scroll = None;
        }
        started.remove(&t.id());
        if thumbs.run == Some(t.id()) {
            thumbs.run = None;
        }
        if thumbs.look == Some(t.id()) {
            thumbs.look = None;
        }
    }
    // The text follows the thumb: dragged up, it scrolls on down.
    if let Some((id, name)) = thumbs.scroll {
        if let Some(t) = touches.get_pressed(id) {
            let dy = t.delta().y;
            if dy != 0.0 && height > 0.0 {
                let by = -dy * UI_HEIGHT / height;
                frames.run.push(format!(
                    "local f = getglobal(\"{name}\") if f then \
                     local v = f:GetVerticalScroll() + ({by:.2}) \
                     local top = f:GetVerticalScrollRange() \
                     if v < 0 then v = 0 end if v > top then v = top end \
                     f:SetVerticalScroll(v) end"
                ));
            }
        }
    }
    steer.run = thumbs
        .run
        .and_then(|id| touches.get_pressed(id))
        .map(|t| t.position() - t.start_position())
        .filter(|d| d.length() > DEAD_ZONE);
    steer.look = thumbs
        .look
        .and_then(|id| touches.get_pressed(id))
        .map_or(Vec2::ZERO, |t| t.delta() * LOOK_GAIN);
}

/// Play the queued click as the mouse would: ahead of bevy's input pass, so the button state and
/// the cursor read the same this frame as for a real click.
fn play_click(
    mut thumbs: ResMut<Thumbs>,
    mut windows: Query<(Entity, &mut Window), With<PrimaryWindow>>,
    mut buttons: MessageWriter<MouseButtonInput>,
) {
    let Some(click) = thumbs.click.as_mut() else {
        return;
    };
    let Ok((window, mut w)) = windows.single_mut() else {
        thumbs.click = None;
        return;
    };
    let state = match click.step {
        0 => {
            w.set_cursor_position(Some(click.at));
            click.step = 1;
            return;
        }
        1 => ButtonState::Pressed,
        _ => ButtonState::Released,
    };
    buttons.write(MouseButtonInput {
        button: MouseButton::Left,
        state,
        window,
    });
    click.step += 1;
    if click.step > 2 {
        thumbs.click = None;
    }
}
