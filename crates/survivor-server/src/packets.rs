//! Server packet bodies, byte for byte as benilla-protocol parses them.

use crate::data::{ChestTemplate, CreatureTemplate, ItemTemplate};
use crate::wire::Body;

/// Update-field indices (vmangos `UpdateFields_1_12_1.h`).
pub(crate) mod f {
    pub const GUID: u16 = 0;
    pub const TYPE: u16 = 2;
    pub const ENTRY: u16 = 3;
    pub const SCALE_X: u16 = 4;
    pub const ITEM_OWNER: u16 = 6;
    pub const ITEM_CONTAINED: u16 = 8;
    pub const ITEM_STACK_COUNT: u16 = 14;
    pub const ITEM_SPELL_CHARGES: u16 = 16;
    pub const ITEM_DURABILITY: u16 = 46;
    pub const ITEM_MAXDURABILITY: u16 = 47;
    pub const TARGET: u16 = 16;
    pub const HEALTH: u16 = 22;
    pub const POWER1: u16 = 23;
    pub const MAXHEALTH: u16 = 28;
    pub const MAXPOWER1: u16 = 29;
    pub const LEVEL: u16 = 34;
    pub const FACTIONTEMPLATE: u16 = 35;
    pub const BYTES_0: u16 = 36;
    pub const FLAGS: u16 = 46;
    pub const BASEATTACKTIME: u16 = 126;
    pub const BOUNDINGRADIUS: u16 = 129;
    pub const COMBATREACH: u16 = 130;
    pub const DISPLAYID: u16 = 131;
    pub const NATIVEDISPLAYID: u16 = 132;
    pub const MINDAMAGE: u16 = 134;
    pub const MAXDAMAGE: u16 = 135;
    pub const MOD_CAST_SPEED: u16 = 145;
    pub const NPC_FLAGS: u16 = 147;
    pub const GO_DISPLAYID: u16 = 8;
    pub const GO_ROTATION: u16 = 10;
    pub const GO_STATE: u16 = 14;
    pub const GO_POS_X: u16 = 15;
    pub const GO_POS_Y: u16 = 16;
    pub const GO_POS_Z: u16 = 17;
    pub const GO_FACING: u16 = 18;
    pub const GO_TYPE_ID: u16 = 21;
    pub const STAT0: u16 = 150;
    pub const BASE_HEALTH: u16 = 163;
    pub const PLAYER_BYTES: u16 = 193;
    pub const PLAYER_BYTES_2: u16 = 194;
    pub const PLAYER_BYTES_3: u16 = 195;
    pub const PLAYER_VISIBLE_ITEM_ENTRY: u16 = 260;
    pub const PLAYER_VISIBLE_ITEM_STRIDE: u16 = 12;
    pub const PLAYER_INV_SLOT_HEAD: u16 = 486;
    pub const PLAYER_PACK_SLOT_1: u16 = 532;
    /// Twenty slots of `[quest id, counters | state << 24, timer]`.
    pub const PLAYER_QUEST_LOG_1_1: u16 = 198;
    pub const PLAYER_XP: u16 = 716;
    pub const PLAYER_COINAGE: u16 = 1176;
    /// Byte 2: the extra action bars shown (`GetActionBarToggles`).
    pub const PLAYER_FIELD_BYTES: u16 = 1222;
    pub const PLAYER_NEXT_LEVEL_XP: u16 = 717;
    pub const PLAYER_MOD_DAMAGE_DONE_PCT: u16 = 1215;
    pub const PLAYER_WATCHED_FACTION_INDEX: u16 = 1261;
}

/// `UPDATETYPE_*`.
const VALUES: u8 = 0;
const CREATE_OBJECT: u8 = 2;
const CREATE_OBJECT2: u8 = 3;
const OUT_OF_RANGE: u8 = 4;
/// `TYPEID_*`.
const TYPEID_ITEM: u8 = 1;
const TYPEID_UNIT: u8 = 3;
const TYPEID_GAMEOBJECT: u8 = 5;
pub(crate) const TYPEID_PLAYER: u8 = 4;
/// `UPDATEFLAG_*`.
pub(crate) const UPDATEFLAG_SELF: u8 = 0x01;
const UPDATEFLAG_ALL: u8 = 0x10;
const UPDATEFLAG_LIVING: u8 = 0x20;
const UPDATEFLAG_HAS_POSITION: u8 = 0x40;
/// The stock 1.12.1 movement speeds: walk, run, run back, swim, swim back, turn rate.
pub(crate) const STOCK_SPEEDS: [f32; 6] = [2.5, 7.0, 4.5, 4.722_222, 2.5, std::f32::consts::PI];

/// Sparse update fields, written as the block mask and the set values in index order.
#[derive(Default)]
pub(crate) struct Fields(std::collections::BTreeMap<u16, u32>);

impl Fields {
    pub(crate) fn set(&mut self, index: u16, value: u32) -> &mut Self {
        self.0.insert(index, value);
        self
    }
    pub(crate) fn setf(&mut self, index: u16, value: f32) -> &mut Self {
        self.set(index, value.to_bits())
    }
    pub(crate) fn guid(&mut self, index: u16, guid: u64) -> &mut Self {
        self.set(index, guid as u32).set(index + 1, (guid >> 32) as u32)
    }
    fn write(&self, b: &mut Body) {
        let top = self.0.keys().next_back().map_or(0, |&i| usize::from(i));
        let blocks = top / 32 + 1;
        let mut masks = vec![0u32; blocks];
        for &i in self.0.keys() {
            masks[usize::from(i) / 32] |= 1 << (i % 32);
        }
        b.u8(blocks as u8);
        for m in masks {
            b.u32(m);
        }
        for &v in self.0.values() {
            b.u32(v);
        }
    }
}

/// One entry of an `SMSG_UPDATE_OBJECT`.
pub(crate) enum Update {
    /// A unit or player with a `LIVING` movement block.
    Living {
        create2: bool,
        guid: u64,
        type_id: u8,
        self_flag: bool,
        pos: [f32; 3],
        facing: f32,
        speeds: [f32; 6],
        fields: Fields,
    },
    Item {
        guid: u64,
        fields: Fields,
    },
    Values {
        guid: u64,
        fields: Fields,
    },
    /// A game object standing still: a position block, no movement.
    GameObject {
        guid: u64,
        pos: [f32; 3],
        facing: f32,
        fields: Fields,
    },
    OutOfRange(Vec<u64>),
}

/// An `SMSG_UPDATE_OBJECT` body holding `updates`.
pub(crate) fn update_object(updates: &[Update]) -> Vec<u8> {
    let mut b = Body::new();
    b.u32(updates.len() as u32).u8(0);
    for u in updates {
        match u {
            Update::Living {
                create2,
                guid,
                type_id,
                self_flag,
                pos,
                facing,
                speeds,
                fields,
            } => {
                let mut flags = UPDATEFLAG_ALL | UPDATEFLAG_LIVING | UPDATEFLAG_HAS_POSITION;
                if *self_flag {
                    flags |= UPDATEFLAG_SELF;
                }
                b.u8(if *create2 { CREATE_OBJECT2 } else { CREATE_OBJECT })
                    .packed_guid(*guid)
                    .u8(*type_id)
                    .u8(flags)
                    .u32(0) // movement flags
                    .u32(0); // time
                for v in pos {
                    b.f32(*v);
                }
                b.f32(*facing).f32(0.0); // facing, fall time
                for s in speeds {
                    b.f32(*s);
                }
                b.u32(1); // UPDATEFLAG_ALL's value
                fields.write(&mut b);
            }
            Update::Item { guid, fields } => {
                b.u8(CREATE_OBJECT2)
                    .packed_guid(*guid)
                    .u8(TYPEID_ITEM)
                    .u8(UPDATEFLAG_ALL)
                    .u32(1);
                fields.write(&mut b);
            }
            Update::Values { guid, fields } => {
                b.u8(VALUES).packed_guid(*guid);
                fields.write(&mut b);
            }
            Update::GameObject {
                guid,
                pos,
                facing,
                fields,
            } => {
                b.u8(CREATE_OBJECT)
                    .packed_guid(*guid)
                    .u8(TYPEID_GAMEOBJECT)
                    .u8(UPDATEFLAG_ALL | UPDATEFLAG_HAS_POSITION);
                for v in pos {
                    b.f32(*v);
                }
                b.f32(*facing).u32(1);
                fields.write(&mut b);
            }
            Update::OutOfRange(guids) => {
                b.u8(OUT_OF_RANGE).u32(guids.len() as u32);
                for g in guids {
                    b.packed_guid(*g);
                }
            }
        }
    }
    b.0
}

/// `TYPEMASK_OBJECT | TYPEMASK_UNIT`.
const UNIT_TYPE_MASK: u32 = 0x09;
/// `TYPEMASK_OBJECT | TYPEMASK_ITEM`.
const ITEM_TYPE_MASK: u32 = 0x03;

/// A creature's create entry.
pub(crate) fn creature_create(
    guid: u64,
    t: &CreatureTemplate,
    scale: f32,
    level: u32,
    health: u32,
    max_health: u32,
    pos: [f32; 3],
    facing: f32,
) -> Update {
    let mut fields = Fields::default();
    fields
        .guid(f::GUID, guid)
        .set(f::TYPE, UNIT_TYPE_MASK)
        .set(f::ENTRY, t.entry)
        .setf(f::SCALE_X, if scale > 0.0 { scale } else { 1.0 })
        .set(f::HEALTH, health)
        .set(f::MAXHEALTH, max_health)
        .set(f::LEVEL, level)
        .set(f::FACTIONTEMPLATE, t.faction)
        .set(f::BASEATTACKTIME, t.attack_ms.max(1000))
        .setf(f::BOUNDINGRADIUS, 0.5)
        .setf(f::COMBATREACH, 1.5)
        .set(f::DISPLAYID, t.display)
        .set(f::NATIVEDISPLAYID, t.display)
        .setf(f::MINDAMAGE, t.damage.0)
        .setf(f::MAXDAMAGE, t.damage.1)
        .setf(f::MOD_CAST_SPEED, 1.0)
        .set(f::NPC_FLAGS, t.npc_flags);
    let mut speeds = STOCK_SPEEDS;
    speeds[0] *= t.speed.0.max(0.1);
    speeds[1] *= t.speed.1.max(0.1);
    Update::Living {
        create2: false,
        guid,
        type_id: TYPEID_UNIT,
        self_flag: false,
        pos,
        facing,
        speeds,
        fields,
    }
}

/// `TYPEMASK_OBJECT | TYPEMASK_GAMEOBJECT`.
const GAMEOBJECT_TYPE_MASK: u32 = 0x21;
/// `GAMEOBJECT_TYPE_CHEST`.
const GAMEOBJECT_TYPE_CHEST: u32 = 3;

/// A chest's create, closed (`GO_STATE_READY`).
pub(crate) fn gameobject_create(
    guid: u64,
    t: &ChestTemplate,
    pos: [f32; 3],
    facing: f32,
    rotation: [f32; 4],
) -> Update {
    let mut fields = Fields::default();
    fields
        .guid(f::GUID, guid)
        .set(f::TYPE, GAMEOBJECT_TYPE_MASK)
        .set(f::ENTRY, t.entry)
        .setf(f::SCALE_X, if t.size > 0.0 { t.size } else { 1.0 })
        .set(f::GO_DISPLAYID, t.display)
        .set(f::GO_STATE, 1)
        .setf(f::GO_POS_X, pos[0])
        .setf(f::GO_POS_Y, pos[1])
        .setf(f::GO_POS_Z, pos[2])
        .setf(f::GO_FACING, facing)
        .set(f::GO_TYPE_ID, GAMEOBJECT_TYPE_CHEST);
    for (k, r) in rotation.iter().enumerate() {
        fields.setf(f::GO_ROTATION + k as u16, *r);
    }
    Update::GameObject {
        guid,
        pos,
        facing,
        fields,
    }
}

/// `SMSG_GAMEOBJECT_QUERY_RESPONSE`: a chest's name and look, or the "no such object" form.
pub(crate) fn gameobject_query_response(entry: u32, t: Option<&ChestTemplate>) -> Vec<u8> {
    let mut b = Body::new();
    let Some(t) = t else {
        b.u32(entry | 0x8000_0000);
        return b.0;
    };
    b.u32(entry)
        .u32(GAMEOBJECT_TYPE_CHEST)
        .u32(t.display)
        .cstr(&t.name)
        .cstr("")
        .cstr("")
        .cstr("")
        .cstr("");
    for _ in 0..24 {
        b.u32(0);
    }
    b.0
}

/// `SMSG_FORCE_RUN_SPEED_CHANGE`: the player runs at `speed` yards a second.
pub(crate) fn force_run_speed(guid: u64, counter: u32, speed: f32) -> Vec<u8> {
    let mut b = Body::new();
    b.packed_guid(guid).u32(counter).f32(speed);
    b.0
}

/// An item's create entry, owned by and contained in `owner`.
pub(crate) fn item_create(guid: u64, owner: u64, t: &ItemTemplate, count: u32) -> Update {
    let mut fields = Fields::default();
    fields
        .guid(f::GUID, guid)
        .set(f::TYPE, ITEM_TYPE_MASK)
        .set(f::ENTRY, t.entry)
        .setf(f::SCALE_X, 1.0)
        .guid(f::ITEM_OWNER, owner)
        .guid(f::ITEM_CONTAINED, owner)
        .set(f::ITEM_STACK_COUNT, count.max(1))
        .set(f::ITEM_DURABILITY, t.durability)
        .set(f::ITEM_MAXDURABILITY, t.durability);
    // A use spell's charges, -1 for an item used up by its use (food, drink), as the template
    // has them; 0 reads "0 Charges" in the tooltip.
    if t.spell != 0 {
        fields.set(f::ITEM_SPELL_CHARGES, u32::MAX);
    }
    Update::Item { guid, fields }
}

/// `SMSG_CREATURE_QUERY_RESPONSE`.
pub(crate) fn creature_query_response(entry: u32, t: Option<&CreatureTemplate>) -> Vec<u8> {
    let mut b = Body::new();
    let Some(t) = t else {
        b.u32(entry | 0x8000_0000);
        return b.0;
    };
    b.u32(entry).cstr(&t.name).cstr("").cstr("").cstr("").cstr("");
    b.u32(0) // type flags
        .u32(t.kind)
        .u32(0) // pet family
        .u32(t.rank)
        .u32(0) // unknown
        .u32(0) // pet spell list
        .u32(t.display)
        .u8(0) // civilian
        .u8(0); // racial leader
    b.0
}

/// `SMSG_ITEM_QUERY_SINGLE_RESPONSE`, with `description`, the tooltip's quoted gold line at its
/// foot: a survivor item's own stats, or empty.
pub(crate) fn item_query_response_described(
    entry: u32,
    t: Option<&ItemTemplate>,
    description: &str,
) -> Vec<u8> {
    let mut b = Body::new();
    let Some(t) = t else {
        b.u32(entry | 0x8000_0000);
        return b.0;
    };
    b.u32(t.entry)
        .u32(t.class)
        .u32(t.subclass)
        .cstr(&t.name)
        .cstr("")
        .cstr("")
        .cstr("")
        .u32(t.display)
        .u32(t.quality)
        .u32(t.flags)
        .u32(t.buy)
        .u32(t.sell)
        .u32(t.inventory_type)
        .u32(u32::MAX) // allowable class: all
        .u32(u32::MAX) // allowable race: all
        .u32(t.item_level);
    for _ in 0..8 {
        b.u32(0); // required level, skill, skill rank, spell, honor rank, city rank, rep faction, rep rank
    }
    b.u32(t.max_count).u32(t.stackable.max(1)).u32(0); // container slots
    for _ in 0..10 {
        b.u32(0).u32(0); // stats
    }
    b.f32(t.damage.0).f32(t.damage.1).u32(t.damage_type);
    for _ in 1..5 {
        b.f32(0.0).f32(0.0).u32(0);
    }
    b.u32(t.armor);
    for _ in 0..6 {
        b.u32(0); // resistances
    }
    b.u32(t.delay).u32(0).f32(0.0); // delay, ammo type, range mod
    // Spell slots: id, trigger, charges, cooldown, category, category cooldown. The first holds
    // a use spell (food, drink: trigger 0 "on use", charges -1 "the item is used up"), the client
    // offers the item for use only with one.
    for slot in 0..5 {
        if slot == 0 && t.spell != 0 {
            b.u32(t.spell).u32(0).u32(u32::MAX).u32(u32::MAX).u32(0).u32(u32::MAX);
        } else {
            b.u32(0).u32(0).u32(0).u32(u32::MAX).u32(0).u32(u32::MAX);
        }
    }
    b.u32(0) // bonding
        .cstr(description)
        .u32(0) // page text
        .u32(0) // language
        .u32(0) // page material
        .u32(0) // start quest
        .u32(0) // lock
        .u32(t.material as u32)
        .u32(t.sheath)
        .u32(0) // random property
        .u32(0) // block
        .u32(0) // item set
        .u32(t.durability)
        .u32(0) // area
        .u32(0) // map
        .u32(0); // bag family
    b.0
}

/// `SMSG_MONSTER_MOVE` from `start` to `dest` over `duration_ms`, running.
pub(crate) fn monster_move(
    guid: u64,
    start: [f32; 3],
    spline_id: u32,
    dest: [f32; 3],
    duration_ms: u32,
) -> Vec<u8> {
    monster_move_as(guid, start, spline_id, dest, duration_ms, true)
}

/// `SMSG_MONSTER_MOVE` walking: a guard on its beat.
pub(crate) fn monster_walk(
    guid: u64,
    start: [f32; 3],
    spline_id: u32,
    dest: [f32; 3],
    duration_ms: u32,
) -> Vec<u8> {
    monster_move_as(guid, start, spline_id, dest, duration_ms, false)
}

fn monster_move_as(
    guid: u64,
    start: [f32; 3],
    spline_id: u32,
    dest: [f32; 3],
    duration_ms: u32,
    run: bool,
) -> Vec<u8> {
    let mut b = Body::new();
    b.packed_guid(guid);
    for v in start {
        b.f32(v);
    }
    b.u32(spline_id)
        .u8(0) // normal facing
        .u32(if run { 0x100 } else { 0 }) // run mode, else walking
        .u32(duration_ms.max(1))
        .u32(1);
    for v in dest {
        b.f32(v);
    }
    b.0
}

/// `SMSG_MONSTER_MOVE` that stops the unit where it stands.
pub(crate) fn monster_stop(guid: u64, pos: [f32; 3], spline_id: u32) -> Vec<u8> {
    let mut b = Body::new();
    b.packed_guid(guid);
    for v in pos {
        b.f32(v);
    }
    b.u32(spline_id).u8(1);
    b.0
}

pub(crate) fn attack_start(attacker: u64, victim: u64) -> Vec<u8> {
    let mut b = Body::new();
    b.u64(attacker).u64(victim);
    b.0
}

pub(crate) fn attack_stop(attacker: u64, victim: u64) -> Vec<u8> {
    let mut b = Body::new();
    b.packed_guid(attacker).packed_guid(victim).u32(0);
    b.0
}

/// How a swing landed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Swing {
    Hit,
    Crit,
    Miss,
}

/// `SMSG_ATTACKERSTATEUPDATE` for one melee swing.
pub(crate) fn attacker_state(attacker: u64, victim: u64, swing: Swing, damage: u32) -> Vec<u8> {
    let (hit_info, victim_state, damage) = match swing {
        Swing::Hit => (0x2, 1, damage),
        Swing::Crit => (0x82, 1, damage),
        Swing::Miss => (0x10, 0, 0),
    };
    let mut b = Body::new();
    b.u32(hit_info)
        .packed_guid(attacker)
        .packed_guid(victim)
        .u32(damage)
        .u8(1)
        .u32(0) // physical
        .f32(damage as f32)
        .u32(damage)
        .u32(0)
        .u32(0)
        .u32(victim_state)
        .u32(0)
        .u32(0) // no melee spell
        .u32(0); // blocked
    b.0
}

/// `SMSG_LOG_XPGAIN` for a kill.
pub(crate) fn xp_gain(victim: u64, xp: u32) -> Vec<u8> {
    let mut b = Body::new();
    b.u64(victim).u32(xp).u8(0).u32(xp).f32(1.0);
    b.0
}

/// `SMSG_LEVELUP_INFO`.
pub(crate) fn levelup_info(level: u32, health_gain: u32) -> Vec<u8> {
    let mut b = Body::new();
    b.u32(level).u32(health_gain);
    for _ in 0..10 {
        b.u32(0); // power and stat gains
    }
    b.0
}

/// `SMSG_INITIAL_SPELLS`: the spellbook at login, no cooldowns.
pub(crate) fn initial_spells(spells: &[u32]) -> Vec<u8> {
    let mut b = Body::new();
    b.u8(0);
    b.0.extend_from_slice(&(spells.len() as u16).to_le_bytes());
    for &s in spells {
        b.0.extend_from_slice(&(s as u16).to_le_bytes());
        b.0.extend_from_slice(&0u16.to_le_bytes());
    }
    b.0.extend_from_slice(&0u16.to_le_bytes());
    b.0
}

/// `SMSG_ACTION_BUTTONS`: all 120 slots, each `action | kind << 24`.
pub(crate) fn action_buttons(buttons: &[u32; 120]) -> Vec<u8> {
    let mut b = Body::new();
    for &w in buttons {
        b.u32(w);
    }
    b.0
}

/// `SMSG_SPELL_COOLDOWN`: one spell of `caster`'s resting `ms`.
pub(crate) fn spell_cooldown(caster: u64, spell: u32, ms: u32) -> Vec<u8> {
    let mut b = Body::new();
    b.u64(caster).u32(spell).u32(ms);
    b.0
}

/// `SMSG_LEARNED_SPELL`.
pub(crate) fn learned_spell(spell: u32) -> Vec<u8> {
    let mut b = Body::new();
    b.u32(spell);
    b.0
}

/// A SpellCastTargets block naming one unit, or none.
fn cast_targets(b: &mut Body, target: Option<u64>) {
    match target {
        Some(t) => {
            b.0.extend_from_slice(&0x0002u16.to_le_bytes());
            b.packed_guid(t);
        }
        None => b.0.extend_from_slice(&0u16.to_le_bytes()),
    }
}

/// `SMSG_SPELL_START`: the caster in both guid slots, so the client sees no item cast.
pub(crate) fn spell_start(caster: u64, spell: u32, cast_ms: u32, target: Option<u64>) -> Vec<u8> {
    let mut b = Body::new();
    b.packed_guid(caster).packed_guid(caster).u32(spell);
    b.0.extend_from_slice(&0x0002u16.to_le_bytes());
    b.u32(cast_ms);
    cast_targets(&mut b, target);
    b.0
}

/// `SMSG_SPELL_GO` hitting `hits`; a missile flies to each.
pub(crate) fn spell_go(caster: u64, spell: u32, hits: &[u64], target: Option<u64>) -> Vec<u8> {
    let mut b = Body::new();
    b.packed_guid(caster).packed_guid(caster).u32(spell);
    b.0.extend_from_slice(&0x0100u16.to_le_bytes());
    b.u8(hits.len() as u8);
    for &h in hits {
        b.u64(h);
    }
    b.u8(0); // misses
    cast_targets(&mut b, target);
    b.0
}

/// `SMSG_CAST_RESULT`: success, or a failure reason (`SPELL_FAILED_*`).
pub(crate) fn cast_result(spell: u32, failure: Option<u8>) -> Vec<u8> {
    let mut b = Body::new();
    b.u32(spell);
    match failure {
        None => b.u8(0),
        Some(reason) => b.u8(2).u8(reason),
    };
    b.0
}

/// `SMSG_SPELLNONMELEEDAMAGELOG`: a spell's damage number and combat-log line.
pub(crate) fn spell_damage(target: u64, caster: u64, spell: u32, damage: u32, school: u8, crit: bool) -> Vec<u8> {
    let mut b = Body::new();
    b.packed_guid(target)
        .packed_guid(caster)
        .u32(spell)
        .u32(damage)
        .u8(school)
        .u32(0) // absorbed
        .u32(0) // resisted
        .u8(0) // periodic
        .u8(0)
        .u32(0) // blocked
        .u32(if crit { 0x2 } else { 0 })
        .u8(0);
    b.0
}

/// `SMSG_DESTROY_OBJECT`.
pub(crate) fn destroy(guid: u64) -> Vec<u8> {
    guid.to_le_bytes().to_vec()
}

/// `SMSG_SUPERCEDED_SPELL`: `new` replaces `old` in the book, and on the bar with it.
pub(crate) fn superceded_spell(old: u32, new: u32) -> Vec<u8> {
    let mut b = Body::new();
    b.0.extend_from_slice(&(old as u16).to_le_bytes());
    b.0.extend_from_slice(&(new as u16).to_le_bytes());
    b.0
}

/// `SMSG_FORCE_MOVE_ROOT` / `_UNROOT`: the packed guid and a counter the ack echoes.
pub(crate) fn move_root(guid: u64, counter: u32) -> Vec<u8> {
    let mut b = Body::new();
    b.packed_guid(guid).u32(counter);
    b.0
}

/// `MSG_MOVE_TELEPORT_ACK` to our own player: the packed guid, the counter its ack echoes, and a
/// standing `MovementInfo` at the destination.
pub(crate) fn teleport(guid: u64, counter: u32, pos: [f32; 3], facing: f32, time_ms: u32) -> Vec<u8> {
    let mut b = Body::new();
    b.packed_guid(guid).u32(counter).u32(0).u32(time_ms);
    for v in pos {
        b.f32(v);
    }
    b.f32(facing).u32(0); // fall time
    b.0
}

/// `SMSG_REMOVED_SPELL`: the spell id as a `u16`.
pub(crate) fn removed_spell(spell: u32) -> Vec<u8> {
    (spell as u16).to_le_bytes().to_vec()
}

/// `SMSG_NOTIFICATION`: a line of yellow text mid-screen.
pub(crate) fn notification(text: &str) -> Vec<u8> {
    let mut b = Body::new();
    b.cstr(text);
    b.0
}

/// Quest panels and the quest log (vmangos `Quest.cpp`, as benilla-protocol `quest` parses).
pub(crate) mod quest {
    use super::Body;
    use crate::data::Quest;

    /// `DIALOG_STATUS_*`: the mark over a quest giver's head.
    pub const STATUS_NONE: u32 = 0;
    pub const STATUS_INCOMPLETE: u32 = 3;
    pub const STATUS_AVAILABLE: u32 = 5;
    pub const STATUS_REWARD: u32 = 7;

    pub(crate) fn status(npc: u64, status: u32) -> Vec<u8> {
        let mut b = Body::new();
        b.u64(npc).u32(status);
        b.0
    }

    /// The greeting panel: one row per quest, `(quest, icon)`.
    #[allow(dead_code)]
    pub(crate) fn list(npc: u64, greeting: &str, rows: &[(&Quest, u32)]) -> Vec<u8> {
        let mut b = Body::new();
        b.u64(npc).cstr(greeting).u32(0).u32(0).u8(rows.len() as u8);
        for (q, icon) in rows {
            b.u32(q.id).u32(*icon).u32(q.level).cstr(&q.title);
        }
        b.0
    }

    /// The accept panel (the stock one; the survivor shows its own, see `quest_board`).
    #[allow(dead_code)]
    pub(crate) fn details(npc: u64, q: &Quest) -> Vec<u8> {
        let mut b = Body::new();
        b.u64(npc)
            .u32(q.id)
            .cstr(&q.title)
            .cstr(&q.details)
            .cstr(&q.objectives)
            .u32(0) // auto-finish
            .u32(0) // choice items
            .u32(0) // reward items
            .u32(q.money)
            .u32(0) // reward spell
            .u32(4);
        for _ in 0..4 {
            b.u32(0).u32(0); // emote, delay
        }
        b.0
    }

    /// The reward panel of a finished quest (the stock one, as `details`).
    #[allow(dead_code)]
    pub(crate) fn offer_reward(npc: u64, q: &Quest) -> Vec<u8> {
        let mut b = Body::new();
        b.u64(npc)
            .u32(q.id)
            .cstr(&q.title)
            .cstr(&q.offer_text)
            .u32(0) // auto-finish
            .u32(0) // emotes
            .u32(0) // choice items
            .u32(0) // reward items
            .u32(q.money)
            .u32(0) // quest flags
            .u32(0); // reward spell
        b.0
    }

    /// The turn-in: XP and money granted, no items.
    pub(crate) fn complete(q: &Quest, xp: u32) -> Vec<u8> {
        let mut b = Body::new();
        b.u32(q.id).u32(3).u32(xp).u32(q.money).u32(0);
        b.0
    }

    /// `SMSG_QUEST_QUERY_RESPONSE`: the template the quest log shows.
    pub(crate) fn query_response(q: &Quest) -> Vec<u8> {
        let mut b = Body::new();
        b.u32(q.id)
            .u32(2) // method: normal
            .u32(q.level)
            .u32(crate::roster::START_ZONE)
            .u32(0); // type
        for _ in 0..4 {
            b.u32(0); // reputation objective, opposite reputation
        }
        b.u32(0) // next in chain
            .u32(q.money)
            .u32(0) // money at max level
            .u32(0) // reward spell
            .u32(0) // source item
            .u32(0); // flags
        for _ in 0..(4 + 6) {
            b.u32(0).u32(0); // reward and choice items
        }
        b.u32(0).f32(0.0).f32(0.0).u32(0); // point of interest
        b.cstr(&q.title)
            .cstr(&q.objectives)
            .cstr(&q.details)
            .cstr(&q.end_text);
        for &(creature, count) in &q.kills {
            b.u32(creature).u32(count).u32(0).u32(0);
        }
        for t in &q.objective_texts {
            b.cstr(t);
        }
        b.0
    }

    /// `SMSG_QUESTUPDATE_ADD_KILL`: the toast line "Mindless Zombie slain: 3/8".
    pub(crate) fn add_kill(quest: u32, entry: u32, count: u32, required: u32, guid: u64) -> Vec<u8> {
        let mut b = Body::new();
        b.u32(quest).u32(entry).u32(count).u32(required).u64(guid);
        b.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use benilla_protocol::messages::{opcode, parse_server, ServerPacket};

    fn template() -> CreatureTemplate {
        CreatureTemplate {
            entry: 1512,
            name: "Duskbat".into(),
            level: (1, 2),
            faction: 7,
            display: 4732,
            scale: 0.4,
            speed: (1.0, 0.86),
            rank: 0,
            kind: 1,
            attack_ms: 2000,
            health: (42, 55),
            damage: (1.6, 2.3),
            detection: 10.0,
            hostile: true,
            npc_flags: 0,
        }
    }

    /// Every body parses with nothing left over.
    fn whole(op: u16, body: &[u8]) -> ServerPacket {
        let (p, tail) = benilla_protocol::messages::parse_server_with_tail(op, body)
            .unwrap_or_else(|e| panic!("opcode {op:#x}: {e}"));
        assert_eq!(tail, 0, "opcode {op:#x} left {tail} bytes");
        p
    }

    #[test]
    fn creature_and_item_creates_parse() {
        let guid = 0xF130_0000_0000_0000 | (1512 << 24) | 1;
        let body = update_object(&[
            creature_create(guid, &template(), 0.4, 2, 19, 19, [1.0, 2.0, 3.0], 0.5),
            item_create(0x4000_0000_0000_0001, 7, &ItemTemplate::default(), 1),
            Update::OutOfRange(vec![guid]),
        ]);
        let ServerPacket::UpdateObject { objects, .. } = whole(opcode::SMSG_UPDATE_OBJECT, &body)
        else {
            panic!("an update object");
        };
        assert_eq!(objects.len(), 3);
    }

    #[test]
    fn combat_and_progress_packets_parse() {
        let (a, v) = (7u64, 0xF130_0000_0000_0000 | (1512 << 24) | 1);
        whole(opcode::SMSG_MONSTER_MOVE, &monster_move(v, [0.0; 3], 1, [5.0, 5.0, 0.0], 900));
        whole(opcode::SMSG_MONSTER_MOVE, &monster_stop(v, [0.0; 3], 2));
        whole(opcode::SMSG_ATTACKSTART, &attack_start(a, v));
        whole(opcode::SMSG_ATTACKSTOP, &attack_stop(a, v));
        for s in [Swing::Hit, Swing::Crit, Swing::Miss] {
            whole(opcode::SMSG_ATTACKERSTATEUPDATE, &attacker_state(a, v, s, 12));
        }
        whole(opcode::SMSG_LOG_XPGAIN, &xp_gain(v, 55));
        whole(opcode::SMSG_LEVELUP_INFO, &levelup_info(2, 15));
        whole(opcode::SMSG_DESTROY_OBJECT, &destroy(v));
        whole(
            opcode::SMSG_CREATURE_QUERY_RESPONSE,
            &creature_query_response(1512, Some(&template())),
        );
        whole(opcode::SMSG_CREATURE_QUERY_RESPONSE, &creature_query_response(9, None));
        let item = ItemTemplate {
            entry: 25,
            name: "Worn Shortsword".into(),
            ..Default::default()
        };
        whole(
            opcode::SMSG_ITEM_QUERY_SINGLE_RESPONSE,
            &item_query_response_described(25, Some(&item), ""),
        );
        whole(opcode::SMSG_INITIAL_SPELLS, &initial_spells(&[6603, 133, 168]));
        whole(opcode::SMSG_ACTION_BUTTONS, &action_buttons(&[0; 120]));
        whole(opcode::SMSG_LEARNED_SPELL, &learned_spell(143));
        whole(opcode::SMSG_SUPERCEDED_SPELL, &superceded_spell(133, 143));
        whole(opcode::SMSG_FORCE_MOVE_ROOT, &move_root(a, 1));
        whole(opcode::SMSG_FORCE_MOVE_UNROOT, &move_root(a, 2));
        whole(opcode::SMSG_SPELL_START, &spell_start(a, 133, 1500, Some(v)));
        whole(opcode::SMSG_SPELL_GO, &spell_go(a, 133, &[v], Some(v)));
        whole(opcode::SMSG_SPELL_GO, &spell_go(a, 168, &[a], None));
        whole(opcode::SMSG_CAST_RESULT, &cast_result(133, None));
        whole(opcode::SMSG_CAST_RESULT, &cast_result(133, Some(0x4D)));
        whole(opcode::SMSG_SPELLNONMELEEDAMAGELOG, &spell_damage(v, a, 133, 14, 2, false));
        let _ = parse_server; // the plain parser is the same path
    }

    #[test]
    fn quest_and_teleport_packets_parse() {
        use crate::data::Quest;
        let npc = 0xF130_0000_0000_0000 | (1569 << 24) | 1;
        let q = Quest {
            id: 364,
            level: 2,
            title: "The Mindless Ones".into(),
            details: "Go.$B$BNow.".into(),
            kills: [(1501, 8), (1502, 8), (0, 0), (0, 0)],
            ..Default::default()
        };
        whole(opcode::SMSG_QUESTGIVER_STATUS, &quest::status(npc, quest::STATUS_AVAILABLE));
        whole(
            opcode::SMSG_QUESTGIVER_QUEST_LIST,
            &quest::list(npc, "Hello", &[(&q, quest::STATUS_AVAILABLE)]),
        );
        whole(opcode::SMSG_QUESTGIVER_QUEST_DETAILS, &quest::details(npc, &q));
        whole(opcode::SMSG_QUESTGIVER_OFFER_REWARD, &quest::offer_reward(npc, &q));
        whole(opcode::SMSG_QUESTGIVER_QUEST_COMPLETE, &quest::complete(&q, 170));
        whole(opcode::SMSG_QUEST_QUERY_RESPONSE, &quest::query_response(&q));
        whole(opcode::SMSG_QUESTUPDATE_ADD_KILL, &quest::add_kill(364, 1501, 3, 8, npc));
        whole(opcode::SMSG_QUESTUPDATE_COMPLETE, &364u32.to_le_bytes());
        whole(opcode::MSG_MOVE_TELEPORT_ACK, &teleport(7, 1, [1.0, 2.0, 3.0], 0.5, 1000));
        whole(opcode::SMSG_REMOVED_SPELL, &removed_spell(143));
        whole(opcode::SMSG_NOTIFICATION, &notification("Safe in Deathknell"));
        let chest = ChestTemplate {
            entry: 106318,
            name: "Battered Chest".into(),
            display: 259,
            size: 1.0,
        };
        let go = 0xF110_0000_0000_0000 | (106318 << 24) | 3;
        let ServerPacket::UpdateObject { objects, .. } = whole(
            opcode::SMSG_UPDATE_OBJECT,
            &update_object(&[gameobject_create(go, &chest, [1.0, 2.0, 3.0], 0.5, [0.0, 0.0, 0.2, 0.98])]),
        ) else {
            panic!("an update object");
        };
        assert_eq!(objects.len(), 1);
        whole(opcode::SMSG_GAMEOBJECT_QUERY_RESPONSE, &gameobject_query_response(106318, Some(&chest)));
        whole(opcode::SMSG_GAMEOBJECT_QUERY_RESPONSE, &gameobject_query_response(9, None));
        whole(opcode::SMSG_FORCE_RUN_SPEED_CHANGE, &force_run_speed(7, 1, 7.7));
        whole(opcode::SMSG_LOOT_MONEY_NOTIFY, &17u32.to_le_bytes());
    }
}
