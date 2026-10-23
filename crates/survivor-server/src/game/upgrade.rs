//! Upgrade cards: a level-up card can improve a skill already known instead of teaching one. Each
//! comes in a rarity, rolled when the card is dealt:
//!
//! - common: one stat, by its base amount;
//! - uncommon: one stat, 40% more; rare: one stat, 80% more;
//! - epic: one stat as rare, and a second at the base amount;
//! - legendary: the skill changes: it strikes every enemy round its target too, and harder.
//!
//! The stats: damage (or healing), cooldown, mana cost, cast time, and extra targets for a
//! single-target attack. Upgrades last the run.

use std::collections::HashMap;

use super::Game;
use crate::data::SpellKind;

/// Rarities, by index: names, how often each is dealt, and how far it pushes its main stat.
pub(crate) const RARITY_NAMES: [&str; 5] = ["Common", "Uncommon", "Rare", "Epic", "Legendary"];
const RARITY_WEIGHTS: [u32; 5] = [50, 27, 14, 7, 2];
const RARITY_RATE: [f32; 5] = [1.0, 1.4, 1.8, 1.8, 1.8];
pub(crate) const LEGENDARY: u8 = 4;
const EPIC: u8 = 3;

/// The base amounts, a common card's.
const DAMAGE: f32 = 0.12;
const COOLDOWN: f32 = 0.12;
const COST: f32 = 0.15;
const CAST: f32 = 0.12;
const TARGETS: f32 = 1.0;
/// A legendary skill also hits every enemy this near its target, and this much harder.
pub(super) const LEGENDARY_SPLASH: f32 = 8.0;
const LEGENDARY_DAMAGE: f32 = 0.25;
/// Every spell costs this share of its 1.12 mana, rage or energy.
const BASE_COST: f32 = 0.6;
/// No upgrade takes a stat past these.
const FLOOR: f32 = 0.3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Stat {
    Damage,
    Cooldown,
    Cost,
    Cast,
    Targets,
}

/// A skill's upgrades so far: multipliers on its damage, cooldown, cost and cast time, and its
/// extra targets.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Mods {
    pub(crate) damage: f32,
    pub(crate) cooldown: f32,
    pub(crate) cost: f32,
    pub(crate) cast: f32,
    pub(crate) targets: u32,
    pub(crate) legendary: bool,
}

impl Default for Mods {
    fn default() -> Self {
        Self {
            damage: 1.0,
            cooldown: 1.0,
            cost: 1.0,
            cast: 1.0,
            targets: 0,
            legendary: false,
        }
    }
}

/// One upgrade card's content.
#[derive(Clone, Debug)]
pub(crate) struct Boost {
    /// The skill's rank chain (its rank 1).
    pub(crate) chain: u32,
    pub(crate) rarity: u8,
    pub(crate) stats: Vec<(Stat, f32)>,
}

impl Boost {
    /// The card's lines: "+22% damage", "-12% mana cost", ... A spell that `heals` names its
    /// healing instead, so a damage card never reads as healing the player.
    pub(crate) fn lines(&self, heals: bool, resource: &str) -> Vec<String> {
        let power = if heals { "healing" } else { "damage" };
        if self.rarity == LEGENDARY {
            return vec![
                format!("Also strikes every enemy within {LEGENDARY_SPLASH:.0} yards of the target"),
                format!("+{:.0}% {power}", LEGENDARY_DAMAGE * 100.0),
            ];
        }
        self.stats
            .iter()
            .map(|&(stat, x)| match stat {
                Stat::Damage => format!("+{:.0}% {power}", x * 100.0),
                Stat::Cooldown => format!("-{:.0}% cooldown", x * 100.0),
                Stat::Cost => format!("-{:.0}% {resource} cost", x * 100.0),
                Stat::Cast => format!("-{:.0}% cast time", x * 100.0),
                Stat::Targets => {
                    let n = x.round() as u32;
                    format!("Hits {n} more {}", if n == 1 { "enemy" } else { "enemies" })
                }
            })
            .collect()
    }
}

fn base(stat: Stat) -> f32 {
    match stat {
        Stat::Damage => DAMAGE,
        Stat::Cooldown => COOLDOWN,
        Stat::Cost => COST,
        Stat::Cast => CAST,
        Stat::Targets => TARGETS,
    }
}

impl Game {
    /// Whether `spell` heals at all, at once or over time.
    pub(super) fn heals_any(&self, spell: u32) -> bool {
        let (now, over_time) = self.heals(spell);
        now || over_time
    }

    /// The upgrades `spell`'s skill carries.
    /// The resource `spell` costs: "mana", "rage" or "energy".
    pub(super) fn resource_of(&self, spell: u32) -> &'static str {
        match self.spell(spell).map_or(0, |s| s.power_type) {
            1 => "rage",
            3 => "energy",
            _ => "mana",
        }
    }

    pub(super) fn mods(&self, spell: u32) -> Mods {
        self.upgrades.get(&self.chain(spell).0).copied().unwrap_or_default()
    }

    /// The mana, rage or energy `s` costs: the survivor's share of 1.12's, then its upgrades.
    pub(super) fn cost_of(&self, s: &crate::data::SpellInfo) -> u32 {
        let gear = (1.0 - self.gear(super::gear::GearStat::Cost)).max(FLOOR);
        (s.cost as f32 * BASE_COST * self.mods(s.id).cost * gear).round() as u32
    }

    /// The cast time of `s` with its upgrades, ms.
    pub(super) fn cast_ms_of(&self, s: &crate::data::SpellInfo) -> u32 {
        (s.cast_ms as f32 * self.mods(s.id).cast).round() as u32
    }

    /// An upgrade card for one of the player's skills, its rarity rolled, or none when nothing
    /// known can take one.
    pub(super) fn roll_boost(&mut self, taken: &[u32]) -> Option<Boost> {
        // Each known skill with the stats it has to improve: damage or healing only for an
        // attack or a heal, a cooldown only where there is one, and so on.
        let skills: Vec<(u32, SpellKind, Vec<Stat>)> = self
            .player
            .spells
            .iter()
            .filter_map(|&s| {
                let info = self.spell(s)?;
                let kind = info.kind;
                // Level-ups improve the damage skills only; defensive ones are the trainers'.
                if kind != SpellKind::Damage {
                    return None;
                }
                let heals = info.effects.contains(&10)
                    || info.effects.iter().zip(info.auras).any(|(&e, a)| e == 6 && a == 8);
                let mut stats = Vec::new();
                if kind == SpellKind::Damage || heals {
                    stats.push(Stat::Damage);
                }
                if kind == SpellKind::Damage || info.recovery_ms > 0 {
                    stats.push(Stat::Cooldown);
                }
                if info.cost > 0 {
                    stats.push(Stat::Cost);
                }
                if info.cast_ms > 0 {
                    stats.push(Stat::Cast);
                }
                if Self::needs_target(info) {
                    stats.push(Stat::Targets);
                }
                (!stats.is_empty()).then(|| (self.chain(s).0, kind, stats))
            })
            .filter(|s| !taken.contains(&s.0))
            .collect();
        if skills.is_empty() {
            return None;
        }
        let (chain, kind, stats) = skills[(self.rng.next() % skills.len() as u64) as usize].clone();
        let mut rarity = self.roll_rarity();
        if rarity == LEGENDARY && (kind != SpellKind::Damage || self.mods(chain).legendary) {
            rarity = EPIC;
        }
        let pick = |g: &mut Game, from: &[Stat]| from[(g.rng.next() % from.len() as u64) as usize];
        let main = pick(self, &stats);
        let mut chosen = vec![(main, base(main) * RARITY_RATE[usize::from(rarity)])];
        if rarity == EPIC {
            let rest: Vec<Stat> = stats.iter().copied().filter(|&s| s != main).collect();
            if !rest.is_empty() {
                let second = pick(self, &rest);
                chosen.push((second, base(second)));
            }
        }
        Some(Boost {
            chain,
            rarity,
            stats: chosen,
        })
    }

    fn roll_rarity(&mut self) -> u8 {
        // `$SURVIVOR_RARITY=0..4`: every upgrade card of that rarity, for trying one out.
        if let Some(r) = std::env::var("SURVIVOR_RARITY").ok().and_then(|v| v.parse::<u8>().ok()) {
            return r.min(LEGENDARY);
        }
        let total: u32 = RARITY_WEIGHTS.iter().sum();
        let mut roll = (self.rng.next() % u64::from(total)) as u32;
        for (r, &w) in RARITY_WEIGHTS.iter().enumerate() {
            if roll < w {
                return r as u8;
            }
            roll -= w;
        }
        0
    }

    /// Take an upgrade card.
    pub(super) fn apply_boost(&mut self, b: &Boost) {
        let m = self.upgrades.entry(b.chain).or_default();
        if b.rarity == LEGENDARY {
            m.legendary = true;
            m.damage *= 1.0 + LEGENDARY_DAMAGE;
        }
        for &(stat, x) in &b.stats {
            if b.rarity == LEGENDARY {
                break;
            }
            match stat {
                Stat::Damage => m.damage *= 1.0 + x,
                Stat::Cooldown => m.cooldown = (m.cooldown * (1.0 - x)).max(FLOOR),
                Stat::Cost => m.cost = (m.cost * (1.0 - x)).max(FLOOR),
                Stat::Cast => m.cast = (m.cast * (1.0 - x)).max(FLOOR),
                Stat::Targets => m.targets += x.round() as u32,
            }
        }
        let name = self.spell(b.chain).map_or("", |s| s.name.as_str()).to_owned();
        eprintln!(
            "survivor: {} upgrade to {name}: {:?}",
            RARITY_NAMES[usize::from(b.rarity)],
            b.lines(self.heals_any(b.chain), self.resource_of(b.chain))
        );
    }
}

/// Upgrades by skill (rank chain), for one run.
pub(crate) type Upgrades = HashMap<u32, Mods>;
