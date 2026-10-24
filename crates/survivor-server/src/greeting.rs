//! A townsperson's greeting between the server and the survivor's screen, as [`crate::quest_board`]
//! is for a quest: the server posts who speaks, what they say and what they offer, a link each;
//! the screen shows it and answers with the link picked (or closes it).

use std::sync::Mutex;

/// What a link leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Service {
    /// One of their quests: to take, or (`reward`) to hand in.
    Quest { id: u32, reward: bool },
    /// Their wares.
    Shop,
    /// Making things with them.
    Craft,
}

/// One of the options under the greeting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub service: Service,
    pub label: String,
    /// Not there yet: shown dimmed, no answer.
    pub soon: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Greeting {
    /// Distinct per post, so an answer to an old one is never taken for the new one.
    pub id: u64,
    /// Who speaks: their guid (for their portrait) and name.
    pub giver: u64,
    pub giver_name: String,
    /// What they say, `$N` and the like filled in.
    pub text: String,
    pub links: Vec<Link>,
}

/// The screen's answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Pick(Service),
    Close,
}

struct Board {
    greeting: Option<Greeting>,
    answer: Option<(u64, Answer)>,
    next_id: u64,
}

static BOARD: Mutex<Board> = Mutex::new(Board {
    greeting: None,
    answer: None,
    next_id: 1,
});

fn board() -> std::sync::MutexGuard<'static, Board> {
    BOARD.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The greeting to show, if one is open.
pub fn current() -> Option<Greeting> {
    board().greeting.clone()
}

/// The player answered greeting `id`.
pub fn answer(id: u64, a: Answer) {
    board().answer = Some((id, a));
}

/// Open a greeting; its id comes back.
pub(crate) fn post(mut g: Greeting) -> u64 {
    let mut b = board();
    g.id = b.next_id;
    b.next_id += 1;
    let id = g.id;
    b.greeting = Some(g);
    b.answer = None;
    id
}

pub(crate) fn take_answer() -> Option<(u64, Answer)> {
    board().answer.take()
}

pub(crate) fn clear() {
    let mut b = board();
    b.greeting = None;
    b.answer = None;
}
