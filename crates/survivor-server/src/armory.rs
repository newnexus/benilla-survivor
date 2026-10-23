//! The collection chest's window between the server and the screen, as [`crate::picks`] is for
//! the cards: the server posts every slot's items, the screen shows them in tabs and answers
//! with what the player did.

use std::sync::Mutex;

/// One slot's tab.
#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    /// The equipment slot (0 head ... 17 ranged).
    pub slot: u16,
    pub name: String,
    pub rows: Vec<Row>,
}

/// One item in a tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The owned item's id, 0 for one only seen.
    pub key: u64,
    pub entry: u32,
    pub name: String,
    /// `ItemDisplayInfo.dbc` row, for its icon.
    pub display: u32,
    /// 0 common ... 4 legendary.
    pub rarity: u8,
    pub upgrades: u32,
    pub favourite: bool,
    /// Worn: the player starts their runs in it.
    pub worn: bool,
    /// Seen in a chest, never taken home: shown in shadow.
    pub seen_only: bool,
    /// Where it is worn and what it is, as its tooltip says: `Chest` and `Cloth`, `One-Hand`
    /// and `Dagger`.
    pub slot_text: String,
    pub kind: String,
    pub durability: u32,
    /// Armour, or damage and speed.
    pub base: Vec<String>,
    /// The survivor stats, a line each.
    pub own: Vec<String>,
    /// Its own 1.12 bonuses, a line each.
    pub bonus: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Armory {
    /// Distinct per post, so an answer to an old one is never taken for the new one.
    pub id: u64,
    pub tabs: Vec<Tab>,
}

/// What the player did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Wear the owned item, or take it off when it is worn.
    Toggle(u64),
    Favourite(u64),
    Close,
}

struct Board {
    armory: Option<Armory>,
    action: Option<(u64, Action)>,
    next_id: u64,
}

static BOARD: Mutex<Board> = Mutex::new(Board {
    armory: None,
    action: None,
    next_id: 1,
});

fn board() -> std::sync::MutexGuard<'static, Board> {
    BOARD.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The window to show, if it is open.
pub fn current() -> Option<Armory> {
    board().armory.clone()
}

/// The player did `a` in window `id`.
pub fn act(id: u64, a: Action) {
    board().action = Some((id, a));
}

pub(crate) fn post(tabs: Vec<Tab>) -> u64 {
    let mut b = board();
    let id = b.next_id;
    b.next_id += 1;
    b.armory = Some(Armory { id, tabs });
    b.action = None;
    id
}

pub(crate) fn take_action() -> Option<(u64, Action)> {
    board().action.take()
}

pub(crate) fn clear() {
    let mut b = board();
    b.armory = None;
    b.action = None;
}
