//! Class trainers: in town, standing by the class's trainer opens its window ([`crate::trainer`]),
//! where the town purse buys a skill's ranks (refunded in full on taking one back). What is
//! bought is kept, and comes to the player on a run as the run's level reaches each rank's own
//! level: a new skill on the bar, or the next rank in place of the one before. The level-up cards
//! then only improve skills ([`super::pick`]).

use std::collections::HashMap;
use std::time::Instant;

use benilla_protocol::messages::opcode;

use super::Game;
use crate::data::SpellKind;
use crate::packets;
use crate::trainer::Row;

/// How close the player stands to open the window, and how far to walk off to close it.
const RANGE: f32 = 4.0;
const RESET: f32 = 7.0;
/// Each class's trainers: Deathknell's, then Brill's.
const TRAINERS: &[(u8, u32)] = &[
    (1, 2119), // Dannal Stern
    (1, 2131), // Austil de Mon
    (4, 2122), // David Trias
    (4, 2130), // Marion Call
    (5, 2123), // Dark Cleric Duesten
    (5, 2129), // Dark Cleric Beryl
    (8, 2124), // Isabella
    (8, 2128), // Cain Firesong
    (9, 2126), // Maximillion
    (9, 2127), // Rupert Boch
];

/// A rank's price in gold: five for each level it comes at.
fn price(req_level: u32) -> u32 {
    5 * req_level.max(1)
}

impl Game {
    /// The class's skills a trainer sells: the damage and defensive chains, each its ranks
    /// `(spell, req_level)` lowest first.
    fn trainer_chains(&self) -> Vec<(u32, Vec<(u32, u32)>)> {
        let mut chains: HashMap<u32, Vec<(u32, u32, u32)>> = HashMap::new();
        for c in self.data.class_spells.iter().filter(|c| c.class == self.player.class) {
            let kind = self.spell(c.spell).map_or(SpellKind::Other, |s| s.kind);
            if kind == SpellKind::Other {
                continue;
            }
            chains.entry(c.first).or_default().push((c.rank, c.spell, c.req_level));
        }
        let mut out: Vec<(u32, Vec<(u32, u32)>)> = chains
            .into_iter()
            .map(|(first, mut ranks)| {
                ranks.sort_unstable();
                (first, ranks.into_iter().map(|(_, s, l)| (s, l)).collect())
            })
            .collect();
        out.sort_by_key(|(_, ranks)| ranks.first().map_or(0, |r| r.1));
        out
    }

    /// The class trainer of town `town`, when it has something new for the player: a rank not
    /// yet bought, of a level the camp level has reached, that the purse can pay for.
    pub(super) fn trainer_trail(&self, town: usize, now: Instant) -> Option<[f32; 3]> {
        let level = self.meta.level;
        let new = self.trainer_chains().iter().any(|(first, ranks)| {
            let bought = self.owned.get(first).copied().unwrap_or(0) as usize;
            ranks
                .get(bought)
                .is_some_and(|&(_, l)| l <= level && price(l) <= self.money)
        });
        if !new {
            return None;
        }
        self.npcs
            .iter()
            .filter(|n| TRAINERS.iter().any(|&(c, e)| c == self.player.class && e == n.entry))
            .map(|n| n.here(now))
            .find(|&p| crate::town::town_at(p) == Some(town))
    }

    /// The class trainer the player stands by in town, if any: its name.
    fn trainer_near(&self, now: Instant, range: f32) -> Option<String> {
        let me = self.player.pos;
        self.npcs.iter().find_map(|n| {
            let ours = TRAINERS.iter().any(|&(c, e)| c == self.player.class && e == n.entry);
            (ours && super::dist2d(n.here(now), me) <= range)
                .then(|| self.data.creatures.get(&n.entry).map(|t| t.name.clone()))
                .flatten()
        })
    }

    pub(super) fn open_trainer_nearby(&mut self, now: Instant) {
        if self.trainer_open.is_some() {
            if self.in_run() || self.trainer_near(now, RESET).is_none() {
                self.trainer_open = None;
                crate::trainer::clear();
            }
            return;
        }
        if self.trainer_closed {
            self.trainer_closed = self.trainer_near(now, RESET).is_some();
            return;
        }
        if self.in_run()
            || self.paused()
            || !self.walked
            || self.armory_open.is_some()
            || self.spirit_open.is_some()
        {
            return;
        }
        if let Some(name) = self.trainer_near(now, RANGE) {
            self.close_quest_panel();
            self.post_trainer(name);
        }
    }

    fn post_trainer(&mut self, name: String) {
        let rows = self
            .trainer_chains()
            .into_iter()
            .map(|(first, ranks)| {
                let bought = self.owned.get(&first).copied().unwrap_or(0);
                let shown = ranks[(bought as usize).saturating_sub(1).min(ranks.len() - 1)].0;
                let info = self.spell(shown);
                Row {
                    chain: first,
                    name: info.map_or_else(String::new, |s| s.name.clone()),
                    spell: shown,
                    defensive: info.is_some_and(|s| s.kind == SpellKind::Defensive),
                    bought,
                    ranks: ranks.len() as u32,
                    next: ranks.get(bought as usize).map(|&(_, l)| (l, price(l))),
                    levels: ranks.iter().take(bought as usize).map(|r| r.1).collect(),
                }
            })
            .collect();
        self.trainer_open = Some(crate::trainer::post(name, self.money, rows));
    }

    /// The window's answer: buy a rank, take the last one back, or close.
    pub(super) fn take_trainer_action(&mut self) {
        let Some((id, action)) = crate::trainer::take_action() else { return };
        if self.trainer_open != Some(id) {
            return;
        }
        let chains = self.trainer_chains();
        let ranks = |first: u32| chains.iter().find(|c| c.0 == first).map(|c| c.1.clone());
        match action {
            crate::trainer::Action::Close => {
                self.trainer_open = None;
                crate::trainer::clear();
                self.trainer_closed = true;
                return;
            }
            crate::trainer::Action::Buy(first) => {
                let Some(ranks) = ranks(first) else { return };
                let bought = self.owned.get(&first).copied().unwrap_or(0);
                let Some(&(spell, level)) = ranks.get(bought as usize) else { return };
                // Six damage skills at most.
                let damage = |g: &Game, s: u32| g.spell(s).is_some_and(|i| i.kind == SpellKind::Damage);
                let held = chains
                    .iter()
                    .filter(|(f, r)| self.owned.contains_key(f) && damage(self, r[0].0))
                    .count();
                if bought == 0 && damage(self, spell) && held >= super::pick::MAX_DAMAGE {
                    return;
                }
                let cost = price(level);
                if self.money < cost {
                    return;
                }
                self.money -= cost;
                self.owned.insert(first, bought + 1);
                eprintln!("survivor: bought rank {} of {first} for {cost}c", bought + 1);
            }
            crate::trainer::Action::Refund(first) => {
                let Some(ranks) = ranks(first) else { return };
                let bought = self.owned.get(&first).copied().unwrap_or(0);
                if bought == 0 {
                    return;
                }
                self.money += price(ranks[bought as usize - 1].1);
                if bought == 1 {
                    self.owned.remove(&first);
                } else {
                    self.owned.insert(first, bought - 1);
                }
                eprintln!("survivor: took back rank {bought} of {first}");
            }
        }
        self.send_money();
        self.save();
        let name = crate::trainer::current().map_or_else(String::new, |w| w.trainer);
        self.post_trainer(name);
    }

    /// On a run: every bought skill whose rank's level the run has reached, on the bar, the
    /// highest such rank in place of any lower one the player has.
    pub(super) fn grant_owned(&mut self) {
        if !self.in_run() {
            return;
        }
        let level = self.player.level;
        for (first, ranks) in self.trainer_chains() {
            let bought = self.owned.get(&first).copied().unwrap_or(0) as usize;
            let Some(&(spell, _)) = ranks[..bought.min(ranks.len())]
                .iter()
                .rev()
                .find(|&&(_, l)| l <= level)
            else {
                continue;
            };
            if self.player.spells.contains(&spell) {
                continue;
            }
            let have = self.player.spells.iter().position(|&s| self.chain(s).0 == first);
            let (_, new_rank) = self.chain(spell);
            match have {
                // Never a lower rank over a higher one (a starting rank above what is bought).
                Some(i) if self.chain(self.player.spells[i]).1 >= new_rank => {}
                Some(i) => {
                    let old = std::mem::replace(&mut self.player.spells[i], spell);
                    self.send(opcode::SMSG_SUPERCEDED_SPELL, &packets::superceded_spell(old, spell));
                    self.bar_learn(Some(old), spell);
                }
                None => {
                    self.player.spells.push(spell);
                    self.send(opcode::SMSG_LEARNED_SPELL, &packets::learned_spell(spell));
                    self.bar_learn(None, spell);
                }
            }
            let name = self.spell(spell).map_or(String::new(), |s| s.name.clone());
            eprintln!("survivor: {name} ({spell}) comes at level {level}");
        }
    }
}
