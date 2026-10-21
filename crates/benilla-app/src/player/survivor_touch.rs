//! survivor: touch steering for a crate on top of benilla (the Android build's thumbs). It writes
//! [`SurvivorTouch`] each frame; [`steer_touch`] turns it into the same levers `/follow` pulls:
//! the facing and the held forward ([`Player::auto_forward`]), and the camera's yaw and pitch, as
//! a mouse-look drag moves them. Nothing writes the resource on a desktop, so it does nothing there.

use bevy::prelude::*;

use super::camera::{FlyCam, LookConfig, CAM_PITCH_LIMIT};
use super::Player;

/// One frame of touch steering.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct SurvivorTouch {
    /// The move stick's direction on screen (x right, y down; any length but zero), or `None`
    /// while no thumb holds it. The character runs that way as the camera saw it when the thumb
    /// went down, so the direction holds still while the camera swings in behind.
    pub run: Option<Vec2>,
    /// The look drag this frame, in logical pixels, spent as a mouse-look drag of that size.
    pub look: Vec2,
}

/// Apply [`SurvivorTouch`]: after `/follow` and Click to Move have written the facing and the
/// forward, before `control` reads them.
pub(super) fn steer_touch(
    touch: Res<SurvivorTouch>,
    look_cfg: Res<LookConfig>,
    mut player: ResMut<Player>,
    mut cams: Query<&mut FlyCam>,
    // The camera yaw the current stick press is measured from.
    mut basis: Local<Option<f32>>,
) {
    let Ok(mut cam) = cams.single_mut() else {
        return;
    };
    if touch.look != Vec2::ZERO {
        cam.yaw -= touch.look.x * look_cfg.yaw_rate();
        let dy = if look_cfg.invert_pitch { -touch.look.y } else { touch.look.y };
        cam.pitch = (cam.pitch - dy * look_cfg.pitch_rate()).clamp(-CAM_PITCH_LIMIT, CAM_PITCH_LIMIT);
    }
    let Some(dir) = touch.run.filter(|d| *d != Vec2::ZERO) else {
        *basis = None;
        return;
    };
    if player.control_lost {
        return;
    }
    // A free look turns the basis with it, so the stick stays true to what is on screen.
    let from = *basis.get_or_insert(cam.yaw) - touch.look.x * look_cfg.yaw_rate();
    *basis = Some(from);
    // Yaw grows to the left: up the screen is the camera's heading, right is a right turn.
    player.face_yaw = from + (-dir.x).atan2(-dir.y);
    player.auto_forward = true;
}
