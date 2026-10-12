//! "Finished while you were not looking": the ◆ mark.
//!
//! The listing only says what a session does now. [`Seen`] remembers what each did at the
//! host's previous snapshot, and turns an `Idle` that was `Running` (or `Waiting`) before into
//! `Unseen` until the session's tab is opened or the session works again. `Waiting` and `Error`
//! are never replaced: they still need the user.
//!
//! A session is known by its [`SessionKey`], so one renamed outside the app is a new session:
//! its ◆ is lost with the old name.

use std::collections::{HashMap, HashSet};

use crate::listing::{HostSnapshot, KITTY, SessionKey, Status};

#[derive(Debug, Default)]
pub struct Seen {
    /// What the listing said of each session at its host's last snapshot.
    last: HashMap<SessionKey, Status>,
    /// Sessions that finished and have not been opened since.
    unseen: HashSet<SessionKey>,
}

impl Seen {
    pub fn new() -> Seen {
        Seen::default()
    }

    /// Call with every snapshot, as it comes from the poller: marks its unseen sessions.
    /// A session's first listing is never unseen (it finished before anyone could look).
    /// Applying a snapshot again is fine: its marks stay as they are until [`Seen::opened`].
    pub fn apply(&mut self, snap: &mut HostSnapshot) {
        for session in &mut snap.sessions {
            // a snapshot that went through here before says Unseen where the listing said Idle
            let listed = if session.status == Status::Unseen { Status::Idle } else { session.status };
            let before = self.last.insert(session.key.clone(), listed);
            if listed != Status::Idle {
                self.unseen.remove(&session.key); // working again, or it needs the user
                continue;
            }
            if matches!(before, Some(Status::Running | Status::Waiting)) {
                self.unseen.insert(session.key.clone());
            }
            session.status = if self.unseen.contains(&session.key) { Status::Unseen } else { Status::Idle };
        }
    }

    /// The user opened the session's tab: it is seen up to now.
    pub fn opened(&mut self, key: &SessionKey) {
        self.unseen.remove(key);
    }

    /// Forgets the sessions of `snap`'s host that it no longer lists. An offline host lists
    /// nothing and forgets nothing: its sessions are likely still there. For the listing of
    /// another host than this machine: see [`Seen::forget_missing_local`].
    pub fn forget_missing(&mut self, snap: &HostSnapshot) {
        self.forget(snap, false);
    }

    /// [`Seen::forget_missing`] for this machine's own listing, which is also the one that has
    /// its Claude sessions outside tmux (under the host [`KITTY`], not its own name): those it
    /// no longer lists are forgotten too. Only this machine's may come to [`Seen::apply`]:
    /// their key does not say which machine they are on.
    pub fn forget_missing_local(&mut self, snap: &HostSnapshot) {
        self.forget(snap, true);
    }

    fn forget(&mut self, snap: &HostSnapshot, outside_too: bool) {
        if !snap.online {
            return;
        }
        let of_snap = |key: &SessionKey| key.host == snap.host || (outside_too && key.host == KITTY);
        let listed = |key: &SessionKey| !of_snap(key) || snap.sessions.iter().any(|s| s.key == *key);
        self.last.retain(|key, _| listed(key));
        self.unseen.retain(listed);
    }
}
