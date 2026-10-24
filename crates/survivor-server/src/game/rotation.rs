//! The survivor's action bars and its automatic rotation.
//!
//! - The cast queue is the damage spells in the player's own order (dragged on the queue strip in
//!   town, kept in the save), laid on the main bar's front. The auto-cast goes round it: from the
//!   place after the last spell cast, the first that is ready; every damage spell gets a cooldown
//!   of its own, so a slow one is skipped for a turn. Spells around the caster (Frost Nova, Arcane
//!   Explosion) wait for a crowd: three creatures close by, or an elite.
//! - The main bar's end holds the buffs and heals: a buff is cast while it is not up, a heal when
//!   health runs low.
//! - The right-hand vertical bar (1.12's MultiBarRight, buttons 25-36) holds the rest: food,
//!   drink, the Hearthstone, the racial and the Attack toggle.

use std::time::{Duration, Instant};

use benilla_protocol::messages::opcode;

use super::{dist2d, Game, AOE_RADIUS};
use crate::data::SpellKind;
use crate::packets;

/// The main bar's buttons.
const MAIN_BAR: usize = 12;
/// MultiBarRight's first button (1.12's action 25).
const RIGHT_BAR: usize = 24;
/// MultiBarBottomLeft's first button (action 61): the bar over the main one, for the
/// defensives the player fires by key (Q, E, Shift+Q, Shift+E).
const KEY_BAR: usize = 60;
const KEY_BAR_SLOTS: usize = 4;
/// A defensive with a cooldown this long or longer (Sprint, Evasion) is fired by the player, not
/// the auto-cast.
const KEY_COOLDOWN_MS: u32 = 20_000;
/// An action button's kind for an item (1.12's `ACTION_BUTTON_ITEM`).
const ITEM_BUTTON: u32 = 0x80;
/// A damage spell's own cooldown, on top of its cast time, when its 1.12 one is shorter.
const QUEUE_COOLDOWN_MS: u32 = 2000;
/// A rogue's energy abilities without a cooldown of their own get this one.
const ROGUE_COOLDOWN_MS: u32 = 4000;
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
    /// The place in the queue the auto-cast tries next.
    next: usize,
    /// Buffs and heals, filling the main bar from its end.
    keep_up: Vec<u32>,
    /// Defensives with long cooldowns, on the key bar over the main one: the player's to fire.
    by_key: Vec<u32>,
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
                (0, Some(SpellKind::Defensive)) if self.by_key(action) => bars.by_key.push(action),
                (0, Some(SpellKind::Defensive)) => bars.keep_up.push(action),
                // Items: the consumables come from the backpack itself (`consumables`).
                (_, _) if kind == ITEM_BUTTON => {}
                _ => bars.extras.push(w),
            }
        }
        self.bars = bars;
        self.order_queue();
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
                SpellKind::Defensive if self.by_key(id) => self.bars.by_key.push(id),
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
            for s in b.queue.iter_mut().chain(b.keep_up.iter_mut()).chain(b.by_key.iter_mut()) {
                if *s == old {
                    *s = spell;
                }
            }
        } else {
            match self.spell(spell).map(|s| s.kind) {
                Some(SpellKind::Damage) => {
                    self.bars.queue.push(spell);
                    self.order_queue();
                }
                Some(SpellKind::Defensive) if self.by_key(spell) => self.bars.by_key.push(spell),
                Some(SpellKind::Defensive) => self.bars.keep_up.push(spell),
                _ => self.bars.extras.push(spell),
            }
        }
        self.arrange_bars(true, None);
    }

    /// On the key bar: the player's to fire.
    pub(super) fn is_key_spell(&self, spell: u32) -> bool {
        self.bars.by_key.contains(&spell)
    }

    /// A defensive the player fires by key: a long cooldown and no healing.
    fn by_key(&self, spell: u32) -> bool {
        let (heal, hot) = self.heals(spell);
        !heal && !hot && self.spell(spell).is_some_and(|s| s.recovery_ms >= KEY_COOLDOWN_MS)
    }

    /// The right bar's items: the Hearthstone, then every food and drink in the backpack, one
    /// button each, as button words.
    fn consumables(&self) -> Vec<u32> {
        let mut out = vec![super::hearth::HEARTHSTONE | ITEM_BUTTON << 24];
        for p in self.pack.iter().filter(|p| super::loot::is_backpack(p.field)) {
            let w = p.entry | ITEM_BUTTON << 24;
            if self.food_effect(p.entry).is_some() && !out.contains(&w) {
                out.push(w);
            }
        }
        out
    }

    /// The backpack changed: the right bar's items follow it.
    pub(super) fn refresh_consumables(&mut self) {
        let mut buttons = self.buttons;
        self.place_extras(&mut buttons);
        if buttons != self.buttons {
            self.buttons = buttons;
            self.send(opcode::SMSG_ACTION_BUTTONS, &packets::action_buttons(&self.buttons));
        }
    }

    /// The right bar: the spells kept there and the consumables, centred (its ends run under
    /// the minimap and the bags on a phone's bigger UI).
    fn place_extras(&self, buttons: &mut [u32; 120]) {
        for b in &mut buttons[RIGHT_BAR..RIGHT_BAR + MAIN_BAR] {
            *b = 0;
        }
        let mut extras: Vec<u32> = self.bars.extras.clone();
        extras.extend(self.consumables());
        extras.truncate(MAIN_BAR);
        let first = RIGHT_BAR + (MAIN_BAR - extras.len()) / 2;
        for (i, &w) in extras.iter().enumerate() {
            buttons[first + i] = w;
        }
    }

    /// Lay the roles out on the buttons, post the queue for the UI, and tell the client when
    /// `send`.
    pub(super) fn arrange_bars(&mut self, send: bool, fired: Option<u32>) {
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
        for (i, &s) in b.by_key.iter().take(KEY_BAR_SLOTS).enumerate() {
            buttons[KEY_BAR + i] = s;
        }
        self.place_extras(&mut buttons);
        self.buttons = buttons;
        let in_town = !self.in_run();
        let hidden = !in_town && self.pick.is_some();
        crate::bar::post(self.strip_spells(), fired, b.next, self.queue_editable(), in_town, hidden);
        if send {
            self.send(opcode::SMSG_ACTION_BUTTONS, &packets::action_buttons(&self.buttons));
        }
    }

    /// A queue spell started: the auto-cast tries the one after it next.
    pub(super) fn rotate(&mut self, spell: u32) {
        let Some(i) = self.bars.queue.iter().position(|&s| s == spell) else {
            return;
        };
        self.bars.next = (i + 1) % self.bars.queue.len().max(1);
        self.arrange_bars(false, Some(spell));
    }

    /// The queue in the player's order (by rank chain); spells not placed yet go at its end.
    fn order_queue(&mut self) {
        let order = self.queue_order.clone();
        let mut q = std::mem::take(&mut self.bars.queue);
        q.sort_by_key(|&s| {
            let chain = self.chain(s).0;
            order.iter().position(|&c| c == chain).unwrap_or(usize::MAX)
        });
        self.bars.queue = q;
        self.bars.next = 0;
    }

    /// What the queue strip shows: on a run the queue; in town also every damage skill bought
    /// from the trainer (by its first rank), so the player can place it before it comes.
    fn strip_spells(&self) -> Vec<u32> {
        let mut list = self.bars.queue.clone();
        if self.in_run() {
            return list;
        }
        let have: Vec<u32> = list.iter().map(|&s| self.chain(s).0).collect();
        let mut bought: Vec<u32> = self
            .owned
            .keys()
            .copied()
            .filter(|c| !have.contains(c))
            .filter(|&c| self.spell(c).is_some_and(|s| s.kind == SpellKind::Damage))
            .collect();
        bought.sort_unstable();
        list.extend(bought);
        let order = &self.queue_order;
        list.sort_by_key(|&s| {
            let chain = self.chain(s).0;
            order.iter().position(|&c| c == chain).unwrap_or(usize::MAX)
        });
        list
    }

    /// The order may be changed in town, and on a run while it is paused by the game menu or a
    /// quest window; not over a level-up's or a chest's cards, where the strip is hidden.
    pub(super) fn queue_editable(&self) -> bool {
        !self.in_run() || (self.paused() && self.pick.is_none())
    }

    /// The strip's answer: the spell at `from` moved to `to` (in town, or paused); the order is kept.
    pub(super) fn take_queue_move(&mut self) {
        let Some((from, to)) = crate::bar::take_move() else { return };
        let mut list = self.strip_spells();
        let len = list.len();
        if !self.queue_editable() || from >= len || to >= len || from == to {
            return;
        }
        let s = list.remove(from);
        list.insert(to, s);
        self.queue_order = list.iter().map(|&s| self.chain(s).0).collect();
        self.order_queue();
        eprintln!("survivor: queue order {:?}", self.queue_order);
        self.arrange_bars(true, None);
        self.save();
    }

    /// How long `spell` rests after its cast: its 1.12 cooldown, and for a damage spell at least
    /// its cast time and [`QUEUE_COOLDOWN_MS`] more.
    pub(super) fn survivor_cooldown(&self, spell: u32) -> Duration {
        let gear = (1.0 - self.gear(super::gear::GearStat::Cooldown)).max(0.3);
        self.base_cooldown(spell).mul_f32(self.mods(spell).cooldown * gear)
    }

    fn base_cooldown(&self, spell: u32) -> Duration {
        let Some(s) = self.spell(spell) else {
            return Duration::ZERO;
        };
        let own = if s.kind == SpellKind::Damage {
            s.cast_ms + QUEUE_COOLDOWN_MS
        } else {
            0
        };
        // An energy ability with no cooldown of its own takes the rogue's.
        let own = if s.power_type == 3 && s.recovery_ms == 0 { own.max(ROGUE_COOLDOWN_MS) } else { own };
        Duration::from_millis(u64::from(s.recovery_ms.max(own)))
    }

    /// Whether `spell` heals: at once, and over time.
    pub(super) fn heals(&self, spell: u32) -> (bool, bool) {
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
        let usable = |id: u32| {
            ready(id) && (!self.spell(id).is_some_and(Self::is_area) || self.crowded(now))
        };
        // Round the player's order from where it stands: the first spell that is ready.
        let q = &self.bars.queue;
        (0..q.len())
            .map(|k| q[(self.bars.next + k) % q.len()])
            .find(|&id| usable(id))
    }
}
