//! The other players of the party ([`crate::party`]) in our world: shown as they come, moved as
//! their clients move them, gone as they leave.

use benilla_protocol::messages::opcode;

use super::Game;
use crate::packets::{self, Update};
use crate::party::{self, Event};
use crate::wire::Body;

impl Game {
    /// Bring the others up to date: everyone already there at the first call, then each change.
    pub(super) fn sync_party(&mut self) {
        if !self.party_seen {
            self.party_seen = true;
            // What came before our world existed is in the roll call.
            let _ = party::take_events();
            for (guid, h) in party::everyone() {
                self.show_party_member(guid, &h);
            }
            return;
        }
        for e in party::take_events() {
            match e {
                Event::Here(guid, h) => self.show_party_member(guid, &h),
                Event::Moved(guid, op, info) => {
                    let mut b = Body::new();
                    b.packed_guid(guid);
                    b.0.extend_from_slice(&info);
                    self.send(op, &b.0);
                }
                Event::Gone(guid) => self.hide_party_member(guid),
            }
        }
    }

    fn show_party_member(&mut self, guid: u64, h: &party::Hello) {
        // Shown again from scratch: their character may have changed.
        self.hide_party_member(guid);
        self.send(
            opcode::SMSG_UPDATE_OBJECT,
            &packets::update_object(&[crate::world::party_create(guid, h)]),
        );
        self.party_shown.insert(guid);
    }

    fn hide_party_member(&mut self, guid: u64) {
        if self.party_shown.remove(&guid) {
            self.send(
                opcode::SMSG_UPDATE_OBJECT,
                &packets::update_object(&[Update::OutOfRange(vec![guid])]),
            );
        }
    }
}
