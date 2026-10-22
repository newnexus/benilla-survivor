//! Talents: one point per camp (meta) level from level 2, spent in the client's own talent
//! window (N, in town), kept in the save, and reset free at the spirit healer. The window and its
//! rules are 1.12's: five points in a tree open its next tier, an arrow's talent first.
//!
//! What a talent does is ours, from Donis's review of the 1.12 trees (Keep / Change / Replace):
//! [`EFFECTS`] lists the ones working so far as a survivor stat each rank adds to.

use benilla_protocol::messages::opcode;

use super::gear::GearStat;
use super::Game;
use crate::packets::{self, f, Fields};

/// Points in a tree that open each next tier.
const POINTS_PER_TIER: u32 = 5;

/// Talents that work, by talent id: the stat each rank adds to, and how much.
const EFFECTS: &[(u32, GearStat, f32)] = &[
    // Rogue
    (270, GearStat::Crit, 0.02),         // Malice: 10% at 5
    (274, GearStat::Damage, 0.06),       // Murder: 12% at 2
    (382, GearStat::EnergyRegen, 0.20),  // Vigor: 20% energy regeneration
    (244, GearStat::Speed, 0.06),        // Camouflage: 30% speed at 5
    (1702, GearStat::Damage, 0.02),      // Deadliness: 10% at 5
    // Warrior
    (157, GearStat::Crit, 0.03),         // Cruelty: 15% at 5
    (136, GearStat::Damage, 0.04),       // Two-Handed Weapon Specialization: 4..20%
    (702, GearStat::Damage, 0.08),       // One-Handed Weapon Specialization: 40% at 5
    (137, GearStat::RageRegen, 0.10),    // Anger Management: 1 rage a second
];

impl Game {
    /// The class's talents, from the world data.
    fn class_talents(&self) -> impl Iterator<Item = &crate::data::TalentRow> + '_ {
        let class = self.player.class;
        self.data.talents.iter().filter(move |t| t.class == class)
    }

    fn talent_row(&self, id: u32) -> Option<&crate::data::TalentRow> {
        self.class_talents().find(|t| t.id == id)
    }

    fn talent_points_spent(&self) -> u32 {
        self.talents.values().sum()
    }

    /// Points to spend: one per camp level past the first.
    fn talent_points_free(&self) -> u32 {
        self.meta.level.saturating_sub(1).saturating_sub(self.talent_points_spent())
    }

    /// What the talents add to `stat`.
    pub(super) fn talent_bonus(&self, stat: GearStat) -> f32 {
        EFFECTS
            .iter()
            .filter(|e| e.1 == stat)
            .map(|&(id, _, per)| per * self.talents.get(&id).copied().unwrap_or(0) as f32)
            .sum()
    }

    /// The talent ranks' spells the client knows, so its window shows each talent's rank.
    fn talent_spells(&self) -> Vec<u32> {
        self.talents
            .iter()
            .filter_map(|(&id, &rank)| {
                let t = self.talent_row(id)?;
                Some(t.ranks.iter().take(rank as usize).copied().collect::<Vec<_>>())
            })
            .flatten()
            .collect()
    }

    /// The talents on the client: their spells and the points left.
    pub(super) fn send_talents(&mut self) {
        for s in self.talent_spells() {
            self.send(opcode::SMSG_LEARNED_SPELL, &packets::learned_spell(s));
        }
        self.send_talent_points();
    }

    pub(super) fn send_talent_points(&mut self) {
        let mut fl = Fields::default();
        fl.set(f::PLAYER_CHARACTER_POINTS1, self.talent_points_free());
        self.values(self.player.guid, fl);
    }

    /// `CMSG_LEARN_TALENT`: the next rank of `id`, when the rules allow it.
    pub(crate) fn learn_talent(&mut self, id: u32, rank: u32) {
        let Some(t) = self.talent_row(id).cloned() else { return };
        let have = self.talents.get(&id).copied().unwrap_or(0);
        let spent_in_tree: u32 = self
            .talents
            .iter()
            .filter(|(i, _)| self.talent_row(**i).is_some_and(|o| o.tab == t.tab))
            .map(|(_, r)| r)
            .sum();
        let prereq_met = t.prereq == 0
            || self.talents.get(&t.prereq).copied().unwrap_or(0) > t.prereq_rank;
        let ok = !self.in_run()
            && rank == have
            && (have as usize) < t.ranks.len()
            && self.talent_points_free() > 0
            && spent_in_tree >= t.row * POINTS_PER_TIER
            && prereq_met;
        if !ok {
            return;
        }
        self.talents.insert(id, have + 1);
        let spell = t.ranks[have as usize];
        eprintln!("survivor: talent {id} rank {}", have + 1);
        self.send(opcode::SMSG_LEARNED_SPELL, &packets::learned_spell(spell));
        self.send_talent_points();
        self.apply_gear();
        self.save();
    }

    /// Every point back: the spirit healer's free reset.
    pub(super) fn reset_talents(&mut self) {
        for s in self.talent_spells() {
            self.send(opcode::SMSG_REMOVED_SPELL, &packets::removed_spell(s));
        }
        self.talents.clear();
        eprintln!("survivor: talents reset");
        self.send_talent_points();
        self.apply_gear();
        self.save();
    }
}
