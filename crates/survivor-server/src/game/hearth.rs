//! The Hearthstone: used on a run from the right-hand bar, it charges for [`HEARTH_TIME`] while
//! the player stands still, then takes them to the nearest town alive, the run ended as if they
//! had walked in (what they found is kept). A step, death or the run ending some other way
//! breaks it. The screen draws the charge from [`crate::hearth`].

use std::time::{Duration, Instant};

use benilla_protocol::messages::opcode;

use super::Game;
use crate::packets;

/// The Hearthstone's item entry.
pub(crate) const HEARTHSTONE: u32 = 6948;
/// How long it charges.
const HEARTH_TIME: Duration = Duration::from_secs(10);
/// A step this far from where it started breaks it, yards.
const STILL: f32 = 0.5;

/// A charge under way: where the player stood, when it began and when it is done.
pub(crate) struct Hearth {
    at: [f32; 3],
    start: Instant,
    done: Instant,
}

impl Game {
    /// The Hearthstone used: on a run, start charging.
    pub(super) fn use_hearthstone(&mut self, now: Instant) {
        if !self.in_run() {
            self.send(
                opcode::SMSG_NOTIFICATION,
                &packets::notification("You are already in town"),
            );
            return;
        }
        if self.hearth.is_some() {
            return;
        }
        eprintln!("survivor: hearthstone charging");
        self.hearth = Some(Hearth {
            at: self.player.pos,
            start: now,
            done: now + HEARTH_TIME,
        });
    }

    /// Charge on, break, or take the player home.
    pub(super) fn tick_hearth(&mut self, now: Instant) {
        let Some(h) = &self.hearth else {
            crate::hearth::set(None);
            return;
        };
        let broken = !self.in_run()
            || self.player.dead_until.is_some()
            || super::dist2d(h.at, self.player.pos) > STILL;
        if broken {
            eprintln!("survivor: hearthstone broken");
            self.hearth = None;
            crate::hearth::set(None);
            return;
        }
        if now < h.done {
            let part = now.saturating_duration_since(h.start).as_secs_f32()
                / h.done.saturating_duration_since(h.start).as_secs_f32();
            crate::hearth::set(Some(part));
            return;
        }
        self.hearth = None;
        crate::hearth::set(None);
        eprintln!("survivor: hearthstone home");
        self.go_home(now);
    }

    /// A pause holds the charge where it is.
    pub(super) fn shift_hearth(&mut self, by: Duration) {
        if let Some(h) = &mut self.hearth {
            h.start += by;
            h.done += by;
        }
    }
}
