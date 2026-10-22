//! The spirit healer in town and what it sells for souls: rerolls and banishes of the level-up
//! cards (charges a run), and the runes that appear on a run (how often, and which kinds).
//!
//! Souls are the soul orbs a run picks up, one each; walking back into town alive brings them
//! all home, dying loses them. Standing by the healer opens its window ([`crate::spirit`]).

use super::Game;
use crate::spirit::{Row, Upgrade};

/// How close the player stands to open the window, and how far to walk off to close it.
const RANGE: f32 = 4.0;
const RESET: f32 = 7.0;
/// The spirit healer's creature entry.
const SPIRIT_HEALER: u32 = 6491;
/// Souls for each level of a reroll, a banish and the rune rate; a rune kind costs one price.
const REROLL_COSTS: [u32; 3] = [30, 60, 120];
const BANISH_COSTS: [u32; 3] = [30, 60, 120];
const RUNE_RATE_COSTS: [u32; 3] = [40, 80, 160];
const RUNE_COST: u32 = 50;
const GCD_COSTS: [u32; 3] = [40, 80, 160];
/// Each global cooldown level takes this off its 1.5 s.
const GCD_STEP_MS: u64 = 150;
/// The rune kinds, by number: their names, what they do and their icons.
pub(crate) const RUNE_NAMES: [&str; 4] = ["Rune of Power", "Rune of Haste", "Rune of Focus", "Rune of Mending"];
const RUNE_TEXTS: [&str; 4] = [
    "Double damage for 20 s",
    "Run 50% faster for 20 s",
    "Cooldowns 50% shorter for 20 s",
    "Heals all your health over 20 s",
];
const RUNE_ICONS: [&str; 4] = [
    r"Interface\Icons\Spell_Fire_SelfDestruct",
    r"Interface\Icons\Ability_Rogue_Sprint",
    r"Interface\Icons\Spell_Nature_StarFall",
    r"Interface\Icons\Spell_Holy_Heal",
];
/// Seconds between runes on a run, by rune rate level.
pub(crate) const RUNE_EVERY: [u64; 4] = [75, 55, 40, 30];

/// What the player has bought from the healer.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Healer {
    pub(crate) rerolls: u32,
    pub(crate) banishes: u32,
    pub(crate) rune_rate: u32,
    pub(crate) gcd: u32,
    /// Each rune kind, bought.
    pub(crate) runes: [bool; 4],
}

impl Healer {
    fn level(&self, u: Upgrade) -> u32 {
        match u {
            Upgrade::Reroll => self.rerolls,
            Upgrade::Banish => self.banishes,
            Upgrade::RuneRate => self.rune_rate,
            Upgrade::Gcd => self.gcd,
            Upgrade::Rune(k) => u32::from(self.runes[usize::from(k)]),
        }
    }

    fn costs(u: Upgrade) -> &'static [u32] {
        match u {
            Upgrade::Reroll => &REROLL_COSTS,
            Upgrade::Banish => &BANISH_COSTS,
            Upgrade::RuneRate => &RUNE_RATE_COSTS,
            Upgrade::Gcd => &GCD_COSTS,
            Upgrade::Rune(_) => &[RUNE_COST],
        }
    }

    /// The souls for `u`'s next level, if there is one.
    fn cost(&self, u: Upgrade) -> Option<u32> {
        Self::costs(u).get(self.level(u) as usize).copied()
    }

    /// The souls `u`'s last level cost, given back when it is taken off.
    fn refund(&self, u: Upgrade) -> Option<u32> {
        let level = self.level(u) as usize;
        (level > 0).then(|| Self::costs(u).get(level - 1).copied()).flatten()
    }

    fn lower(&mut self, u: Upgrade) {
        match u {
            Upgrade::Reroll => self.rerolls = self.rerolls.saturating_sub(1),
            Upgrade::Banish => self.banishes = self.banishes.saturating_sub(1),
            Upgrade::RuneRate => self.rune_rate = self.rune_rate.saturating_sub(1),
            Upgrade::Gcd => self.gcd = self.gcd.saturating_sub(1),
            Upgrade::Rune(k) => self.runes[usize::from(k)] = false,
        }
    }

    fn raise(&mut self, u: Upgrade) {
        match u {
            Upgrade::Reroll => self.rerolls += 1,
            Upgrade::Banish => self.banishes += 1,
            Upgrade::RuneRate => self.rune_rate += 1,
            Upgrade::Gcd => self.gcd += 1,
            Upgrade::Rune(k) => self.runes[usize::from(k)] = true,
        }
    }

    /// For the save file: `rerolls banishes rune_rate runes gcd` (runes a bit each).
    pub(crate) fn to_line(self) -> String {
        let runes = self.runes.iter().enumerate().fold(0u32, |m, (k, &on)| m | u32::from(on) << k);
        format!("{} {} {} {runes} {}", self.rerolls, self.banishes, self.rune_rate, self.gcd)
    }

    pub(crate) fn from_line(w: &mut std::str::SplitWhitespace<'_>) -> Self {
        let mut num = || w.next().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
        let (rerolls, banishes, rune_rate, runes, gcd) = (num(), num(), num(), num(), num());
        Self {
            rerolls: rerolls.min(REROLL_COSTS.len() as u32),
            banishes: banishes.min(BANISH_COSTS.len() as u32),
            rune_rate: rune_rate.min(RUNE_RATE_COSTS.len() as u32),
            gcd: gcd.min(GCD_COSTS.len() as u32),
            runes: std::array::from_fn(|k| runes & 1 << k != 0),
        }
    }
}

impl Game {
    /// The global cooldown, shortened by the healer.
    pub(super) fn gcd(&self) -> std::time::Duration {
        super::GCD - std::time::Duration::from_millis(GCD_STEP_MS * u64::from(self.healer.gcd))
    }

    /// The spirit healers standing in the towns.
    fn healers(&self, now: std::time::Instant) -> Vec<[f32; 3]> {
        self.npcs
            .iter()
            .filter(|n| n.entry == SPIRIT_HEALER)
            .map(|n| n.here(now))
            .collect()
    }

    /// In town by a spirit healer, its window opens; walking off closes it.
    pub(super) fn open_spirit_nearby(&mut self, now: std::time::Instant) {
        let me = self.player.pos;
        let healers = self.healers(now);
        let far = healers.iter().all(|&h| super::dist2d(h, me) > RESET);
        if self.spirit_open.is_some() {
            if far || self.in_run() {
                self.spirit_open = None;
                crate::spirit::clear();
            }
            return;
        }
        if self.spirit_closed {
            self.spirit_closed = !far;
            return;
        }
        if self.in_run()
            || self.paused()
            || self.player.dead_until.is_some()
            || !self.walked
            || self.armory_open.is_some()
        {
            return;
        }
        if healers.iter().any(|&h| super::dist2d(h, me) <= RANGE) {
            self.close_quest_panel();
            self.post_spirit();
        }
    }

    fn post_spirit(&mut self) {
        let h = self.healer;
        let row = |u: Upgrade, name: &str, text: String, icon: &'static str| Row {
            upgrade: u,
            name: name.to_owned(),
            text,
            level: h.level(u),
            max: Healer::costs(u).len() as u32,
            cost: h.cost(u),
            refund: h.refund(u),
            icon,
        };
        let mut rows = vec![
            row(
                Upgrade::Reroll,
                "Reroll",
                format!("Deal the level-up cards again: {} a run", h.rerolls + 1),
                r"Interface\Icons\Spell_Nature_Reincarnation",
            ),
            row(
                Upgrade::Banish,
                "Banish",
                format!("Strike a skill from a run's cards: {} a run", h.banishes + 1),
                r"Interface\Icons\Spell_Shadow_Teleport",
            ),
            row(
                Upgrade::RuneRate,
                "Rune Call",
                format!(
                    "A rune every {} s on a run",
                    RUNE_EVERY[(h.rune_rate as usize + 1).min(RUNE_EVERY.len() - 1)]
                ),
                r"Interface\Icons\INV_Misc_Rune_06",
            ),
            row(
                Upgrade::Gcd,
                "Quickening",
                format!(
                    "Global cooldown {:.2} s",
                    (super::GCD.as_millis() as u64
                        - GCD_STEP_MS * u64::from((h.gcd + 1).min(GCD_COSTS.len() as u32)))
                        as f32
                        / 1000.0
                ),
                r"Interface\Icons\Spell_Nature_Slow",
            ),
        ];
        for k in 0..4u8 {
            rows.push(row(
                Upgrade::Rune(k),
                RUNE_NAMES[usize::from(k)],
                RUNE_TEXTS[usize::from(k)].to_owned(),
                RUNE_ICONS[usize::from(k)],
            ));
        }
        self.spirit_open = Some(crate::spirit::post(self.souls, rows));
    }

    /// The window's answer: buy an upgrade when the souls are there, or close.
    pub(super) fn take_spirit_action(&mut self) {
        let Some((id, action)) = crate::spirit::take_action() else { return };
        if self.spirit_open != Some(id) {
            return;
        }
        match action {
            crate::spirit::Action::Close => {
                self.spirit_open = None;
                crate::spirit::clear();
                self.spirit_closed = true;
            }
            crate::spirit::Action::Buy(u) => {
                let Some(cost) = self.healer.cost(u) else { return };
                if self.souls < cost {
                    return;
                }
                self.souls -= cost;
                self.healer.raise(u);
                crate::spirit::souls::set(self.souls);
                eprintln!("survivor: bought {u:?} for {cost} souls, {} left", self.souls);
                self.save();
                self.post_spirit();
            }
            crate::spirit::Action::ResetTalents => self.reset_talents(),
            // Free to undo: the level's souls come back in full.
            crate::spirit::Action::Refund(u) => {
                let Some(back) = self.healer.refund(u) else { return };
                self.healer.lower(u);
                self.souls += back;
                crate::spirit::souls::set(self.souls);
                eprintln!("survivor: refunded {u:?} for {back} souls, {} now", self.souls);
                self.save();
                self.post_spirit();
            }
        }
    }

    /// A soul orb picked up on a run.
    pub(super) fn soul_found(&mut self) {
        // Counted quietly: the purse shows the banked souls, in town.
        if self.in_run() {
            self.run_souls += 1;
        }
    }

    /// The run is over: alive, its souls come home; dead, they are lost.
    pub(super) fn end_souls(&mut self, died: bool) {
        if !died {
            self.souls += self.run_souls;
        }
        self.run_souls = 0;
        crate::spirit::souls::set(self.souls);
    }
}
