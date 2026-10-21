//! The survivor world one player plays in: Tirisfall's creatures at their vmangos spawns, the
//! player's server-side auto-attack on the nearest foe in front, the creatures' chase and
//! swings, death, respawn, XP and levels. [`Game::tick`] runs ten times a second.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use benilla_protocol::messages::opcode;

use crate::data::{kill_xp, WorldData, XpCurve};
use crate::packets::{self, f, Fields, Swing, Update};
use crate::roster::Character;
use crate::wire::Sender;

mod aura;
mod meal;
mod pick;
mod quest;
mod rotation;
mod town;
pub(crate) use meal::PackItem;

/// Creatures keep this share of their 1.12 health.
pub(crate) const HEALTH_SCALE: f32 = 0.35;
/// Creatures within this many yards are sent to the client.
const VISIBLE: f32 = 90.0;
/// Hysteresis on [`VISIBLE`] before a creature is taken away again.
const VISIBLE_DROP: f32 = 105.0;
/// How far ahead the auto-target looks, and the half-angle of the cone in front, in radians.
const TARGET_RANGE: f32 = 30.0;
const TARGET_CONE: f32 = 1.05;
/// Melee reach between two unit centres, in yards.
const MELEE_REACH: f32 = 4.0;
/// A chasing creature this far from its spawn gives up and walks home.
const LEASH: f32 = 70.0;
/// A corpse stays this long before it is taken away.
const CORPSE_TIME: Duration = Duration::from_secs(6);
/// The player's health gain per level.
const HEALTH_PER_LEVEL: u32 = 18;
/// The player's weapon damage gain per level.
const DAMAGE_PER_LEVEL: f32 = 1.5;
/// How long the player lies dead before standing up again where they fell.
const REVIVE_TIME: Duration = Duration::from_secs(5);

/// A tiny xorshift: rolls for damage and crits need no crate.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// Uniform in `[0, 1)`.
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum MobState {
    Idle,
    Chasing,
    /// Walking back to its spawn after the leash broke, ignoring the player.
    Returning,
    Dead {
        despawn: Instant,
        respawn: Instant,
    },
}

/// A spline the client is playing for a creature, kept so the server knows where it is.
#[derive(Clone, Copy)]
struct Path {
    from: [f32; 3],
    to: [f32; 3],
    start: Instant,
    duration: Duration,
}

impl Path {
    fn at(&self, now: Instant) -> [f32; 3] {
        let t = (now.saturating_duration_since(self.start).as_secs_f32()
            / self.duration.as_secs_f32().max(0.001))
        .min(1.0);
        std::array::from_fn(|i| self.from[i] + (self.to[i] - self.from[i]) * t)
    }
    fn done(&self, now: Instant) -> bool {
        now >= self.start + self.duration
    }
}

struct Mob {
    guid: u64,
    entry: u32,
    level: u32,
    max_health: u32,
    health: u32,
    damage: (f32, f32),
    attack: Duration,
    run_speed: f32,
    detection: f32,
    hostile: bool,
    home: [f32; 3],
    facing: f32,
    pos: [f32; 3],
    path: Option<Path>,
    state: MobState,
    next_swing: Instant,
    respawn: Duration,
}

impl Mob {
    fn alive(&self) -> bool {
        !matches!(self.state, MobState::Dead { .. })
    }
    fn here(&self, now: Instant) -> [f32; 3] {
        self.path.map_or(self.pos, |p| p.at(now))
    }
}

/// A cast with a cast time, finishing at `done`.
struct Casting {
    spell: u32,
    target: Option<u64>,
    done: Instant,
}

/// A missile in flight: its damage lands at `at`.
struct Missile {
    target: u64,
    at: Instant,
    spell: u32,
    school: u8,
    damage: u32,
    crit: bool,
}

/// Damage over time on a creature.
struct Dot {
    target: u64,
    spell: u32,
    school: u8,
    every: Duration,
    next: Instant,
    ticks_left: u32,
    amount: u32,
}

/// The player's power: mana, rage (wire tenths) or energy.
pub(crate) struct Power {
    pub(crate) kind: u8,
    pub(crate) current: u32,
    pub(crate) max: u32,
}

pub(crate) struct Player {
    pub(crate) guid: u64,
    pub(crate) class: u8,
    pub(crate) power: Power,
    /// Spells the player knows, in the order they were learned.
    pub(crate) spells: Vec<u32>,
    /// Cast spells by themselves at the target (the survivor default), or wait for the bar.
    pub(crate) autocast: bool,
    cooldowns: std::collections::HashMap<u32, Instant>,
    gcd_until: Instant,
    casting: Option<Casting>,
    in_combat_until: Instant,
    pub(crate) pos: [f32; 3],
    pub(crate) facing: f32,
    pub(crate) level: u32,
    pub(crate) xp: u32,
    pub(crate) health: u32,
    pub(crate) max_health: u32,
    /// The equipped weapon's damage and speed, or bare hands.
    pub(crate) weapon: (f32, f32, Duration),
    pub(crate) target: Option<u64>,
    next_swing: Instant,
    dead_until: Option<Instant>,
    /// Buffs from the player's own spells.
    buffs: Vec<aura::Buff>,
}

pub(crate) struct Game {
    data: Arc<WorldData>,
    curve: XpCurve,
    out: Sender,
    pub(crate) player: Player,
    mobs: Vec<Mob>,
    visible: HashSet<usize>,
    next_spline: u32,
    rng: Rng,
    missiles: Vec<Missile>,
    dots: Vec<Dot>,
    next_regen: Instant,
    /// The action bar as the client last got it, `action | kind << 24` per button.
    buttons: [u32; 120],
    /// Levels gained whose pick has not been made yet.
    picks_pending: u32,
    /// The open level-up pick; the world is paused while it is set.
    pick: Option<pick::Pick>,
    /// The root and unroot counter the client's acks echo.
    root_counter: u32,
    /// What the player carries and wears.
    pack: Vec<PackItem>,
    /// Food and drink being taken.
    meals: Vec<meal::Meal>,
    /// What the action bars hold, by role ([`rotation`]).
    bars: rotation::Bars,
    /// In a safe town or out on a run ([`town`]).
    mode: town::Mode,
    /// The lasting level shown in town.
    meta: town::Meta,
    /// The towns' people, and which of them the client has.
    npcs: Vec<town::Npc>,
    npcs_shown: HashSet<usize>,
    /// The quest giver that spoke up last, quiet until the player walks off.
    talked: Option<u64>,
    quests: quest::QuestLog,
    /// Copper.
    money: u32,
    /// What a run starts with: the class's first spells and its starting bar.
    start_spells: Vec<u32>,
    start_buttons: [u32; 120],
    teleport_counter: u32,
    /// The player has moved since the world entry: the client is past its loading screen, so a
    /// quest giver's panel will be seen.
    walked: bool,
    /// The test switches' state is in (`$SURVIVOR_QUESTS`), done on the first tick.
    seeded: bool,
    /// The world entry, which the client's movement clock counts from.
    started: Instant,
}

/// `SPELL_FAILED_*` reasons the client words.
mod fail {
    pub const BAD_TARGETS: u8 = 0x0A;
    /// The client says nothing.
    pub const DONT_REPORT: u8 = 0x17;
    pub const NOT_READY: u8 = 0x3C;
    pub const NO_POWER: u8 = 0x4D;
    pub const OUT_OF_RANGE: u8 = 0x59;
    pub const SPELL_IN_PROGRESS: u8 = 0x61;
    pub const TARGETS_DEAD: u8 = 0x65;
}

/// `Attack`: the auto-attack toggle every bar starts with.
const SPELL_ATTACK: u32 = 6603;
/// The global cooldown between spells.
const GCD: Duration = Duration::from_millis(1500);
/// Radius of a spell centred on the caster (Arcane Explosion, Frost Nova).
const AOE_RADIUS: f32 = 10.0;
/// Mana gained per level, and spell damage gained per level.
const MANA_PER_LEVEL: u32 = 20;
const SPELL_DAMAGE_PER_LEVEL: f32 = 1.0;

fn dist2d(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// The class's power type and pool at level 1: rage (wire tenths) and energy for the melee
/// classes, mana for the casters.
pub(crate) fn class_power(class: u8) -> (u8, u32) {
    match class {
        1 => (1, 1000),
        4 => (3, 100),
        5 => (0, 160),
        8 => (0, 165),
        _ => (0, 125),
    }
}

/// The player's level-1 health for each Forsaken class (vmangos `player_classlevelstats`,
/// rounded).
pub(crate) fn base_health(class: u8) -> u32 {
    match class {
        1 => 60,
        4 => 55,
        9 => 63,
        _ => 52,
    }
}

impl Game {
    pub(crate) fn new(
        data: Arc<WorldData>,
        out: Sender,
        c: &Character,
        weapon: (f32, f32, Duration),
        buttons: [u32; 120],
        pack: Vec<PackItem>,
        pos: [f32; 3],
        facing: f32,
    ) -> Self {
        let now = Instant::now();
        let curve = XpCurve::for_world(&data);
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ c.guid);
        let mobs = data
            .spawns
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let t = &data.creatures[&s.entry];
                let span = t.level.1.saturating_sub(t.level.0);
                let level = t.level.0 + (rng.next() % u64::from(span + 1)) as u32;
                let frac = if span == 0 {
                    0.0
                } else {
                    (level - t.level.0) as f32 / span as f32
                };
                let health = t.health.0 as f32 + (t.health.1 as f32 - t.health.0 as f32) * frac;
                let damage = t.damage.0 + (t.damage.1 - t.damage.0) * frac;
                let max_health = ((health * HEALTH_SCALE).round() as u32).max(1);
                Mob {
                    guid: 0xF130_0000_0000_0000 | u64::from(t.entry) << 24 | (i as u64 + 1),
                    entry: t.entry,
                    level,
                    max_health,
                    health: max_health,
                    damage: (damage * 0.85, damage * 1.15),
                    attack: Duration::from_millis(u64::from(t.attack_ms.max(1000))),
                    run_speed: packets::STOCK_SPEEDS[1] * t.speed.1.max(0.5),
                    detection: t.detection.clamp(8.0, 20.0),
                    hostile: t.hostile,
                    home: s.pos,
                    facing: s.facing,
                    pos: s.pos,
                    path: None,
                    state: MobState::Idle,
                    next_swing: now,
                    respawn: Duration::from_secs(u64::from(s.respawn_s.clamp(30, 300))),
                }
            })
            .collect();
        let max_health = base_health(c.class);
        let (kind, max) = class_power(c.class);
        let spells = data
            .start_actions
            .iter()
            .filter(|a| a.0 == c.class && a.3 == 0 && data.spells.contains_key(&a.2))
            .map(|a| a.2)
            .collect();
        let mut game = Self {
            curve,
            out,
            missiles: Vec::new(),
            dots: Vec::new(),
            next_regen: now,
            buttons,
            picks_pending: 0,
            pick: None,
            root_counter: 0,
            pack,
            meals: Vec::new(),
            bars: rotation::Bars::default(),
            mode: town::mode_at(pos),
            meta: town::Meta::default(),
            npcs: town::npcs(&data, now),
            npcs_shown: HashSet::new(),
            talked: None,
            quests: quest::QuestLog::default(),
            money: 0,
            start_spells: Vec::new(),
            start_buttons: buttons,
            teleport_counter: 0,
            seeded: false,
            walked: false,
            started: now,
            player: Player {
                guid: c.guid,
                class: c.class,
                // Rage starts empty; energy and mana start full.
                power: Power {
                    kind,
                    current: if kind == 1 { 0 } else { max },
                    max,
                },
                spells,
                // `$SURVIVOR_AUTOCAST=0` starts with it off, for looking at creatures alive.
                autocast: std::env::var("SURVIVOR_AUTOCAST").as_deref() != Ok("0"),
                cooldowns: std::collections::HashMap::new(),
                gcd_until: now,
                casting: None,
                in_combat_until: now,
                pos,
                facing,
                level: 1,
                xp: 0,
                health: max_health,
                max_health,
                weapon,
                target: None,
                next_swing: now,
                dead_until: None,
                buffs: Vec::new(),
            },
            data,
            mobs,
            visible: HashSet::new(),
            next_spline: 1,
            rng,
        };
        game.start_spells = game.player.spells.clone();
        game.sort_start_bar(&buttons);
        game
    }

    /// The action bars as the client is to get them at login.
    pub(crate) fn buttons(&self) -> &[u32; 120] {
        &self.buttons
    }

    /// XP needed from level 1 to 2, for the player's create.
    pub(crate) fn first_level_xp(&self) -> u32 {
        self.curve.next_level(1)
    }

    fn send(&self, op: u16, body: &[u8]) {
        let _ = self.out.send(op, body);
    }

    fn values(&self, guid: u64, fields: Fields) {
        self.send(
            opcode::SMSG_UPDATE_OBJECT,
            &packets::update_object(&[Update::Values { guid, fields }]),
        );
    }

    fn spline_id(&mut self) -> u32 {
        self.next_spline = self.next_spline.wrapping_add(1);
        self.next_spline
    }

    /// The client moved its player.
    pub(crate) fn player_moved(&mut self, pos: [f32; 3], facing: f32) {
        self.walked |= dist2d(pos, self.player.pos) > 0.1;
        self.player.pos = pos;
        self.player.facing = facing;
    }

    /// The client picked a target itself (a click or Tab): the auto-attack follows it.
    pub(crate) fn player_selected(&mut self, guid: u64) {
        if self.mobs.iter().any(|m| m.guid == guid && m.alive()) {
            self.player.target = Some(guid);
        }
    }

    pub(crate) fn tick(&mut self) {
        if !self.seeded {
            self.seeded = true;
            self.seed_test_quests();
        }
        self.take_pick_choice();
        if self.paused() {
            return;
        }
        let now = Instant::now();
        self.revive(now);
        self.check_barrier(now);
        self.update_visibility(now);
        self.walk_beats(now);
        self.update_npcs(now);
        self.talk_nearby(now);
        self.player_attack(now);
        self.tick_spells(now);
        self.tick_meals(now);
        self.tick_buffs(now);
        for i in 0..self.mobs.len() {
            self.mob_ai(i, now);
        }
        crate::aim::set(self.player.target);
        crate::town::set_in_town(!self.in_run());
        self.open_pending_pick(now);
    }

    fn revive(&mut self, now: Instant) {
        if self.player.dead_until.is_some_and(|t| now >= t) {
            self.player.dead_until = None;
            // Up again, alive, in the nearest town: the run is over.
            self.carry_to_town(now);
        }
    }

    fn update_visibility(&mut self, now: Instant) {
        let me = self.player.pos;
        let mut creates = Vec::new();
        let mut gone = Vec::new();
        for (i, m) in self.mobs.iter().enumerate() {
            let shown = self.visible.contains(&i);
            let d = dist2d(m.here(now), me);
            let despawned = matches!(m.state, MobState::Dead { despawn, .. } if now >= despawn);
            if shown && (d > VISIBLE_DROP || despawned) {
                gone.push(i);
            } else if !shown && d <= VISIBLE && !despawned && m.alive() {
                creates.push(i);
            }
        }
        for i in &gone {
            self.visible.remove(i);
        }
        if !creates.is_empty() || !gone.is_empty() {
            eprintln!(
                "survivor: {} creatures come into view, {} leave ({} shown)",
                creates.len(),
                gone.len(),
                self.visible.len() + creates.len()
            );
        }
        if !gone.is_empty() {
            let guids = gone.iter().map(|&i| self.mobs[i].guid).collect();
            self.send(
                opcode::SMSG_UPDATE_OBJECT,
                &packets::update_object(&[Update::OutOfRange(guids)]),
            );
        }
        // A few dozen creates per packet keeps each well inside the 16-bit size field.
        for chunk in creates.chunks(40) {
            let updates: Vec<Update> = chunk
                .iter()
                .map(|&i| {
                    let m = &self.mobs[i];
                    packets::creature_create(
                        m.guid,
                        &self.data.creatures[&m.entry],
                        m.level,
                        m.health,
                        m.max_health,
                        m.here(now),
                        m.facing,
                    )
                })
                .collect();
            self.send(
                opcode::SMSG_UPDATE_OBJECT,
                &packets::update_object(&updates),
            );
            self.visible.extend(chunk.iter().copied());
        }
    }

    /// The nearest living creature in the cone in front of the player, within range.
    fn auto_target(&self, now: Instant) -> Option<u64> {
        let me = self.player.pos;
        let facing = self.player.facing;
        self.mobs
            .iter()
            .filter(|m| m.alive())
            .filter_map(|m| {
                let p = m.here(now);
                let d = dist2d(p, me);
                if d > TARGET_RANGE {
                    return None;
                }
                let bearing = (p[1] - me[1]).atan2(p[0] - me[0]);
                let mut off = (bearing - facing).rem_euclid(std::f32::consts::TAU);
                if off > std::f32::consts::PI {
                    off -= std::f32::consts::TAU;
                }
                (off.abs() <= TARGET_CONE || d <= MELEE_REACH).then_some((d, m.guid))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, g)| g)
    }

    fn mob_index(&self, guid: u64) -> Option<usize> {
        self.mobs.iter().position(|m| m.guid == guid)
    }

    fn player_attack(&mut self, now: Instant) {
        if self.player.dead_until.is_some() || !self.in_run() {
            return;
        }
        // Keep a living target; otherwise take the nearest foe in front.
        let keep = self
            .player
            .target
            .and_then(|g| self.mob_index(g))
            .filter(|&i| {
                let m = &self.mobs[i];
                m.alive() && dist2d(m.here(now), self.player.pos) <= TARGET_RANGE
            });
        let target = match keep {
            Some(i) => i,
            None => {
                let new = self.auto_target(now);
                if new != self.player.target {
                    let mut fl = Fields::default();
                    fl.guid(f::TARGET, new.unwrap_or(0));
                    self.values(self.player.guid, fl);
                    if let Some(g) = new {
                        self.send(
                            opcode::SMSG_ATTACKSTART,
                            &packets::attack_start(self.player.guid, g),
                        );
                    }
                }
                self.player.target = new;
                match new.and_then(|g| self.mob_index(g)) {
                    Some(i) => i,
                    None => return,
                }
            }
        };
        if now < self.player.next_swing {
            return;
        }
        let mob_pos = self.mobs[target].here(now);
        if dist2d(mob_pos, self.player.pos) > MELEE_REACH {
            return;
        }
        let (lo, hi, speed) = self.player.weapon;
        self.player.next_swing = now + speed;
        let bonus = (self.player.level - 1) as f32 * DAMAGE_PER_LEVEL;
        let roll = self.rng.unit();
        let (swing, damage) = if roll < 0.05 {
            (Swing::Miss, 0)
        } else {
            let d = self.rng.range(lo + bonus, hi + bonus).round().max(1.0) as u32;
            if roll > 0.9 {
                (Swing::Crit, d * 2)
            } else {
                (Swing::Hit, d)
            }
        };
        let guid = self.mobs[target].guid;
        self.send(
            opcode::SMSG_ATTACKERSTATEUPDATE,
            &packets::attacker_state(self.player.guid, guid, swing, damage),
        );
        self.player.in_combat_until = now + Duration::from_secs(5);
        if swing == Swing::Miss {
            return;
        }
        // A warrior's blow builds 7.5 rage.
        self.gain_rage(75);
        let m = &mut self.mobs[target];
        m.health = m.health.saturating_sub(damage);
        if m.state == MobState::Idle {
            m.state = MobState::Chasing;
        }
        let health = m.health;
        let mut fl = Fields::default();
        fl.set(f::HEALTH, health);
        self.values(guid, fl);
        if health == 0 {
            self.kill(target, now);
        }
    }

    fn kill(&mut self, i: usize, now: Instant) {
        let (guid, level, here, was_moving) = {
            let m = &mut self.mobs[i];
            m.state = MobState::Dead {
                despawn: now + CORPSE_TIME,
                respawn: now + m.respawn,
            };
            // The corpse stays where it fell: without a stop the client plays the rest of the
            // run's spline and the body slides along the ground.
            let here = m.here(now);
            let was_moving = m.path.take().is_some();
            m.pos = here;
            (m.guid, m.level, here, was_moving)
        };
        if was_moving {
            let id = self.spline_id();
            self.send(opcode::SMSG_MONSTER_MOVE, &packets::monster_stop(guid, here, id));
        }
        self.send(
            opcode::SMSG_ATTACKSTOP,
            &packets::attack_stop(self.player.guid, guid),
        );
        self.send(opcode::SMSG_ATTACKSTOP, &packets::attack_stop(guid, self.player.guid));
        self.player.target = None;
        let entry = self.mobs[i].entry;
        self.quest_kill(entry, guid);
        self.grant_meta_xp(self.meta_kill_xp(level));
        let xp = kill_xp(level);
        eprintln!(
            "survivor: killed {} (level {level}) for {xp} xp",
            self.data.creatures[&self.mobs[i].entry].name
        );
        self.grant_xp(guid, xp);
    }

    fn grant_xp(&mut self, victim: u64, xp: u32) {
        self.send(opcode::SMSG_LOG_XPGAIN, &packets::xp_gain(victim, xp));
        self.player.xp += xp;
        let mut fl = Fields::default();
        while self.player.xp >= self.curve.next_level(self.player.level) {
            self.player.xp -= self.curve.next_level(self.player.level);
            self.player.level += 1;
            self.picks_pending += 1;
            eprintln!("survivor: reached level {}", self.player.level);
            self.player.max_health += HEALTH_PER_LEVEL;
            self.player.health = self.player.max_health;
            if self.player.power.kind == 0 {
                self.player.power.max += MANA_PER_LEVEL;
                self.player.power.current = self.player.power.max;
                fl.set(f::MAXPOWER1, self.player.power.max)
                    .set(f::POWER1, self.player.power.current);
            }
            self.send(
                opcode::SMSG_LEVELUP_INFO,
                &packets::levelup_info(self.player.level, HEALTH_PER_LEVEL),
            );
            fl.set(f::LEVEL, self.player.level)
                .set(f::MAXHEALTH, self.player.max_health)
                .set(f::HEALTH, self.player.health)
                .set(f::PLAYER_NEXT_LEVEL_XP, self.curve.next_level(self.player.level));
        }
        fl.set(f::PLAYER_XP, self.player.xp);
        self.values(self.player.guid, fl);
    }

    fn mob_ai(&mut self, i: usize, now: Instant) {
        let me = self.player.pos;
        // In town the player is out of reach: nothing comes for them, and a chase breaks off.
        let player_alive = self.player.dead_until.is_none() && self.mode == town::Mode::Run;
        let shown = self.visible.contains(&i);
        let m = &mut self.mobs[i];
        match m.state {
            MobState::Dead { respawn, .. } => {
                if now >= respawn && !shown {
                    m.state = MobState::Idle;
                    m.health = m.max_health;
                    m.pos = m.home;
                    m.path = None;
                }
                return;
            }
            MobState::Idle => {
                if m.path.is_some_and(|p| p.done(now)) {
                    m.pos = m.here(now);
                    m.path = None;
                }
                if player_alive && m.hostile && dist2d(m.pos, me) <= m.detection {
                    m.state = MobState::Chasing;
                } else {
                    return;
                }
            }
            MobState::Returning => {
                if m.path.is_none_or(|p| p.done(now)) {
                    m.pos = m.home;
                    m.path = None;
                    m.state = MobState::Idle;
                    m.health = m.max_health;
                    let (guid, health) = (m.guid, m.health);
                    let mut fl = Fields::default();
                    fl.set(f::HEALTH, health);
                    self.values(guid, fl);
                }
                return;
            }
            MobState::Chasing => {}
        }
        // Chasing.
        let m = &mut self.mobs[i];
        let here = m.here(now);
        if !player_alive || dist2d(here, m.home) > LEASH {
            m.state = MobState::Returning;
            let dur = Duration::from_secs_f32(dist2d(here, m.home) / m.run_speed);
            m.path = Some(Path {
                from: here,
                to: m.home,
                start: now,
                duration: dur,
            });
            let (guid, home) = (m.guid, m.home);
            let id = self.spline_id();
            self.send(
                opcode::SMSG_MONSTER_MOVE,
                &packets::monster_move(guid, here, id, home, dur.as_millis() as u32),
            );
            self.send(opcode::SMSG_ATTACKSTOP, &packets::attack_stop(guid, self.player.guid));
            return;
        }
        let d = dist2d(here, me);
        if d > MELEE_REACH * 0.8 {
            // Run to a point just short of the player, re-aimed when the player has moved off it.
            let aim_off = m.path.is_none_or(|p| dist2d(p.to, me) > 1.5);
            if aim_off {
                let k = (d - MELEE_REACH * 0.6) / d;
                let to = [
                    here[0] + (me[0] - here[0]) * k,
                    here[1] + (me[1] - here[1]) * k,
                    me[2],
                ];
                let dur = Duration::from_secs_f32(dist2d(here, to) / m.run_speed);
                m.pos = here;
                m.path = Some(Path {
                    from: here,
                    to,
                    start: now,
                    duration: dur,
                });
                let guid = m.guid;
                let id = self.spline_id();
                self.send(
                    opcode::SMSG_MONSTER_MOVE,
                    &packets::monster_move(guid, here, id, to, dur.as_millis() as u32),
                );
            }
            return;
        }
        // In reach: stop and swing.
        if m.path.is_some() {
            m.pos = here;
            m.path = None;
            let guid = m.guid;
            let id = self.spline_id();
            self.send(opcode::SMSG_MONSTER_MOVE, &packets::monster_stop(guid, here, id));
            self.send(
                opcode::SMSG_ATTACKSTART,
                &packets::attack_start(guid, self.player.guid),
            );
        }
        let m = &mut self.mobs[i];
        if now < m.next_swing {
            return;
        }
        m.next_swing = now + m.attack;
        let (guid, lo, hi) = (m.guid, m.damage.0, m.damage.1);
        let roll = self.rng.unit();
        let (swing, damage) = if roll < 0.08 {
            (Swing::Miss, 0)
        } else {
            (Swing::Hit, self.rng.range(lo, hi).round().max(1.0) as u32)
        };
        self.send(
            opcode::SMSG_ATTACKERSTATEUPDATE,
            &packets::attacker_state(guid, self.player.guid, swing, damage),
        );
        self.player.in_combat_until = now + Duration::from_secs(5);
        if damage == 0 {
            return;
        }
        // Taking a blow builds 5 rage.
        self.gain_rage(50);
        self.player.health = self.player.health.saturating_sub(damage);
        let mut fl = Fields::default();
        fl.set(f::HEALTH, self.player.health);
        self.values(self.player.guid, fl);
        if self.player.health == 0 {
            self.player.dead_until = Some(now + REVIVE_TIME);
            if let Some(t) = self.player.target.take() {
                self.send(opcode::SMSG_ATTACKSTOP, &packets::attack_stop(self.player.guid, t));
            }
            self.player.casting = None;
        }
    }
}

/// Spells: casting, missiles, damage over time, power and auto-cast.
impl Game {
    fn send_power(&self) {
        let mut fl = Fields::default();
        fl.set(f::POWER1 + u16::from(self.player.power.kind), self.player.power.current);
        self.values(self.player.guid, fl);
    }

    fn spell(&self, id: u32) -> Option<&crate::data::SpellInfo> {
        self.data.spells.get(&id)
    }

    /// Whether the spell hits creatures around the caster rather than one target.
    fn is_area(s: &crate::data::SpellInfo) -> bool {
        s.targets.iter().any(|t| matches!(t, 15 | 16 | 22 | 24))
            && s.kind == crate::data::SpellKind::Damage
    }

    /// Whether a cast must name a living hostile unit.
    fn needs_target(s: &crate::data::SpellInfo) -> bool {
        s.kind == crate::data::SpellKind::Damage && !Self::is_area(s)
    }

    /// The reason `spell` cannot be cast at `target` now, if any.
    fn cast_refusal(&self, spell: u32, target: Option<u64>, now: Instant) -> Option<u8> {
        let s = self.spell(spell)?;
        if !self.player.spells.contains(&spell) || self.player.dead_until.is_some() {
            return Some(fail::BAD_TARGETS);
        }
        if self.player.casting.is_some() {
            return Some(fail::SPELL_IN_PROGRESS);
        }
        if now < self.player.gcd_until
            || self.player.cooldowns.get(&spell).is_some_and(|&t| now < t)
        {
            return Some(fail::NOT_READY);
        }
        if s.power_type == self.player.power.kind && self.player.power.current < s.cost {
            return Some(fail::NO_POWER);
        }
        // In town nothing is fought: no spell at a creature, none around the caster.
        if !self.in_run() && (Self::needs_target(s) || Self::is_area(s)) {
            return Some(fail::BAD_TARGETS);
        }
        if Self::needs_target(s) {
            let Some(i) = target.and_then(|g| self.mob_index(g)) else {
                return Some(fail::BAD_TARGETS);
            };
            if !self.mobs[i].alive() {
                return Some(fail::TARGETS_DEAD);
            }
            let reach = if s.dmg_class == 2 { MELEE_REACH } else { s.range.max(MELEE_REACH) };
            if dist2d(self.mobs[i].here(now), self.player.pos) > reach + 2.0 {
                return Some(fail::OUT_OF_RANGE);
            }
        }
        None
    }

    /// A `CMSG_CAST_SPELL` from the client, or the auto-cast's own.
    pub(crate) fn cast(&mut self, spell: u32, target: Option<u64>) {
        let now = Instant::now();
        // Paused for a level-up pick: refused without a word, as the 1-3 keys that pick a
        // card also press the bar's buttons.
        if self.paused() {
            self.send(opcode::SMSG_CAST_RESULT, &packets::cast_result(spell, Some(fail::DONT_REPORT)));
            return;
        }
        if spell == SPELL_ATTACK {
            if let Some(g) = target.or(self.player.target) {
                self.player_selected(g);
            }
            self.send(opcode::SMSG_CAST_RESULT, &packets::cast_result(spell, None));
            return;
        }
        let target = target.or(self.player.target);
        let spell = self.known_rank(spell);
        if let Some(reason) = self.cast_refusal(spell, target, now) {
            eprintln!("survivor: refused cast {spell}: reason {reason:#x}");
            self.send(opcode::SMSG_CAST_RESULT, &packets::cast_result(spell, Some(reason)));
            return;
        }
        let Some(s) = self.spell(spell).cloned() else { return };
        let target = if Self::needs_target(&s) { target } else { None };
        self.player.gcd_until = now + GCD;
        self.send(
            opcode::SMSG_SPELL_START,
            &packets::spell_start(self.player.guid, spell, s.cast_ms, target),
        );
        self.rotate(spell);
        if s.cast_ms > 0 {
            self.player.casting = Some(Casting {
                spell,
                target,
                done: now + Duration::from_millis(u64::from(s.cast_ms)),
            });
        } else {
            self.finish_cast(spell, target, now);
        }
    }

    /// `CMSG_CANCEL_CAST`: the player pressed Escape.
    pub(crate) fn cancel_cast(&mut self) {
        if let Some(c) = self.player.casting.take() {
            self.send(opcode::SMSG_CAST_RESULT, &packets::cast_result(c.spell, Some(0x23)));
        }
    }

    fn finish_cast(&mut self, spell: u32, target: Option<u64>, now: Instant) {
        let Some(s) = self.spell(spell).cloned() else { return };
        // The target may have died while the cast ran.
        if Self::needs_target(&s)
            && !target
                .and_then(|g| self.mob_index(g))
                .is_some_and(|i| self.mobs[i].alive())
        {
            self.send(opcode::SMSG_CAST_RESULT, &packets::cast_result(spell, Some(fail::TARGETS_DEAD)));
            return;
        }
        if s.power_type == self.player.power.kind {
            self.player.power.current = self.player.power.current.saturating_sub(s.cost);
            self.send_power();
        }
        let rest = self.survivor_cooldown(spell);
        if !rest.is_zero() {
            self.player.cooldowns.insert(spell, now + rest);
            self.send(
                opcode::SMSG_SPELL_COOLDOWN,
                &packets::spell_cooldown(self.player.guid, spell, rest.as_millis() as u32),
            );
        }
        self.player.in_combat_until = now + Duration::from_secs(5);
        // Who it hits: one target, everything around the caster, or the caster itself.
        let hits: Vec<usize> = if Self::is_area(&s) {
            (0..self.mobs.len())
                .filter(|&i| {
                    self.mobs[i].alive()
                        && self.visible.contains(&i)
                        && dist2d(self.mobs[i].here(now), self.player.pos) <= AOE_RADIUS
                })
                .collect()
        } else {
            target.and_then(|g| self.mob_index(g)).into_iter().collect()
        };
        let hit_guids: Vec<u64> = if hits.is_empty() {
            vec![self.player.guid]
        } else {
            hits.iter().map(|&i| self.mobs[i].guid).collect()
        };
        eprintln!(
            "survivor: cast {} {} at {} target(s), power left {}",
            s.name,
            s.rank_text,
            hits.len(),
            self.player.power.current
        );
        self.send(opcode::SMSG_CAST_RESULT, &packets::cast_result(spell, None));
        self.send(
            opcode::SMSG_SPELL_GO,
            &packets::spell_go(self.player.guid, spell, &hit_guids, target),
        );
        if self.is_buff(spell) && s.kind != crate::data::SpellKind::Damage {
            self.apply_buff(spell, now);
        }
        let bonus = (self.player.level - 1) as f32 * SPELL_DAMAGE_PER_LEVEL;
        for e in 0..3 {
            let (effect, base, die) = (s.effects[e], s.base_points[e], s.die_sides[e]);
            let roll = |rng: &mut Rng| {
                let hi = base + die.max(1);
                rng.range((base + 1) as f32, hi as f32 + 0.999).floor().max(0.0)
            };
            match effect {
                // School damage.
                2 => {
                    for &i in &hits {
                        let d = roll(&mut self.rng) + bonus;
                        self.deliver(i, &s, d, now);
                    }
                }
                // Weapon damage plus the ability's bonus, or a share of the weapon.
                17 | 58 | 121 | 31 => {
                    let (lo, hi, _) = self.player.weapon;
                    for &i in &hits {
                        let w = self.rng.range(lo, hi) + bonus;
                        let d = if effect == 31 {
                            w * (base + 1) as f32 / 100.0
                        } else {
                            w + (base + 1) as f32
                        };
                        self.deliver(i, &s, d, now);
                    }
                }
                // Damage over time.
                6 if s.auras[e] == 3 => {
                    let every = Duration::from_millis(u64::from(s.amplitudes[e].max(1000)));
                    let ticks = (s.duration_ms / every.as_millis().max(1) as u32).max(1);
                    for &i in &hits {
                        self.mobs[i].state = match self.mobs[i].state {
                            MobState::Idle => MobState::Chasing,
                            other => other,
                        };
                        self.dots.push(Dot {
                            target: self.mobs[i].guid,
                            spell,
                            school: s.school,
                            every,
                            next: now + every,
                            ticks_left: ticks,
                            amount: (base + 1).max(1) as u32 + bonus as u32,
                        });
                    }
                }
                // A heal on oneself.
                10 => {
                    let h = roll(&mut self.rng) as u32;
                    self.player.health = (self.player.health + h).min(self.player.max_health);
                    let mut fl = Fields::default();
                    fl.set(f::HEALTH, self.player.health);
                    self.values(self.player.guid, fl);
                }
                _ => {}
            }
        }
    }

    /// Damage to creature `i`: at once, or when the missile lands.
    fn deliver(&mut self, i: usize, s: &crate::data::SpellInfo, damage: f32, now: Instant) {
        let crit = self.rng.unit() < 0.1;
        let damage = (damage.max(1.0) * if crit { 1.5 } else { 1.0 }).round() as u32;
        let m = &mut self.mobs[i];
        if m.state == MobState::Idle {
            m.state = MobState::Chasing;
        }
        let flight = if s.speed > 0.0 {
            Duration::from_secs_f32(dist2d(m.here(now), self.player.pos) / s.speed)
        } else {
            Duration::ZERO
        };
        self.missiles.push(Missile {
            target: m.guid,
            at: now + flight,
            spell: s.id,
            school: s.school,
            damage,
            crit,
        });
    }

    /// Spell damage landing on a creature.
    fn spell_hit(&mut self, guid: u64, spell: u32, school: u8, damage: u32, crit: bool, now: Instant) {
        let Some(i) = self.mob_index(guid) else { return };
        if !self.mobs[i].alive() {
            return;
        }
        self.send(
            opcode::SMSG_SPELLNONMELEEDAMAGELOG,
            &packets::spell_damage(guid, self.player.guid, spell, damage, school, crit),
        );
        let m = &mut self.mobs[i];
        m.health = m.health.saturating_sub(damage);
        let health = m.health;
        let mut fl = Fields::default();
        fl.set(f::HEALTH, health);
        self.values(guid, fl);
        if health == 0 {
            self.kill(i, now);
        }
    }

    /// Casts finishing, missiles landing, damage over time ticking, power regenerating and the
    /// auto-cast picking its next spell.
    fn tick_spells(&mut self, now: Instant) {
        if self.player.dead_until.is_some() {
            self.player.casting = None;
            self.missiles.clear();
        }
        if self.player.casting.as_ref().is_some_and(|c| now >= c.done) {
            let c = self.player.casting.take().expect("checked");
            self.finish_cast(c.spell, c.target, now);
        }
        let (due, flying): (Vec<Missile>, Vec<Missile>) =
            self.missiles.drain(..).partition(|m| now >= m.at);
        self.missiles = flying;
        for m in due {
            self.spell_hit(m.target, m.spell, m.school, m.damage, m.crit, now);
        }
        let mut ticking = Vec::new();
        for mut d in std::mem::take(&mut self.dots) {
            if now >= d.next {
                self.spell_hit(d.target, d.spell, d.school, d.amount, false, now);
                d.ticks_left -= 1;
                d.next = now + d.every;
            }
            let alive = self.mob_index(d.target).is_some_and(|i| self.mobs[i].alive());
            if d.ticks_left > 0 && alive {
                ticking.push(d);
            }
        }
        self.dots = ticking;

        if now >= self.next_regen {
            self.next_regen = now + Duration::from_secs(1);
            let p = &mut self.player.power;
            let before = p.current;
            p.current = match p.kind {
                // Rage fades out of combat.
                1 if now > self.player.in_combat_until => p.current.saturating_sub(20),
                1 => p.current,
                // Energy: 10 a second.
                3 => (p.current + 10).min(p.max),
                // Mana: 4% of the pool a second, a survivor's pace.
                _ => (p.current + (p.max / 25).max(1)).min(p.max),
            };
            if p.current != before {
                self.send_power();
            }
        }

        if self.player.autocast && self.in_run() && self.player.casting.is_none() && now >= self.player.gcd_until {
            if let Some(id) = self.rotation_pick(now) {
                self.cast(id, self.player.target);
            }
        }
    }

    /// Rage from a blow dealt or taken, in wire tenths.
    fn gain_rage(&mut self, amount: u32) {
        if self.player.power.kind == 1 {
            let p = &mut self.player.power;
            p.current = (p.current + amount).min(p.max);
            self.send_power();
        }
    }

    pub(crate) fn toggle_autocast(&mut self) -> bool {
        self.player.autocast = !self.player.autocast;
        self.player.autocast
    }
}
