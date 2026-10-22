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

    static GIVERS: Mutex<Vec<[f32; 3]>> = Mutex::new(Vec::new());

    pub fn current() -> Vec<[f32; 3]> {
        GIVERS.lock().map(|g| g.clone()).unwrap_or_default()
    }

    pub(crate) fn set(list: Vec<[f32; 3]>) {
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
mod game;
mod packets;
pub mod picks;
pub mod quest_board;

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
