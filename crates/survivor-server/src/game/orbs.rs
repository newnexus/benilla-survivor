//! What a kill leaves behind: a soul (the creature's experience, a light blue orb) and its gold
//! (coins: copper, silver and gold pieces, single or worth 5 or 25, the fewest that make it). Both pop out of the corpse, then come back to the player: slowly from afar, fast
//! once inside the pickup range (which hands' gear widens). Reaching the player, the soul's
//! experience and the coin's gold are theirs. The screen draws them from [`crate::orbs`].
//!
//! Souls also lie about the zone outside the towns, one beside every creature's home, worth a
//! part of a kill at the player's level, each drifting slowly round its spot. One taken comes
//! back to its spot a few minutes later.

use std::time::{Duration, Instant};

use super::Game;
use crate::orbs::{Kind, Orb};

/// How long an orb hangs where it fell before it moves.
const POP: Duration = Duration::from_millis(450);
/// The pickup range, yards, before gear; drift and rush speeds, yards a second.
const PICKUP: f32 = 12.0;
const DRIFT: f32 = 5.0;
const RUSH: f32 = 22.0;
/// Close enough to be taken.
const TAKE: f32 = 1.2;
/// Orbs float this high off the ground.
const FLOAT: f32 = 1.0;
/// The coins there are, in copper, largest first.
const COINS: [u32; 9] = [250_000, 50_000, 10_000, 2_500, 500, 100, 25, 5, 1];
/// Coins fall scattered this far round the corpse.
const SCATTER: f32 = 0.9;
/// A field soul lies beside every this many creature homes, this far from it; it is worth
/// this part of a kill, comes back this long after it was taken, and shows within this range.
const FIELD_EVERY: usize = 1;
const FIELD_OFFSET: f32 = 2.5;
const FIELD_WORTH: f32 = 0.25;
const FIELD_BACK: Duration = Duration::from_secs(180);
const FIELD_SHOW: f32 = 80.0;
/// A field soul wanders a loop this far round its spot, taking about this long to go round.
const WANDER: f32 = 3.0;
const WANDER_ROUND: f32 = 40.0;

/// A soul lying in the zone.
pub(crate) struct FieldSoul {
    id: u64,
    /// The spot it wanders round, and where along its loop it started.
    home: [f32; 3],
    phase: f32,
    /// Taken: when it comes back.
    back: Option<Instant>,
}

/// An orb in the world.
pub(crate) struct Loose {
    id: u64,
    kind: Kind,
    amount: u32,
    pos: [f32; 3],
    born: Instant,
}

impl Game {
    /// A kill's soul and coins, at the corpse.
    pub(super) fn drop_orbs(&mut self, at: [f32; 3], xp: u32, copper: u32, now: Instant) {
        let mut drops = Vec::new();
        if xp > 0 {
            drops.push((Kind::Soul, xp));
        }
        let mut left = copper;
        for coin in COINS {
            while left >= coin {
                drops.push((Kind::Coin, coin));
                left -= coin;
            }
        }
        // Round the corpse, the soul first: a golden-angle spiral, so none land together.
        for (k, (kind, amount)) in drops.into_iter().enumerate() {
            let a = k as f32 * 2.4;
            let r = SCATTER * (0.4 + 0.6 * ((k as f32 + 0.5) / 8.0).sqrt().min(1.0));
            self.next_orb += 1;
            self.orbs.push(Loose {
                id: self.next_orb,
                kind,
                amount,
                pos: [at[0] + r * a.cos(), at[1] + r * a.sin(), at[2] + FLOAT],
                born: now,
            });
        }
    }

    /// The zone's souls, beside every [`FIELD_EVERY`]th creature home outside the towns.
    fn lay_field_souls(&mut self) {
        let spots: Vec<([f32; 3], f32)> = self
            .data
            .spawns
            .iter()
            .filter(|s| crate::town::town_at(s.pos).is_none())
            .step_by(FIELD_EVERY)
            .map(|s| {
                // Off to a side of the home that differs from one to the next.
                let a = (s.guid as f32 * 2.4).rem_euclid(std::f32::consts::TAU);
                (
                    [
                        s.pos[0] + FIELD_OFFSET * a.cos(),
                        s.pos[1] + FIELD_OFFSET * a.sin(),
                        s.pos[2] + FLOAT,
                    ],
                    a,
                )
            })
            .collect();
        for (home, phase) in spots {
            self.next_orb += 1;
            self.field_souls.push(FieldSoul {
                id: self.next_orb,
                home,
                phase,
                back: None,
            });
        }
        eprintln!("survivor: {} souls lie about the zone", self.field_souls.len());
    }

    /// Where field soul `s` is now along its loop: a slow figure of eight round its spot, each
    /// at its own place along it.
    fn field_soul_at(&self, s: &FieldSoul, now: Instant) -> [f32; 3] {
        let t = now.saturating_duration_since(self.started).as_secs_f32();
        let a = t / WANDER_ROUND * std::f32::consts::TAU + s.phase;
        [
            s.home[0] + WANDER * a.cos(),
            s.home[1] + WANDER * (2.0 * a).sin() * 0.6,
            s.home[2],
        ]
    }

    /// The orbs move toward the player; those that reach them are taken. Field souls in the
    /// pickup range start toward the player like a kill's.
    pub(super) fn tick_orbs(&mut self, now: Instant, dt: f32) {
        if self.field_souls.is_empty() && !self.data.spawns.is_empty() {
            self.lay_field_souls();
        }
        let me = self.player.pos;
        let range = PICKUP * (1.0 + self.gear(super::gear::GearStat::Pickup));
        let alive = self.player.dead_until.is_none();
        for s in &mut self.field_souls {
            if s.back.is_some_and(|t| now >= t) {
                s.back = None;
            }
        }
        if alive && self.in_run() {
            let worth = (super::kill_xp(self.player.level) as f32
                * FIELD_WORTH
                * (1.0 + self.gear(super::gear::GearStat::Experience)))
            .max(1.0) as u32;
            let near: Vec<(usize, [f32; 3])> = self
                .field_souls
                .iter()
                .enumerate()
                .filter(|(_, s)| s.back.is_none())
                .map(|(i, s)| (i, self.field_soul_at(s, now)))
                .filter(|&(_, p)| super::dist2d(p, me) <= range && (p[2] - me[2]).abs() < range)
                .collect();
            for (i, pos) in near {
                let s = &mut self.field_souls[i];
                s.back = Some(now + FIELD_BACK);
                self.orbs.push(Loose {
                    id: s.id,
                    kind: Kind::Soul,
                    amount: worth,
                    pos,
                    born: now - POP,
                });
            }
        }
        let mut taken = Vec::new();
        for o in &mut self.orbs {
            if !alive || now < o.born + POP {
                continue;
            }
            let goal = [me[0], me[1], me[2] + FLOAT];
            let d = (0..3).map(|k| (goal[k] - o.pos[k]).powi(2)).sum::<f32>().sqrt();
            if d <= TAKE {
                taken.push((o.kind, o.amount));
                o.amount = 0;
                continue;
            }
            let speed = if super::dist2d(o.pos, me) <= range { RUSH } else { DRIFT };
            let step = (speed * dt).min(d);
            for k in 0..3 {
                o.pos[k] += (goal[k] - o.pos[k]) / d * step;
            }
        }
        self.orbs.retain(|o| o.amount > 0);
        for (kind, amount) in taken {
            match kind {
                Kind::Soul => {
                    self.grant_xp(0, amount);
                    self.soul_found();
                }
                Kind::Coin => self.add_money(amount),
                // Runes never fly to the player: they are walked over ([`super::runes`]).
                Kind::Rune(_) => {}
            }
        }
        let lying = self
            .field_souls
            .iter()
            .filter(|s| s.back.is_none() && super::dist2d(s.home, me) <= FIELD_SHOW)
            .map(|s| Orb {
                id: s.id,
                kind: Kind::Soul,
                pos: self.field_soul_at(s, now),
                amount: 0,
            });
        crate::orbs::set(
            self.orbs
                .iter()
                .map(|o| Orb {
                    id: o.id,
                    kind: o.kind,
                    pos: o.pos,
                    amount: o.amount,
                })
                .chain(lying)
                .chain(self.rune_orbs())
                .collect(),
        );
    }

    /// The run is over: alive, the coins still out are gathered up; the souls go either way.
    pub(super) fn end_orbs(&mut self, died: bool) {
        let coins: u32 = self
            .orbs
            .iter()
            .filter(|o| o.kind == Kind::Coin)
            .map(|o| o.amount)
            .sum();
        self.orbs.clear();
        crate::orbs::set(Vec::new());
        // Called once the player is back in town: the coins count toward what the run found.
        if !died && coins > 0 {
            self.run_found += coins;
            self.add_money(coins);
        }
    }
}
