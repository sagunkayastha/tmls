//! Messages for a Claude session that is busy (D6: in memory only, gone at quit, as in tmls):
//! kept here and sent one at a time, the oldest first, each when the session's turn ends —
//! its status goes from running or waiting to idle or done, by a listing or a pushed event
//! (tmls `app.py`: `queue`, `_deliver_queued`). A send that fails keeps its message for the
//! next turn end; one that is on its way is not started again.
//!
//! A message for an idle session with nothing on its way goes at once (`goes_now`), but through
//! here too (`add`, then `start`): the head goes first, so one that waits still goes before it,
//! and while it is on its way the next one waits, so two pastes never mix in one prompt.
//!
//! Each send has an id (`SendId`) that its answer brings back to `sent`: the session may have
//! been renamed meanwhile (the id follows it) or killed or cleared (the id is forgotten, and
//! the answer then takes nothing from the queue).
//!
//! A message is typed only into Claude's own pane (`claude_pane`), as the listing has it at the
//! moment it goes: never into the session's active pane (`=NAME:`). When Claude has exited, the
//! session is the shell it left, which would run the text as a command; the message stays
//! queued (`Turn::Gone`) and the user is told.

use std::collections::{HashMap, VecDeque};

use client_core::tmux::valid_pane;
use client_core::{KITTY, SessionInfo, SessionKey, Status, Tool};

/// One send that `due` or `start` began; `sent` takes its answer.
pub type SendId = u64;

/// What to do with a session's queue now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Turn {
    /// Nothing goes now.
    Wait,
    /// Type `text` into Claude's `pane`; the answer goes to `sent` with `id`.
    Send {
        id: SendId,
        text: String,
        pane: String,
    },
    /// It would go now, but the session is no longer Claude's (or Claude's pane is not known):
    /// nothing is typed anywhere, and this many messages stay queued.
    Gone(usize),
}

/// Claude's own pane of the session, the only place a message is typed into: while the listing
/// has it as a Claude session in tmux with a valid pane id. `None` otherwise — a row that turned
/// into the shell Claude left (`Tool::Other`, no pane), or one the listing names Claude's by its
/// pane's command alone.
pub fn claude_pane(info: Option<&SessionInfo>) -> Option<&str> {
    let info = info.filter(|info| info.tool == Tool::Claude && info.key.host != KITTY)?;
    info.pane.as_deref().filter(|pane| valid_pane(pane))
}

/// What the status line says for `Turn::Gone`.
pub fn gone_note(name: &str, kept: usize) -> String {
    format!("Claude is gone from {name}; {kept} kept")
}

#[derive(Debug, Default)]
pub struct Queue {
    pending: HashMap<SessionKey, VecDeque<String>>,
    /// Sessions whose first message is being sent now: the send's id and its text.
    sending: HashMap<SessionKey, (SendId, String)>,
    last_id: SendId,
}

impl Queue {
    /// Whether a message for a session in `status` waits in the queue rather than going now.
    pub fn busy(status: Option<Status>) -> bool {
        matches!(status, Some(Status::Running | Status::Waiting))
    }

    /// Whether the session's queue starts with a new message for it, in `status`: it is not
    /// busy and none is on its way. Messages that already wait go first (`start` sends the
    /// head): a head left behind — its send failed while the session was idle, or its host was
    /// away when the turn ended — would otherwise wait for a turn end that never comes.
    pub fn goes_now(&self, key: &SessionKey, status: Option<Status>) -> bool {
        !Queue::busy(status) && !self.sending.contains_key(key)
    }

    /// Queues `text`; returns how many wait for the session now.
    pub fn add(&mut self, key: &SessionKey, text: String) -> usize {
        let queue = self.pending.entry(key.clone()).or_default();
        queue.push_back(text);
        queue.len()
    }

    pub fn count(&self, key: &SessionKey) -> usize {
        self.pending.get(key).map_or(0, VecDeque::len)
    }

    /// The sessions with messages waiting.
    pub fn keys(&self) -> impl Iterator<Item = &SessionKey> {
        self.pending.keys()
    }

    /// After a listing or an event moved the session from `before` to `after`: the message to
    /// send now, if its turn just ended and none is on its way. The caller sends it and reports
    /// with `sent`.
    pub fn due(
        &mut self,
        key: &SessionKey,
        before: Option<Status>,
        after: Option<Status>,
    ) -> Option<(SendId, String)> {
        if !Queue::ended(before, after) {
            return None;
        }
        self.start(key)
    }

    /// A session's turn ended: it was busy and is idle or done now.
    fn ended(before: Option<Status>, after: Option<Status>) -> bool {
        Queue::busy(before) && matches!(after, Some(Status::Idle | Status::Unseen))
    }

    /// `due`, for the session whose Claude pane is `pane` as the listing has it now
    /// (`claude_pane`): the head to send, or `Gone` when the turn ended but there is no Claude
    /// to send it to — then nothing is started and the queue is kept.
    pub fn turn_ended(
        &mut self,
        key: &SessionKey,
        before: Option<Status>,
        after: Option<Status>,
        pane: Option<&str>,
    ) -> Turn {
        if !Queue::ended(before, after) || self.count(key) == 0 || self.sending.contains_key(key) {
            return Turn::Wait;
        }
        let Some(pane) = pane else {
            return Turn::Gone(self.count(key));
        };
        self.send(key, pane)
    }

    /// The Ask form's message for the session, in `status`, whose Claude pane is `pane` now:
    /// queued, and sent at once when it `goes_now`. `Gone` when there is no Claude to send it
    /// to: it is kept all the same.
    pub fn submit(
        &mut self,
        key: &SessionKey,
        status: Option<Status>,
        pane: Option<&str>,
        text: String,
    ) -> Turn {
        let now = self.goes_now(key, status);
        let waiting = self.add(key, text);
        match (pane, now) {
            (None, _) => Turn::Gone(waiting),
            (Some(pane), true) => self.send(key, pane),
            (Some(_), false) => Turn::Wait,
        }
    }

    /// `start`, as a `Turn` for Claude's `pane`.
    fn send(&mut self, key: &SessionKey, pane: &str) -> Turn {
        match self.start(key) {
            Some((id, text)) => Turn::Send {
                id,
                text,
                pane: pane.to_string(),
            },
            None => Turn::Wait,
        }
    }

    /// The session's oldest message, to send now, unless one is on its way already.
    pub fn start(&mut self, key: &SessionKey) -> Option<(SendId, String)> {
        if self.sending.contains_key(key) {
            return None;
        }
        let head = self.pending.get(key)?.front()?.clone();
        self.last_id += 1;
        self.sending
            .insert(key.clone(), (self.last_id, head.clone()));
        Some((self.last_id, head))
    }

    /// The send `id` is over: when it worked its text leaves the queue. Returns the session
    /// it was for, under its name now; `None` when the session was killed or its queue
    /// cleared since.
    pub fn sent(&mut self, id: SendId, ok: bool) -> Option<SessionKey> {
        let key = self
            .sending
            .iter()
            .find(|(_, (sending, _))| *sending == id)
            .map(|(key, _)| key.clone())?;
        let (_, text) = self.sending.remove(&key)?;
        if ok {
            if let Some(queue) = self.pending.get_mut(&key)
                && queue.front() == Some(&text)
            {
                queue.pop_front();
            }
            if self.pending.get(&key).is_some_and(VecDeque::is_empty) {
                self.pending.remove(&key);
            }
        }
        Some(key)
    }

    /// Clear queue, in the form. A message on its way is not called back, but it is no longer
    /// the queue's: its answer takes nothing, and the session is free for the next one.
    pub fn clear(&mut self, key: &SessionKey) {
        self.pending.remove(key);
        self.sending.remove(key);
    }

    /// The session was renamed: its messages, and the one on its way, follow it.
    pub fn renamed(&mut self, old: &SessionKey, new: &SessionKey) {
        if let Some(queue) = self.pending.remove(old) {
            self.pending.entry(new.clone()).or_default().extend(queue);
        }
        if let Some(sending) = self.sending.remove(old) {
            self.sending.insert(new.clone(), sending);
        }
    }

    /// The session was killed: its messages go with it.
    pub fn forget(&mut self, key: &SessionKey) {
        self.clear(key);
    }
}
