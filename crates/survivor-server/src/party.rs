//! Playing together on one home network. The first game started becomes the party's host; a
//! game started after it finds the host and joins. Each player keeps their own pretend server
//! (the survivor screens read it in-process), and the servers talk to each other over this link:
//! who each player is and how they move, each game drawing the others as players.
//!
//! - Finding the host: a UDP shout on [`PORT`] to the network's broadcast address (and to this
//!   PC, for two games on one machine). A host answers with its link's TCP port; with no answer
//!   within [`LISTEN_FOR`] the game becomes the host and answers others from then on.
//! - The link: TCP, messages framed as `u32 length, u8 kind, u32 sender, body`. The host passes
//!   every message on to the other joiners, so all see all.
//! - `$SURVIVOR_PARTY=off` plays alone.
//!
//! The WoW servers stay on loopback; only this link listens on the network.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

/// The UDP port a host listens on for shouts.
const PORT: u16 = 47_123;
const SHOUT: &[u8] = b"TIRISFALL?";
const ANSWER: &str = "TIRISFALL!";
/// How long a starting game listens for a host's answer.
const LISTEN_FOR: Duration = Duration::from_millis(1200);
/// Another player's guid in our world: this base, the low bits their party id's.
const GUID_BASE: u64 = 0x0070_0000;

const KIND_HELLO: u8 = 1;
const KIND_MOVE: u8 = 2;
const KIND_BYE: u8 = 3;

/// Who a player is: their character, and where they stand.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Hello {
    pub(crate) name: String,
    pub(crate) race: u8,
    pub(crate) class: u8,
    pub(crate) gender: u8,
    pub(crate) skin: u8,
    pub(crate) face: u8,
    pub(crate) hair_style: u8,
    pub(crate) hair_color: u8,
    pub(crate) facial_hair: u8,
    pub(crate) level: u8,
    pub(crate) pos: [f32; 3],
    pub(crate) facing: f32,
}

/// What happened to another player since the game last looked.
#[derive(Clone, Debug)]
pub(crate) enum Event {
    /// In the world (or their character changed): show them.
    Here(u64, Hello),
    /// Moved: a movement opcode and its `MovementInfo`, as their client sent it.
    Moved(u64, u16, Vec<u8>),
    Gone(u64),
}

/// The links to the other games: the host's to each joiner, a joiner's to the host.
struct Links(Vec<TcpStream>);

struct State {
    /// This game's party id.
    me: u32,
    /// Our own player while in the world, for whoever joins later.
    my_hello: Option<Hello>,
    /// The others in the world, by their guid here.
    others: HashMap<u64, Hello>,
    /// For the game, oldest first.
    events: Vec<Event>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static LINKS: Mutex<Links> = Mutex::new(Links(Vec::new()));

fn state() -> std::sync::MutexGuard<'static, Option<State>> {
    STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn links() -> std::sync::MutexGuard<'static, Links> {
    LINKS.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Another player's guid in our world.
pub(crate) fn guid_of(id: u32) -> u64 {
    GUID_BASE | u64::from(id & 0xF_FFFF)
}

/// Find a host or become one, on background threads. Called once, as the servers start.
pub(crate) fn start() {
    if std::env::var("SURVIVOR_PARTY").is_ok_and(|v| v.eq_ignore_ascii_case("off")) {
        return;
    }
    // Distinct enough for a few games on one network.
    let me = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.subsec_nanos() ^ std::process::id().rotate_left(16))
        | 1;
    *state() = Some(State {
        me,
        my_hello: None,
        others: HashMap::new(),
        events: Vec::new(),
    });
    let _ = thread::Builder::new().name("survivor-party".into()).spawn(move || {
        match find_host() {
            Some(host) => join(host, me),
            None => host(me),
        }
    });
}

/// Shout for a host; its link's address if one answers.
fn find_host() -> Option<SocketAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.set_broadcast(true).ok()?;
    socket.set_read_timeout(Some(LISTEN_FOR)).ok()?;
    for to in ["255.255.255.255", "127.0.0.1"] {
        let _ = socket.send_to(SHOUT, (to, PORT));
    }
    let mut buf = [0u8; 64];
    let (n, from) = socket.recv_from(&mut buf).ok()?;
    let text = std::str::from_utf8(&buf[..n]).ok()?;
    let port: u16 = text.strip_prefix(ANSWER)?.trim().parse().ok()?;
    eprintln!("survivor-party: a host at {}, joining", from.ip());
    Some(SocketAddr::new(from.ip(), port))
}

/// Be the host: answer shouts, take joiners, pass each one's messages on to the rest.
fn host(me: u32) {
    let Ok(listener) = TcpListener::bind("0.0.0.0:0") else { return };
    let Ok(port) = listener.local_addr().map(|a| a.port()) else { return };
    let Ok(udp) = UdpSocket::bind(("0.0.0.0", PORT)) else {
        // Another game on this PC has the port and did not answer: play alone.
        eprintln!("survivor-party: port {PORT} taken, playing alone");
        return;
    };
    eprintln!("survivor-party: hosting (link port {port})");
    let _ = thread::Builder::new().name("survivor-party-shouts".into()).spawn(move || {
        let mut buf = [0u8; 64];
        while let Ok((n, from)) = udp.recv_from(&mut buf) {
            if &buf[..n] == SHOUT {
                let _ = udp.send_to(format!("{ANSWER}{port}").as_bytes(), from);
            }
        }
    });
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let _ = stream.set_nodelay(true);
        eprintln!("survivor-party: {} joined", stream.peer_addr().map_or_else(|_| "?".into(), |a| a.ip().to_string()));
        welcome(&stream, me);
        read_link(stream, me, true);
    }
}

/// A joiner: link to the host and read it.
fn join(host: SocketAddr, me: u32) {
    let Ok(stream) = TcpStream::connect_timeout(&host, Duration::from_secs(3)) else {
        eprintln!("survivor-party: the host did not take the link, playing alone");
        return;
    };
    let _ = stream.set_nodelay(true);
    welcome(&stream, me);
    // Read on this thread: the link lives as long as the game.
    read_link_blocking(stream, me, false);
}

/// A new link: our own player, and (the host) everyone else already here.
fn welcome(stream: &TcpStream, me: u32) {
    let Ok(mut w) = stream.try_clone() else { return };
    let (mine, others) = {
        let s = state();
        let Some(s) = s.as_ref() else { return };
        (s.my_hello.clone(), s.others.clone())
    };
    if let Some(h) = mine {
        let _ = w.write_all(&frame(KIND_HELLO, me, &hello_body(&h)));
    }
    for (guid, h) in others {
        let id = (guid & 0xF_FFFF) as u32;
        let _ = w.write_all(&frame(KIND_HELLO, id, &hello_body(&h)));
    }
    if let Ok(w) = stream.try_clone() {
        links().0.push(w);
    }
}

fn read_link(stream: TcpStream, me: u32, host: bool) {
    let _ = thread::Builder::new()
        .name("survivor-party-link".into())
        .spawn(move || read_link_blocking(stream, me, host));
}

/// Read one link until it closes. The host passes each message on to its other links.
fn read_link_blocking(mut stream: TcpStream, me: u32, host: bool) {
    let mut seen: Vec<u32> = Vec::new();
    let me_link = stream.peer_addr().ok();
    loop {
        let mut head = [0u8; 4];
        if stream.read_exact(&mut head).is_err() {
            break;
        }
        let len = u32::from_le_bytes(head) as usize;
        if !(5..=4096).contains(&len) {
            break;
        }
        let mut msg = vec![0u8; len];
        if stream.read_exact(&mut msg).is_err() {
            break;
        }
        let kind = msg[0];
        let from = u32::from_le_bytes([msg[1], msg[2], msg[3], msg[4]]);
        if from == me {
            continue;
        }
        if !seen.contains(&from) {
            seen.push(from);
        }
        if host {
            pass_on(me_link, &head, &msg);
        }
        take(kind, from, &msg[5..]);
    }
    // The link closed: everyone who came over it is gone.
    if let Some(addr) = me_link {
        links().0.retain(|s| s.peer_addr().ok() != Some(addr));
    }
    for id in seen {
        take(KIND_BYE, id, &[]);
    }
    eprintln!("survivor-party: a link closed");
}

/// The host: a message from one joiner to every other.
fn pass_on(from: Option<SocketAddr>, head: &[u8; 4], msg: &[u8]) {
    let mut l = links();
    for s in &mut l.0 {
        if s.peer_addr().ok() == from {
            continue;
        }
        let _ = s.write_all(head).and_then(|()| s.write_all(msg));
    }
}

/// A message from player `from`, for our game.
fn take(kind: u8, from: u32, body: &[u8]) {
    let mut s = state();
    let Some(s) = s.as_mut() else { return };
    let guid = guid_of(from);
    match kind {
        KIND_HELLO => {
            if let Some(h) = read_hello(body) {
                eprintln!("survivor-party: {} is here", h.name);
                s.others.insert(guid, h.clone());
                s.events.push(Event::Here(guid, h));
            }
        }
        KIND_MOVE if body.len() >= 2 + 24 => {
            let op = u16::from_le_bytes([body[0], body[1]]);
            let info = body[2..].to_vec();
            if let Some(h) = s.others.get_mut(&guid) {
                let rd = |o: usize| f32::from_le_bytes(info[o..o + 4].try_into().expect("4"));
                h.pos = [rd(8), rd(12), rd(16)];
                h.facing = rd(20);
            }
            s.events.push(Event::Moved(guid, op, info));
        }
        KIND_BYE => {
            if s.others.remove(&guid).is_some() {
                s.events.push(Event::Gone(guid));
            }
        }
        _ => {}
    }
}

/// Send to every link.
fn send_all(kind: u8, body: &[u8]) {
    let Some(me) = state().as_ref().map(|s| s.me) else { return };
    let msg = frame(kind, me, body);
    let mut l = links();
    l.0.retain_mut(|s| s.write_all(&msg).is_ok());
}

/// Our player came into the world (or back): the others are told.
pub(crate) fn here(h: Hello) {
    {
        let mut s = state();
        let Some(s) = s.as_mut() else { return };
        s.my_hello = Some(h.clone());
    }
    send_all(KIND_HELLO, &hello_body(&h));
}

/// Our player moved: a movement opcode and its `MovementInfo` from our client.
pub(crate) fn moved(op: u16, info: &[u8]) {
    {
        let mut s = state();
        let Some(s) = s.as_mut() else { return };
        let Some(h) = s.my_hello.as_mut() else { return };
        if info.len() >= 24 {
            let rd = |o: usize| f32::from_le_bytes(info[o..o + 4].try_into().expect("4"));
            h.pos = [rd(8), rd(12), rd(16)];
            h.facing = rd(20);
        }
    }
    let mut body = op.to_le_bytes().to_vec();
    body.extend_from_slice(info);
    send_all(KIND_MOVE, &body);
}

/// Our player left the world.
pub(crate) fn left() {
    {
        let mut s = state();
        let Some(s) = s.as_mut() else { return };
        if s.my_hello.take().is_none() {
            return;
        }
    }
    send_all(KIND_BYE, &[]);
}

/// What happened to the others since the last call.
pub(crate) fn take_events() -> Vec<Event> {
    state().as_mut().map(|s| std::mem::take(&mut s.events)).unwrap_or_default()
}

/// Everyone else in the world now, for a game just entered.
pub(crate) fn everyone() -> Vec<(u64, Hello)> {
    state()
        .as_ref()
        .map(|s| s.others.iter().map(|(g, h)| (*g, h.clone())).collect())
        .unwrap_or_default()
}

/// The name of another player, by their guid here.
pub(crate) fn hello_of(guid: u64) -> Option<Hello> {
    state().as_ref()?.others.get(&guid).cloned()
}

fn frame(kind: u8, from: u32, body: &[u8]) -> Vec<u8> {
    let mut m = Vec::with_capacity(9 + body.len());
    m.extend_from_slice(&(5 + body.len() as u32).to_le_bytes());
    m.push(kind);
    m.extend_from_slice(&from.to_le_bytes());
    m.extend_from_slice(body);
    m
}

fn hello_body(h: &Hello) -> Vec<u8> {
    let mut b = vec![
        h.race,
        h.class,
        h.gender,
        h.skin,
        h.face,
        h.hair_style,
        h.hair_color,
        h.facial_hair,
        h.level,
    ];
    for v in [h.pos[0], h.pos[1], h.pos[2], h.facing] {
        b.extend_from_slice(&v.to_le_bytes());
    }
    b.extend_from_slice(h.name.as_bytes());
    b
}

fn read_hello(b: &[u8]) -> Option<Hello> {
    if b.len() < 9 + 16 {
        return None;
    }
    let rd = |o: usize| f32::from_le_bytes(b[o..o + 4].try_into().expect("4"));
    Some(Hello {
        race: b[0],
        class: b[1],
        gender: b[2],
        skin: b[3],
        face: b[4],
        hair_style: b[5],
        hair_color: b[6],
        facial_hair: b[7],
        level: b[8],
        pos: [rd(9), rd(13), rd(17)],
        facing: rd(21),
        name: String::from_utf8_lossy(&b[25..]).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hello_reads_back_as_written() {
        let h = Hello {
            name: "Anna".into(),
            race: 5,
            class: 8,
            gender: 1,
            skin: 2,
            face: 3,
            hair_style: 4,
            hair_color: 5,
            facial_hair: 0,
            level: 3,
            pos: [1860.5, 1600.25, 94.0],
            facing: 1.5,
        };
        assert_eq!(read_hello(&hello_body(&h)), Some(h));
    }
}
