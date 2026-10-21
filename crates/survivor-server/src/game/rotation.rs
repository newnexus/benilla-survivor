//! The survivor's action bars and its automatic rotation.
//!
//! - The main bar's front is the cast queue: the damage spells, button 1 first. The auto-cast
//!   fires the first one that is ready, and that spell goes to the back of the queue, so the
//!   order cycles; every damage spell also gets a cooldown of its own, so a slow one is skipped
//!   for a turn and the order shifts. Spells around the caster (Frost Nova, Arcane Explosion)
//!   wait for a crowd: three creatures close by, or an elite.
//! - The main bar's end holds the buffs and heals: a buff is cast while it is not up, a heal when
//!   health runs low.
//! - The right-hand vertical bar (1.12's MultiBarRight, buttons 25-36) holds the rest: food,
//!   drink, the racial and the Attack toggle.

use std::time::{Duration, Instant};

use benilla_protocol::messages::opcode;

use super::{dist2d, Game, AOE_RADIUS};
use crate::data::SpellKind;
use crate::packets;

/// The main bar's buttons.
const MAIN_BAR: usize = 12;
/// MultiBarRight's first button (1.12's action 25).
const RIGHT_BAR: usize = 24;
/// A damage spell's own cooldown, on top of its cast time, when its 1.12 one is shorter.
const QUEUE_COOLDOWN_MS: u32 = 2000;
/// A heal goes off below this share of health, a heal over time below the second.
const HEAL_BELOW: f32 = 0.5;
const HOT_BELOW: f32 = 0.7;
/// Creatures close by that make a spell around the caster worth casting.
const CROWD: usize = 3;
/// `SPELL_EFFECT_HEAL` and `SPELL_AURA_PERIODIC_HEAL`.
const EFFECT_HEAL: u32 = 10;
const AURA_PERIODIC_HEAL: u32 = 8;

/// What the bars hold, by role.
#[derive(Default)]
pub(crate) struct Bars {
    /// The cast queue, front first.
    queue: Vec<u32>,
    /// Buffs and heals, filling the main bar from its end.
    keep_up: Vec<u32>,
    /// Button words (`action | kind << 24`) for the right-hand bar.
    extras: Vec<u32>,
}

impl Game {
    /// Sort the class's starting bar into the three roles.
    pub(super) fn sort_start_bar(&mut self, start: &[u32; 120]) {
        let mut bars = Bars::default();
        for &w in start.iter().filter(|&&w| w != 0) {
            let (action, kind) = (w & 0x00FF_FFFF, w >> 24);
            match (kind, self.spell(action).map(|s| s.kind)) {
                (0, Some(SpellKind::Damage)) => bars.queue.push(action),
                (0, Some(SpellKind::Defensive)) => bars.keep_up.push(action),
                _ => bars.extras.push(w),
            }
        }
        self.bars = bars;
        // `$SURVIVOR_EXTRA_SPELLS=116,122,...`: known from the start, for trying a rotation
        // without levelling up to it.
        for id in std::env::var("SURVIVOR_EXTRA_SPELLS")
            .unwrap_or_default()
            .split(',')
            .filter_map(|v| v.trim().parse::<u32>().ok())
        {
            let Some(kind) = self.spell(id).map(|s| s.kind) else { continue };
            if self.player.spells.contains(&id) {
                continue;
            }
            self.player.spells.push(id);
            match kind {
                SpellKind::Damage => self.bars.queue.push(id),
                SpellKind::Defensive => self.bars.keep_up.push(id),
                SpellKind::Other => {}
            }
        }
        self.arrange_bars(false, None);
    }

    /// A spell learned at a level-up: a new rank takes its old rank's place, a new spell joins
    /// its role's end.
    pub(super) fn bar_learn(&mut self, old: Option<u32>, spell: u32) {
        let b = &mut self.bars;
        if let Some(old) = old {
            for s in b.queue.iter_mut().chain(b.keep_up.iter_mut()) {
                if *s == old {
                    *s = spell;
                }
            }
        } else {
            match self.spell(spell).map(|s| s.kind) {
                Some(SpellKind::Damage) => self.bars.queue.push(spell),
                Some(SpellKind::Defensive) => self.bars.keep_up.push(spell),
                _ => self.bars.extras.push(spell),
            }
        }
        self.arrange_bars(true, None);
    }

    /// Lay the roles out on the buttons, post the queue for the UI, and tell the client when
    /// `send`.
    fn arrange_bars(&mut self, send: bool, fired: Option<u32>) {
        let mut buttons = [0u32; 120];
        let b = &self.bars;
        for (slot, &s) in b.queue.iter().take(MAIN_BAR).enumerate() {
            buttons[slot] = s;
        }
        let room = MAIN_BAR.saturating_sub(b.queue.len());
        let keep: Vec<u32> = b.keep_up.iter().copied().take(room).collect();
        for (i, &s) in keep.iter().enumerate() {
            buttons[MAIN_BAR - keep.len() + i] = s;
        }
        // Centred on the bar: its ends run under the minimap and the bags on a phone's bigger UI.
        let extras: Vec<u32> = b.extras.iter().copied().take(MAIN_BAR).collect();
        let first = RIGHT_BAR + (MAIN_BAR - extras.len()) / 2;
        for (i, &w) in extras.iter().enumerate() {
            buttons[first + i] = w;
        }
        self.buttons = buttons;
        crate::bar::post(b.queue.clone(), fired);
        if send {
            self.send(opcode::SMSG_ACTION_BUTTONS, &packets::action_buttons(&self.buttons));
        }
    }

    /// A queue spell started: it goes to the back.
    pub(super) fn rotate(&mut self, spell: u32) {
        let Some(i) = self.bars.queue.iter().position(|&s| s == spell) else {
            return;
        };
        let s = self.bars.queue.remove(i);
        self.bars.queue.push(s);
        self.arrange_bars(true, Some(spell));
    }

    /// How long `spell` rests after its cast: its 1.12 cooldown, and for a damage spell at least
    /// its cast time and [`QUEUE_COOLDOWN_MS`] more.
    pub(super) fn survivor_cooldown(&self, spell: u32) -> Duration {
        let Some(s) = self.spell(spell) else {
            return Duration::ZERO;
        };
        let own = if s.kind == SpellKind::Damage {
            s.cast_ms + QUEUE_COOLDOWN_MS
        } else {
            0
        };
        Duration::from_millis(u64::from(s.recovery_ms.max(own)))
    }

    fn heals(&self, spell: u32) -> (bool, bool) {
        self.spell(spell).map_or((false, false), |s| {
            (
                s.effects.contains(&EFFECT_HEAL),
                (0..3).any(|e| s.effects[e] == 6 && s.auras[e] == AURA_PERIODIC_HEAL),
            )
        })
    }

    /// Three creatures around the caster, or an elite.
    fn crowded(&self, now: Instant) -> bool {
        let close: Vec<usize> = (0..self.mobs.len())
            .filter(|&i| {
                self.mobs[i].alive()
                    && self.visible.contains(&i)
                    && dist2d(self.mobs[i].here(now), self.player.pos) <= AOE_RADIUS
            })
            .collect();
        close.len() >= CROWD
            || close.iter().any(|&i| {
                self.data
                    .creatures
                    .get(&self.mobs[i].entry)
                    .is_some_and(|t| t.rank > 0)
            })
    }

    /// The auto-cast's next spell: a heal health asks for, a buff that is down, then the first
    /// ready spell of the queue.
    pub(super) fn rotation_pick(&self, now: Instant) -> Option<u32> {
        let target = self.player.target;
        let ready = |id: u32| self.cast_refusal(id, target, now).is_none();
        let health = self.player.health as f32 / self.player.max_health.max(1) as f32;
        for &id in &self.bars.keep_up {
            let (heal, hot) = self.heals(id);
            if hot && health < HOT_BELOW && !self.buff_up(id) && ready(id)
                || heal && !hot && health < HEAL_BELOW && ready(id)
            {
                return Some(id);
            }
        }
        for &id in &self.bars.keep_up {
            let (heal, hot) = self.heals(id);
            if !heal && !hot && self.is_buff(id) && !self.buff_up(id) && ready(id) {
                return Some(id);
            }
        }
        self.bars.queue.iter().copied().find(|&id| {
            ready(id) && (!self.spell(id).is_some_and(Self::is_area) || self.crowded(now))
        })
    }
}
