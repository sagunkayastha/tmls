//! "Finished while you were not looking": the ◆ mark.
//!
//! The listing only says what a session does now. [`Seen`] remembers what each did at the
//! host's previous snapshot, and turns an `Idle` that was `Running` (or `Waiting`) before into
//! `Unseen` until the session's tab is opened or the session works again. `Waiting` and `Error`
//! are never replaced: they still need the user.
//!
//! A session is known by its [`SessionKey`], so one renamed outside the app is a new session:
//! its ◆ is lost with the old name.
//!
//! **Unread (`•`)**: a session whose `activity` is newer than when the user last looked at it
//! (`looked`: its tab in front while the window has the focus). A session's first listing sets
//! what it was looked at; sessions outside tmux are never unread.

use std::collections::{HashMap, HashSet};

use crate::listing::{HostSnapshot, KITTY, SessionKey, Status};

#[derive(Debug, Default)]
pub struct Seen {
    /// What the listing said of each session at its host's last snapshot.
    last: HashMap<SessionKey, Status>,
    /// Sessions that finished and have not been opened since.
    unseen: HashSet<SessionKey>,
    /// Each session's `activity` at its last listing.
    activity: HashMap<SessionKey, u64>,
    /// The `activity` each session had when the user last looked at it.
    looked_at: HashMap<SessionKey, u64>,
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
            if let (Some(at), false) = (session.activity, session.key.host == KITTY) {
                self.activity.insert(session.key.clone(), at);
                // a first listing: what is there now was there before anyone could look
                self.looked_at.entry(session.key.clone()).or_insert(at);
            }
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

    /// The user is looking at the session (its tab is in front and the window has the focus):
    /// what it has shown so far is read.
    pub fn looked(&mut self, key: &SessionKey) {
        if let Some(at) = self.activity.get(key) {
            self.looked_at.insert(key.clone(), *at);
        }
    }

    /// The session has news the user has not looked at: the `•` after its name.
    pub fn unread(&self, key: &SessionKey) -> bool {
        matches!((self.activity.get(key), self.looked_at.get(key)), (Some(now), Some(looked)) if now > looked)
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
        self.activity.retain(|key, _| listed(key));
        self.looked_at.retain(|key, _| listed(key));
    }
}
