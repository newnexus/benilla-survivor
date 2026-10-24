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
    /// Souls back for taking the last level off (free refund); none at level 0.
    pub refund: Option<u32>,
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
    /// Take a level off, its souls back.
    Refund(Upgrade),
    /// Every talent point back, free.
    ResetTalents,
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

/// Whether quest `id`'s words are the spirit healer's broadcast: the quest window tells them over
/// and over, and goes on to the objectives only when asked.
pub fn quest_broadcasts(id: u32) -> bool {
    id == quest::ID
}

/// The spirit healer's own quest: the wandering souls (the field souls a run walks through)
/// gathered for it. Its words come through broken, a message sent over and over that never
/// quite reaches the living.
pub(crate) mod quest {
    use crate::data::OBJECTIVE_GAMEOBJECT;

    /// The spirit healer's creature entry.
    pub(crate) const SPIRIT_HEALER: u32 = 6491;
    pub(crate) const ID: u32 = 91_001;
    /// The objective's target: no creature, no object in the world; a field soul counts it.
    pub(crate) const SOULS: u32 = OBJECTIVE_GAMEOBJECT | ID;
    pub(crate) const COUNT: u32 = 30;
    pub(crate) const XP: u32 = 300;
    pub(crate) const MONEY: u32 = 10;
    pub(crate) const TITLE: &str = "...The Wandering... Wandering...";
    // No number in its words: how many shows only in the objective's count.
    pub(crate) const DETAILS: &str = "...can... can you hear... living-not-living one... \
        do you hear...$B$B...souls. Lost ones. They wander... wander under the trees, round and \
        round... cannot find... the light cannot find them...$B$B...gather. Gather them... many. \
        Walk through them... they follow you... bring... bring them...$B$B...can... can you \
        hear... living-not-living one... do you hear...$B$B...souls. Lost ones. They wander... \
        wander...$B$B...gather. Gather th-- ...bring them to the li--$B$B...can... can you \
        hear...";
    pub(crate) const OBJECTIVES: &str = "...the wandering ones... beyond the fire wall... walk \
        through... gather... gather... ...the wandering ones... gath--";
    pub(crate) const OBJECTIVE: &str = "...wandering ones... gathered...";
    /// Said by the healer, its upgrades still shut, while the first quest is out.
    pub(crate) const NOT_YET: &str = "...not yet... the wandering ones... not yet... not y--";
    /// Part two: its mind a little clearer for the souls brought, more of them; it pays a reroll.
    pub(crate) const ID2: u32 = 91_002;
    pub(crate) const COUNT2: u32 = 40;
    pub(crate) const XP2: u32 = 450;
    pub(crate) const MONEY2: u32 = 15;
    pub(crate) const TITLE2: &str = "The Wandering Ones";
    pub(crate) const DETAILS2: &str = "You. Yes... you. The one who walked through them.\
        $B$BSince they came into the light, the fog in me has thinned. I can... think. A \
        little. I remember this yard. Some of the names on the stones, I remember.$B$BBut \
        there are more out there. Many more. They drift between the trees and do not know \
        they are dead... as you once did not.$B$BGather them again, living-not-living one. \
        Bring them home, and I will give you something of mine: another look at fate, when \
        the cards fall wrong.$B$B...the fog... it is still there... at the edges...";
    pub(crate) const OBJECTIVES2: &str = "Gather the wandering souls beyond the barrier, then \
        return to the spirit healer.";
    pub(crate) const OBJECTIVE2: &str = "Wandering souls gathered";
    /// Said instead of its upgrades while the second quest is out.
    pub(crate) const NOT_YET2: &str = "Not yet. There are more of them out there. Bring them home first.";
    pub(crate) const DONE2: &str = "More of them home. More of me, too.$B$BI said I would give \
        you something. Here: when the cards are dealt to you and you do not like them, ask \
        again. Once more, each time you go out.$B$BThank you, $N. I had forgotten that word. \
        Thank... you.";
    /// What a quest of the healer's gives beside XP and money, for the quest window.
    pub(crate) fn perk(id: u32) -> Option<&'static str> {
        (id == ID2).then_some("Reroll: +1 a run")
    }
    pub(crate) const DONE: &str = "...the light... the light holds them now...$B$B...you \
        heard... you heard...$B$B...the light holds them now... the light hol--$B$B...can... \
        can you hear...";
}
