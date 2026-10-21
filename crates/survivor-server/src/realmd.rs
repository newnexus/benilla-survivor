//! The logon server: the server half of SRP6 for any account name with [`crate::PASSWORD`], then
//! a one-realm list pointing at the world server. Adapted from benilla-app's test fake
//! (`net/io/fake.rs`).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::Sender;
use std::thread;

use benilla_srp::{
    password_verifier, NormalizedString, GENERATOR, LARGE_SAFE_PRIME_LITTLE_ENDIAN,
    SESSION_KEY_LENGTH,
};
use num_bigint::BigUint;
use sha1::{Digest, Sha1};

/// A session key, realmd to the world server.
pub(crate) type Key = [u8; SESSION_KEY_LENGTH];

const SALT: [u8; 32] = [0x5a; 32];
use crate::REALM_NAME;

fn sha1(parts: &[&[u8]]) -> [u8; 20] {
    let mut h = Sha1::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

fn le32(v: &BigUint) -> [u8; 32] {
    let raw = v.to_bytes_le();
    let mut out = [0u8; 32];
    out[..raw.len()].copy_from_slice(&raw);
    out
}

/// `SHA1_Interleave` over all 32 bytes of `S`, as vmangos's `SRP6::HashSessionKey` does.
fn interleave(s: &[u8; 32]) -> Key {
    let even: Vec<u8> = s.iter().step_by(2).copied().collect();
    let odd: Vec<u8> = s.iter().skip(1).step_by(2).copied().collect();
    let (g, h) = (sha1(&[&even]), sha1(&[&odd]));
    let mut out = [0u8; SESSION_KEY_LENGTH];
    for (i, (gi, hi)) in g.iter().zip(&h).enumerate() {
        out[i * 2] = *gi;
        out[i * 2 + 1] = *hi;
    }
    out
}

pub(crate) fn serve(listener: TcpListener, world_addr: String, keys: Sender<Key>) {
    thread::Builder::new()
        .name("survivor-realmd".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(s) = stream else { return };
                let (world_addr, keys) = (world_addr.clone(), keys.clone());
                thread::spawn(move || serve_link(s, &world_addr, &keys));
            }
        })
        .expect("spawn survivor-realmd");
}

/// One realmd connection: the logon, then the realm list for every request until EOF.
fn serve_link(mut s: TcpStream, world_addr: &str, keys: &Sender<Key>) {
    // CMD_AUTH_LOGON_CHALLENGE_Client: opcode, protocol, u16 size, then the body, whose account
    // name is the length-prefixed tail at offset 29.
    let mut head = [0u8; 4];
    if s.read_exact(&mut head).is_err() {
        return;
    }
    let mut body = vec![0u8; usize::from(u16::from_le_bytes([head[2], head[3]]))];
    if s.read_exact(&mut body).is_err() || body.len() < 30 {
        return;
    }
    let name_len = usize::from(body[29]);
    let Some(name) = body.get(30..30 + name_len) else {
        return;
    };
    let Ok(user) = NormalizedString::new(String::from_utf8_lossy(name)) else {
        return;
    };
    let pass = NormalizedString::new(crate::PASSWORD).expect("a valid password");

    let n = BigUint::from_bytes_le(&LARGE_SAFE_PRIME_LITTLE_ENDIAN);
    let g = BigUint::from(GENERATOR);
    let v = BigUint::from_bytes_le(&password_verifier(&user, &pass, &SALT));

    // B = k·v + g^b, with k = 3; `b` walks until `B`'s top byte is non-zero, since the client
    // redials on a zero one.
    let (b, b_pub) = (1u32..)
        .map(|i| {
            let b = BigUint::from_bytes_le(&[0x3c; 32]) + i;
            let b_pub = le32(&((BigUint::from(3u8) * &v + g.modpow(&b, &n)) % &n));
            (b, b_pub)
        })
        .find(|(_, b_pub)| b_pub[31] != 0)
        .expect("some b gives a full-width B");
    let mut reply = vec![0x00, 0x00, 0x00];
    reply.extend_from_slice(&b_pub);
    reply.extend_from_slice(&[1, GENERATOR, 32]);
    reply.extend_from_slice(&LARGE_SAFE_PRIME_LITTLE_ENDIAN);
    reply.extend_from_slice(&SALT);
    reply.extend_from_slice(&[0u8; 16]); // crc_salt
    reply.push(0); // security_flag
    if s.write_all(&reply).is_err() {
        return;
    }

    // CMD_AUTH_LOGON_PROOF_Client: opcode, A, M1, crc_hash, key count, security flag. A wrong
    // password gives a key the client's own proof check rejects.
    let mut proof = [0u8; 75];
    if s.read_exact(&mut proof).is_err() {
        return;
    }
    let (a_pub, m1) = (&proof[1..33], &proof[33..53]);
    let u = BigUint::from_bytes_le(&sha1(&[a_pub, &b_pub]));
    let a = BigUint::from_bytes_le(a_pub);
    let big_s = (a * v.modpow(&u, &n) % &n).modpow(&b, &n);
    let key = interleave(&le32(&big_s));
    let mut reply = vec![0x01, 0x00];
    reply.extend_from_slice(&sha1(&[a_pub, m1, &key]));
    reply.extend_from_slice(&0u32.to_le_bytes()); // hardware_survey_id
    let _ = keys.send(key);
    if s.write_all(&reply).is_err() {
        return;
    }

    // CMD_REALM_LIST_Client (0x10 and a u32), answered every time, until the client closes it.
    let mut request = [0u8; 5];
    while s.read_exact(&mut request).is_ok() {
        if request[0] != 0x10 {
            return;
        }
        let mut record = Vec::new();
        record.extend_from_slice(&0u32.to_le_bytes()); // type: Normal
        record.push(0); // flags
        record.extend_from_slice(format!("{REALM_NAME}\0{world_addr}\0").as_bytes());
        record.extend_from_slice(&0.5f32.to_le_bytes()); // population
        record.extend_from_slice(&[0, 1, 1]); // characters, category, id
        let mut list = vec![0x10];
        list.extend_from_slice(&((4 + 1 + record.len() + 2) as u16).to_le_bytes());
        list.extend_from_slice(&0u32.to_le_bytes());
        list.push(1);
        list.extend_from_slice(&record);
        list.extend_from_slice(&0x0002u16.to_le_bytes()); // vmangos's trailer
        if s.write_all(&list).is_err() {
            return;
        }
    }
}
