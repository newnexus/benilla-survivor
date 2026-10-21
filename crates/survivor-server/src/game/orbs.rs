//! What a kill leaves behind: a soul (the creature's experience, a light blue orb) and its gold
//! (a coin). Both pop out of the corpse, then come back to the player: slowly from afar, fast
//! once inside the pickup range (which hands' gear widens). Reaching the player, the soul's
//! experience and the coin's gold are theirs. The screen draws them from [`crate::orbs`].

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

/// An orb in the world.
pub(crate) struct Loose {
    id: u64,
    kind: Kind,
    amount: u32,
    pos: [f32; 3],
    born: Instant,
}

impl Game {
    /// A kill's soul and coin, at the corpse.
    pub(super) fn drop_orbs(&mut self, at: [f32; 3], xp: u32, copper: u32, now: Instant) {
        for (kind, amount, side) in [(Kind::Soul, xp, -0.6f32), (Kind::Coin, copper, 0.6)] {
            if amount == 0 {
                continue;
            }
            self.next_orb += 1;
            self.orbs.push(Loose {
                id: self.next_orb,
                kind,
                amount,
                pos: [at[0] + side, at[1], at[2] + FLOAT],
                born: now,
            });
        }
    }

    /// The orbs move toward the player; those that reach them are taken.
    pub(super) fn tick_orbs(&mut self, now: Instant, dt: f32) {
        if self.orbs.is_empty() {
            crate::orbs::set(Vec::new());
            return;
        }
        let me = self.player.pos;
        let range = PICKUP * (1.0 + self.gear(super::gear::GearStat::Pickup));
        let alive = self.player.dead_until.is_none();
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
                Kind::Soul => self.grant_xp(0, amount),
                Kind::Coin => self.add_money(amount),
            }
        }
        crate::orbs::set(
            self.orbs
                .iter()
                .map(|o| Orb {
                    id: o.id,
                    kind: o.kind,
                    pos: o.pos,
                })
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
        if !died && coins > 0 {
            self.add_money(coins);
        }
    }
}
