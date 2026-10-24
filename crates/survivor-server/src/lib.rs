//! The pretend 1.12.1 server the survivor-like plays against: a realmd and a world server on
//! loopback, started inside the `survivor` launcher before the client boots. The client speaks its
//! ordinary protocol to it and never knows there is no real server.
//!
//! Loopback only, by construction: both listeners bind `127.0.0.1`.

pub mod aim;
pub mod armory;

/// A run's clock, for the screen: how long it has lasted and when the next boss comes. Frozen
/// while the world is paused (the server's tick stops); none in town.
pub mod run_clock {
    use std::sync::Mutex;

    #[derive(Clone, Debug, PartialEq)]
    pub struct Clock {
        /// Seconds since the run began.
        pub survived: f32,
        /// Seconds until the next boss, and its name.
        pub boss_in: f32,
        pub boss: String,
    }

    static CLOCK: Mutex<Option<Clock>> = Mutex::new(None);

    pub fn current() -> Option<Clock> {
        CLOCK.lock().ok()?.clone()
    }

    pub(crate) fn set(clock: Option<Clock>) {
        if let Ok(mut c) = CLOCK.lock() {
            *c = clock;
        }
    }
}

/// Where the quest givers with something for the player stand, while in town, for the screen's
/// trails to them.
pub mod trails {
    use std::sync::Mutex;

    /// A trail's end: where it leads, and whether it is a trainer's (blue) rather than a quest
    /// giver's (gold).
    pub type End = ([f32; 3], bool);

    static GIVERS: Mutex<Vec<End>> = Mutex::new(Vec::new());

    pub fn current() -> Vec<End> {
        GIVERS.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub(crate) fn set(list: Vec<End>) {
        if let Ok(mut g) = GIVERS.lock() {
            *g = list;
        }
    }
}


/// The souls and coins loose in the world, for the screen to draw.
pub mod orbs {
    use std::sync::Mutex;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Kind {
        /// A creature's experience: a light blue orb.
        Soul,
        /// Its gold: one coin per piece of copper, silver or gold, single or worth 5 or 25.
        Coin,
        /// A rune on a run, by kind: power, haste, focus, mending.
        Rune(u8),
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Orb {
        pub id: u64,
        pub kind: Kind,
        /// World yards.
        pub pos: [f32; 3],
        /// A coin's worth in copper (1, 5, 25, 100, ...); a soul's experience.
        pub amount: u32,
    }

    static ORBS: Mutex<(u64, Vec<Orb>)> = Mutex::new((0, Vec::new()));

    /// Every orb, and a count that moves on with each server tick.
    pub fn current() -> (u64, Vec<Orb>) {
        ORBS.lock().map(|o| o.clone()).unwrap_or_default()
    }

    pub(crate) fn set(list: Vec<Orb>) {
        if let Ok(mut o) = ORBS.lock() {
            if list.is_empty() && o.1.is_empty() {
                return;
            }
            o.0 += 1;
            o.1 = list;
        }
    }
}

/// The screen's game menu (Escape) is open: the world stands still until it closes.
pub mod menu_pause {
    use std::sync::atomic::{AtomicBool, Ordering};

    static OPEN: AtomicBool = AtomicBool::new(false);

    pub fn set(open: bool) {
        OPEN.store(open, Ordering::Relaxed);
    }

    pub(crate) fn open() -> bool {
        OPEN.load(Ordering::Relaxed)
    }
}

/// The Hearthstone's charge for the screen: how far along, 0 to 1, while it charges.
pub mod hearth {
    use std::sync::Mutex;

    static CHARGE: Mutex<Option<f32>> = Mutex::new(None);

    pub fn charge() -> Option<f32> {
        *CHARGE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn set(part: Option<f32>) {
        *CHARGE.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = part;
    }
}

/// A pickup charging (a gather object being gathered), for the screen: where, how far along,
/// and the icon to show.
pub mod pickup {
    use std::sync::Mutex;

    #[derive(Clone, Copy, Debug)]
    pub struct Pickup {
        /// WoW world coordinates of the object.
        pub pos: [f32; 3],
        /// 0 to 1.
        pub part: f32,
        /// An interface texture path, without `.blp`.
        pub icon: &'static str,
    }

    static NOW: Mutex<Option<Pickup>> = Mutex::new(None);

    pub fn current() -> Option<Pickup> {
        *NOW.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn set(p: Option<Pickup>) {
        *NOW.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = p;
    }
}

/// The elites and bosses in view, for the big bars along the screen's bottom: name, health,
/// most first (the newest last).
pub mod big_bars {
    use std::sync::Mutex;

    #[derive(Clone, Debug, PartialEq)]
    pub struct Bar {
        pub guid: u64,
        pub name: String,
        pub health: u32,
        pub max: u32,
    }

    static BARS: Mutex<Vec<Bar>> = Mutex::new(Vec::new());

    pub fn current() -> Vec<Bar> {
        BARS.lock().map(|b| b.clone()).unwrap_or_default()
    }

    pub(crate) fn set(bars: Vec<Bar>) {
        if let Ok(mut b) = BARS.lock() {
            *b = bars;
        }
    }
}

/// The unit the player watches over (a trainer defended or escorted), for a focus frame: 1.12
/// has none, the later game's shows it under the target frame.
pub mod focus {
    use std::sync::Mutex;

    #[derive(Clone, Debug, PartialEq)]
    pub struct Focus {
        pub guid: u64,
        pub name: String,
        pub level: u32,
        pub health: u32,
        pub max: u32,
    }

    static FOCUS: Mutex<Option<Focus>> = Mutex::new(None);

    pub fn current() -> Option<Focus> {
        FOCUS.lock().ok().and_then(|f| f.clone())
    }

    pub(crate) fn set(focus: Option<Focus>) {
        if let Ok(mut f) = FOCUS.lock() {
            *f = focus;
        }
    }
}

/// A run's end, for the summary window that stays up until the player closes it.
pub mod run_summary {
    use std::sync::Mutex;

    #[derive(Clone, Debug, PartialEq)]
    pub struct Summary {
        /// Distinct per run, so a closed one is never shown again.
        pub id: u64,
        pub died: bool,
        pub town: String,
        pub level: u32,
        pub seconds: f32,
        pub kills: u32,
        /// Copper picked up on the run, and the part brought home.
        pub found: u32,
        pub brought: u32,
        /// Souls picked up, and the ones brought home (none after a death).
        pub souls: u32,
        pub souls_home: u32,
    }

    static NOW: Mutex<(u64, Option<Summary>)> = Mutex::new((0, None));

    pub fn current() -> Option<Summary> {
        NOW.lock().ok().and_then(|n| n.1.clone())
    }

    /// The player closed it.
    pub fn close() {
        if let Ok(mut n) = NOW.lock() {
            n.1 = None;
        }
    }

    pub(crate) fn post(mut s: Summary) {
        if let Ok(mut n) = NOW.lock() {
            n.0 += 1;
            s.id = n.0;
            n.1 = Some(s);
        }
    }
}

/// Loot picked up, for the screen's feed over the backpack.
pub mod loot_feed {
    use std::sync::Mutex;

    #[derive(Clone, Debug, PartialEq)]
    pub struct Pickup {
        pub entry: u32,
        pub count: u32,
        /// `ItemDisplayInfo.dbc` row, for its icon.
        pub display: u32,
        /// 0 poor ... 5 legendary.
        pub quality: u32,
        pub name: String,
    }

    /// Picked up, and whether some or all of it did not fit (the backpack full).
    static FEED: Mutex<Vec<(Pickup, bool)>> = Mutex::new(Vec::new());

    /// Everything picked up since the last call, oldest first.
    pub fn take() -> Vec<(Pickup, bool)> {
        FEED.lock().map(|mut f| std::mem::take(&mut *f)).unwrap_or_default()
    }

    pub(crate) fn post(p: Pickup, full: bool) {
        if let Ok(mut f) = FEED.lock() {
            f.push((p, full));
        }
    }
}

/// The player's purse, copper, for the screen's corner.
pub mod purse {
    use std::sync::atomic::{AtomicU32, Ordering};

    static MONEY: AtomicU32 = AtomicU32::new(0);

    pub fn money() -> u32 {
        MONEY.load(Ordering::Relaxed)
    }

    pub(crate) fn set(copper: u32) {
        MONEY.store(copper, Ordering::Relaxed);
    }
}
pub mod bar;
mod data;
pub use data::{quest_voice, quest_voice_seconds};
mod game;
mod packets;
pub mod picks;
pub mod quest_board;
pub mod greeting;
pub mod events;
pub mod shop;
pub mod spirit;
pub mod trainer;

/// The closed chests and the rarity of the best item in each, for the screen's glow.
pub mod chest_glow {
    use std::sync::Mutex;

    static GLOWS: Mutex<(u64, Vec<([f32; 3], u8)>)> = Mutex::new((0, Vec::new()));

    /// Every closed chest's place and best rarity (0 common to 4 legendary), and a version that
    /// changes with the list.
    pub fn current() -> (u64, Vec<([f32; 3], u8)>) {
        GLOWS.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// The collection chests' glow, gold, past the rarities.
    pub const COLLECTION: u8 = 5;

    pub(crate) fn set(list: Vec<([f32; 3], u8)>) {
        if let Ok(mut g) = GLOWS.lock() {
            g.0 += 1;
            g.1 = list;
        }
    }
}
mod realmd;
mod roster;
pub mod town;
pub mod waves;
mod wire;
mod world;

use std::net::TcpListener;
use std::sync::mpsc;

pub use roster::{UNDEAD, UNDEAD_CLASSES};

/// The password every account logs in with. The launcher hands it to the client through
/// `$WOW_PASS`, so the login screen never asks.
pub const PASSWORD: &str = "survivor";

/// The one realm the logon server lists; the launcher names it in `$WOW_REALM` so the client
/// dials it without showing the realm list.
pub const REALM_NAME: &str = "Tirisfall Survivors";

/// The account the launcher logs in as.
pub const ACCOUNT: &str = "SURVIVOR";

/// What [`start`] hands back: where the client should dial.
pub struct Running {
    /// `127.0.0.1:<port>` of the realmd, for `$WOW_HOST`.
    pub realmd: String,
}

/// Bind both servers on free loopback ports and serve them on background threads for the life of
/// the process.
pub fn start() -> std::io::Result<Running> {
    let world = TcpListener::bind("127.0.0.1:0")?;
    let world_addr = world.local_addr()?.to_string();
    let realmd = TcpListener::bind("127.0.0.1:0")?;
    let realmd_addr = realmd.local_addr()?.to_string();
    let (keys_tx, keys_rx) = mpsc::channel();
    realmd::serve(realmd, world_addr, keys_tx);
    world::serve(world, keys_rx);
    Ok(Running {
        realmd: realmd_addr,
    })
}
