//! Messages for a Claude session that is busy (D6: in memory only, gone at quit, as in tmls):
//! kept here and sent one at a time, the oldest first, each when the session's turn ends —
//! its status goes from running or waiting to idle or done, by a listing or a pushed event
//! (tmls `app.py`: `queue`, `_deliver_queued`). A send that fails keeps its message for the
//! next turn end; one that is on its way is not started again.

use std::collections::{HashMap, HashSet, VecDeque};

use client_core::{SessionKey, Status};

#[derive(Debug, Default)]
pub struct Queue {
    pending: HashMap<SessionKey, VecDeque<String>>,
    /// Sessions whose first message is being sent now.
    sending: HashSet<SessionKey>,
}

impl Queue {
    /// Whether a message for a session in `status` waits in the queue rather than going now.
    pub fn busy(status: Option<Status>) -> bool {
        matches!(status, Some(Status::Running | Status::Waiting))
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
    ) -> Option<String> {
        let ended = Queue::busy(before) && matches!(after, Some(Status::Idle | Status::Unseen));
        if !ended || self.sending.contains(key) {
            return None;
        }
        let head = self.pending.get(key)?.front()?.clone();
        self.sending.insert(key.clone());
        Some(head)
    }

    /// The send `due` started is over: when it worked, `text` leaves the queue.
    pub fn sent(&mut self, key: &SessionKey, text: &str, ok: bool) {
        self.sending.remove(key);
        if !ok {
            return;
        }
        if let Some(queue) = self.pending.get_mut(key)
            && queue.front().map(String::as_str) == Some(text)
        {
            queue.pop_front();
        }
        if self.pending.get(key).is_some_and(VecDeque::is_empty) {
            self.pending.remove(key);
        }
    }

    /// Clear queue, in the form.
    pub fn clear(&mut self, key: &SessionKey) {
        self.pending.remove(key);
    }

    /// The session was renamed: its messages follow it.
    pub fn renamed(&mut self, old: &SessionKey, new: &SessionKey) {
        if let Some(queue) = self.pending.remove(old) {
            self.pending.entry(new.clone()).or_default().extend(queue);
        }
        if self.sending.remove(old) {
            self.sending.insert(new.clone());
        }
    }

    /// The session was killed: its messages go with it.
    pub fn forget(&mut self, key: &SessionKey) {
        self.pending.remove(key);
        self.sending.remove(key);
    }
}
