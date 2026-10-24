//! A profession trainer at home in Deathknell greets the player who walks up, instead of opening
//! a quest straight away: a line of their own, then what they offer as links. A quest link
//! opens the quest window on that quest; the shop and crafting are to come and show dimmed.
//! Out on a run a held trainer still opens their quest at once (see [`Game::talk_nearby`]).

use super::Game;
use crate::greeting::{self, Answer, Greeting, Link, Service};
use crate::quest_board::PanelKind;

impl Game {
    /// Whether `entry` is a profession trainer, who greets in town.
    pub(super) fn greets(&self, entry: u32) -> bool {
        crate::events::trainers().any(|t| t == entry)
    }

    /// `guid` greets the player: their line, their quests to hand in and to take, then the
    /// services every trainer will have.
    pub(super) fn greet(&mut self, guid: u64) -> bool {
        let Some(entry) = self.npc_entry(guid) else {
            return false;
        };
        let mut links: Vec<Link> = self
            .giver_quests(entry)
            .into_iter()
            .filter_map(|(id, kind)| {
                Some(Link {
                    service: Service::Quest {
                        id,
                        reward: kind == PanelKind::Reward,
                    },
                    label: self.quest(id)?.title.clone(),
                    soon: false,
                })
            })
            .collect();
        links.push(Link {
            service: Service::Shop,
            label: "Show me your wares.".to_owned(),
            soon: true,
        });
        links.push(Link {
            service: Service::Craft,
            label: "I'd like to make something.".to_owned(),
            soon: true,
        });
        let giver_name = self
            .data
            .creatures
            .get(&entry)
            .map_or_else(String::new, |t| t.name.clone());
        let id = greeting::post(Greeting {
            id: 0,
            giver: guid,
            giver_name,
            text: self.fill(crate::events::greeting(entry)),
            links,
        });
        self.greeting_open = Some((id, guid));
        true
    }

    /// A link picked: a quest opens the quest window on it (the giver's other quests beside it).
    pub(super) fn take_greeting_answer(&mut self) {
        let Some((id, answer)) = greeting::take_answer() else { return };
        let Some((open, guid)) = self.greeting_open else { return };
        if id != open {
            return;
        }
        self.close_greeting();
        let Answer::Pick(Service::Quest { id: quest, .. }) = answer else { return };
        let Some(entry) = self.npc_entry(guid) else { return };
        let mut list = self.giver_quests(entry);
        if let Some(k) = list.iter().position(|&(q, _)| q == quest) {
            let picked = list.remove(k);
            list.insert(0, picked);
            self.open_quest_panel(guid, &list);
        }
    }

    pub(super) fn close_greeting(&mut self) {
        if self.greeting_open.take().is_some() {
            greeting::clear();
        }
    }
}
