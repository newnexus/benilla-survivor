//! Kill quests from the towns' quest givers.
//!
//! A giver speaks up by itself: walk within [`TALK_RANGE`] of one in town and its panel opens
//! (a finished quest's reward first, else its quest, else the list of them). The answer comes
//! back through the stock quest frames' own messages. Kills count during runs; quests are
//! handed in back in town for meta XP and money.

use std::collections::HashSet;

use benilla_protocol::messages::opcode;

use super::{dist2d, Game};
use crate::data::{Quest, NPC_QUESTGIVER};
use crate::packets::{f, quest, Fields};
use crate::quest_board::{self, Answer, Panel, PanelKind};

/// How close a quest giver must be to speak up, and how far to walk off before it will again.
const TALK_RANGE: f32 = 5.0;
const TALK_RESET: f32 = 9.0;
/// The quest log's slots (`MAX_QUEST_LOG_SIZE`).
const SLOTS: usize = 20;

/// A quest in the log, and its kills so far.
#[derive(Clone, Copy)]
struct Active {
    id: u32,
    counts: [u32; 4],
}

#[derive(Default)]
pub(crate) struct QuestLog {
    slots: [Option<Active>; SLOTS],
    done: HashSet<u32>,
}

impl Game {
    fn quest(&self, id: u32) -> Option<&Quest> {
        self.data.quests.iter().find(|q| q.id == id)
    }

    fn npc_entry(&self, guid: u64) -> Option<u32> {
        self.npcs.iter().find(|n| n.guid == guid).map(|n| n.entry)
    }

    fn slot_of(&self, id: u32) -> Option<usize> {
        self.quests.slots.iter().position(|s| s.is_some_and(|a| a.id == id))
    }

    fn finished(&self, a: &Active) -> bool {
        self.quest(a.id)
            .is_some_and(|q| q.kills.iter().zip(a.counts).all(|(&(_, need), c)| c >= need))
    }

    /// What `entry` offers now: not done, not in the log, its prerequisite handed in and its
    /// level reached.
    fn available(&self, entry: u32) -> Vec<&Quest> {
        self.data
            .quests
            .iter()
            .filter(|q| {
                q.giver == entry
                    && !self.quests.done.contains(&q.id)
                    && self.slot_of(q.id).is_none()
                    && (q.prev == 0 || self.quests.done.contains(&q.prev))
                    && q.min_level <= self.meta.level
            })
            .collect()
    }

    /// Finished quests `entry` takes back.
    fn rewardable(&self, entry: u32) -> Vec<&Quest> {
        self.quests
            .slots
            .iter()
            .flatten()
            .filter(|a| self.finished(a))
            .filter_map(|a| self.quest(a.id))
            .filter(|q| q.ender == entry)
            .collect()
    }

    fn giver_status(&self, entry: u32) -> u32 {
        if !self.rewardable(entry).is_empty() {
            quest::STATUS_REWARD
        } else if !self.available(entry).is_empty() {
            quest::STATUS_AVAILABLE
        } else if self.quests.slots.iter().flatten().any(|a| {
            self.quest(a.id).is_some_and(|q| q.ender == entry || q.giver == entry)
        }) {
            quest::STATUS_INCOMPLETE
        } else {
            quest::STATUS_NONE
        }
    }

    /// The `!` and `?` over every quest giver in view.
    pub(super) fn send_giver_statuses(&self) {
        for &i in &self.npcs_shown {
            let n = &self.npcs[i];
            if self.data.creatures[&n.entry].npc_flags & NPC_QUESTGIVER != 0 {
                self.send(
                    opcode::SMSG_QUESTGIVER_STATUS,
                    &quest::status(n.guid, self.giver_status(n.entry)),
                );
            }
        }
    }

    /// `CMSG_QUESTGIVER_STATUS_QUERY`.
    pub(crate) fn quest_status_query(&self, guid: u64) {
        if let Some(entry) = self.npc_entry(guid) {
            self.send(opcode::SMSG_QUESTGIVER_STATUS, &quest::status(guid, self.giver_status(entry)));
        }
    }

    /// Open `guid`'s panel, if it has something to say: a reward first, then its quests.
    pub(crate) fn quest_hello(&mut self, guid: u64) -> bool {
        let Some(entry) = self.npc_entry(guid) else {
            return false;
        };
        if let Some(q) = self.rewardable(entry).first().map(|q| q.id) {
            self.open_quest_panel(guid, q, PanelKind::Reward);
            return true;
        }
        match self.available(entry).as_slice() {
            [] => false,
            [q] => {
                let q = q.id;
                self.open_quest_panel(guid, q, PanelKind::Offer);
                true
            }
            many => {
                let rows: Vec<(&Quest, u32)> =
                    many.iter().map(|&q| (q, quest::STATUS_AVAILABLE)).collect();
                self.send(
                    opcode::SMSG_QUESTGIVER_QUEST_LIST,
                    &quest::list(guid, "", &rows),
                );
                true
            }
        }
    }

    /// In town, a quest giver within reach speaks up once, until the player walks off.
    pub(super) fn talk_nearby(&mut self, now: std::time::Instant) {
        if self.in_run()
            || self.player.dead_until.is_some()
            || self.paused()
            || !self.walked
            || self.armory_open.is_some()
        {
            return;
        }
        let me = self.player.pos;
        if let Some(g) = self.talked {
            let far = self
                .npcs
                .iter()
                .find(|n| n.guid == g)
                .is_none_or(|n| dist2d(n.here(now), me) > TALK_RESET);
            if far {
                self.talked = None;
                // Walked off: the window goes too.
                if self.open_panel.take().is_some() {
                    quest_board::clear();
                }
            }
            return;
        }
        let near = self
            .npcs
            .iter()
            .filter(|n| self.data.creatures[&n.entry].npc_flags & NPC_QUESTGIVER != 0)
            .map(|n| (dist2d(n.here(now), me), n.guid))
            .filter(|&(d, _)| d <= TALK_RANGE)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, guid)) = near {
            if self.quest_hello(guid) {
                self.talked = Some(guid);
            }
        }
    }

    /// `CMSG_QUESTGIVER_QUERY_QUEST`: a row of the list was picked.
    pub(crate) fn quest_query_quest(&mut self, guid: u64, id: u32) {
        let Some(entry) = self.npc_entry(guid) else { return };
        let offer = self.available(entry).iter().any(|q| q.id == id);
        let reward = self.rewardable(entry).iter().any(|q| q.id == id);
        if offer || reward {
            // The stock list closes; ours opens.
            self.send(opcode::SMSG_GOSSIP_COMPLETE, &[]);
            let kind = if offer { PanelKind::Offer } else { PanelKind::Reward };
            self.open_quest_panel(guid, id, kind);
        }
    }

    /// Put quest `id` of giver `guid` on the survivor's quest window.
    fn open_quest_panel(&mut self, guid: u64, id: u32, kind: PanelKind) {
        let Some(q) = self.quest(id).cloned() else { return };
        let text = match kind {
            PanelKind::Offer => &q.details,
            PanelKind::Reward => &q.offer_text,
        };
        let panel = Panel {
            id: 0,
            kind,
            title: q.title.clone(),
            text: self.fill(text),
            objectives: self.fill(&q.objectives),
            money: q.money,
            xp: q.xp,
        };
        let board_id = quest_board::post(panel);
        self.open_panel = Some((board_id, guid, id, kind));
    }

    /// The window's answer: Accept takes the quest (or hands it in), the X closes it.
    pub(super) fn take_panel_answer(&mut self) {
        let Some((id, answer)) = quest_board::take_answer() else { return };
        let Some((open, guid, quest, kind)) = self.open_panel else { return };
        if id != open {
            return;
        }
        self.open_panel = None;
        quest_board::clear();
        if answer == Answer::Close {
            return;
        }
        match kind {
            PanelKind::Offer => self.quest_accept(guid, quest),
            PanelKind::Reward => self.quest_turn_in(guid, quest),
        }
    }

    /// A quest text with WoW's macros filled in: `$N` the name, `$C` the class, `$R` the race,
    /// `$Gmale:female;` by gender, `$B` a line break.
    fn fill(&self, text: &str) -> String {
        let class = match self.player.class {
            1 => "Warrior",
            4 => "Rogue",
            5 => "Priest",
            8 => "Mage",
            9 => "Warlock",
            _ => "adventurer",
        };
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(at) = rest.find('$') {
            out.push_str(&rest[..at]);
            let tail = &rest[at + 1..];
            let mut chars = tail.chars();
            match chars.next() {
                Some('N' | 'n') => out.push_str(&self.name),
                Some('C' | 'c') => out.push_str(class),
                Some('R' | 'r') => out.push_str("Undead"),
                Some('B' | 'b') => out.push('\n'),
                Some('G' | 'g') => {
                    // `$Gmale:female;`
                    let body = &tail[1..];
                    if let Some(end) = body.find(';') {
                        let (m, f) = body[..end].split_once(':').unwrap_or((&body[..end], ""));
                        out.push_str(if self.gender == 0 { m } else { f });
                        rest = &body[end + 1..];
                        continue;
                    }
                }
                Some(other) => {
                    out.push('$');
                    out.push(other);
                }
                None => out.push('$'),
            }
            rest = &tail[tail.chars().next().map_or(0, char::len_utf8)..];
        }
        out.push_str(rest);
        out
    }

    /// `CMSG_QUESTGIVER_ACCEPT_QUEST`.
    pub(crate) fn quest_accept(&mut self, guid: u64, id: u32) {
        let Some(entry) = self.npc_entry(guid) else { return };
        if !self.available(entry).iter().any(|q| q.id == id) {
            return;
        }
        let Some(slot) = self.quests.slots.iter().position(Option::is_none) else {
            self.send(opcode::SMSG_QUESTLOG_FULL, &[]);
            return;
        };
        self.quests.slots[slot] = Some(Active { id, counts: [0; 4] });
        eprintln!("survivor: accepted quest {id} in slot {slot}");
        self.write_slot(slot);
        self.send(opcode::SMSG_GOSSIP_COMPLETE, &[]);
        self.send_giver_statuses();
        // The giver goes on to its next quest, if it has one.
        self.quest_hello(guid);
    }

    /// `CMSG_QUESTGIVER_COMPLETE_QUEST` and `_REQUEST_REWARD`: the reward panel of a finished
    /// quest.
    pub(crate) fn quest_reward_panel(&mut self, guid: u64, id: u32) {
        let Some(entry) = self.npc_entry(guid) else { return };
        if self.rewardable(entry).iter().any(|q| q.id == id) {
            self.send(opcode::SMSG_GOSSIP_COMPLETE, &[]);
            self.open_quest_panel(guid, id, PanelKind::Reward);
        }
    }

    /// `CMSG_QUESTGIVER_CHOOSE_REWARD`: hand the quest in for its XP and money.
    pub(crate) fn quest_turn_in(&mut self, guid: u64, id: u32) {
        let Some(entry) = self.npc_entry(guid) else { return };
        let Some(q) = self.rewardable(entry).into_iter().find(|q| q.id == id).cloned() else {
            return;
        };
        let Some(slot) = self.slot_of(id) else { return };
        self.quests.slots[slot] = None;
        self.quests.done.insert(id);
        self.write_slot(slot);
        self.money += q.money;
        let mut fl = Fields::default();
        fl.set(f::PLAYER_COINAGE, self.money);
        self.values(self.player.guid, fl);
        eprintln!("survivor: handed in quest {id} for {} xp", q.xp);
        self.send(opcode::SMSG_QUESTGIVER_QUEST_COMPLETE, &quest::complete(&q, q.xp));
        self.grant_meta_xp(q.xp);
        self.send_giver_statuses();
        self.save();
        // The next of the chain, or another finished quest, straight away.
        self.quest_hello(guid);
    }

    /// `CMSG_QUEST_QUERY`: the template behind the quest log.
    pub(crate) fn quest_query(&self, id: u32) {
        if let Some(q) = self.quest(id) {
            self.send(opcode::SMSG_QUEST_QUERY_RESPONSE, &quest::query_response(q));
        }
    }

    /// `CMSG_QUESTLOG_REMOVE_QUEST`: abandoned.
    pub(crate) fn quest_abandon(&mut self, slot: u8) {
        let slot = usize::from(slot);
        if slot < SLOTS && self.quests.slots[slot].take().is_some() {
            self.write_slot(slot);
            self.send_giver_statuses();
        }
    }

    /// A creature of `entry` killed: every quest wanting it counts one.
    pub(super) fn quest_kill(&mut self, entry: u32, guid: u64) {
        for slot in 0..SLOTS {
            let Some(mut a) = self.quests.slots[slot] else { continue };
            let Some(q) = self.quest(a.id) else { continue };
            let Some(k) = q.kills.iter().position(|&(e, _)| e == entry) else { continue };
            let need = q.kills[k].1;
            if a.counts[k] >= need {
                continue;
            }
            a.counts[k] += 1;
            self.quests.slots[slot] = Some(a);
            self.send(
                opcode::SMSG_QUESTUPDATE_ADD_KILL,
                &quest::add_kill(a.id, entry, a.counts[k], need, guid),
            );
            if self.finished(&a) {
                eprintln!("survivor: quest {} complete", a.id);
                self.send(opcode::SMSG_QUESTUPDATE_COMPLETE, &a.id.to_le_bytes());
            }
            self.write_slot(slot);
        }
    }

    /// The quests handed in, for the save file.
    pub(super) fn quests_done(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.quests.done.iter().copied().collect();
        v.sort_unstable();
        v
    }

    pub(super) fn set_quests_done(&mut self, ids: impl Iterator<Item = u32>) {
        self.quests.done.extend(ids);
    }

    /// `$SURVIVOR_QUESTS=364,380`: in the log from the start, for trying a quest's kills
    /// without walking to its giver.
    pub(super) fn seed_test_quests(&mut self) {
        let ids: Vec<u32> = std::env::var("SURVIVOR_QUESTS")
            .unwrap_or_default()
            .split(',')
            .filter_map(|v| v.trim().parse().ok())
            .filter(|&id| self.quest(id).is_some())
            .collect();
        for (slot, id) in ids.into_iter().take(SLOTS).enumerate() {
            self.quests.slots[slot] = Some(Active { id, counts: [0; 4] });
            self.write_slot(slot);
        }
    }

    /// The log slot's three fields as the client reads them.
    fn write_slot(&self, slot: usize) {
        let (id, state) = match self.quests.slots[slot] {
            Some(a) => {
                let counters = a
                    .counts
                    .iter()
                    .enumerate()
                    .fold(0u32, |acc, (i, &c)| acc | (c.min(63) << (6 * i)));
                let complete = if self.finished(&a) { 1u32 << 24 } else { 0 };
                (a.id, counters | complete)
            }
            None => (0, 0),
        };
        let base = f::PLAYER_QUEST_LOG_1_1 + 3 * slot as u16;
        let mut fl = Fields::default();
        fl.set(base, id).set(base + 1, state).set(base + 2, 0);
        self.values(self.player.guid, fl);
    }
}
