//! The way through a town along its roads: a yard grid over the town where painted road is
//! cheap to walk and grass dear, searched outward from the player once for every goal at once
//! (Dijkstra), so trails to several givers share the same road until they part.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use survivor_server::town::Town;

use crate::ground::Ground;

/// The grid's cell, in yards, and how far past the barrier it reaches.
const CELL: f32 = 1.0;
const MARGIN: f32 = 8.0;
/// A yard of grass costs this many yards of road.
const OFF_ROAD: f32 = 3.5;
/// Road this strong counts as fully road: the paint is speckled.
const FULL: f32 = 0.45;

pub(crate) struct Roads {
    /// The grid's corner at the smallest x and y.
    min: [f32; 2],
    w: usize,
    h: usize,
    /// Each cell's cost per yard crossed.
    cost: Vec<f32>,
}

/// Every cell's way back to the start, after a search.
pub(crate) struct Ways {
    from: Vec<u32>,
    start: usize,
}

impl Roads {
    pub(crate) fn of(town: &Town, ground: &mut Ground) -> Self {
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in town.outline {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k] - MARGIN);
                hi[k] = hi[k].max(p[k] + MARGIN);
            }
        }
        let w = ((hi[0] - lo[0]) / CELL).ceil() as usize;
        let h = ((hi[1] - lo[1]) / CELL).ceil() as usize;
        // The road's paint, sampled at each cell's middle, then evened over its neighbours.
        let mut paint = vec![0.0; w * h];
        for j in 0..h {
            for i in 0..w {
                let (x, y) = (lo[0] + (i as f32 + 0.5) * CELL, lo[1] + (j as f32 + 0.5) * CELL);
                paint[j * w + i] = ground.road(x, y);
            }
        }
        let mut cost = vec![OFF_ROAD; w * h];
        for j in 0..h {
            for i in 0..w {
                let (mut sum, mut n) = (0.0, 0.0);
                for (dj, di) in [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 0), (0, 1), (1, -1), (1, 0), (1, 1)] {
                    let (ii, jj) = (i as i32 + di, j as i32 + dj);
                    if ii >= 0 && jj >= 0 && (ii as usize) < w && (jj as usize) < h {
                        sum += paint[jj as usize * w + ii as usize];
                        n += 1.0;
                    }
                }
                let road = (sum / n / FULL).min(1.0);
                cost[j * w + i] = 1.0 + (OFF_ROAD - 1.0) * (1.0 - road);
            }
        }
        Self { min: lo, w, h, cost }
    }

    fn cell(&self, p: [f32; 2]) -> usize {
        let i = (((p[0] - self.min[0]) / CELL) as usize).min(self.w - 1);
        let j = (((p[1] - self.min[1]) / CELL) as usize).min(self.h - 1);
        j * self.w + i
    }

    fn centre(&self, c: usize) -> [f32; 2] {
        [
            self.min[0] + ((c % self.w) as f32 + 0.5) * CELL,
            self.min[1] + ((c / self.w) as f32 + 0.5) * CELL,
        ]
    }

    /// The cheapest way from `start` to every cell.
    pub(crate) fn search(&self, start: [f32; 2]) -> Ways {
        let start = self.cell(start);
        let mut best = vec![f32::INFINITY; self.cost.len()];
        let mut from = vec![u32::MAX; self.cost.len()];
        let mut open = BinaryHeap::new();
        best[start] = 0.0;
        from[start] = start as u32;
        // Costs are whole thousandths of a yard, so the heap orders integers.
        open.push(Reverse((0u32, start)));
        while let Some(Reverse((d, c))) = open.pop() {
            let d = d as f32 / 1000.0;
            if d > best[c] {
                continue;
            }
            let (i, j) = ((c % self.w) as i32, (c / self.w) as i32);
            for (di, dj) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                let (ii, jj) = (i + di, j + dj);
                if ii < 0 || jj < 0 || ii as usize >= self.w || jj as usize >= self.h {
                    continue;
                }
                let n = jj as usize * self.w + ii as usize;
                let step = if di != 0 && dj != 0 { std::f32::consts::SQRT_2 } else { 1.0 };
                let nd = d + step * CELL * 0.5 * (self.cost[c] + self.cost[n]);
                if nd < best[n] {
                    best[n] = nd;
                    from[n] = c as u32;
                    open.push(Reverse(((nd * 1000.0) as u32, n)));
                }
            }
        }
        Ways { from, start }
    }

    /// The way from the search's start to `goal`, as points from `start` to `goal`, smoothed so
    /// it bends instead of zigzagging along the grid.
    pub(crate) fn way(&self, ways: &Ways, start: [f32; 2], goal: [f32; 2]) -> Vec<[f32; 2]> {
        let mut cells = Vec::new();
        let mut c = self.cell(goal);
        while c != ways.start {
            cells.push(c);
            let f = ways.from[c];
            if f == u32::MAX {
                return vec![start, goal];
            }
            c = f as usize;
        }
        cells.reverse();
        let mut points = vec![start];
        // The cells next to either end are left out: the ends themselves stand there.
        if cells.len() > 2 {
            points.extend(cells[..cells.len() - 1].iter().skip(1).map(|&c| self.centre(c)));
        }
        points.push(goal);
        smooth(&points)
    }
}

/// A moving average over a few yards, ends kept where they are.
fn smooth(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    const REACH: usize = 3;
    let n = points.len();
    (0..n)
        .map(|k| {
            let r = REACH.min(k).min(n - 1 - k);
            let span = &points[k - r..=k + r];
            let len = span.len() as f32;
            [
                span.iter().map(|p| p[0]).sum::<f32>() / len,
                span.iter().map(|p| p[1]).sum::<f32>() / len,
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draws each town's road costs and the ways from its arrival point to every friendly NPC
    /// into `SURVIVOR_ROADS_OUT` as grey maps, north up. Needs the install.
    #[test]
    #[ignore = "needs WOW_DATA"]
    fn draw_town_ways() {
        let out = std::path::PathBuf::from(std::env::var("SURVIVOR_ROADS_OUT").unwrap());
        let npcs = std::fs::read_to_string("../survivor-server/data/npcs.tsv").unwrap();
        let mut ground = Ground::open();
        for town in survivor_server::town::TOWNS {
            let roads = Roads::of(town, &mut ground);
            let start = [town.arrive[0], town.arrive[1]];
            let ways = roads.search(start);
            let (w, h) = (roads.w, roads.h);
            // Image column = west to east (-y), row = north to south (-x).
            let mut img: Vec<u8> = vec![0; w * h];
            for c in 0..w * h {
                let (i, j) = (c % w, c / w);
                img[(w - 1 - i) * h + (h - 1 - j)] =
                    (255.0 * (OFF_ROAD - roads.cost[c]) / (OFF_ROAD - 1.0)) as u8 / 2;
            }
            for line in npcs.lines().skip(1) {
                let f: Vec<f32> = line.split('\t').map(|s| s.parse().unwrap_or(0.0)).collect();
                let goal = [f[2], f[3]];
                if survivor_server::town::town_at([f[2], f[3], f[4]]).map(|t| t)
                    != survivor_server::town::town_at(town.arrive)
                {
                    continue;
                }
                for p in roads.way(&ways, start, goal) {
                    let c = roads.cell(p);
                    let (i, j) = (c % w, c / w);
                    img[(w - 1 - i) * h + (h - 1 - j)] = 255;
                }
            }
            let mut f = format!("P5\n{h} {w}\n255\n").into_bytes();
            f.extend(img);
            std::fs::write(out.join(format!("{}-ways.pgm", town.name)), f).unwrap();
        }
    }
}
