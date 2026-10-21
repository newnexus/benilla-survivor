//! The pretend 1.12.1 server the survivor-like plays against: a realmd and a world server on
//! loopback, started inside the `survivor` launcher before the client boots. The client speaks its
//! ordinary protocol to it and never knows there is no real server.
//!
//! Loopback only, by construction: both listeners bind `127.0.0.1`.

pub mod aim;
pub mod bar;
mod data;
mod game;
mod packets;
pub mod picks;
mod realmd;
mod roster;
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
