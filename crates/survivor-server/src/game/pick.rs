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
    /// Each card's upgrade, by its place; none for a card that teaches.
    boosts: Vec<Option<super::upgrade::Boost>>,
    /// A chest's items, by card; a chest's pick is one card and back to the world.
    items: Vec<Option<super::gear::GearItem>>,
    chest: bool,
    /// A chest's copper back when it is recycled instead.
    refund: u32,
}

impl Pick {
    /// A chest's offer of `loot`.
    pub(super) fn chest(
        id: u64,
        paused_at: Instant,
        loot: Vec<super::gear::GearItem>,
        refund: u32,
    ) -> Self {
        Self {
            id,
            paused_at,
            boosts: Vec::new(),
            items: loot.into_iter().map(Some).collect(),
            chest: true,
            refund,
        }
    }
}

/// How long the world plays on after a level-up before its cards come: the client's level-up
/// flash, sped up (`survivor-ui`'s `DING_RATE`).
const DING_HOLD: std::time::Duration = std::time::Duration::from_millis(800);

/// Of the cards dealt, about this share are upgrades to known skills.
const BOOST_SHARE: f32 = 0.5;

impl Game {
    pub(crate) fn paused(&self) -> bool {
        self.pick.is_some() || self.menu_paused.is_some()
    }

    /// The game menu opening stops the world as a pick does; closing it lets it run on.
    pub(super) fn pause_for_menu(&mut self) {
        let open = crate::menu_pause::open();
        if open && self.menu_paused.is_none() && self.pick.is_none() {
            let now = Instant::now();
            eprintln!("survivor: paused for the game menu");
            self.menu_paused = Some(now);
            self.freeze(now);
        } else if !open {
            if let Some(at) = self.menu_paused.take() {
                eprintln!("survivor: game menu closed, the world runs on");
                self.thaw(at, Instant::now());
            }
        }
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
                // Damage skills only, six at most: defensive ones are for the trainers.
                if kind != SpellKind::Damage {
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
                    rarity: 0,
                    boost: Vec::new(),
                    item: None,
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

    /// The cards for one level: learning cards and upgrade cards mixed, each upgrade's rarity
    /// rolled, no skill on two cards of one offer.
    fn deal(&mut self) -> (Vec<Card>, Vec<Option<super::upgrade::Boost>>) {
        let offer = self.roll_offer();
        let mut learn = offer.into_iter();
        // A banished skill is never dealt again this run.
        let (mut cards, mut boosts) = (Vec::new(), Vec::new());
        let mut taken: Vec<u32> = self.banished.iter().copied().collect();
        for _ in 0..OFFER {
            let boost = if self.rng.unit() < BOOST_SHARE {
                self.roll_boost(&taken)
            } else {
                None
            };
            let (card, boost) = match boost {
                Some(b) => {
                    let card = self.boost_card(&b);
                    (card, Some(b))
                }
                None => match learn.by_ref().find(|c| !taken.contains(&self.chain(c.spell).0)) {
                    Some(c) => (c, None),
                    None => match self.roll_boost(&taken) {
                        Some(b) => (self.boost_card(&b), Some(b)),
                        None => break,
                    },
                },
            };
            taken.push(match &boost {
                Some(b) => b.chain,
                None => self.chain(card.spell).0,
            });
            cards.push(card);
            boosts.push(boost);
        }
        (cards, boosts)
    }

    /// The level-up's cards dealt again, for one of the run's rerolls.
    fn reroll_cards(&mut self) {
        if self.rerolls_left == 0 {
            return;
        }
        let (cards, boosts) = self.deal();
        if cards.is_empty() {
            return;
        }
        self.rerolls_left -= 1;
        eprintln!("survivor: cards rerolled, {} rerolls left", self.rerolls_left);
        self.repost(cards, boosts);
    }

    /// Card `k`'s skill struck from the run's cards, for one of its banishes; a fresh card takes
    /// its place.
    fn banish_card(&mut self, offer: &picks::Offer, k: usize) {
        if self.banishes_left == 0 {
            return;
        }
        let Some(card) = offer.cards.get(k) else { return };
        let gone = self.chain(card.spell).0;
        self.banished.insert(gone);
        self.banishes_left -= 1;
        eprintln!("survivor: banished {gone}, {} banishes left", self.banishes_left);
        let shown: Vec<u32> = offer.cards.iter().map(|c| self.chain(c.spell).0).collect();
        let (fresh, fresh_boosts) = self.deal();
        let new = fresh
            .into_iter()
            .zip(fresh_boosts)
            .find(|(c, _)| !shown.contains(&self.chain(c.spell).0));
        let mut cards = offer.cards.clone();
        let mut boosts: Vec<_> = self.pick.as_ref().map(|p| p.boosts.clone()).unwrap_or_default();
        match new {
            Some((c, b)) => {
                cards[k] = c;
                if let Some(slot) = boosts.get_mut(k) {
                    *slot = b;
                }
            }
            None => {
                cards.remove(k);
                if k < boosts.len() {
                    boosts.remove(k);
                }
            }
        }
        if cards.is_empty() {
            self.next_pick();
            return;
        }
        self.repost(cards, boosts);
    }

    /// The open level-up shows `cards` instead.
    fn repost(&mut self, cards: Vec<Card>, boosts: Vec<Option<super::upgrade::Boost>>) {
        let id = picks::post(self.player.level, cards, self.rerolls_left, self.banishes_left);
        if let Some(p) = self.pick.as_mut() {
            p.id = id;
            p.boosts = boosts;
        }
    }

    /// The card that shows upgrade `b`.
    fn boost_card(&self, b: &super::upgrade::Boost) -> Card {
        let spell = self.known_rank(b.chain);
        Card {
            spell,
            replaces: None,
            defensive: self.kind(spell) == SpellKind::Defensive,
            rarity: b.rarity,
            boost: b.lines(self.heals_any(spell)),
            item: None,
        }
    }

    /// The level-up screen's answer, if one came; asked every tick, paused or not.
    pub(super) fn take_pick_choice(&mut self) {
        let Some((id, index)) = picks::take_choice() else {
            return;
        };
        if !self.pick.as_ref().is_some_and(|p| p.id == id) {
            return;
        }
        let Some(offer) = picks::offer() else { return };
        // A chest recycled: none of its things, a part of its price back.
        if index == offer.cards.len() && offer.recycle.is_some() {
            let refund = self.pick.as_ref().map_or(0, |p| p.refund);
            eprintln!("survivor: chest recycled for {refund} copper");
            if refund > 0 {
                self.add_money(refund);
            }
            picks::clear();
            self.resume(Instant::now());
            return;
        }
        if index == picks::REROLL && !offer.cards.is_empty() && offer.title.is_none() {
            self.reroll_cards();
            return;
        }
        if let Some(k) = index.checked_sub(picks::BANISH) {
            if offer.title.is_none() {
                self.banish_card(&offer, k);
            }
            return;
        }
        let Some(card) = offer.cards.get(index).cloned() else {
            return;
        };
        if self.pick.as_ref().is_some_and(|p| p.chest) {
            let item = self
                .pick
                .as_mut()
                .and_then(|p| p.items.get_mut(index))
                .and_then(Option::take);
            if let Some(g) = item {
                self.take_item(g);
            }
            picks::clear();
            self.resume(Instant::now());
            return;
        }
        let boost = self
            .pick
            .as_mut()
            .and_then(|p| p.boosts.get_mut(index))
            .and_then(Option::take);
        match boost {
            Some(b) => self.apply_boost(&b),
            None => self.learn(card.spell),
        }
        self.next_pick();
    }

    /// At the end of a tick: a level gained and no pick open stops the world for one.
    pub(super) fn open_pending_pick(&mut self, now: Instant) {
        if self.picks_pending == 0 || self.pick.is_some() || self.player.dead_until.is_some() {
            self.pick_due = None;
            return;
        }
        // The world plays on while WoW's level-up flash plays on the player; the cards come as
        // it ends.
        let due = *self.pick_due.get_or_insert(now + DING_HOLD);
        if now < due {
            return;
        }
        self.pick_due = None;
        let (cards, boosts) = self.deal();
        if cards.is_empty() {
            self.picks_pending = 0;
            return;
        }
        eprintln!(
            "survivor: level-up pick offers {:?}",
            cards.iter().map(|c| (c.spell, c.rarity, c.boost.len())).collect::<Vec<_>>()
        );
        let id = picks::post(self.player.level, cards, self.rerolls_left, self.banishes_left);
        self.pick = Some(Pick {
            id,
            paused_at: now,
            boosts,
            items: Vec::new(),
            chest: false,
            refund: 0,
        });
        self.freeze(now);
    }

    /// Stop everything that moves: the player is rooted, a cast in progress is cut, and every
    /// creature stands where it is.
    pub(super) fn freeze(&mut self, now: Instant) {
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
    /// A card taken: the next pick, or the world.
    fn next_pick(&mut self) {
        self.picks_pending = self.picks_pending.saturating_sub(1);
        if self.picks_pending > 0 {
            let (cards, boosts) = self.deal();
            if !cards.is_empty() {
                let id = picks::post(self.player.level, cards, self.rerolls_left, self.banishes_left);
                let pick = self.pick.as_mut().expect("paused");
                pick.id = id;
                pick.boosts = boosts;
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
    pub(super) fn resume(&mut self, now: Instant) {
        let Some(pick) = self.pick.take() else { return };
        self.thaw(pick.paused_at, now);
    }

    /// The world stopped at `paused_at` runs on from `now`.
    fn thaw(&mut self, paused_at: Instant, now: Instant) {
        self.shift_timers(now.saturating_duration_since(paused_at));
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
        self.shift_hearth(by);
        self.shift_gathering(by);
        self.shift_runes(by);
        let p = &mut self.player;
        p.in_combat_until += by;
        p.next_swing += by;
        if let Some(t) = &mut p.dead_until {
            *t += by;
        }
        if let Some(w) = &mut self.waves {
            w.shift(by);
        }
        for m in &mut self.mobs {
            m.next_swing += by;
            m.idle_until += by;
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
        crate::town::set_in_town(false);
        crate::town::set_countdown(None);
        crate::quest_board::clear();
        crate::trails::set(Vec::new());
        if self.pick.is_some() {
            picks::clear();
        }
    }
}
