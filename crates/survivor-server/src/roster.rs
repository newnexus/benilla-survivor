//! The account's characters: Undead only, created at the character screen and held in memory for
//! the life of the process (a survivor run starts fresh anyway).

use crate::wire::{split_cstr, Body};

/// `ChrRaces.dbc` id of the Forsaken (`Scourge`).
pub const UNDEAD: u8 = 5;
/// The classes 1.12.1 lets the Forsaken be: warrior, rogue, priest, mage, warlock.
pub const UNDEAD_CLASSES: [u8; 5] = [1, 4, 5, 8, 9];

/// Map 0 (Eastern Kingdoms), zone 85 (Tirisfall Glades): the Forsaken start in Deathknell.
pub(crate) const START_MAP: u32 = 0;
pub(crate) const START_ZONE: u32 = 85;
/// Not in the crypt, as vmangos's `playercreateinfo` has it (1676.71, 1678.31, 121.67), but in
/// the safe village of Deathknell, where the quest givers stand ([`crate::town`]).
const START_POS: [f32; 3] = crate::town::TOWNS[0].arrive;
const START_FACING: f32 = crate::town::TOWNS[0].arrive_facing;

/// Where a character enters the world: Deathknell's village, or `$SURVIVOR_START=x,y,z[,facing]`
/// for an unattended run that must stand somewhere in particular.
pub(crate) fn start() -> ([f32; 3], f32) {
    let parsed = std::env::var("SURVIVOR_START").ok().and_then(|v| {
        let c: Vec<f32> = v.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        (c.len() >= 3).then(|| ([c[0], c[1], c[2]], c.get(3).copied().unwrap_or(START_FACING)))
    });
    parsed.unwrap_or((START_POS, START_FACING))
}

/// `SMSG_CHAR_CREATE` results (`ResponseCodes`, 1.12.1).
pub(crate) const CREATE_SUCCESS: u8 = 0x2E;
pub(crate) const CREATE_NAME_IN_USE: u8 = 0x31;
/// `CHAR_CREATE_DISABLED`: "Creation of that race/class is disabled."
pub(crate) const CREATE_DISABLED: u8 = 0x32;

#[derive(Clone, Debug)]
pub(crate) struct Character {
    pub(crate) guid: u64,
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
}

#[derive(Default)]
pub(crate) struct Roster {
    pub(crate) characters: Vec<Character>,
}

impl Roster {
    /// The roster at startup: empty, or with `$SURVIVOR_SEED=<name>` one Undead of that name
    /// (a mage, or `$SURVIVOR_SEED_CLASS`), for unattended runs that skip the create screen.
    pub(crate) fn from_env() -> Self {
        let mut r = Roster::default();
        if let Ok(name) = std::env::var("SURVIVOR_SEED") {
            let class = std::env::var("SURVIVOR_SEED_CLASS")
                .ok()
                .and_then(|c| c.parse().ok())
                .unwrap_or(8u8);
            let mut b = Body::new();
            b.cstr(&name);
            for v in [UNDEAD, class, 0, 1, 2, 3, 4, 0, 0] {
                b.u8(v);
            }
            r.create(&b.0);
        }
        r
    }

    /// Apply a `CMSG_CHAR_CREATE` body, answering its `SMSG_CHAR_CREATE` result: anything but an
    /// Undead of an Undead class is refused.
    pub(crate) fn create(&mut self, body: &[u8]) -> u8 {
        let (name, b) = split_cstr(body);
        let Some(&[race, class, gender, skin, face, hair_style, hair_color, facial_hair]) =
            b.get(..8).map(|s| <&[u8; 8]>::try_from(s).expect("eight bytes"))
        else {
            return CREATE_DISABLED;
        };
        if race != UNDEAD || !UNDEAD_CLASSES.contains(&class) {
            return CREATE_DISABLED;
        }
        if self
            .characters
            .iter()
            .any(|c| c.name.eq_ignore_ascii_case(&name))
        {
            return CREATE_NAME_IN_USE;
        }
        let guid = self.characters.iter().map(|c| c.guid).max().unwrap_or(0) + 1;
        self.characters.push(Character {
            guid,
            name,
            race,
            class,
            gender,
            skin,
            face,
            hair_style,
            hair_color,
            facial_hair,
            level: 1,
        });
        CREATE_SUCCESS
    }

    pub(crate) fn delete(&mut self, guid: u64) {
        self.characters.retain(|c| c.guid != guid);
    }

    pub(crate) fn get(&self, guid: u64) -> Option<&Character> {
        self.characters.iter().find(|c| c.guid == guid)
    }

    /// The `SMSG_CHAR_ENUM` body, in the order the client parses it (benilla-protocol
    /// `Character::read`).
    pub(crate) fn enum_body(&self) -> Vec<u8> {
        let mut b = Body::new();
        b.u8(self.characters.len() as u8);
        for c in &self.characters {
            b.u64(c.guid).cstr(&c.name);
            for v in [
                c.race,
                c.class,
                c.gender,
                c.skin,
                c.face,
                c.hair_style,
                c.hair_color,
                c.facial_hair,
                c.level,
            ] {
                b.u8(v);
            }
            b.u32(START_ZONE).u32(START_MAP);
            for v in start().0 {
                b.f32(v);
            }
            b.u32(0) // guild
                .u32(0) // character flags
                .u8(0) // first login
                .u32(0) // pet display
                .u32(0) // pet level
                .u32(0); // pet family
            for _ in 0..19 {
                b.u32(0).u8(0); // equipment: display id, inventory type
            }
            b.u32(0).u8(0); // first bag
        }
        b.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_body(name: &str, race: u8, class: u8) -> Vec<u8> {
        let mut b = Body::new();
        b.cstr(name);
        for v in [race, class, 0, 1, 2, 3, 4, 5, 0] {
            b.u8(v);
        }
        b.0
    }

    #[test]
    fn only_undead_of_an_undead_class_is_created() {
        let mut r = Roster::default();
        assert_eq!(r.create(&create_body("Human", 1, 1)), CREATE_DISABLED);
        assert_eq!(r.create(&create_body("Druidy", UNDEAD, 11)), CREATE_DISABLED);
        assert_eq!(r.create(&create_body("Bones", UNDEAD, 8)), CREATE_SUCCESS);
        assert_eq!(r.create(&create_body("bones", UNDEAD, 1)), CREATE_NAME_IN_USE);
        assert_eq!(r.characters.len(), 1);
    }

    #[test]
    fn the_roster_parses_as_benilla_reads_it() {
        let mut r = Roster::default();
        r.create(&create_body("Bones", UNDEAD, 8));
        let packet = benilla_protocol::messages::parse_server(
            benilla_protocol::messages::opcode::SMSG_CHAR_ENUM,
            &r.enum_body(),
        )
        .expect("parses");
        let benilla_protocol::messages::ServerPacket::CharEnum { characters } = packet else {
            panic!("a roster");
        };
        assert_eq!(characters.len(), 1);
        let c = &characters[0];
        assert_eq!((c.name.as_str(), c.race, c.class), ("Bones", UNDEAD, 8));
        assert_eq!((c.map, c.zone), (START_MAP, START_ZONE));
    }
}
