//! The level-up pick: each level gained pauses the world and offers three of the class's own
//! skills on the survivor's level-up screen ([`crate::picks`]). A skill already known comes back
//! as its next rank. Choosing one learns it and the world runs on.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use benilla_protocol::messages::opcode;

use super::{dist2d, Game, MobState, Path};
use crate::data::SpellKind;
use crate::packets;
use crate::picks::{self, Card};

/// Choices per level-up.
const OFFER: usize = 3;
/// The most damage and defensive skills a run can hold.
const MAX_DAMAGE: usize = 6;
const MAX_DEFENSIVE: usize = 2;
/// Skills a trainer would teach up to this many levels ahead may be offered.
const LEAD: u32 = 4;

/// An open pick: the board's offer id, and when the world stopped.
pub(crate) struct Pick {
    id: u64,
    paused_at: Instant,
}

impl Game {
    pub(crate) fn paused(&self) -> bool {
        self.pick.is_some()
    }

    /// The spell's rank chain (its rank 1) and rank, from the class's trainer rows; a spell no
    /// trainer teaches is a chain of its own.
    pub(super) fn chain(&self, spell: u32) -> (u32, u32) {
        self.data
            .class_spells
            .iter()
            .find(|c| c.class == self.player.class && c.spell == spell)
            .map_or((spell, 1), |c| (c.first, c.rank))
    }

    /// The rank the player knows of `spell`'s chain: a bar button or macro may still name an
    /// older one.
    pub(super) fn known_rank(&self, spell: u32) -> u32 {
        if self.player.spells.contains(&spell) {
            return spell;
        }
        let first = self.chain(spell).0;
        self.player
            .spells
            .iter()
            .copied()
            .find(|&s| self.chain(s).0 == first)
            .unwrap_or(spell)
    }

    fn kind(&self, spell: u32) -> SpellKind {
        self.spell(spell).map_or(SpellKind::Other, |s| s.kind)
    }

    /// Up to [`OFFER`] cards: the next rank of a known damage or defensive skill, or a new one
    /// while its kind has room. The nearest levels come first, reaching further when too few
    /// are near.
    fn roll_offer(&mut self) -> Vec<Card> {
        // Chain -> (known spell, its rank).
        let known: HashMap<u32, (u32, u32)> = self
            .player
            .spells
            .iter()
            .map(|&s| {
                let (first, rank) = self.chain(s);
                (first, (s, rank))
            })
            .collect();
        let held = |kind| {
            self.player
                .spells
                .iter()
                .filter(|&&s| self.kind(s) == kind)
                .count()
        };
        let room_damage = held(SpellKind::Damage) < MAX_DAMAGE;
        let room_defensive = held(SpellKind::Defensive) < MAX_DEFENSIVE;
        let mut candidates: Vec<(u32, Card)> = self
            .data
            .class_spells
            .iter()
            .filter(|c| c.class == self.player.class && self.data.spells.contains_key(&c.spell))
            .filter_map(|c| {
                let kind = self.kind(c.spell);
                if !matches!(kind, SpellKind::Damage | SpellKind::Defensive) {
                    return None;
                }
                let replaces = match known.get(&c.first) {
                    Some(&(old, rank)) if c.rank == rank + 1 => Some(old),
                    Some(_) => return None,
                    None if c.rank != 1 => return None,
                    None if kind == SpellKind::Damage && !room_damage => return None,
                    None if kind == SpellKind::Defensive && !room_defensive => return None,
                    None => None,
                };
                let card = Card {
                    spell: c.spell,
                    replaces,
                    defensive: kind == SpellKind::Defensive,
                };
                Some((c.req_level, card))
            })
            .collect();
        candidates.sort_by_key(|c| (c.0, c.1.spell));
        let near = candidates
            .iter()
            .filter(|c| c.0 <= self.player.level + LEAD)
            .count()
            .max(OFFER)
            .min(candidates.len());
        let mut pool: Vec<Card> = candidates.drain(..near).map(|c| c.1).collect();
        for i in (1..pool.len()).rev() {
            let j = (self.rng.next() % (i as u64 + 1)) as usize;
            pool.swap(i, j);
        }
        pool.truncate(OFFER);
        pool
    }

    /// The level-up screen's answer, if one came; asked every tick, paused or not.
    pub(super) fn take_pick_choice(&mut self) {
        let Some((id, spell)) = picks::take_choice() else {
            return;
        };
        let offered = self.pick.as_ref().is_some_and(|p| p.id == id)
            && picks::offer().is_some_and(|o| o.cards.iter().any(|c| c.spell == spell));
        if offered {
            self.choose(spell);
        }
    }

    /// At the end of a tick: a level gained and no pick open stops the world for one.
    pub(super) fn open_pending_pick(&mut self, now: Instant) {
        if self.picks_pending == 0 || self.pick.is_some() || self.player.dead_until.is_some() {
            return;
        }
        let cards = self.roll_offer();
        if cards.is_empty() {
            self.picks_pending = 0;
            return;
        }
        eprintln!("survivor: level-up pick offers {:?}", cards.iter().map(|c| c.spell).collect::<Vec<_>>());
        let id = picks::post(self.player.level, cards);
        self.pick = Some(Pick { id, paused_at: now });
        self.freeze(now);
    }

    /// Stop everything that moves: the player is rooted, a cast in progress is cut, and every
    /// creature stands where it is.
    fn freeze(&mut self, now: Instant) {
        self.cancel_cast();
        self.root_counter += 1;
        self.send(
            opcode::SMSG_FORCE_MOVE_ROOT,
            &packets::move_root(self.player.guid, self.root_counter),
        );
        for i in 0..self.mobs.len() {
            let Some(_) = self.mobs[i].path else { continue };
            let here = self.mobs[i].here(now);
            self.mobs[i].pos = here;
            self.mobs[i].path = None;
            if self.visible.contains(&i) {
                let (guid, id) = (self.mobs[i].guid, self.spline_id());
                self.send(opcode::SMSG_MONSTER_MOVE, &packets::monster_stop(guid, here, id));
            }
        }
    }

    /// Learn the choice, then the next pick or the world.
    fn choose(&mut self, spell: u32) {
        self.learn(spell);
        self.picks_pending = self.picks_pending.saturating_sub(1);
        if self.picks_pending > 0 {
            let cards = self.roll_offer();
            if !cards.is_empty() {
                let id = picks::post(self.player.level, cards);
                self.pick.as_mut().expect("paused").id = id;
                return;
            }
            self.picks_pending = 0;
        }
        picks::clear();
        self.resume(Instant::now());
    }

    /// Learn `spell`: a next rank replaces the rank before it, a new skill takes the first free
    /// main-bar button.
    fn learn(&mut self, spell: u32) {
        let (first, rank) = self.chain(spell);
        let before = self
            .player
            .spells
            .iter()
            .position(|&s| self.chain(s) == (first, rank - 1));
        let name = self.spell(spell).map_or(String::new(), |s| {
            format!("{} {}", s.name, s.rank_text).trim().to_owned()
        });
        eprintln!("survivor: picked {name} ({spell})");
        match before {
            Some(i) => {
                let old = std::mem::replace(&mut self.player.spells[i], spell);
                self.send(
                    opcode::SMSG_SUPERCEDED_SPELL,
                    &packets::superceded_spell(old, spell),
                );
                // The client keeps the old rank on its bar unless told.
                self.bar_learn(Some(old), spell);
            }
            None => {
                self.player.spells.push(spell);
                self.send(opcode::SMSG_LEARNED_SPELL, &packets::learned_spell(spell));
                self.bar_learn(None, spell);
            }
        }
    }

    /// The world runs on: every timer moves past the pause, the player is free, and the
    /// creatures walking home set off again.
    fn resume(&mut self, now: Instant) {
        let Some(pick) = self.pick.take() else { return };
        self.shift_timers(now.saturating_duration_since(pick.paused_at));
        // The client's buff timers ran on through the pause.
        self.send_auras(now);
        self.root_counter += 1;
        self.send(
            opcode::SMSG_FORCE_MOVE_UNROOT,
            &packets::move_root(self.player.guid, self.root_counter),
        );
        for i in 0..self.mobs.len() {
            if self.mobs[i].state != MobState::Returning {
                continue;
            }
            let m = &mut self.mobs[i];
            let (from, home) = (m.pos, m.home);
            let dur = Duration::from_secs_f32(dist2d(from, home) / m.run_speed);
            m.path = Some(Path {
                from,
                to: home,
                start: now,
                duration: dur,
            });
            let guid = m.guid;
            if self.visible.contains(&i) {
                let id = self.spline_id();
                self.send(
                    opcode::SMSG_MONSTER_MOVE,
                    &packets::monster_move(guid, from, id, home, dur.as_millis() as u32),
                );
            }
        }
    }

    /// Every deadline the world keeps, moved later by the pause.
    fn shift_timers(&mut self, by: Duration) {
        let p = &mut self.player;
        for t in p.cooldowns.values_mut() {
            *t += by;
        }
        p.gcd_until += by;
        p.in_combat_until += by;
        p.next_swing += by;
        if let Some(t) = &mut p.dead_until {
            *t += by;
        }
        for m in &mut self.mobs {
            m.next_swing += by;
            if let MobState::Dead { despawn, respawn } = &mut m.state {
                *despawn += by;
                *respawn += by;
            }
        }
        for m in &mut self.missiles {
            m.at += by;
        }
        for d in &mut self.dots {
            d.next += by;
        }
        self.next_regen += by;
        self.shift_meals(by);
        self.shift_buffs(by);
    }
}

/// A game ending (logout, disconnect) takes its open offer off the screen.
impl Drop for Game {
    fn drop(&mut self) {
        crate::aim::set(None);
        crate::bar::clear();
        if self.pick.is_some() {
            picks::clear();
        }
    }
}
