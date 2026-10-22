//! The ground, read from the Eastern Kingdoms' map tiles in the install as asked: its height
//! (`MCVT`) and how much of it is painted road (`MCLY` + `MCAL`). The server never knows either.

use std::collections::HashMap;

/// A map tile's edge and a terrain chunk's, in yards.
const TILE: f32 = 533.333_3;
const CHUNK: f32 = TILE / 16.0;
/// The roads' ground texture in Tirisfall.
const ROAD: &str = "tirisfallstoneroad";

pub(crate) struct Ground {
    chain: Option<benilla_formats::Chain>,
    tiles: HashMap<(i32, i32), Option<Tile>>,
}

struct Tile {
    chunks: Vec<benilla_adt::McnkChunk>,
    /// Which of the tile's textures are road.
    road: Vec<bool>,
    /// Each chunk's decoded alpha map, made when first asked.
    alpha: HashMap<usize, benilla_adt::CombinedAlphaMap>,
}

impl Ground {
    pub(crate) fn open() -> Self {
        let chain = benilla_formats::wow_data()
            .and_then(|d| benilla_formats::Chain::open(&d).ok());
        Self {
            chain,
            tiles: HashMap::new(),
        }
    }

    /// The chunk under WoW `(x, y)`, by tile and index.
    fn chunk(&mut self, x: f32, y: f32) -> Option<(&mut Tile, usize)> {
        let (col, row) = ((32.0 - y / TILE).floor() as i32, (32.0 - x / TILE).floor() as i32);
        let chain = self.chain.as_mut()?;
        let tile = self
            .tiles
            .entry((col, row))
            .or_insert_with(|| {
                let bytes = chain
                    .read(&format!("World\\Maps\\Azeroth\\Azeroth_{col}_{row}.adt"))
                    .ok()?;
                let benilla_adt::ParsedAdt::Root(adt) =
                    benilla_adt::parse_adt(&mut std::io::Cursor::new(&bytes[..])).ok()?;
                let road = adt
                    .textures
                    .iter()
                    .map(|t| t.to_ascii_lowercase().contains(ROAD))
                    .collect();
                Some(Tile {
                    chunks: adt.mcnk_chunks,
                    road,
                    alpha: HashMap::new(),
                })
            })
            .as_mut()?;
        // A chunk's `position` is its corner at the largest x and y.
        let i = tile.chunks.iter().position(|c| {
            let p = c.header.position;
            x <= p[0] && x > p[0] - CHUNK && y <= p[1] && y > p[1] - CHUNK
        })?;
        Some((tile, i))
    }

    /// The ground at WoW `(x, y)`, on the terrain's own triangles: each cell of the outer grid
    /// is four, meeting at its inner vertex.
    pub(crate) fn height(&mut self, x: f32, y: f32) -> Option<f32> {
        let (tile, i) = self.chunk(x, y)?;
        let c = &tile.chunks[i];
        let heights = &c.heights.as_ref()?.heights;
        let unit = CHUNK / 8.0;
        let p = c.header.position;
        let (fi, fj) = ((p[0] - x) / unit, (p[1] - y) / unit);
        let (i, j) = ((fi.floor() as usize).min(7), (fj.floor() as usize).min(7));
        let (u, v) = (fi - i as f32, fj - j as f32);
        // Outer vertex (i, j) is row i of 17 (nine outer, eight inner), column j; the inner
        // vertex of cell (i, j) follows its row's nine outer ones.
        let outer = |di: usize, dj: usize| heights.get((i + di) * 17 + j + dj).copied();
        let mid = heights.get(i * 17 + 9 + j).copied()?;
        // The triangle `(u, v)` is in: toward the nearest edge of the cell.
        let (a, b, pa, pb) = if u.min(1.0 - u) < v.min(1.0 - v) {
            if u < 0.5 {
                (outer(0, 0)?, outer(0, 1)?, [0.0, 0.0], [0.0, 1.0])
            } else {
                (outer(1, 0)?, outer(1, 1)?, [1.0, 0.0], [1.0, 1.0])
            }
        } else if v < 0.5 {
            (outer(0, 0)?, outer(1, 0)?, [0.0, 0.0], [1.0, 0.0])
        } else {
            (outer(0, 1)?, outer(1, 1)?, [0.0, 1.0], [1.0, 1.0])
        };
        let [wa, wb, wm] = barycentric([u, v], pa, pb, [0.5, 0.5]);
        Some(p[2] + a * wa + b * wb + mid * wm)
    }

    /// How much of the ground at WoW `(x, y)` shows road, 0 to 1: the road layers' opacity
    /// after every layer above them is blended over.
    pub(crate) fn road(&mut self, x: f32, y: f32) -> f32 {
        let Some((tile, i)) = self.chunk(x, y) else {
            return 0.0;
        };
        let c = &tile.chunks[i];
        let Some(layers) = c.layers.as_ref().map(|l| &l.layers) else {
            return 0.0;
        };
        let p = c.header.position;
        let alpha = tile
            .alpha
            .entry(i)
            .or_insert_with(|| benilla_adt::CombinedAlphaMap::new(c, false, true));
        let (ai, aj) = (
            (((p[0] - x) / CHUNK * 64.0) as usize).min(63),
            (((p[1] - y) / CHUNK * 64.0) as usize).min(63),
        );
        let px = &alpha.as_slice()[(ai * 64 + aj) * 4..][..3];
        let mut shown = 0.0;
        for (k, layer) in layers.iter().enumerate() {
            let a = if k == 0 { 1.0 } else { f32::from(px[k - 1]) / 255.0 };
            let is_road = tile.road.get(layer.texture_id as usize).copied().unwrap_or(false);
            shown = shown * (1.0 - a) + if is_road { a } else { 0.0 };
        }
        shown
    }
}

/// The weights of `a`, `b` and `c` that make `p`.
fn barycentric(p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> [f32; 3] {
    let d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    let wa = ((b[1] - c[1]) * (p[0] - c[0]) + (c[0] - b[0]) * (p[1] - c[1])) / d;
    let wb = ((c[1] - a[1]) * (p[0] - c[0]) + (a[0] - c[0]) * (p[1] - c[1])) / d;
    [wa, wb, 1.0 - wa - wb]
}
