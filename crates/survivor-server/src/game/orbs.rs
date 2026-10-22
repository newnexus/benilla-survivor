//! What a kill leaves behind: a soul (the creature's experience, a light blue orb) and its gold
//! (coins: copper, silver and gold pieces, single or worth 5 or 25, the fewest that make it). Both pop out of the corpse, then come back to the player: slowly from afar, fast
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
/// The coins there are, in copper, largest first.
const COINS: [u32; 9] = [250_000, 50_000, 10_000, 2_500, 500, 100, 25, 5, 1];
/// Coins fall scattered this far round the corpse.
const SCATTER: f32 = 0.9;

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
                    amount: o.amount,
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
