//! survivor: where named stock frames sit on screen, for a crate on top of benilla that draws
//! over them (the survivor's cast-queue animation over the action buttons). The crate names the
//! frames in [`SurvivorFrames::watch`]; each frame their window rects (logical px, y down) are in
//! [`SurvivorFrames::rects`], a frame not laid out being absent. Lua lines put in
//! [`SurvivorFrames::run`] run in the interface once, in order (showing and hiding stock frames).

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use benilla_ui::script::UiScript;

#[derive(Resource, Default)]
pub struct SurvivorFrames {
    pub watch: Vec<String>,
    pub rects: std::collections::HashMap<String, Rect>,
    pub run: Vec<String>,
}

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<SurvivorFrames>()
        .add_systems(PostUpdate, locate);
}

fn locate(
    script: Option<NonSend<UiScript>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut frames: ResMut<SurvivorFrames>,
) {
    if let Some(script) = script.as_ref() {
        for chunk in std::mem::take(&mut frames.run) {
            if let Err(e) = script.eval::<()>(&chunk) {
                warn!("survivor: interface line failed: {e}");
            }
        }
    }
    if frames.watch.is_empty() {
        return;
    }
    let (Some(script), Ok(window)) = (script, windows.single()) else {
        frames.rects.clear();
        return;
    };
    let h = window.height();
    let s = crate::ui_script::seam_scale(h);
    let rects = frames
        .watch
        .iter()
        .filter_map(|name| {
            let r = script.named_frame_rect(name)?;
            let min = Vec2::new(r.left * s, h - r.top * s);
            let max = Vec2::new(r.right * s, h - r.bottom * s);
            Some((name.clone(), Rect::from_corners(min, max)))
        })
        .collect();
    frames.rects = rects;
}
