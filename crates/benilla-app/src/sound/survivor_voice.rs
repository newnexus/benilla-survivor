//! survivor: a game mode's spoken line (a quest giver reading their quest), held by a handle so
//! the next line, or none, cuts it off. The mode hands over the encoded bytes through
//! [`crate::survivor_voice`]; this plays them as the cinematic narration plays its own.

use bevy::prelude::*;

use super::mixer::{self, StreamingSoundHandle};
use super::{SoundConfig, SoundOutput};

/// A cut line stops with this short fade, so it never clicks.
const CUT_FADE_MS: u64 = 80;

/// The line playing and the request it answers.
#[derive(Default)]
pub(super) struct SurvivorVoice {
    seen: u64,
    handle: Option<StreamingSoundHandle<kira::sound::FromFileError>>,
}

pub(super) fn plugin(app: &mut App) {
    app.add_systems(Update, speak);
}

fn speak(mut voice: Local<SurvivorVoice>, out: Option<NonSendMut<SoundOutput>>, config: Res<SoundConfig>) {
    let Some(mut out) = out else { return };
    let Some((generation, bytes)) = crate::take_survivor_voice(voice.seen) else {
        return;
    };
    voice.seen = generation;
    if let Some(mut h) = voice.handle.take() {
        h.stop(mixer::fade(CUT_FADE_MS));
    }
    let Some(bytes) = bytes else { return };
    info!("survivor voice: a line of {} KB", bytes.len() / 1024);
    let Some(mixer_ref) = out.mixer.as_mut() else {
        return;
    };
    // On the effects slider, as an addon's `PlaySoundFile` would be.
    let amp = config.category_amp(super::kit::SoundCategory::Sfx);
    match mixer::stream_from_bytes(bytes)
        .and_then(|d| mixer_ref.play_stream(d.volume(mixer::amp_to_db(amp))))
    {
        Ok(h) => voice.handle = Some(h),
        Err(e) => warn!("survivor voice: {e:#}"),
    }
}
