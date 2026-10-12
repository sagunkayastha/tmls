//! Messages for a Claude session that is busy (D6: in memory only, gone at quit, as in tmls):
//! kept here and sent one at a time, the oldest first, each when the session's turn ends —
//! its status goes from running or waiting to idle or done, by a listing or a pushed event
//! (tmls `app.py`: `queue`, `_deliver_queued`). A send that fails keeps its message for the
//! next turn end; one that is on its way is not started again.
//!
//! A message for an idle session with nothing before it goes at once (`goes_now`), but through
//! here too (`add`, then `start`): while it is on its way the next one waits, so two pastes
//! never mix in one prompt and the order is kept.
//!
//! Each send has an id (`SendId`) that its answer brings back to `sent`: the session may have
//! been renamed meanwhile (the id follows it) or killed or cleared (the id is forgotten, and
//! the answer then takes nothing from the queue).

use std::collections::{HashMap, VecDeque};

use client_core::{SessionKey, Status};

/// One send that `due` or `start` began; `sent` takes its answer.
pub type SendId = u64;

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

    /// Whether a new message for the session, in `status`, goes at once: it is not busy, none
    /// waits for it and none is on its way.
    pub fn goes_now(&self, key: &SessionKey, status: Option<Status>) -> bool {
        !Queue::busy(status) && self.count(key) == 0 && !self.sending.contains_key(key)
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
        let ended = Queue::busy(before) && matches!(after, Some(Status::Idle | Status::Unseen));
        if !ended {
            return None;
        }
        self.start(key)
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
