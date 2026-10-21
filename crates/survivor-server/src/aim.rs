//! The creature the server's auto-attack is aimed at, shared with the client in the same process
//! so it can select it too: the target frame shows it and the action bar casts at it.

use std::sync::atomic::{AtomicU64, Ordering};

static AIM: AtomicU64 = AtomicU64::new(0);

/// The auto-target's guid, or `None` with nothing in reach.
pub fn current() -> Option<u64> {
    Some(AIM.load(Ordering::Relaxed)).filter(|&g| g != 0)
}

pub(crate) fn set(guid: Option<u64>) {
    AIM.store(guid.unwrap_or(0), Ordering::Relaxed);
}
