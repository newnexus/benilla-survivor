//! What a run's spawner sends at the player: the zone's waves of the dead, their bosses, and the
//! marked areas that send their own people instead. Public so `survivor-data` exports every
//! kind named here, spawned in the zone or not.

/// Tirisfall's waves, weakest first: the run moves on to the next every one to three minutes.
pub const KINDS: &[u32] = &[
    1501, // Mindless Zombie
    1502, // Wretched Zombie
    1890, // Rattlecage Skeleton
    1525, // Rotting Dead
    1527, // Hungering Dead
    1523, // Cracked Skull Soldier
    1529, // Bleeding Horror
    1530, // Rotting Ancestor
];

/// A boss every four minutes, in this order, then round again stronger: `(entry, model scale)`.
pub const BOSSES: &[(u32, f32)] = &[
    (1554, 4.0),  // Vampiric Duskbat (0.44 a bat), a large bat
    (10464, 2.0), // Wailing Banshee
    (3976, 1.8),  // Scarlet Commander Mograine, the Scarlet Monastery's knight
];

/// A marked area of the map: while the player is in it, the spawner sends its kinds instead of
/// the zone's waves, and stepping in says `entered`.
pub struct Area {
    pub name: &'static str,
    pub centre: [f32; 2],
    pub radius: f32,
    pub kinds: &'static [u32],
    pub entered: &'static str,
}

/// The Scarlet Crusade's camps in Tirisfall, from where their people stand in the world data.
pub const AREAS: &[Area] = &[
    Area {
        name: "Solliden Farmstead",
        centre: [1793.0, 1340.0],
        radius: 130.0,
        kinds: &[1506, 1507], // Scarlet Convert, Scarlet Initiate
        entered: "The Scarlet Crusade has seen you!",
    },
    Area {
        name: "the crusader camp",
        centre: [2400.0, 1560.0],
        radius: 140.0,
        kinds: &[1535], // Scarlet Warrior
        entered: "The Scarlet Crusade has seen you!",
    },
    Area {
        name: "Scarlet Watch Post",
        centre: [1808.0, 710.0],
        radius: 140.0,
        kinds: &[1536, 1535], // Scarlet Missionary, Scarlet Warrior
        entered: "The Scarlet Crusade has seen you!",
    },
    Area {
        name: "the Zealots' camp",
        centre: [2154.0, -540.0],
        radius: 130.0,
        kinds: &[1537, 1538], // Scarlet Zealot, Scarlet Friar
        entered: "The Scarlet Crusade has seen you!",
    },
    Area {
        name: "the road to the Monastery",
        centre: [2800.0, -650.0],
        radius: 200.0,
        kinds: &[1538, 1539, 1540], // Scarlet Friar, Neophyte, Vanguard
        entered: "The Scarlet Crusade guards its Monastery!",
    },
];

/// The marked area `pos` stands in, by index.
pub fn area_at(pos: [f32; 3]) -> Option<usize> {
    AREAS
        .iter()
        .position(|a| (pos[0] - a.centre[0]).hypot(pos[1] - a.centre[1]) <= a.radius)
}

/// Every kind the spawner can send.
pub fn all_kinds() -> impl Iterator<Item = u32> {
    KINDS
        .iter()
        .copied()
        .chain(BOSSES.iter().map(|b| b.0))
        .chain(AREAS.iter().flat_map(|a| a.kinds.iter().copied()))
}
