//! The world server: the session handshake, the character screen, and the world entry that seats
//! the player in Deathknell with their class's starting gear, then hands the world to [`Game`].

use std::net::TcpListener;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use benilla_protocol::messages::opcode;
use benilla_srp::vanilla_header::HeaderCrypto;

use crate::data::WorldData;
use crate::game::{base_health, class_power, Game, PackItem};
use crate::packets::{self, f, Fields, Update};
use crate::realmd::Key;
use crate::roster::{start, Character, Roster, START_MAP};
use crate::wire::{Body, Link, Sender};

/// `AUTH_OK`.
const AUTH_OK: u8 = 0x0C;
/// `SMSG_CHAR_DELETE` success.
const CHAR_DELETE_SUCCESS: u8 = 0x39;
/// The game loop's period.
const TICK: Duration = Duration::from_millis(100);

/// The client's movement opcodes, `MSG_MOVE_START_FORWARD` through `MSG_MOVE_SET_PITCH`, plus
/// the heartbeat: each body is a `MovementInfo` with the position at byte 8.
fn is_move(op: u16) -> bool {
    // The teleport's ack sits in the range but carries no position at byte 8.
    ((0xB5..=0xDB).contains(&op) && op != opcode::MSG_MOVE_TELEPORT_ACK) || op == 0xEE
}

/// `u64 guid, u32 quest` at the front of a questgiver message.
fn guid_quest(body: &[u8]) -> Option<(u64, u32)> {
    let guid = u64::from_le_bytes(body.get(..8)?.try_into().ok()?);
    let quest = u32::from_le_bytes(body.get(8..12)?.try_into().ok()?);
    Some((guid, quest))
}

pub(crate) fn serve(listener: TcpListener, keys: Receiver<Key>) {
    let roster = Arc::new(Mutex::new(Roster::from_env()));
    let data = Arc::new(WorldData::load());
    let curve = crate::data::XpCurve::for_world(&data);
    eprintln!(
        "survivor-server: xp to next level, levels 1-12: {:?}",
        (1..=12).map(|l| curve.next_level(l)).collect::<Vec<_>>()
    );
    thread::Builder::new()
        .name("survivor-world".into())
        .spawn(move || {
            let mut last: Option<Key> = None;
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let mut link = Link {
                    stream,
                    crypto: None,
                };
                if link
                    .send(opcode::SMSG_AUTH_CHALLENGE, &0xDEAD_BEEFu32.to_le_bytes())
                    .is_err()
                {
                    continue;
                }
                match link.recv() {
                    Ok((op, _)) if op == opcode::CMSG_AUTH_SESSION => {}
                    _ => continue,
                }
                // The account's latest session key: a newer logon's replaces it, and a second
                // dial on the same logon reuses it.
                while let Ok(k) = keys.try_recv() {
                    last = Some(k);
                }
                if last.is_none() {
                    last = keys.recv_timeout(Duration::from_secs(20)).ok();
                }
                let Some(key) = last else { continue };
                // From here every packet goes out through the shared sender, whose cipher has
                // sent nothing yet; the link keeps its own instance for reading.
                let Ok(write_half) = link.stream.try_clone() else {
                    continue;
                };
                link.crypto = Some(HeaderCrypto::from_session_key(key));
                let out = Sender::new(write_half, HeaderCrypto::from_session_key(key));
                if out.send(opcode::SMSG_AUTH_RESPONSE, &[AUTH_OK]).is_err() {
                    continue;
                }
                // Every tutorial already seen, so no hint pops up. The client keeps the bank only
                // from a packet that lands at character select, ahead of the world entry.
                if out.send(opcode::SMSG_TUTORIAL_FLAGS, &[0xFF; 32]).is_err() {
                    continue;
                }
                let (roster, data) = (Arc::clone(&roster), Arc::clone(&data));
                thread::spawn(move || {
                    let _ = session(link, &out, &roster, &data);
                });
            }
        })
        .expect("spawn survivor-world");
}

/// The game a session is playing, shared with its tick thread.
type Playing = Arc<Mutex<Option<Game>>>;

/// One world session: the character screen until a login, the world until a logout, repeat.
fn session(
    mut link: Link,
    out: &Sender,
    roster: &Mutex<Roster>,
    data: &Arc<WorldData>,
) -> std::io::Result<()> {
    let playing: Playing = Arc::new(Mutex::new(None));
    {
        let playing = Arc::clone(&playing);
        thread::spawn(move || loop {
            thread::sleep(TICK);
            // The session's end drops the other handle: the loop ends with it.
            if Arc::strong_count(&playing) == 1 {
                return;
            }
            if let Some(game) = playing.lock().expect("game").as_mut() {
                game.tick();
            }
        });
    }
    let mut in_world: Option<Character> = None;
    loop {
        let (op, body) = link.recv()?;
        match op {
            opcode::CMSG_CHAR_ENUM => {
                let body = roster.lock().expect("roster").enum_body();
                out.send(opcode::SMSG_CHAR_ENUM, &body)?;
            }
            opcode::CMSG_CHAR_CREATE => {
                let result = roster.lock().expect("roster").create(&body);
                out.send(opcode::SMSG_CHAR_CREATE, &[result])?;
            }
            opcode::CMSG_CHAR_DELETE => {
                if let Some(guid) = body.get(..8) {
                    let guid = u64::from_le_bytes(guid.try_into().expect("eight bytes"));
                    roster.lock().expect("roster").delete(guid);
                }
                out.send(opcode::SMSG_CHAR_DELETE, &[CHAR_DELETE_SUCCESS])?;
            }
            opcode::CMSG_PLAYER_LOGIN => {
                let Some(guid) = body.get(..8) else { continue };
                let guid = u64::from_le_bytes(guid.try_into().expect("eight bytes"));
                let Some(c) = roster.lock().expect("roster").get(guid).cloned() else {
                    // `CHAR_LOGIN_NO_CHARACTER`.
                    out.send(opcode::SMSG_CHARACTER_LOGIN_FAILED, &[0x47])?;
                    continue;
                };
                let game = enter_world(out, &c, data)?;
                *playing.lock().expect("game") = Some(game);
                in_world = Some(c);
            }
            op if is_move(op) => {
                if body.len() >= 24 {
                    let rd = |o: usize| f32::from_le_bytes(body[o..o + 4].try_into().expect("4"));
                    if let Some(g) = playing.lock().expect("game").as_mut() {
                        g.player_moved([rd(8), rd(12), rd(16)], rd(20));
                    }
                }
            }
            opcode::CMSG_SET_SELECTION | opcode::CMSG_ATTACKSWING => {
                if let Some(g8) = body.get(..8) {
                    let guid = u64::from_le_bytes(g8.try_into().expect("eight bytes"));
                    if let Some(g) = playing.lock().expect("game").as_mut() {
                        g.player_selected(guid);
                    }
                }
            }
            opcode::CMSG_CAST_SPELL => {
                // `u32 spell, u16 target mask`, then the unit's packed guid under mask 0x2.
                let Some(id) = body.get(..4) else { continue };
                let spell = u32::from_le_bytes(id.try_into().expect("four bytes"));
                let mask = body.get(4..6).map_or(0, |m| u16::from_le_bytes([m[0], m[1]]));
                let target = (mask & 0x2 != 0)
                    .then(|| read_packed_guid(&body[6..]))
                    .flatten();
                if let Some(g) = playing.lock().expect("game").as_mut() {
                    g.cast(spell, target);
                }
            }
            opcode::CMSG_USE_ITEM => {
                // `u8 bag, u8 slot, u8 spell index`, then the targets.
                let (Some(&bag), Some(&slot)) = (body.first(), body.get(1)) else { continue };                if let Some(g) = playing.lock().expect("game").as_mut() {
                    g.use_item(bag, slot);
                }
            }
            opcode::CMSG_CANCEL_CAST => {
                if let Some(g) = playing.lock().expect("game").as_mut() {
                    g.cancel_cast();
                }
            }
            opcode::CMSG_MESSAGECHAT => {
                // The message is the last string of the body, whatever the chat type.
                let text = body
                    .get(8..)
                    .unwrap_or_default()
                    .split(|&b| b == 0)
                    .rfind(|s| !s.is_empty())
                    .map(|s| String::from_utf8_lossy(s).trim().to_ascii_lowercase());
                if text.as_deref() == Some("autocast") {
                    if let Some(g) = playing.lock().expect("game").as_mut() {
                        let on = g.toggle_autocast();
                        let mut b = Body::new();
                        b.cstr(if on { "Auto-cast ON" } else { "Auto-cast OFF" });
                        out.send(opcode::SMSG_NOTIFICATION, &b.0)?;
                    }
                }
            }
            opcode::CMSG_QUESTGIVER_STATUS_QUERY
            | opcode::CMSG_QUESTGIVER_HELLO
            | opcode::CMSG_QUESTGIVER_QUERY_QUEST
            | opcode::CMSG_QUESTGIVER_ACCEPT_QUEST
            | opcode::CMSG_QUESTGIVER_COMPLETE_QUEST
            | opcode::CMSG_QUESTGIVER_REQUEST_REWARD
            | opcode::CMSG_QUESTGIVER_CHOOSE_REWARD
            | opcode::CMSG_QUEST_QUERY
            | opcode::CMSG_QUESTLOG_REMOVE_QUEST => {
                if let Some(g) = playing.lock().expect("game").as_mut() {
                    quest_message(g, op, &body);
                }
            }
            opcode::CMSG_GAMEOBJECT_QUERY => {
                let Some(e) = body.get(..4) else { continue };
                let entry = u32::from_le_bytes(e.try_into().expect("four bytes"));
                if let Some(g) = playing.lock().expect("game").as_mut() {
                    g.gameobject_query(entry);
                }
            }
            opcode::CMSG_CREATURE_QUERY => {
                let Some(e) = body.get(..4) else { continue };
                let entry = u32::from_le_bytes(e.try_into().expect("four bytes"));
                let reply = packets::creature_query_response(entry, data.creatures.get(&entry));
                out.send(opcode::SMSG_CREATURE_QUERY_RESPONSE, &reply)?;
            }
            opcode::CMSG_ITEM_QUERY_SINGLE => {
                let Some(e) = body.get(..4) else { continue };
                let entry = u32::from_le_bytes(e.try_into().expect("four bytes"));
                let reply = packets::item_query_response(entry, data.items.get(&entry));
                out.send(opcode::SMSG_ITEM_QUERY_SINGLE_RESPONSE, &reply)?;
            }
            opcode::CMSG_NAME_QUERY => {
                let Some(asked) = body.get(..8) else { continue };
                let asked = u64::from_le_bytes(asked.try_into().expect("eight bytes"));
                if let Some(c) = in_world.as_ref().filter(|c| c.guid == asked) {
                    out.send(opcode::SMSG_NAME_QUERY_RESPONSE, &name_response(c))?;
                }
            }
            opcode::CMSG_PING => {
                // `u32 sequence, u32 latency`: the pong echoes the sequence.
                if let Some(seq) = body.get(..4) {
                    out.send(opcode::SMSG_PONG, seq)?;
                }
            }
            opcode::CMSG_QUERY_TIME => {
                out.send(opcode::SMSG_QUERY_TIME_RESPONSE, &unix_now().to_le_bytes())?;
            }
            opcode::CMSG_LOGOUT_REQUEST => {
                // Result 0 with the instant flag set, then the logout itself.
                let mut b = Body::new();
                b.u32(0).u8(1);
                out.send(opcode::SMSG_LOGOUT_RESPONSE, &b.0)?;
                out.send(opcode::SMSG_LOGOUT_COMPLETE, &[])?;
                *playing.lock().expect("game") = None;
                in_world = None;
            }
            _ => {}
        }
    }
}

/// The quest frames' messages, to the game's quest givers.
fn quest_message(g: &mut Game, op: u16, body: &[u8]) {
    let guid = body.get(..8).map(|b| u64::from_le_bytes(b.try_into().expect("eight bytes")));
    match op {
        opcode::CMSG_QUESTGIVER_STATUS_QUERY => {
            if let Some(guid) = guid {
                g.quest_status_query(guid);
            }
        }
        opcode::CMSG_QUESTGIVER_HELLO => {
            if let Some(guid) = guid {
                g.quest_hello(guid);
            }
        }
        opcode::CMSG_QUEST_QUERY => {
            if let Some(id) = body.get(..4) {
                g.quest_query(u32::from_le_bytes(id.try_into().expect("four bytes")));
            }
        }
        opcode::CMSG_QUESTLOG_REMOVE_QUEST => {
            if let Some(&slot) = body.first() {
                g.quest_abandon(slot);
            }
        }
        _ => {
            let Some((guid, id)) = guid_quest(body) else { return };
            match op {
                opcode::CMSG_QUESTGIVER_QUERY_QUEST => g.quest_query_quest(guid, id),
                opcode::CMSG_QUESTGIVER_ACCEPT_QUEST => g.quest_accept(guid, id),
                opcode::CMSG_QUESTGIVER_CHOOSE_REWARD => g.quest_turn_in(guid, id),
                _ => g.quest_reward_panel(guid, id),
            }
        }
    }
}

/// A packed guid at the front of `b`.
fn read_packed_guid(b: &[u8]) -> Option<u64> {
    let (&mask, mut rest) = b.split_first()?;
    let mut guid = 0u64;
    for i in 0..8 {
        if mask & (1 << i) != 0 {
            let (&byte, r) = rest.split_first()?;
            guid |= u64::from(byte) << (8 * i);
            rest = r;
        }
    }
    Some(guid)
}

/// The class's starting action bar as `SMSG_ACTION_BUTTONS` words. A warrior's sits on the
/// Battle Stance bar (72-83) in 1.12; with no stances here it moves to the main bar.
fn start_buttons(data: &WorldData, class: u8) -> [u32; 120] {
    let mut buttons = [0u32; 120];
    for &(c, button, action, kind) in &data.start_actions {
        if c != class {
            continue;
        }
        let slot = if (72..84).contains(&button) {
            button - 72
        } else {
            button
        };
        if let Some(w) = buttons.get_mut(usize::from(slot)) {
            *w = (action & 0x00FF_FFFF) | u32::from(kind) << 24;
        }
    }
    buttons
}

fn unix_now() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as u32)
}

/// The equipment slot an `InventoryType` goes in, or `None` for the backpack.
pub(crate) fn equip_slot(inventory_type: u32) -> Option<u16> {
    Some(match inventory_type {
        1 => 0,       // head
        2 => 1,       // neck
        3 => 2,       // shoulders
        4 => 3,       // shirt
        5 | 20 => 4,  // chest, robe
        6 => 5,       // waist
        7 => 6,       // legs
        8 => 7,       // feet
        9 => 8,       // wrists
        10 => 9,      // hands
        11 => 10,     // finger
        12 => 12,     // trinket
        16 => 14,     // back
        13 | 17 | 21 => 15, // one-hand, two-hand, main hand
        14 | 22 | 23 => 16, // shield, off hand, held
        15 | 25 | 26 => 17, // ranged, thrown
        19 => 18,     // tabard
        _ => return None,
    })
}

/// The world entry, in vmangos's `HandlePlayerLogin` order for the packets the client needs,
/// then the game that runs the world.
fn enter_world(out: &Sender, c: &Character, data: &Arc<WorldData>) -> std::io::Result<Game> {
    let (pos, facing) = start();
    let mut b = Body::new();
    b.u32(START_MAP);
    for v in pos {
        b.f32(v);
    }
    b.f32(facing);
    out.send(opcode::SMSG_LOGIN_VERIFY_WORLD, &b.0)?;

    // Two in the afternoon, game time at the stock rate.
    let mut b = Body::new();
    b.u32(game_time(14, 0)).f32(1.0 / 60.0);
    out.send(opcode::SMSG_LOGIN_SETTIMESPEED, &b.0)?;

    // The class's starting gear: equipment in its slot, the rest in the backpack.
    let mut item_creates = Vec::new();
    let mut slots: Vec<(u16, u64, u32)> = Vec::new(); // (field, item guid, entry)
    let mut weapon = (1.0, 3.0, Duration::from_millis(2000));
    let mut pack = 0u16;
    // Every item with its field and stack, for the game's item use.
    let mut carried = Vec::new();
    for (n, &(_, entry, amount)) in data
        .start_items
        .iter()
        .filter(|(class, _, _)| *class == c.class)
        .enumerate()
    {
        let Some(t) = data.items.get(&entry) else { continue };
        let guid = 0x4000_0000_0000_0000 | (u64::from(c.guid) << 16) | (n as u64 + 1);
        item_creates.push(packets::item_create(guid, c.guid, t, amount));
        match equip_slot(t.inventory_type) {
            Some(slot) if !slots.iter().any(|s| s.0 == f::PLAYER_INV_SLOT_HEAD + slot * 2) => {
                slots.push((f::PLAYER_INV_SLOT_HEAD + slot * 2, guid, entry));
                if slot == 15 {
                    weapon = (
                        t.damage.0.max(1.0),
                        t.damage.1.max(2.0),
                        Duration::from_millis(u64::from(t.delay.max(1000))),
                    );
                }
            }
            _ => {
                slots.push((f::PLAYER_PACK_SLOT_1 + pack * 2, guid, 0));
                pack += 1;
            }
        }
        let field = slots.last().expect("pushed above").0;
        carried.push(PackItem {
            guid,
            entry,
            count: amount.max(1),
            field,
        });
    }

    let buttons = start_buttons(data, c.class);
    let game = Game::new(Arc::clone(data), out.clone(), c, weapon, buttons, carried, pos, facing);
    out.send(
        opcode::SMSG_INITIAL_SPELLS,
        &packets::initial_spells(&game.player.spells),
    )?;
    out.send(
        opcode::SMSG_ACTION_BUTTONS,
        &packets::action_buttons(game.buttons()),
    )?;
    out.send(
        opcode::SMSG_UPDATE_OBJECT,
        &packets::update_object(&[self_create(
            c,
            &slots,
            weapon,
            game.first_level_xp(),
            (pos, facing),
        )]),
    )?;
    if !item_creates.is_empty() {
        out.send(
            opcode::SMSG_UPDATE_OBJECT,
            &packets::update_object(&item_creates),
        )?;
    }
    out.send(opcode::SMSG_NAME_QUERY_RESPONSE, &name_response(c))?;
    Ok(game)
}

/// A packed `WowTime`: minute, hour, weekday, month day, month and year since 2000 in bit fields.
fn game_time(hour: u32, minute: u32) -> u32 {
    let (weekday, month_day, month, year) = (2, 20, 9, 26);
    minute | hour << 6 | weekday << 11 | month_day << 14 | month << 20 | year << 24
}

fn name_response(c: &Character) -> Vec<u8> {
    let mut b = Body::new();
    b.u64(c.guid)
        .cstr(&c.name)
        .cstr("")
        .u32(c.race.into())
        .u32(c.gender.into())
        .u32(c.class.into());
    b.0
}

/// `ChrRaces.dbc` Scourge (row 5) male and female display ids.
const UNDEAD_MALE_DISPLAY: u32 = 57;
const UNDEAD_FEMALE_DISPLAY: u32 = 58;
/// `FactionTemplate.dbc` 5: Undercity, the Forsaken player faction.
const UNDEAD_FACTION_TEMPLATE: u32 = 5;
/// `TYPEMASK_OBJECT | TYPEMASK_UNIT | TYPEMASK_PLAYER`.
const PLAYER_TYPE_MASK: u32 = 0x19;
/// `UNIT_FLAG_PLAYER_CONTROLLED`, as every player carries.
const UNIT_FLAG_PLAYER_CONTROLLED: u32 = 0x8;

/// Our own player's create at the start position, wearing `slots`' items.
fn self_create(
    c: &Character,
    slots: &[(u16, u64, u32)],
    weapon: (f32, f32, Duration),
    next_level_xp: u32,
    (pos, facing): ([f32; 3], f32),
) -> Update {
    let health = base_health(c.class);
    let (power_type, power) = class_power(c.class);
    let display = if c.gender == 0 {
        UNDEAD_MALE_DISPLAY
    } else {
        UNDEAD_FEMALE_DISPLAY
    };
    let mut fields = Fields::default();
    fields
        .guid(f::GUID, c.guid)
        .set(f::TYPE, PLAYER_TYPE_MASK)
        .setf(f::SCALE_X, 1.0)
        .set(f::HEALTH, health)
        .set(f::MAXHEALTH, health)
        .set(f::LEVEL, c.level.into())
        .set(f::FACTIONTEMPLATE, UNDEAD_FACTION_TEMPLATE)
        .set(
            f::BYTES_0,
            u32::from(c.race)
                | u32::from(c.class) << 8
                | u32::from(c.gender) << 16
                | u32::from(power_type) << 24,
        )
        .set(f::FLAGS, UNIT_FLAG_PLAYER_CONTROLLED)
        .set(f::BASEATTACKTIME, weapon.2.as_millis() as u32)
        .set(f::BASEATTACKTIME + 1, 2000)
        .setf(f::BOUNDINGRADIUS, 0.383)
        .setf(f::COMBATREACH, 1.5)
        .set(f::DISPLAYID, display)
        .set(f::NATIVEDISPLAYID, display)
        .setf(f::MINDAMAGE, weapon.0)
        .setf(f::MAXDAMAGE, weapon.1)
        .setf(f::MOD_CAST_SPEED, 1.0)
        .set(f::BASE_HEALTH, health)
        .set(
            f::PLAYER_BYTES,
            u32::from(c.skin)
                | u32::from(c.face) << 8
                | u32::from(c.hair_style) << 16
                | u32::from(c.hair_color) << 24,
        )
        // Facial hair, and rest state 2 (normal, not rested) in the top byte.
        .set(f::PLAYER_BYTES_2, u32::from(c.facial_hair) | 0x02 << 24)
        .set(f::PLAYER_BYTES_3, c.gender.into())
        .set(f::PLAYER_XP, 0)
        // The right-hand vertical bar (toggle 0x4), where food, drink and the racial sit.
        .set(f::PLAYER_FIELD_BYTES, 0x04 << 16)
        .set(f::PLAYER_NEXT_LEVEL_XP, next_level_xp)
        .set(f::PLAYER_WATCHED_FACTION_INDEX, u32::MAX);
    let pt = u16::from(power_type);
    // Rage starts empty; energy and mana start full.
    fields
        .set(f::POWER1 + pt, if power_type == 1 { 0 } else { power })
        .set(f::MAXPOWER1 + pt, power);
    for stat in 0..5 {
        fields.set(f::STAT0 + stat, 20);
    }
    for school in 0..7 {
        fields.setf(f::PLAYER_MOD_DAMAGE_DONE_PCT + school, 1.0);
    }
    for &(field, guid, entry) in slots {
        fields.guid(field, guid);
        // An equipment slot also shows on the model: its visible-item entry.
        if (f::PLAYER_INV_SLOT_HEAD..f::PLAYER_INV_SLOT_HEAD + 38).contains(&field) {
            let slot = (field - f::PLAYER_INV_SLOT_HEAD) / 2;
            fields.set(
                f::PLAYER_VISIBLE_ITEM_ENTRY + slot * f::PLAYER_VISIBLE_ITEM_STRIDE,
                entry,
            );
        }
    }
    Update::Living {
        create2: true,
        guid: c.guid,
        type_id: packets::TYPEID_PLAYER,
        self_flag: true,
        pos,
        facing,
        speeds: packets::STOCK_SPEEDS,
        fields,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use benilla_protocol::messages::{parse_server, ServerPacket};

    fn bones() -> Character {
        Character {
            guid: 7,
            name: "Bones".into(),
            race: crate::UNDEAD,
            class: 1,
            gender: 1,
            skin: 2,
            face: 3,
            hair_style: 4,
            hair_color: 5,
            facial_hair: 0,
            level: 1,
        }
    }

    #[test]
    fn the_self_create_with_gear_parses_as_benilla_reads_it() {
        let slots = [
            (f::PLAYER_INV_SLOT_HEAD + 15 * 2, 0x4000_0000_0007_0001, 25),
            (f::PLAYER_PACK_SLOT_1, 0x4000_0000_0007_0002, 0),
        ];
        let body = packets::update_object(&[self_create(
            &bones(),
            &slots,
            (2.0, 5.0, Duration::from_millis(1900)),
            400,
            start(),
        )]);
        let packet = parse_server(opcode::SMSG_UPDATE_OBJECT, &body).expect("parses");
        let ServerPacket::UpdateObject { objects, .. } = packet else {
            panic!("an update object");
        };
        let benilla_protocol::messages::Object::Create {
            guid,
            movement,
            mask,
            ..
        } = &objects[0]
        else {
            panic!("a create");
        };
        assert_eq!(*guid, 7);
        let (pos, _) = movement.position.expect("a position");
        assert_eq!([pos.x, pos.y, pos.z], start().0);
        let get = |index: u16| mask.raw_fields().find(|&(i, _)| i == index).map(|(_, v)| v);
        assert_eq!(get(f::DISPLAYID), Some(UNDEAD_FEMALE_DISPLAY));
        assert_eq!(get(f::BYTES_0).map(|v| v & 0xFF), Some(5));
        assert_eq!(get(f::PLAYER_VISIBLE_ITEM_ENTRY + 15 * 12), Some(25));
    }

    #[test]
    fn every_class_slot_is_distinct_for_weapons_and_armour() {
        assert_eq!(equip_slot(21), Some(15));
        assert_eq!(equip_slot(14), Some(16));
        assert_eq!(equip_slot(20), Some(4));
        assert_eq!(equip_slot(0), None);
    }
}
