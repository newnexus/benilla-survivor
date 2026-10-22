//! The class trainer's window between the server and the screen, as [`crate::spirit`] is for the
//! spirit healer: a row per skill, what rank is bought, the next rank's level and price.

use std::sync::Mutex;

/// One skill's row.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// The skill's rank chain (its rank 1's spell), what the answers name.
    pub chain: u32,
    pub name: String,
    /// The spell of the rank shown, for its icon.
    pub spell: u32,
    pub defensive: bool,
    /// Ranks bought, and how many there are.
    pub bought: u32,
    pub ranks: u32,
    /// The next rank: the run level it comes at, and its price in copper; none when all are
    /// bought.
    pub next: Option<(u32, u32)>,
    /// The levels each bought rank comes at on a run, lowest first.
    pub levels: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    /// Distinct per post, so an answer to an old one is never taken for the new one.
    pub id: u64,
    pub trainer: String,
    /// The town purse, copper.
    pub money: u32,
    pub rows: Vec<Row>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// The chain's next rank.
    Buy(u32),
    /// The chain's last bought rank back, its price refunded in full.
    Refund(u32),
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

pub fn current() -> Option<Window> {
    board().window.clone()
}

pub fn act(id: u64, a: Action) {
    board().action = Some((id, a));
}

pub(crate) fn post(trainer: String, money: u32, rows: Vec<Row>) -> u64 {
    let mut b = board();
    let id = b.next_id;
    b.next_id += 1;
    b.window = Some(Window {
        id,
        trainer,
        money,
        rows,
    });
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
