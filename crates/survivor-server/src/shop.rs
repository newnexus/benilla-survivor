//! The innkeeper's window between the server and the screen, as [`crate::trainer`] is for the
//! class trainer: food and drink to buy with the town purse, and what the backpack holds to sell.

use std::sync::Mutex;

/// One line of the window: a ware to buy, or a backpack find to sell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ware {
    pub entry: u32,
    pub name: String,
    /// `ItemDisplayInfo.dbc` row, for its icon.
    pub display: u32,
    /// 0 poor ... 5 legendary.
    pub quality: u32,
    /// How many the backpack holds.
    pub count: u32,
    /// Copper: one to buy, or all of them to sell.
    pub price: u32,
    /// To buy: the camp level it sells from (above the player's, it shows but cannot be bought).
    pub level: u32,
    /// What it does when eaten (restores, run buff); empty for what is not food.
    pub about: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    /// Distinct per post, so an answer to an old one is never taken for the new one.
    pub id: u64,
    pub keeper: String,
    /// The keeper's guid, for their portrait.
    pub keeper_guid: u64,
    /// The town purse, copper.
    pub money: u32,
    /// The camp level.
    pub level: u32,
    pub buy: Vec<Ware>,
    pub sell: Vec<Ware>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// One of the ware.
    Buy(u32),
    /// Every one of the item the backpack holds.
    Sell(u32),
    /// Every grey (poor quality) item the backpack holds.
    SellTrash,
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

pub(crate) fn post(mut w: Window) -> u64 {
    let mut b = board();
    w.id = b.next_id;
    b.next_id += 1;
    let id = w.id;
    b.window = Some(w);
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
