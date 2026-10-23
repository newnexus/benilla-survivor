//! `cargo run -p survivor-pack --example minimap -- <Data> <out.ppm> <x0> <x1> <y0> <y1>`: the
//! game's own minimap art for the world rectangle x0..x1, y0..y1, stitched into one picture,
//! north up, a pixel every half yard, for planning places on the map.

use anyhow::{Context, Result};
use benilla_formats::Chain;

const TILE: f32 = 533.333_3;

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let data = a.get(1).context("Data dir")?;
    let out = a.get(2).context("out file")?;
    let n = |i: usize| -> Result<f32> { Ok(a.get(i).context("bounds")?.parse()?) };
    let (x0, x1, y0, y1) = (n(3)?, n(4)?, n(5)?, n(6)?);
    let mut chain = Chain::open(std::path::Path::new(data))?;
    let trs = benilla_formats::load_minimap_translate(&mut chain)?;
    // Two pixels a yard: image column runs west to east (y falling), row north to south.
    let (w, h) = (((y1 - y0) * 2.0) as usize, ((x1 - x0) * 2.0) as usize);
    let mut img = vec![0u8; w * h * 3];
    let mut tiles: std::collections::HashMap<(u32, u32), Option<(usize, Vec<u8>)>> =
        std::collections::HashMap::new();
    for row in 0..h {
        let x = x1 - row as f32 / 2.0;
        for col in 0..w {
            let y = y1 - col as f32 / 2.0;
            let (tx, ty) = (32.0 - y / TILE, 32.0 - x / TILE);
            let key = (tx.floor() as u32, ty.floor() as u32);
            let tile = tiles.entry(key).or_insert_with(|| {
                let name = trs.tile("Azeroth", key.0, key.1)?;
                let bytes = chain.read(&format!("textures\\Minimap\\{name}")).ok()?;
                let b = benilla_blp::decode(&bytes).ok()?;
                let m = b.mips.into_iter().next()?;
                Some((m.width as usize, m.rgba))
            });
            let Some((tw, rgba)) = tile.as_ref() else { continue };
            let tw = *tw;
            let px = ((tx.fract() * tw as f32) as usize).min(tw - 1);
            let py = ((ty.fract() * tw as f32) as usize).min(tw - 1);
            let s = (py * tw + px) * 4;
            img[(row * w + col) * 3..][..3].copy_from_slice(&rgba[s..s + 3]);
        }
    }
    let mut f = format!("P6\n{w} {h}\n255\n").into_bytes();
    f.extend(img);
    std::fs::write(out, f)?;
    Ok(())
}
