//! The level-up pick between the server and the screen that shows it. Both run in the one
//! process, so they share this board instead of a packet: the server posts an [`Offer`] and
//! waits, the screen shows it and answers with [`choose`].

use std::sync::Mutex;

/// One choice on the screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    /// The spell taking this card learns.
    pub spell: u32,
    /// The rank it replaces, when it is an upgrade of a skill already known.
    pub replaces: Option<u32>,
    /// A defensive skill; otherwise a damage one.
    pub defensive: bool,
}

/// The cards for one level gained.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    /// Distinct per offer, so an answer to an old one is never taken for the new one.
    pub id: u64,
    /// The level just reached.
    pub level: u32,
    pub cards: Vec<Card>,
}

struct Board {
    offer: Option<Offer>,
    choice: Option<(u64, u32)>,
    next_id: u64,
}

static BOARD: Mutex<Board> = Mutex::new(Board {
    offer: None,
    choice: None,
    next_id: 1,
});

fn board() -> std::sync::MutexGuard<'static, Board> {
    BOARD.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The offer the screen should show, if the game is waiting on one.
pub fn offer() -> Option<Offer> {
    board().offer.clone()
}

/// The player took `spell` from offer `id`.
pub fn choose(id: u64, spell: u32) {
    board().choice = Some((id, spell));
}

/// Show `cards` for `level`; the offer's id comes back.
pub(crate) fn post(level: u32, cards: Vec<Card>) -> u64 {
    let mut b = board();
    let id = b.next_id;
    b.next_id += 1;
    b.offer = Some(Offer { id, level, cards });
    b.choice = None;
    id
}

/// The screen's answer, once.
pub(crate) fn take_choice() -> Option<(u64, u32)> {
    board().choice.take()
}

/// Nothing to show: the pick is made, or the game ended.
pub(crate) fn clear() {
    let mut b = board();
    b.offer = None;
    b.choice = None;
}
