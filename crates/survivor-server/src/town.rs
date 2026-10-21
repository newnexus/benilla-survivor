//! The safe towns: a circle around each village of Tirisfall. Inside one the player is at their
//! meta level, quest givers talk to them and no creature follows; stepping out through the
//! barrier starts a survivor run at level 1, and stepping back in ends it.
//!
//! Public so the client draws the barrier where the server keeps it.

/// One town's barrier: a circle on the map, world yards.
pub struct Town {
    pub name: &'static str,
    pub centre: [f32; 2],
    pub radius: f32,
    /// About the ground height at the centre, for the barrier's foot.
    pub ground: f32,
    /// Where a run that ends in death stands the player up again, and which way they face. A
    /// little above the ground: the client settles the player onto it.
    pub arrive: [f32; 3],
    pub arrive_facing: f32,
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
        ground: 96.0,
        // The road between the church and Executor Arren, facing the church.
        arrive: [1846.0, 1606.0, 99.0],
        arrive_facing: 1.6,
        // The road from the crypt (where Deathguard Oliver's patrol turns), and the road out to
        // the rest of Tirisfall past Deathguard Phillip's.
        entrances: &[3.11, -0.33],
    },
    Town {
        name: "Brill",
        centre: [2268.0, 300.0],
        radius: 80.0,
        ground: 36.0,
        // In front of the Gallows' End Tavern, by Executor Zygand.
        arrive: [2272.0, 300.0, 38.0],
        arrive_facing: 3.1,
        entrances: &[],
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

/// The town nearest `pos`, by index.
pub fn nearest(pos: [f32; 3]) -> usize {
    (0..TOWNS.len())
        .min_by(|&a, &b| {
            let d = |t: &Town| (pos[0] - t.centre[0]).hypot(pos[1] - t.centre[1]);
            d(&TOWNS[a]).total_cmp(&d(&TOWNS[b]))
        })
        .unwrap_or(0)
}

/// The town `pos` stands in, by index.
pub fn town_at(pos: [f32; 3]) -> Option<usize> {
    TOWNS.iter().position(|t| {
        (pos[0] - t.centre[0]).hypot(pos[1] - t.centre[1]) <= t.radius
    })
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
