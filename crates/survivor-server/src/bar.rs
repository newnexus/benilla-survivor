//! The cast queue as the survivor UI draws it: the damage spells in the player's own order, which
//! one goes next, and whether the order may be changed (in town). The server posts every change;
//! the screen draws the strip and answers with a spell moved to a new place.

use std::sync::Mutex;

/// One step of the queue.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Queue {
    /// Bumped by every post: the UI animates each change once.
    pub version: u64,
    /// The spells in queue order, main bar button 1 first.
    pub spells: Vec<u32>,
    /// The spell whose cast made this step.
    pub fired: Option<u32>,
    /// The place in `spells` the auto-cast tries next.
    pub next: usize,
    /// In town, or paused: the order may be dragged.
    pub editable: bool,
    /// In town: the strip folds away until the player opens it.
    pub in_town: bool,
}

static QUEUE: Mutex<Option<Queue>> = Mutex::new(None);

/// The queue as last posted, or `None` outside a game.
pub fn current() -> Option<Queue> {
    QUEUE.lock().expect("queue").clone()
}

pub(crate) fn post(
    spells: Vec<u32>,
    fired: Option<u32>,
    next: usize,
    editable: bool,
    in_town: bool,
) {
    let mut q = QUEUE.lock().expect("queue");
    let version = q.as_ref().map_or(0, |q| q.version) + 1;
    *q = Some(Queue {
        version,
        spells,
        fired,
        next,
        editable,
        in_town,
    });
}

/// The screen's last move: the spell at `from` put at `to`.
static MOVE: Mutex<Option<(usize, usize)>> = Mutex::new(None);

/// The player dragged the spell at `from` to place `to`.
pub fn move_spell(from: usize, to: usize) {
    *MOVE.lock().expect("move") = Some((from, to));
}

pub(crate) fn take_move() -> Option<(usize, usize)> {
    MOVE.lock().expect("move").take()
}

pub(crate) fn clear() {
    *QUEUE.lock().expect("queue") = None;
}
