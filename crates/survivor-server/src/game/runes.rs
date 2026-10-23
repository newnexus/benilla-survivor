//! Runes on a run: once the spirit healer has sold a kind, a rune of a kind bought appears near the
//! player now and then (more often with the healer's Rune Call), lies there a while, and gives
//! [`BUFF_TIME`] of its power to the player who walks over it: double damage, faster running,
//! shorter cooldowns, or healing. The screen draws them among the orbs ([`crate::orbs`]).

use std::time::{Duration, Instant};

use benilla_protocol::messages::opcode;

use super::gear::GearStat;
use super::spirit::{RUNE_EVERY, RUNE_NAMES};
use super::Game;
use crate::packets::{self, f, Fields};

/// How long a rune's power lasts, and how long one lies unclaimed.
const BUFF_TIME: Duration = Duration::from_secs(20);
const LIES: Duration = Duration::from_secs(30);
/// A rune appears this far from the player, yards; walking this close takes it.
const NEAR: f32 = 8.0;
const FAR: f32 = 14.0;
const TAKE: f32 = 1.8;
/// Floats this high off the ground.
const FLOAT: f32 = 1.0;
/// Runes' orb ids, clear of the souls' and coins'.
const IDS: u64 = 1 << 40;
/// The kinds, by number.
pub(crate) const POWER: u8 = 0;
pub(crate) const HASTE: u8 = 1;
pub(crate) const FOCUS: u8 = 2;
pub(crate) const MENDING: u8 = 3;

/// A rune lying on the ground.
pub(crate) struct Rune {
    id: u64,
    kind: u8,
    pos: [f32; 3],
    gone: Instant,
}

/// The runes' state on a run: those lying, when the next comes, and each kind's power until.
#[derive(Default)]
pub(crate) struct Runes {
    lying: Vec<Rune>,
    next: Option<Instant>,
    until: [Option<Instant>; 4],
    next_id: u64,
    /// Mending's health not yet given, below one point.
    carry: f32,
}

impl Game {
    /// Whether rune `kind`'s power is on the player now.
    pub(super) fn rune_on(&self, kind: u8) -> bool {
        self.runes.until[usize::from(kind)].is_some_and(|t| Instant::now() < t)
    }

    /// What the runes add to a gear stat: Power doubles damage, Haste and Focus give half again.
    pub(super) fn rune_bonus(&self, stat: GearStat) -> f32 {
        match stat {
            GearStat::SpellDamage | GearStat::PhysicalDamage if self.rune_on(POWER) => 1.0,
            GearStat::Speed if self.rune_on(HASTE) => 0.5,
            GearStat::Cooldown if self.rune_on(FOCUS) => 0.5,
            _ => 0.0,
        }
    }

    /// Bring runes, let the old ones fade, give the one walked over, and heal under Mending.
    pub(super) fn tick_runes(&mut self, now: Instant, dt: f32) {
        let kinds: Vec<u8> = (0..4u8).filter(|&k| self.healer.runes[usize::from(k)]).collect();
        if !self.in_run() || self.player.dead_until.is_some() {
            return;
        }
        // Powers running out: Haste's speed goes back.
        let mut ended = false;
        for t in &mut self.runes.until {
            if t.is_some_and(|t| now >= t) {
                *t = None;
                ended = true;
            }
        }
        if ended {
            self.apply_gear();
        }
        if !kinds.is_empty() {
            let every = Duration::from_secs(RUNE_EVERY[(self.healer.rune_rate as usize).min(3)]);
            let next = *self.runes.next.get_or_insert(now + every);
            if now >= next {
                self.runes.next = Some(now + every);
                let kind = kinds[(self.rng.unit() * kinds.len() as f32) as usize % kinds.len()];
                let a = self.rng.range(0.0, std::f32::consts::TAU);
                let r = self.rng.range(NEAR, FAR);
                let me = self.player.pos;
                self.runes.next_id += 1;
                self.runes.lying.push(Rune {
                    id: IDS + self.runes.next_id,
                    kind,
                    pos: [me[0] + r * a.cos(), me[1] + r * a.sin(), me[2] + FLOAT],
                    gone: now + LIES,
                });
                eprintln!("survivor: a {} appears", RUNE_NAMES[usize::from(kind)]);
            }
        }
        self.runes.lying.retain(|r| now < r.gone);
        let me = self.player.pos;
        if let Some(i) = self
            .runes
            .lying
            .iter()
            .position(|r| super::dist2d(r.pos, me) <= TAKE && (r.pos[2] - FLOAT - me[2]).abs() < 4.0)
        {
            let kind = self.runes.lying.remove(i).kind;
            self.runes.until[usize::from(kind)] = Some(now + BUFF_TIME);
            let name = RUNE_NAMES[usize::from(kind)];
            eprintln!("survivor: took a {name}");
            self.send(opcode::SMSG_NOTIFICATION, &packets::notification(&format!("{name}!")));
            self.apply_gear();
        }
        if self.rune_on(MENDING) && self.player.health < self.player.max_health {
            self.runes.carry += self.player.max_health as f32 / BUFF_TIME.as_secs_f32() * dt;
            let h = self.runes.carry as u32;
            if h > 0 {
                self.runes.carry -= h as f32;
                self.player.health = (self.player.health + h).min(self.player.max_health);
                let mut fl = Fields::default();
                fl.set(f::HEALTH, self.player.health);
                self.values(self.player.guid, fl);
            }
        }
    }

    /// The runes lying now, for the screen.
    pub(super) fn rune_orbs(&self) -> impl Iterator<Item = crate::orbs::Orb> + '_ {
        self.runes.lying.iter().map(|r| crate::orbs::Orb {
            id: r.id,
            kind: crate::orbs::Kind::Rune(r.kind),
            pos: r.pos,
            amount: 0,
        })
    }

    /// A pause holds the runes' clocks.
    pub(super) fn shift_runes(&mut self, by: Duration) {
        let r = &mut self.runes;
        if let Some(t) = &mut r.next {
            *t += by;
        }
        for t in r.until.iter_mut().flatten() {
            *t += by;
        }
        for l in &mut r.lying {
            l.gone += by;
        }
    }

    /// The run is over: the runes and their powers go.
    pub(super) fn end_runes(&mut self) {
        self.runes = super::runes::Runes::default();
        self.apply_gear();
    }
}
