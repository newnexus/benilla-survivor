//! World-socket framing and a little-endian body builder.

use std::io::{Read, Write};
use std::net::TcpStream;

use benilla_srp::vanilla_header::HeaderCrypto;

/// A packet body under construction, little-endian as the 1.12.1 wire is.
#[derive(Default)]
pub(crate) struct Body(pub(crate) Vec<u8>);

impl Body {
    pub(crate) fn new() -> Self {
        Self::default()
    }
    pub(crate) fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    pub(crate) fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub(crate) fn u64(&mut self, v: u64) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub(crate) fn f32(&mut self, v: f32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub(crate) fn cstr(&mut self, s: &str) -> &mut Self {
        self.0.extend_from_slice(s.as_bytes());
        self.0.push(0);
        self
    }
    /// A packed guid: a mask byte naming the non-zero bytes, then those bytes.
    pub(crate) fn packed_guid(&mut self, guid: u64) -> &mut Self {
        let at = self.0.len();
        self.0.push(0);
        for (i, byte) in guid.to_le_bytes().into_iter().enumerate() {
            if byte != 0 {
                self.0[at] |= 1 << i;
                self.0.push(byte);
            }
        }
        self
    }
}

/// One world-socket link: the stream and, once authenticated, the header cipher.
pub(crate) struct Link {
    pub(crate) stream: TcpStream,
    pub(crate) crypto: Option<HeaderCrypto>,
}

/// The sending half of an authenticated link, shareable between the session thread and the game
/// loop. Its cipher is its own instance from the same key: the two directions' states are
/// independent, so the reader keeps decrypting with the [`Link`]'s.
#[derive(Clone)]
pub(crate) struct Sender(std::sync::Arc<std::sync::Mutex<(TcpStream, HeaderCrypto)>>);

impl Sender {
    pub(crate) fn new(stream: TcpStream, crypto: HeaderCrypto) -> Self {
        Self(std::sync::Arc::new(std::sync::Mutex::new((stream, crypto))))
    }

    pub(crate) fn send(&self, op: u16, body: &[u8]) -> std::io::Result<()> {
        let mut guard = self
            .0
            .lock()
            .map_err(|_| std::io::Error::other("sender poisoned"))?;
        let (stream, crypto) = &mut *guard;
        let size = ((body.len() + 2) as u16).to_be_bytes();
        let op = op.to_le_bytes();
        let mut header = [size[0], size[1], op[0], op[1]];
        crypto.encrypter().encrypt(&mut header);
        let mut packet = header.to_vec();
        packet.extend_from_slice(body);
        stream.write_all(&packet)
    }
}

impl Link {
    /// A server packet: `u16` BE size counting the opcode, `u16` LE opcode, the body.
    pub(crate) fn send(&mut self, op: u16, body: &[u8]) -> std::io::Result<()> {
        let size = ((body.len() + 2) as u16).to_be_bytes();
        let op = op.to_le_bytes();
        let mut header = [size[0], size[1], op[0], op[1]];
        if let Some(c) = self.crypto.as_mut() {
            c.encrypter().encrypt(&mut header);
        }
        let mut packet = header.to_vec();
        packet.extend_from_slice(body);
        self.stream.write_all(&packet)
    }

    /// A client packet: `u16` BE size counting the 4-byte opcode, `u32` LE opcode, the body.
    pub(crate) fn recv(&mut self) -> std::io::Result<(u16, Vec<u8>)> {
        let mut header = [0u8; 6];
        self.stream.read_exact(&mut header)?;
        if let Some(c) = self.crypto.as_mut() {
            c.decrypter().decrypt(&mut header);
        }
        let size = usize::from(u16::from_be_bytes([header[0], header[1]]));
        let mut body = vec![0u8; size.saturating_sub(4)];
        self.stream.read_exact(&mut body)?;
        Ok((u16::from_le_bytes([header[2], header[3]]), body))
    }
}

/// A NUL-terminated string at the front of `b`, and the rest after its NUL.
pub(crate) fn split_cstr(b: &[u8]) -> (String, &[u8]) {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    let rest = b.get(end + 1..).unwrap_or(&[]);
    (String::from_utf8_lossy(&b[..end]).into_owned(), rest)
}
