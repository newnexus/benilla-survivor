//! The quest giver's window between the server and the survivor's own screen, as
//! [`crate::picks`] is for the level-up cards: the server posts a [`Panel`] instead of the stock
//! quest packets, the screen shows it and answers Accept (or Complete) or closes it.

use std::sync::Mutex;

/// What the window is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelKind {
    /// A quest to take: Accept.
    Offer,
    /// A finished quest to hand in: Complete.
    Reward,
}

/// One quest giver's window: every quest they have for the player, one tab each.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Panel {
    /// Distinct per window, so an answer to an old one is never taken for the new one.
    pub id: u64,
    /// The quest giver speaking: their guid (for their portrait) and name.
    pub giver: u64,
    pub giver_name: String,
    pub entries: Vec<Entry>,
}

/// One quest on the window, its text ready to show (WoW's `$N`, `$B`... already filled in).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub quest: u32,
    pub kind: PanelKind,
    pub title: String,
    /// The giver's story (an offer) or thanks (a hand-in).
    pub text: String,
    /// What to do, for an offer.
    pub objectives: String,
    pub money: u32,
    pub xp: u32,
    /// The items to choose one of on handing it in.
    pub rewards: Vec<RewardItem>,
}

/// A reward item: its entry, name, and `ItemDisplayInfo.dbc` row for its icon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RewardItem {
    pub entry: u32,
    pub name: String,
    pub display: u32,
    /// The item as it will come, its survivor stats already rolled, for its preview.
    pub preview: Option<crate::armory::Row>,
}

/// The screen's answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// Accept (or complete) this quest, with the reward item chosen (0 for none).
    Accept(u32, u32),
    Close,
}

struct Board {
    panel: Option<Panel>,
    answer: Option<(u64, Answer)>,
    next_id: u64,
}

static BOARD: Mutex<Board> = Mutex::new(Board {
    panel: None,
    answer: None,
    next_id: 1,
});

fn board() -> std::sync::MutexGuard<'static, Board> {
    BOARD.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The window to show, if one is open.
pub fn current() -> Option<Panel> {
    board().panel.clone()
}

/// The player answered window `id`.
pub fn answer(id: u64, a: Answer) {
    board().answer = Some((id, a));
}

/// Open a window; its id comes back.
pub(crate) fn post(mut panel: Panel) -> u64 {
    let mut b = board();
    panel.id = b.next_id;
    b.next_id += 1;
    let id = panel.id;
    b.panel = Some(panel);
    b.answer = None;
    id
}

/// The screen's answer, once.
pub(crate) fn take_answer() -> Option<(u64, Answer)> {
    board().answer.take()
}

/// Close whatever is open.
pub(crate) fn clear() {
    let mut b = board();
    b.panel = None;
    b.answer = None;
}
