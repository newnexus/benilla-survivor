//! The cast queue as the survivor UI draws it: the spells on the main action bar's front, in the
//! order they will be cast, and the last one that went off. The server posts every change; the UI
//! reads it each frame and animates the step from the old order to the new one.

use std::sync::Mutex;

/// One step of the queue.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Queue {
    /// Bumped by every post: the UI animates each change once.
    pub version: u64,
    /// The spells in queue order, main bar button 1 first.
    pub spells: Vec<u32>,
    /// The spell whose cast made this step, now at the back.
    pub fired: Option<u32>,
}

static QUEUE: Mutex<Option<Queue>> = Mutex::new(None);

/// The queue as last posted, or `None` outside a game.
pub fn current() -> Option<Queue> {
    QUEUE.lock().expect("queue").clone()
}

pub(crate) fn post(spells: Vec<u32>, fired: Option<u32>) {
    let mut q = QUEUE.lock().expect("queue");
    let version = q.as_ref().map_or(0, |q| q.version) + 1;
    *q = Some(Queue {
        version,
        spells,
        fired,
    });
}

pub(crate) fn clear() {
    *QUEUE.lock().expect("queue") = None;
}
