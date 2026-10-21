//! The player's aura row: food and drink in the first two slots ([`super::meal`]), then the buffs
//! the player's own spells put up (Arcane Intellect, Frost Armor, Power Word: Fortitude). A buff
//! shows its icon and timer, and a recast of the same spell, or of another rank of it, starts it
//! over in its slot.

use std::time::{Duration, Instant};

use benilla_protocol::messages::opcode;

use super::Game;
use crate::packets::Fields;

/// `SPELL_EFFECT_APPLY_AURA`.
const EFFECT_APPLY_AURA: u32 = 6;
/// `SPELL_AURA_PERIODIC_DAMAGE`: a DoT on the target, never a buff.
const AURA_PERIODIC_DAMAGE: u32 = 3;
/// The slots spell buffs may take; 0 and 1 are food and drink, and 16 up are debuffs.
const BUFF_SLOTS: std::ops::Range<u16> = 2..16;
/// `AFLAG_CANCELABLE | AFLAG_EFF_INDEX_0`: a live, positive aura.
const AURA_FLAGS: u32 = 0x03;
/// Unit aura fields (vmangos `UpdateFields_1_12_1.h`): 48 spell ids, then 4 flag bits and 8
/// level bits per slot.
const UNIT_FIELD_AURA: u16 = 47;
const UNIT_FIELD_AURAFLAGS: u16 = 95;
const UNIT_FIELD_AURALEVELS: u16 = 101;

/// A buff from one of the player's spells.
pub(crate) struct Buff {
    pub(crate) spell: u32,
    slot: u16,
    ends: Instant,
}

impl Game {
    /// Whether `spell` puts a buff on the caster: an applied aura that is not a DoT, lasting long
    /// enough to show.
    pub(super) fn is_buff(&self, spell: u32) -> bool {
        self.spell(spell).is_some_and(|s| {
            s.duration_ms >= 10_000
                && (0..3).any(|e| {
                    s.effects[e] == EFFECT_APPLY_AURA && s.auras[e] != AURA_PERIODIC_DAMAGE
                })
        })
    }

    /// The buff of `spell`'s rank chain, if one is up.
    pub(super) fn buff_up(&self, spell: u32) -> bool {
        let chain = self.chain(spell).0;
        self.player.buffs.iter().any(|b| self.chain(b.spell).0 == chain)
    }

    /// Put up `spell`'s buff: in the slot of the same chain if one is up, else the first free one.
    pub(super) fn apply_buff(&mut self, spell: u32, now: Instant) {
        let Some(s) = self.spell(spell) else { return };
        let ends = now + Duration::from_millis(u64::from(s.duration_ms));
        let chain = self.chain(spell).0;
        if let Some(i) = self.player.buffs.iter().position(|b| self.chain(b.spell).0 == chain) {
            let b = &mut self.player.buffs[i];
            b.spell = spell;
            b.ends = ends;
        } else {
            let Some(slot) = BUFF_SLOTS.clone().find(|s| self.player.buffs.iter().all(|b| b.slot != *s))
            else {
                return;
            };
            self.player.buffs.push(Buff { spell, slot, ends });
        }
        self.send_auras(now);
    }

    /// Buffs past their time come off.
    pub(super) fn tick_buffs(&mut self, now: Instant) {
        let before = self.player.buffs.len();
        self.player.buffs.retain(|b| now < b.ends);
        if self.player.buffs.len() != before {
            self.send_auras(now);
        }
    }

    /// The pause moved the clock: every buff ends later by as much.
    pub(super) fn shift_buffs(&mut self, by: Duration) {
        for b in &mut self.player.buffs {
            b.ends += by;
        }
    }

    /// The whole aura row as it stands, meals and buffs: each slot's spell, flags and level, then
    /// the time left on each.
    pub(super) fn send_auras(&self, now: Instant) {
        let mut row: Vec<(u16, u32, Instant)> = self
            .meals
            .iter()
            .map(|m| (m.slot(), m.spell, m.ends))
            .collect();
        row.extend(self.player.buffs.iter().map(|b| (b.slot, b.spell, b.ends)));
        let mut fl = Fields::default();
        let mut flags = [0u32; 2];
        let mut levels = [0u32; 4];
        for slot in 0..BUFF_SLOTS.end {
            let here = row.iter().find(|r| r.0 == slot);
            fl.set(UNIT_FIELD_AURA + slot, here.map_or(0, |r| r.1));
            if here.is_some() {
                flags[usize::from(slot / 8)] |= AURA_FLAGS << (4 * (slot % 8));
                levels[usize::from(slot / 4)] |= self.player.level.min(255) << (8 * (slot % 4));
            }
        }
        for (i, v) in flags.into_iter().enumerate() {
            fl.set(UNIT_FIELD_AURAFLAGS + i as u16, v);
        }
        for (i, v) in levels.into_iter().enumerate() {
            fl.set(UNIT_FIELD_AURALEVELS + i as u16, v);
        }
        self.values(self.player.guid, fl);
        for (slot, _, ends) in row {
            let left = ends.saturating_duration_since(now).as_millis() as u32;
            let mut b = crate::wire::Body::new();
            b.u8(slot as u8).u32(left);
            self.send(opcode::SMSG_UPDATE_AURA_DURATION, &b.0);
        }
    }
}

