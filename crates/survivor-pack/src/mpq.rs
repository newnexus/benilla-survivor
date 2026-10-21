//! A minimal MPQ v1 writer: zlib-compressed 4 KiB sectors, encrypted hash and block tables, no
//! file encryption, exactly the envelope benilla-mpq reads.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

use anyhow::{ensure, Result};

const SIGNATURE: u32 = 0x1A51_504D;
const HEADER_SIZE: u32 = 32;
/// `512 << 3` = 4 KiB sectors, as the 1.12.1 archives use.
const SECTOR_SHIFT: u16 = 3;
const SECTOR_SIZE: usize = 512 << SECTOR_SHIFT;
const FLAG_COMPRESS: u32 = 0x0000_0200;
const FLAG_EXISTS: u32 = 0x8000_0000;
/// The sector codec byte for zlib.
const CODEC_ZLIB: u8 = 0x02;

/// The Storm encryption table, by the client's seed walk.
const fn encryption_table() -> [u32; 0x500] {
    let mut table = [0u32; 0x500];
    let mut seed: u32 = 0x0010_0001;
    let mut i1 = 0;
    while i1 < 0x100 {
        let mut i2 = 0;
        while i2 < 5 {
            let idx = i1 + i2 * 0x100;
            seed = (seed.wrapping_mul(125).wrapping_add(3)) % 0x2A_AAAB;
            let t1 = (seed & 0xFFFF) << 0x10;
            seed = (seed.wrapping_mul(125).wrapping_add(3)) % 0x2A_AAAB;
            let t2 = seed & 0xFFFF;
            table[idx] = t1 | t2;
            i2 += 1;
        }
        i1 += 1;
    }
    table
}

static TABLE: [u32; 0x500] = encryption_table();

const HASH_OFFSET: u32 = 0x000;
const HASH_NAME_A: u32 = 0x100;
const HASH_NAME_B: u32 = 0x200;
const HASH_FILE_KEY: u32 = 0x300;

/// Blizzard's string hash: `/` reads as `\`, ASCII uppercased.
fn hash_string(name: &str, kind: u32) -> u32 {
    let mut seed1: u32 = 0x7FED_7FED;
    let mut seed2: u32 = 0xEEEE_EEEE;
    for &b in name.as_bytes() {
        let ch = match b {
            b'/' => b'\\',
            other => other.to_ascii_uppercase(),
        } as u32;
        seed1 = TABLE[(kind.wrapping_add(ch)) as usize] ^ seed1.wrapping_add(seed2);
        seed2 = ch
            .wrapping_add(seed1)
            .wrapping_add(seed2)
            .wrapping_add(seed2 << 5)
            .wrapping_add(3);
    }
    seed1
}

fn encrypt_block(data: &mut [u32], mut key: u32) {
    let mut seed: u32 = 0xEEEE_EEEE;
    for v in data.iter_mut() {
        seed = seed.wrapping_add(TABLE[0x400 + (key & 0xFF) as usize]);
        let ch = *v;
        *v = ch ^ key.wrapping_add(seed);
        key = (!key << 0x15).wrapping_add(0x1111_1111) | (key >> 0x0B);
        seed = ch
            .wrapping_add(seed)
            .wrapping_add(seed << 5)
            .wrapping_add(3);
    }
}

/// One file's place in the archive.
struct Block {
    name: String,
    pos: u32,
    compressed_size: u32,
    size: u32,
}

/// Writes an archive file by file; [`Writer::finish`] appends the tables and the header.
pub struct Writer {
    out: BufWriter<File>,
    pos: u64,
    blocks: Vec<Block>,
}

impl Writer {
    pub fn create(path: &Path) -> Result<Self> {
        let mut out = BufWriter::new(File::create(path)?);
        out.write_all(&[0u8; HEADER_SIZE as usize])?; // the header, written last
        Ok(Self {
            out,
            pos: u64::from(HEADER_SIZE),
            blocks: Vec::new(),
        })
    }

    /// Add `bytes` as `name`: a sector offset table, then each sector zlib-compressed, or stored
    /// raw where compression would not shrink it.
    pub fn add(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        let sectors: Vec<&[u8]> = bytes.chunks(SECTOR_SIZE).collect();
        let mut table = Vec::with_capacity(sectors.len() + 1);
        let mut payload = Vec::new();
        let table_len = (sectors.len() + 1) * 4;
        for sector in &sectors {
            table.push((table_len + payload.len()) as u32);
            let mut z = flate2::write::ZlibEncoder::new(
                vec![CODEC_ZLIB],
                flate2::Compression::default(),
            );
            z.write_all(sector)?;
            let packed = z.finish()?;
            if packed.len() < sector.len() {
                payload.extend_from_slice(&packed);
            } else {
                payload.extend_from_slice(sector);
            }
        }
        table.push((table_len + payload.len()) as u32);
        let start = self.pos;
        for t in &table {
            self.out.write_all(&t.to_le_bytes())?;
        }
        self.out.write_all(&payload)?;
        self.pos += (table_len + payload.len()) as u64;
        ensure!(self.pos < u64::from(u32::MAX), "archive past 4 GiB");
        self.blocks.push(Block {
            name: name.to_owned(),
            pos: start as u32,
            compressed_size: (table_len + payload.len()) as u32,
            size: bytes.len() as u32,
        });
        Ok(())
    }

    pub fn finish(mut self) -> Result<u64> {
        let hash_count = (self.blocks.len() * 2).next_power_of_two().max(16);
        let mut hash = vec![0xFFFF_FFFFu32; hash_count * 4];
        for (i, b) in self.blocks.iter().enumerate() {
            let mask = (hash_count - 1) as u32;
            let mut slot = (hash_string(&b.name, HASH_OFFSET) & mask) as usize;
            while hash[slot * 4 + 3] != 0xFFFF_FFFF {
                slot = (slot + 1) % hash_count;
            }
            hash[slot * 4] = hash_string(&b.name, HASH_NAME_A);
            hash[slot * 4 + 1] = hash_string(&b.name, HASH_NAME_B);
            hash[slot * 4 + 2] = 0; // locale neutral, platform 0
            hash[slot * 4 + 3] = i as u32;
        }
        let mut block: Vec<u32> = self
            .blocks
            .iter()
            .flat_map(|b| {
                [
                    b.pos,
                    b.compressed_size,
                    b.size,
                    FLAG_EXISTS | FLAG_COMPRESS,
                ]
            })
            .collect();
        encrypt_block(&mut hash, hash_string("(hash table)", HASH_FILE_KEY));
        encrypt_block(&mut block, hash_string("(block table)", HASH_FILE_KEY));

        let hash_pos = self.pos as u32;
        for w in &hash {
            self.out.write_all(&w.to_le_bytes())?;
        }
        let block_pos = hash_pos + (hash.len() * 4) as u32;
        for w in &block {
            self.out.write_all(&w.to_le_bytes())?;
        }
        let archive_size = u64::from(block_pos) + (block.len() * 4) as u64;
        self.out.seek(SeekFrom::Start(0))?;
        let mut header = Vec::with_capacity(32);
        for w in [SIGNATURE, HEADER_SIZE, archive_size as u32] {
            header.extend_from_slice(&w.to_le_bytes());
        }
        header.extend_from_slice(&0u16.to_le_bytes()); // format version 1
        header.extend_from_slice(&SECTOR_SHIFT.to_le_bytes());
        for w in [
            hash_pos,
            block_pos,
            hash_count as u32,
            self.blocks.len() as u32,
        ] {
            header.extend_from_slice(&w.to_le_bytes());
        }
        self.out.write_all(&header)?;
        self.out.flush()?;
        Ok(archive_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benilla_reads_back_what_the_writer_wrote() {
        let path = std::env::temp_dir().join(format!("survivor-pack-{}.mpq", std::process::id()));
        let big: Vec<u8> = (0..20_000u32).map(|i| (i % 251) as u8).collect();
        let noise: Vec<u8> = (0..9_000u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        let mut w = Writer::create(&path).unwrap();
        w.add("World\\Maps\\Azeroth\\Azeroth_28_28.adt", &big).unwrap();
        w.add("Interface\\Glue.toc", b"## tiny").unwrap();
        w.add("Sound\\noise.wav", &noise).unwrap();
        w.add("empty.txt", b"").unwrap();
        w.finish().unwrap();

        let a = benilla_mpq::Archive::open(&path).unwrap();
        assert_eq!(a.read_file("world/maps/azeroth/AZEROTH_28_28.ADT").unwrap(), big);
        assert_eq!(a.read_file("Interface\\Glue.toc").unwrap(), b"## tiny");
        assert_eq!(a.read_file("Sound\\noise.wav").unwrap(), noise);
        assert_eq!(a.read_file("empty.txt").unwrap(), b"");
        assert!(!a.contains("Interface\\missing.toc"));
        let _ = std::fs::remove_file(&path);
    }
}
