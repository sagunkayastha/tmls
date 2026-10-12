//! The session list's model: what tmls shows in its sidebar, without the drawing.
//!
//! One section per host, in the configuration's order (this machine first): a `Host` header,
//! then the host's sessions — the pinned ones first, the rest as the listing has them — and a
//! `Gap` between sections. A host has its header from the start; one that is offline keeps it
//! and loses its sessions.
//!
//! **Claude outside tmux.** This machine's Claude sessions that run in no tmux pane (the
//! listing has them under the host `KITTY`) get a section of their own right under this
//! machine's tmux sessions, for as long as there is one: a rule (`Row::Host` of `KITTY`),
//! then the sessions. They have a status and can be selected, but there is nothing to attach,
//! pin, rename or kill: Enter or a click brings their terminal window to the front (the
//! app's part). Those of other hosts are not shown, as in tmls.
//!
//! **Selection is by identity.** The cursor is on a *session*, not on a row number: when a
//! listing moves rows (a session started above, one renamed or killed elsewhere, a host gone
//! offline) the cursor stays on its session. When that session itself is gone, the cursor goes
//! to the row that took its place on the same host, else to the nearest session; never to a
//! header or a gap. With no session at all nothing is selected. A host that does not answer
//! loses its rows, but its sessions are likely still there: when the cursor was on one of
//! them it is remembered and goes back to it when that session is listed again — unless the
//! user moved the cursor in the meantime, or the host came back without the session.
//!
//! **Lines.** A row is one line of the list, a session with a summary (`summary`) two; the
//! view and the hit test count in lines (`view`, `row_at`). The view follows the cursor
//! whenever the cursor moves; the wheel (`scroll`) moves the view alone.
//!
//! **Pins** are tmls's: slugs in `~/.config/tmls/pins`, the one file this app writes
//! (`toggle_pin`). Slugs of sessions that are not listed now are kept. A pin that could not be
//! written is not shown as one: the ★ means "saved" (`take_pin_error` says what went wrong).

use std::cell::Cell;
use std::collections::{BTreeSet, HashMap};
use std::ops::Range;
use std::path::PathBuf;

use client_core::config::tmls_slug;
use client_core::{
    Config, HostColor, HostSnapshot, KITTY, Seen, SessionInfo, SessionKey, Status, default_color,
};

/// One row of the list.
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    Host {
        host: String,
        online: bool,
        color: HostColor,
    },
    Session(SessionInfo),
    Gap,
}

impl Row {
    /// Lines of the list the row takes.
    pub fn lines(&self) -> usize {
        match self {
            Row::Session(info) if summary(info).is_some() => 2,
            _ => 1,
        }
    }

    fn key(&self) -> Option<&SessionKey> {
        match self {
            Row::Session(info) => Some(&info.key),
            _ => None,
        }
    }
}

/// What the list asks the window to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Open the session's tab, or bring it to the front.
    Attach(SessionKey),
    /// The new-session form, for this host.
    NewSessionOn(String),
    /// The session's menu (rename, kill, …).
    Menu(SessionKey),
}

/// The dim line under a session's name, and whether it is what Claude waits for (tmls
/// `pane_row_text`): the reason while it waits, else Claude's last reply (pushed), else its
/// title when that is not the name.
pub fn summary(info: &SessionInfo) -> Option<(&str, bool)> {
    fn text(text: &Option<String>) -> Option<&str> {
        text.as_deref().map(str::trim).filter(|t| !t.is_empty())
    }
    if info.status == Status::Waiting
        && let Some(reason) = text(&info.reason)
    {
        return Some((reason, true));
    }
    if let Some(reply) = text(&info.last_reply) {
        return Some((reply, false));
    }
    text(&info.title)
        .filter(|title| *title != info.key.name)
        .map(|title| (title, false))
}

/// A session renamed outside the app (`renamed_outside`): its ★ and its queued messages take
/// the new name, whether it has a tab or not (the tab is the app's to follow). Whether either
/// moved; a ★ that could not be moved is in `take_pin_error`.
pub fn follow_outside_rename(
    sidebar: &mut Sidebar,
    queue: &mut crate::queue::Queue,
    old: &SessionKey,
    new: &SessionKey,
) -> bool {
    let pinned = sidebar.renamed(old, new);
    let queued = queue.count(old) > 0;
    queue.renamed(old, new);
    pinned || queued
}

/// The sessions of `snap`'s host renamed outside the app since `before` (that host's rows as
/// the list had them): a name the listing no longer has whose Claude conversation
/// (`session_id`) it lists under a name the list did not have. Pairs (old, new). Only a
/// rename nothing else could explain is followed: one name gone and one name new with that
/// id. Sessions without a Claude id are not followed, nor are two gone or two new with one id
/// (a conversation resumed in two sessions): nothing says which new name is whose.
pub fn renamed_outside(
    before: &[SessionInfo],
    snap: &HostSnapshot,
) -> Vec<(SessionKey, SessionKey)> {
    if !snap.online {
        return Vec::new();
    }
    let ours = |s: &&SessionInfo| s.key.host == snap.host;
    let listed = |key: &SessionKey| snap.sessions.iter().any(|s| &s.key == key);
    let had = |key: &SessionKey| before.iter().any(|s| &s.key == key);
    // The names gone since `before`, and the names new in `snap`, that carry this id.
    let gone = |id: &String| {
        before
            .iter()
            .filter(ours)
            .filter(|s| s.session_id.as_ref() == Some(id) && !listed(&s.key))
            .count()
    };
    let new = |id: &String| -> Vec<&SessionInfo> {
        snap.sessions
            .iter()
            .filter(ours)
            .filter(|s| s.session_id.as_ref() == Some(id) && !had(&s.key))
            .collect()
    };
    before
        .iter()
        .filter(ours)
        .filter(|old| !listed(&old.key))
        .filter_map(|old| {
            let id = old.session_id.as_ref()?;
            match new(id)[..] {
                [new] if gone(id) == 1 => Some((old.key.clone(), new.key.clone())),
                _ => None,
            }
        })
        .collect()
}

#[derive(Debug)]
pub struct Sidebar {
    local_name: String,
    /// Where `pins` is written (`~/.config/tmls`).
    config_dir: PathBuf,
    /// Every pinned session's tmls slug, listed now or not.
    pins: BTreeSet<String>,
    rows: Vec<Row>,
    /// Of every session in `rows`: its place in its host's listing, and whether it is pinned.
    listed: HashMap<SessionKey, (usize, bool)>,
    /// The row of the selected session.
    cursor: Option<usize>,
    /// Why the last `toggle_pin` changed nothing, until `take_pin_error` is asked.
    pin_error: Option<anyhow::Error>,
    /// The session the cursor was on when its host stopped answering, until the host is back
    /// or the user moves the cursor.
    wanted: Option<SessionKey>,
    /// The first row in view. `view` settles it for the height it is given.
    top: Cell<usize>,
    /// The view has to show the cursor: it moved after the wheel last did.
    follow: Cell<bool>,
}

impl Sidebar {
    /// The list for `cfg`'s hosts, each with its header and no session yet.
    pub fn new(cfg: &Config, config_dir: PathBuf) -> Sidebar {
        let mut rows = Vec::new();
        for host in &cfg.hosts {
            if !rows.is_empty() {
                rows.push(Row::Gap);
            }
            let color = cfg.host_colors.get(host).copied();
            rows.push(Row::Host {
                host: host.clone(),
                // Not known to be offline until its first listing says so.
                online: true,
                color: color.unwrap_or_else(|| default_color(host)),
            });
        }
        Sidebar {
            local_name: cfg.local_name.clone(),
            config_dir,
            pins: cfg.pins.clone(),
            rows,
            listed: HashMap::new(),
            cursor: None,
            pin_error: None,
            wanted: None,
            top: Cell::new(0),
            follow: Cell::new(true),
        }
    }

    /// A host's new listing replaces that host's rows; the selection stays on its session
    /// (see the module comment). A host that is not in the configuration is ignored. This
    /// machine's listing also sets the section of its Claude sessions outside tmux.
    pub fn apply(&mut self, mut snap: HostSnapshot) {
        if snap.host == KITTY {
            return;
        }
        let (outside, tmux) = std::mem::take(&mut snap.sessions)
            .into_iter()
            .partition(|info| info.key.host == KITTY);
        snap.sessions = tmux;
        let local = snap.host == self.local_name && self.section(&snap.host).is_some();
        let online = snap.online;
        self.apply_section(snap);
        if local {
            self.apply_outside(if online { outside } else { Vec::new() });
        }
    }

    /// Makes the section of Claude sessions outside tmux what `outside` says: there with a
    /// rule above it when there is one, gone otherwise.
    fn apply_outside(&mut self, outside: Vec<SessionInfo>) {
        let selected = self.selected().cloned();
        let mut at = self.cursor;
        match (self.section(KITTY), outside.is_empty()) {
            (None, true) => return,
            (Some(section), true) => {
                // A cursor in the section goes to the session above it.
                if at.is_some_and(|at| section.contains(&at)) {
                    at = Some(section.start.saturating_sub(1));
                }
                self.rows.drain(section);
                self.listed.retain(|key, _| key.host != KITTY);
            }
            (None, false) => {
                let Some(local) = self.section(&self.local_name) else {
                    return;
                };
                self.rows.insert(
                    local.end,
                    Row::Host {
                        host: KITTY.to_string(),
                        online: true,
                        color: default_color(KITTY),
                    },
                );
            }
            (Some(_), false) => {}
        }
        // The rows moved: the cursor goes back to its session, else to the nearest one.
        self.cursor = match (selected, at) {
            (Some(key), Some(at)) => self.position(&key).or_else(|| self.nearest_session(at)),
            _ => self.cursor.and_then(|at| self.nearest_session(at)),
        };
        if !outside.is_empty() {
            self.apply_section(HostSnapshot {
                host: KITTY.to_string(),
                online: true,
                error: None,
                sessions: outside,
            });
        }
    }

    fn apply_section(&mut self, snap: HostSnapshot) {
        let Some(section) = self.section(&snap.host) else {
            return;
        };
        let before = self.cursor.map(|at| (at, self.rows[at].key().cloned()));
        if let Row::Host { online, .. } = &mut self.rows[section.start] {
            *online = snap.online;
        }
        let sessions = if snap.online {
            snap.sessions
        } else {
            Vec::new()
        };
        self.listed.retain(|key, _| key.host != snap.host);
        for (place, info) in sessions.iter().enumerate() {
            let pinned = self.pins.contains(&tmls_slug(&info.key, &self.local_name));
            self.listed.insert(info.key.clone(), (place, pinned));
        }
        let first = section.start + 1;
        let count = sessions.len();
        self.rows
            .splice(first..section.end, sessions.into_iter().map(Row::Session));
        self.sort(first..first + count);

        // The host is back: the cursor returns to the session it was on, or that one is gone.
        let back = match &self.wanted {
            Some(wanted) if wanted.host == snap.host && snap.online => {
                let at = self.position(wanted);
                self.wanted = None;
                at
            }
            _ => None,
        };
        let kept = back.or_else(|| {
            before
                .as_ref()
                .and_then(|(_, key)| self.position(key.as_ref()?))
        });
        if !snap.online
            && self.wanted.is_none()
            && kept.is_none()
            && let Some((at, Some(key))) = &before
            && section.contains(at)
        {
            self.wanted = Some(key.clone());
        }
        self.cursor = kept.or_else(|| match before {
            // Its session went with this listing: the row that took its place, else the last
            // of the host.
            Some((at, _)) if count > 0 && section.contains(&at) => {
                Some(at.clamp(first, first + count - 1))
            }
            Some((at, _)) => self.nearest_session(at),
            None => self.nearest_session(0),
        });
        if kept.is_none() || back.is_some() {
            self.follow.set(true);
        }
    }

    /// `Seen::apply`, then `apply`: marks what finished unwatched (◆) and shows the listing.
    /// `front` is the session of the tab in front: the user watched that one finish, so it
    /// gets no mark. Snapshots must come here in the order the poller delivers them.
    ///
    /// Claude sessions outside tmux are this machine's only: every host's listing has its
    /// own, all under the host `KITTY`, so another host's are dropped before `seen` could
    /// take them for this machine's of the same name (one's work ending the other's: a false
    /// ◆). This machine's listing is then the one that says which of them are gone.
    pub fn deliver(&mut self, seen: &mut Seen, mut snap: HostSnapshot, front: Option<&SessionKey>) {
        let local = snap.host == self.local_name;
        if !local {
            snap.sessions.retain(|info| info.key.host != KITTY);
        }
        seen.apply(&mut snap);
        if let Some(front) = front.filter(|front| front.host == snap.host) {
            seen.opened(front);
            for info in snap.sessions.iter_mut().filter(|info| info.key == *front) {
                if info.status == Status::Unseen {
                    info.status = Status::Idle;
                }
            }
        }
        match local {
            true => seen.forget_missing_local(&snap),
            false => seen.forget_missing(&snap),
        }
        self.apply(snap);
    }

    /// The session's tab came to the front: its ◆ is gone, here and in `seen`. Returns whether
    /// the list changed.
    pub fn opened(&mut self, seen: &mut Seen, key: &SessionKey) -> bool {
        seen.opened(key);
        let unseen = self.rows.iter_mut().find_map(|row| match row {
            Row::Session(info) if info.key == *key && info.status == Status::Unseen => Some(info),
            _ => None,
        });
        unseen.map(|info| info.status = Status::Idle).is_some()
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The session the cursor is on.
    pub fn selected(&self) -> Option<&SessionKey> {
        self.rows.get(self.cursor?)?.key()
    }

    /// What the listing last said of a session; `None` when it is not listed.
    pub fn status(&self, key: &SessionKey) -> Option<Status> {
        self.rows.iter().find_map(|row| match row {
            Row::Session(info) if info.key == *key => Some(info.status),
            _ => None,
        })
    }

    /// The session as the list shows it, marks included; `None` when it is not listed.
    pub fn info(&self, key: &SessionKey) -> Option<&SessionInfo> {
        self.rows.iter().find_map(|row| match row {
            Row::Session(info) if info.key == *key => Some(info),
            _ => None,
        })
    }

    /// Claude's process, for a listed session outside tmux.
    pub fn pid(&self, key: &SessionKey) -> Option<u32> {
        self.rows.iter().find_map(|row| match row {
            Row::Session(info) if info.key == *key => info.pid,
            _ => None,
        })
    }

    pub fn is_pinned(&self, key: &SessionKey) -> bool {
        self.listed.get(key).is_some_and(|(_, pinned)| *pinned)
    }

    /// The session of a row; `None` for a header, a gap and a row that is not there.
    pub fn key_at(&self, row: usize) -> Option<&SessionKey> {
        self.rows.get(row)?.key()
    }

    /// Moves the cursor `delta` sessions down (up when negative), over headers and gaps,
    /// stopping at the first and the last session.
    pub fn move_by(&mut self, delta: i32) {
        let Some(mut at) = self.cursor.or_else(|| self.nearest_session(0)) else {
            return;
        };
        for _ in 0..delta.unsigned_abs() {
            let next = if delta > 0 {
                (at + 1..self.rows.len()).find(|&row| self.rows[row].key().is_some())
            } else {
                (0..at).rev().find(|&row| self.rows[row].key().is_some())
            };
            match next {
                Some(next) => at = next,
                None => break,
            }
        }
        self.set_cursor(at);
    }

    /// Puts the cursor on a session; nothing happens when it is not listed.
    pub fn select_key(&mut self, key: &SessionKey) {
        if let Some(at) = self.position(key) {
            self.set_cursor(at);
        }
    }

    /// A click on a row: a session is selected and attached; an online host's header (its
    /// `+`) asks for a new session there.
    pub fn click(&mut self, row: usize) -> Option<Action> {
        match self.rows.get(row)? {
            Row::Session(info) => {
                let key = info.key.clone();
                self.set_cursor(row);
                Some(Action::Attach(key))
            }
            // The rule above the sessions outside tmux is no host.
            Row::Host {
                host, online: true, ..
            } if host != KITTY => Some(Action::NewSessionOn(host.clone())),
            Row::Host { .. } | Row::Gap => None,
        }
    }

    /// A right-click on a row: a session is selected and its menu asked for.
    /// A session outside tmux has none.
    pub fn menu(&mut self, row: usize) -> Option<Action> {
        let key = self.key_at(row)?.clone();
        if key.host == KITTY {
            return None;
        }
        self.set_cursor(row);
        Some(Action::Menu(key))
    }

    /// Enter: attach the selected session.
    pub fn activate(&self) -> Option<Action> {
        self.selected().cloned().map(Action::Attach)
    }

    /// Pins the selected session, or unpins it, and writes tmls's `pins` file. Returns whether
    /// it is pinned now. When the file cannot be written nothing changes — the session is as
    /// pinned as it was, in the list and after a restart — and `take_pin_error` has the reason.
    pub fn toggle_pin(&mut self) -> bool {
        let Some(key) = self.selected().cloned() else {
            return false;
        };
        if key.host == KITTY {
            // Nothing to attach: it is in no ring of sessions to walk.
            return false;
        }
        let slug = tmls_slug(&key, &self.local_name);
        let mut pins = self.pins.clone();
        let pinned = !pins.remove(&slug);
        if pinned {
            pins.insert(slug);
        }
        if let Err(e) = Config::save_pins(&self.config_dir, &pins) {
            let what = if pinned { "pin" } else { "unpin" };
            self.pin_error = Some(e.context(format!("{what} of {} not saved", key.name)));
            return !pinned;
        }
        self.pins = pins;
        if let Some(entry) = self.listed.get_mut(&key) {
            entry.1 = pinned;
        }
        if let Some(section) = self.section(&key.host) {
            self.sort(section.start + 1..section.end);
        }
        self.select_key(&key);
        pinned
    }

    /// A session was renamed here: its pin goes to the new name (tmls does the same), in the
    /// `pins` file too. Returns whether there was a pin to move. When the file cannot be
    /// written the old name stays pinned and `take_pin_error` has the reason.
    pub fn renamed(&mut self, old: &SessionKey, new: &SessionKey) -> bool {
        let mut pins = self.pins.clone();
        if !pins.remove(&tmls_slug(old, &self.local_name)) {
            return false;
        }
        pins.insert(tmls_slug(new, &self.local_name));
        if let Err(e) = Config::save_pins(&self.config_dir, &pins) {
            self.pin_error = Some(e.context(format!("pin of {} not moved", new.name)));
            return false;
        }
        // The row still has the old name and its ★: the next listing brings the new one, pinned.
        self.pins = pins;
        true
    }

    /// What went wrong at the last `toggle_pin` or `renamed`, once; `Ok` when nothing did.
    pub fn take_pin_error(&mut self) -> anyhow::Result<()> {
        self.pin_error.take().map_or(Ok(()), Err)
    }

    /// The next session that needs the user after `after` in the list, wrapping round: one
    /// that waits (?), and only when no other does, one that finished unwatched (◆). `after`
    /// itself is never the answer.
    pub fn next_waiting(&self, after: Option<&SessionKey>) -> Option<SessionKey> {
        let count = self.rows.len();
        let from = after
            .and_then(|key| self.position(key))
            .map_or(0, |at| at + 1);
        let next = |status: Status| {
            (0..count)
                .filter_map(|step| match &self.rows[(from + step) % count] {
                    Row::Session(info) => Some(info),
                    _ => None,
                })
                .find(|info| info.status == status && Some(&info.key) != after)
                .map(|info| info.key.clone())
        };
        next(Status::Waiting).or_else(|| next(Status::Unseen))
    }

    /// The next session after `after` with news the user has not looked at (`unread`), wrapping
    /// round: where Alt+Shift+N goes when nothing waits (tmls visits its "done" ones too).
    /// Never `after` itself, never a session outside tmux.
    pub fn next_unread(
        &self,
        after: Option<&SessionKey>,
        unread: &dyn Fn(&SessionKey) -> bool,
    ) -> Option<SessionKey> {
        let count = self.rows.len();
        let from = after
            .and_then(|key| self.position(key))
            .map_or(0, |at| at + 1);
        (0..count)
            .filter_map(|step| match &self.rows[(from + step) % count] {
                Row::Session(info) => Some(&info.key),
                _ => None,
            })
            .find(|key| Some(*key) != after && key.host != KITTY && unread(key))
            .cloned()
    }

    /// `step` places on from `from` in the pinned (★) sessions listed now, in the list's order,
    /// wrapping round; the first of them when `from` is not pinned. `None` when no listed
    /// session is pinned. No key uses it now: the tab keys walk the open tabs.
    pub fn pin_step(&self, from: Option<&SessionKey>, step: i32) -> Option<SessionKey> {
        self.ring_step(from, step, |key| self.is_pinned(key))
    }

    /// The same among all the sessions listed now: where the tab keys go while fewer than two
    /// tabs are open. `None` with an empty list.
    pub fn list_step(&self, from: Option<&SessionKey>, step: i32) -> Option<SessionKey> {
        self.ring_step(from, step, |_| true)
    }

    fn ring_step(
        &self,
        from: Option<&SessionKey>,
        step: i32,
        in_ring: impl Fn(&SessionKey) -> bool,
    ) -> Option<SessionKey> {
        let ring: Vec<&SessionKey> = self
            .rows
            .iter()
            .filter_map(Row::key)
            .filter(|key| key.host != KITTY && in_ring(key))
            .collect();
        let at = match from.and_then(|key| ring.iter().position(|member| *member == key)) {
            Some(at) => (at as i64 + step as i64).rem_euclid(ring.len() as i64) as usize,
            None => 0,
        };
        ring.get(at).map(|key| (*key).clone())
    }

    /// The wheel: moves the view `rows` rows down (up when negative), not the cursor.
    pub fn scroll(&mut self, rows: i32) {
        let last = self.rows.len().saturating_sub(1) as i64;
        let top = (self.top.get() as i64 + rows as i64).clamp(0, last);
        self.top.set(top as usize);
        self.follow.set(false);
    }

    /// The rows in view when `height` lines fit: only whole rows, the last row never above
    /// the bottom while rows are hidden above, and the cursor's row among them unless the
    /// wheel moved the view since the cursor last moved.
    pub fn view(&self, height: usize) -> Range<usize> {
        let count = self.rows.len();
        if height == 0 {
            return 0..0;
        }
        // The first row from which everything to the end fits.
        let mut fits_from = count;
        let mut lines = 0;
        while fits_from > 0 && lines + self.rows[fits_from - 1].lines() <= height {
            fits_from -= 1;
            lines += self.rows[fits_from].lines();
        }
        let mut top = self.top.get().min(fits_from);
        if self.follow.get()
            && let Some(cursor) = self.cursor
        {
            if cursor < top {
                top = cursor;
                // The header of the cursor's host comes along when it is the row above.
                if top > 0 && matches!(self.rows[top - 1], Row::Host { .. }) {
                    top -= 1;
                }
            }
            let lines =
                |from: usize| -> usize { self.rows[from..=cursor].iter().map(Row::lines).sum() };
            while top < cursor && lines(top) > height {
                top += 1;
            }
        }
        self.top.set(top);
        let mut end = top;
        let mut lines = 0;
        while end < count && lines + self.rows[end].lines() <= height {
            lines += self.rows[end].lines();
            end += 1;
        }
        top..end
    }

    /// The row at a line of the list (0 is the first line drawn) when `height` lines fit.
    pub fn row_at(&self, line: usize, height: usize) -> Option<usize> {
        let mut first = 0;
        self.view(height).find(|&row| {
            first += self.rows[row].lines();
            line < first
        })
    }

    /// The user (or the tab in front) puts the cursor somewhere: that is where it stays.
    fn set_cursor(&mut self, row: usize) {
        self.wanted = None;
        self.cursor = Some(row);
        self.follow.set(true);
    }

    fn position(&self, key: &SessionKey) -> Option<usize> {
        self.rows.iter().position(|row| row.key() == Some(key))
    }

    /// A host's rows: its header and its sessions.
    fn section(&self, host: &str) -> Option<Range<usize>> {
        let header = self
            .rows
            .iter()
            .position(|row| matches!(row, Row::Host { host: h, .. } if h == host))?;
        let sessions = self.rows[header + 1..]
            .iter()
            .take_while(|row| row.key().is_some())
            .count();
        Some(header..header + 1 + sessions)
    }

    /// Orders one host's session rows: pinned first, then as listed.
    fn sort(&mut self, sessions: Range<usize>) {
        let listed = &self.listed;
        self.rows[sessions].sort_by_key(|row| {
            let (place, pinned) = row
                .key()
                .and_then(|key| listed.get(key))
                .copied()
                .unwrap_or((usize::MAX, false));
            (!pinned, place)
        });
    }

    /// The session row nearest to row `at`; the one above when two are as near.
    fn nearest_session(&self, at: usize) -> Option<usize> {
        (0..self.rows.len())
            .filter(|&row| self.rows[row].key().is_some())
            .min_by_key(|&row| (row.abs_diff(at), row))
    }
}
