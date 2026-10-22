//! The spirit healer's upgrade window between the server and the screen, as [`crate::armory`] is
//! for the collection: the server posts the upgrades and the souls to spend, the screen shows them
//! and answers with what the player bought.

use std::sync::Mutex;

/// What the healer sells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Upgrade {
    /// One more reroll of the level-up cards a run.
    Reroll,
    /// One more banish of a level-up card a run.
    Banish,
    /// Runes come more often on a run.
    RuneRate,
    /// A shorter global cooldown.
    Gcd,
    /// A kind of rune: double damage, speed, shorter cooldowns, healing.
    Rune(u8),
}

/// One upgrade's row.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub upgrade: Upgrade,
    pub name: String,
    /// What the next level gives, or what it does when bought.
    pub text: String,
    pub level: u32,
    pub max: u32,
    /// Souls for the next level; none at the top.
    pub cost: Option<u32>,
    /// An interface icon (without `.blp`).
    pub icon: &'static str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    /// Distinct per post, so an answer to an old one is never taken for the new one.
    pub id: u64,
    pub souls: u32,
    pub rows: Vec<Row>,
}

/// What the player did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Buy(Upgrade),
    Close,
}

struct Board {
    window: Option<Window>,
    action: Option<(u64, Action)>,
    next_id: u64,
}

static BOARD: Mutex<Board> = Mutex::new(Board {
    window: None,
    action: None,
    next_id: 1,
});

fn board() -> std::sync::MutexGuard<'static, Board> {
    BOARD.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The window to show, if it is open.
pub fn current() -> Option<Window> {
    board().window.clone()
}

/// The player did `a` in window `id`.
pub fn act(id: u64, a: Action) {
    board().action = Some((id, a));
}

pub(crate) fn post(souls: u32, rows: Vec<Row>) -> u64 {
    let mut b = board();
    let id = b.next_id;
    b.next_id += 1;
    b.window = Some(Window { id, souls, rows });
    b.action = None;
    id
}

pub(crate) fn take_action() -> Option<(u64, Action)> {
    board().action.take()
}

pub(crate) fn clear() {
    let mut b = board();
    b.window = None;
    b.action = None;
}

/// The souls on hand, for the purse in the corner.
pub mod souls {
    use std::sync::atomic::{AtomicU32, Ordering};

    static SOULS: AtomicU32 = AtomicU32::new(0);

    pub fn get() -> u32 {
        SOULS.load(Ordering::Relaxed)
    }

    pub(crate) fn set(n: u32) {
        SOULS.store(n, Ordering::Relaxed);
    }
}
