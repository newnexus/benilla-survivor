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
    /// 0 common, 1 uncommon, 2 rare, 3 epic, 4 legendary: the card's colour.
    pub rarity: u8,
    /// An upgrade to the skill `spell`, already known, in lines ("+22% damage"); empty for a
    /// card that teaches.
    pub boost: Vec<String>,
    /// A chest's item instead of a skill (`spell` is then 0).
    pub item: Option<ItemCard>,
}

/// An item on a chest's card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemCard {
    pub entry: u32,
    pub name: String,
    /// `ItemDisplayInfo.dbc` row, for its icon.
    pub display: u32,
    /// "Head", "Main Hand"...
    pub slot: String,
    /// What it gives: armour or damage, and its stats.
    pub lines: Vec<String>,
    /// The item worn already: taking it raises its item level for the run.
    pub upgrade: bool,
}

/// The cards for one level gained.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    /// Distinct per offer, so an answer to an old one is never taken for the new one.
    pub id: u64,
    /// The level just reached.
    pub level: u32,
    pub cards: Vec<Card>,
    /// The screen's heading when it is not a level-up ("Battered Chest").
    pub title: Option<String>,
    /// A chest's way out when no card suits: the button's words ("Recycle: 10c back"). Taking
    /// it answers with the index one past the last card.
    pub recycle: Option<String>,
}

struct Board {
    offer: Option<Offer>,
    choice: Option<(u64, usize)>,
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

/// The player took card `index` of offer `id`.
pub fn choose(id: u64, index: usize) {
    board().choice = Some((id, index));
}

/// Show `cards` for `level`; the offer's id comes back.
pub(crate) fn post(level: u32, cards: Vec<Card>) -> u64 {
    post_titled(level, cards, None, None)
}

/// Show `cards` under `title` (a chest's name), or as a level-up, with a chest's `recycle`
/// button.
pub(crate) fn post_titled(
    level: u32,
    cards: Vec<Card>,
    title: Option<String>,
    recycle: Option<String>,
) -> u64 {
    let mut b = board();
    let id = b.next_id;
    b.next_id += 1;
    b.offer = Some(Offer {
        id,
        level,
        cards,
        title,
        recycle,
    });
    b.choice = None;
    id
}

/// The screen's answer, once.
pub(crate) fn take_choice() -> Option<(u64, usize)> {
    board().choice.take()
}

/// Nothing to show: the pick is made, or the game ended.
pub(crate) fn clear() {
    let mut b = board();
    b.offer = None;
    b.choice = None;
}
