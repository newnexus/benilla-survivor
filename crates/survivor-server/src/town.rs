//! The safe towns: an outline around each village of Tirisfall. Inside one the player is at their
//! meta level, quest givers talk to them and no creature follows; stepping out through the
//! barrier starts a survivor run at level 1, and stepping back in ends it.
//!
//! Public so the client draws the barrier where the server keeps it.

/// One town's barrier: an outline on the map, world yards.
pub struct Town {
    pub name: &'static str,
    pub centre: [f32; 2],
    /// How far from the centre the town's people are gathered (`survivor-data`); the barrier
    /// itself is the outline.
    pub radius: f32,
    /// The barrier, round the town in order: the hull of the town's people and its guards'
    /// beats, 14 yards out.
    pub outline: &'static [[f32; 2]],
    /// About the ground height at the centre, for the barrier's foot.
    pub ground: f32,
    /// Where a run that ends in death stands the player up again, and which way they face. A
    /// little above the ground: the client settles the player onto it.
    pub arrive: [f32; 3],
    pub arrive_facing: f32,
    /// The collection chest: where it stands and which way it faces.
    pub armory: [f32; 4],
    /// Where the roads cross the barrier, as angles from the centre (`atan2(dy, dx)`, WoW's
    /// facing convention): four guards stand just outside each.
    pub entrances: &'static [f32],
}

/// Deathknell's village (the church, the trainers and Executor Arren) and Brill. The circles
/// take in every friendly NPC of the village and none of the hostile spawns.
pub const TOWNS: &[Town] = &[
    Town {
        name: "Deathknell",
        centre: [1852.0, 1600.0],
        radius: 55.0,
        outline: &[
            [1782.2, 1601.4], [1787.5, 1589.5], [1808.1, 1561.5], [1822.8, 1553.0],
            [1857.1, 1543.4], [1867.8, 1543.4], [1875.4, 1551.0], [1895.9, 1585.4],
            [1896.8, 1622.2], [1892.7, 1632.1], [1853.1, 1651.6], [1831.8, 1657.2],
            [1821.1, 1657.2], [1786.3, 1611.3],
        ],
        ground: 96.0,
        // The road between the church and Executor Arren, facing the church.
        arrive: [1844.5, 1606.0, 99.0],
        arrive_facing: 1.6,
        // The road from the crypt (where Deathguard Oliver's patrol turns), and the road out to
        // the rest of Tirisfall past Deathguard Phillip's.
        entrances: &[3.11, -0.33],
        // Beside Harold Raims, the vendor, clear of the quest givers.
        armory: [1838.6, 1571.4, 96.6, 0.8],
    },
    Town {
        name: "Brill",
        centre: [2268.0, 300.0],
        radius: 80.0,
        outline: &[
            [2215.1, 316.8], [2225.4, 257.1], [2233.2, 234.5], [2239.2, 226.9],
            [2251.5, 220.3], [2292.4, 219.1], [2302.3, 223.2], [2329.1, 282.5],
            [2329.1, 293.3], [2275.1, 357.5], [2265.2, 361.6], [2253.7, 360.0],
            [2219.2, 326.7],
        ],
        ground: 36.0,
        // In front of the Gallows' End Tavern, by Executor Zygand.
        arrive: [2272.0, 300.0, 38.0],
        arrive_facing: 3.1,
        entrances: &[],
        // In the Gallows' End Tavern, beside Innkeeper Renee.
        armory: [2267.4, 243.0, 34.34, 0.6],
    },
];

/// Whether the player stands in a town now, for the client's screens.
static IN_TOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn in_town() -> bool {
    IN_TOWN.load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn set_in_town(on: bool) {
    IN_TOWN.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// When the run about to start begins, while the player stands outside a barrier counting down.
static COUNTDOWN: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// Seconds until the run starts, while counting down.
pub fn countdown() -> Option<f32> {
    let at = (*COUNTDOWN.lock().ok()?)?;
    Some(at.saturating_duration_since(std::time::Instant::now()).as_secs_f32())
}

pub(crate) fn set_countdown(at: Option<std::time::Instant>) {
    if let Ok(mut c) = COUNTDOWN.lock() {
        *c = at;
    }
}

/// The town nearest `pos`, by index.
pub fn nearest(pos: [f32; 3]) -> usize {
    (0..TOWNS.len())
        .min_by(|&a, &b| {
            let d = |t: &Town| (pos[0] - t.centre[0]).hypot(pos[1] - t.centre[1]);
            d(&TOWNS[a]).total_cmp(&d(&TOWNS[b]))
        })
        .unwrap_or(0)
}

/// The town `pos` stands in, by index: inside its outline.
pub fn town_at(pos: [f32; 3]) -> Option<usize> {
    TOWNS.iter().position(|t| inside(t.outline, pos[0], pos[1]))
}

/// The town whose people `pos` is among (within its radius), by index: wider than the barrier,
/// for gathering them.
pub fn near_town(pos: [f32; 3]) -> Option<usize> {
    TOWNS
        .iter()
        .position(|t| (pos[0] - t.centre[0]).hypot(pos[1] - t.centre[1]) <= t.radius)
}

/// Whether `(x, y)` is inside the polygon `poly` (an even-odd ray cast).
fn inside(poly: &[[f32; 2]], x: f32, y: f32) -> bool {
    let mut c = false;
    for i in 0..poly.len() {
        let ([x1, y1], [x2, y2]) = (poly[i], poly[(i + 1) % poly.len()]);
        if (y1 > y) != (y2 > y) && x < (x2 - x1) * (y - y1) / (y2 - y1) + x1 {
            c = !c;
        }
    }
    c
}

/// How far from `t`'s centre its barrier stands along the bearing `a` (`atan2(dy, dx)`).
pub fn edge(t: &Town, a: f32) -> f32 {
    let (dx, dy) = (a.cos(), a.sin());
    let [cx, cy] = t.centre;
    let mut best = t.radius;
    let n = t.outline.len();
    for i in 0..n {
        let ([x1, y1], [x2, y2]) = (t.outline[i], t.outline[(i + 1) % n]);
        let (ex, ey) = (x2 - x1, y2 - y1);
        let den = dx * ey - dy * ex;
        if den.abs() < 1e-6 {
            continue;
        }
        // centre + d*(dx,dy) = p1 + s*e
        let (wx, wy) = (x1 - cx, y1 - cy);
        let d = (wx * ey - wy * ex) / den;
        let s = (wx * dy - wy * dx) / den;
        if d > 0.0 && (0.0..=1.0).contains(&s) {
            best = d;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_villages_hold_their_quest_givers_and_not_the_crypt() {
        // Shadow Priest Sarvis, Executor Arren, Executor Zygand.
        assert_eq!(town_at([1843.0, 1640.0, 98.0]), Some(0));
        assert_eq!(town_at([1849.0, 1580.0, 95.0]), Some(0));
        assert_eq!(town_at([2278.0, 296.0, 37.0]), Some(1));
        // Deathknell's crypt door.
        assert_eq!(town_at([1679.0, 1668.0, 136.0]), None);
    }
}
