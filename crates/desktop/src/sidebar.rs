//! The session list's model: what tmls shows in its sidebar, without the drawing.
//!
//! One section per host, in the configuration's order (this machine first): a `Host` header,
//! then the host's sessions — the pinned ones first, the rest as the listing has them — and a
//! `Gap` between sections. A host has its header from the start.
//!
//! **A host that stops answering** keeps its rows as its last good listing had them: they are
//! *stale* (`stale`), drawn dimmed, and the app attaches to none of them and acts on none
//! (`offline_note` says why, with the age of the rows and the last error). Its header says
//! `offline`. The next listing that is an answer replaces them as any listing does.
//!
//! **Claude outside tmux.** This machine's Claude sessions that run in no tmux pane (the
//! listing has them under the host `KITTY`) get a section of their own right under this
//! machine's tmux sessions, for as long as there is one: a rule (`Row::Host` of `KITTY`),
//! then the sessions. They have a status and can be selected, but there is nothing to attach,
//! pin, rename or kill: Enter or a click brings their terminal window to the front (the
//! app's part). Those of other hosts are not shown, as in tmls.
//!
//! **Other tmux servers** (`tmux -L NAME`, spec G5): under each host's sessions (and under
//! this machine's sessions outside tmux) a row `background ▸ · N` when the host has any; `b`
//! or a click on it shows them all (`toggle_background`). Their sessions attach and do nothing
//! else: no menu, no pin (R4).
//!
//! **Selection is by identity.** The cursor is on a *session*, not on a row number: when a
//! listing moves rows (a session started above, one renamed or killed elsewhere, a host gone
//! offline) the cursor stays on its session. When that session itself is gone, the cursor goes
//! to the row that took its place on the same host, else to the nearest session; never to a
//! header or a gap. With no session at all nothing is selected. A host that does not answer
//! keeps its rows, so the cursor stays where it is.
//!
//! **Folded hosts.** A host's section can be folded (`toggle_fold`, `z`, a click on its
//! header): its sessions and its `background` row stay in `rows` — what the list knows of
//! them does not change — but are hidden (`hidden`): they take no line, the cursor never
//! lands on one, the view and the hit test skip them. Putting the cursor on a hidden session
//! (`select_key`: its tab came to the front, `n`, the go-to picker) unfolds its host. The
//! rule above the sessions outside tmux folds them alone.
//!
//! **Lines.** A row is one line of the list, a session with a summary (`summary`) two; the
//! view and the hit test count in lines (`view`, `row_at`); a hidden row has none. The view follows the cursor
//! whenever the cursor moves; the wheel (`scroll`) moves the view alone.
//!
//! **Pins** are tmls's: slugs in `~/.config/tmls/pins`, the one file this app writes
//! (`toggle_pin`). Slugs of sessions that are not listed now are kept. A pin that could not be
//! written is not shown as one: the ★ means "saved" (`take_pin_error` says what went wrong).

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use client_core::config::tmls_slug;
use client_core::listing::split_host;
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
    /// The sessions of `host`'s other tmux servers: `count` of them, listed under it while
    /// `open`.
    Background {
        host: String,
        count: usize,
        open: bool,
    },
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
    /// Show or hide the other tmux servers' sessions.
    ToggleBackground,
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

/// The status line while the pointer is over a session's row (tmls: the row's tooltip): the
/// whole of `summary`, which the list cuts off at its width, after the session's name — and
/// for a session that waits, that it is what it waits for. `None` when the row says it all.
pub fn hover_text(info: &SessionInfo) -> Option<String> {
    let (text, waits) = summary(info)?;
    let name = &info.key.name;
    Some(match waits {
        true => format!("{name} waits for you: {text}"),
        false => format!("{name}: {text}"),
    })
}

/// The host a session's row is under: its machine for a session of another tmux server, the
/// rule's `KITTY` for one outside tmux.
fn owner(key: &SessionKey) -> &str {
    split_host(&key.host).0
}

/// An age in a few columns: `40s`, `5m`, `3h`, `2d`.
pub fn age_text(age: Duration) -> String {
    match age.as_secs() {
        secs @ 0..60 => format!("{secs}s"),
        secs @ 60..3600 => format!("{}m", secs / 60),
        secs @ 3600..86_400 => format!("{}h", secs / 3600),
        secs => format!("{}d", secs / 86_400),
    }
}

/// Why nothing but attaching is done to a session of another tmux server (R4), for the status
/// line; `None` for every other session.
pub fn attach_only(key: &SessionKey) -> Option<String> {
    let server = split_host(&key.host).1?;
    Some(format!("{} is on tmux -L {server}: attach only", key.name))
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
    /// Each host's sessions on its other tmux servers, as the last scan found them.
    background: BTreeMap<String, Vec<SessionInfo>>,
    /// They are listed under their `background` row (`b`).
    background_open: bool,
    /// The row of the selected session.
    cursor: Option<usize>,
    /// Why the last `toggle_pin` changed nothing, until `take_pin_error` is asked.
    pin_error: Option<anyhow::Error>,
    /// When each host last answered with a listing.
    answered: HashMap<String, Instant>,
    /// Why each host that does not answer did not, when that is known.
    offline_error: HashMap<String, Option<String>>,
    /// The hosts whose sections are folded.
    folded: BTreeSet<String>,
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
            background: BTreeMap::new(),
            background_open: false,
            cursor: None,
            pin_error: None,
            answered: HashMap::new(),
            offline_error: HashMap::new(),
            folded: BTreeSet::new(),
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
        // This machine not answering keeps its sessions outside tmux as they were, stale too.
        if local && online {
            self.apply_outside(outside);
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
        if let Row::Host { online, .. } = &mut self.rows[section.start] {
            *online = snap.online;
        }
        if !snap.online {
            // The rows stay as the last answer had them; so does the cursor.
            self.offline_error.insert(snap.host, snap.error);
            return;
        }
        self.offline_error.remove(&snap.host);
        self.answered.insert(snap.host.clone(), Instant::now());
        let before = self.cursor.map(|at| (at, self.rows[at].key().cloned()));
        let sessions = snap.sessions;
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

        let kept = before
            .as_ref()
            .and_then(|(_, key)| self.position(key.as_ref()?));
        self.cursor = kept.or_else(|| match before {
            // Its session went with this listing: the row that took its place, else the last
            // of the host.
            Some((at, _)) if count > 0 && section.contains(&at) => {
                Some(at.clamp(first, first + count - 1))
            }
            Some((at, _)) => self.nearest_session(at),
            None => self.nearest_session(0),
        });
        if kept.is_none() {
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

    /// The machine whose listing has the session: this machine for one outside tmux, the
    /// machine of another tmux server.
    fn machine<'a>(&'a self, key: &'a SessionKey) -> &'a str {
        match key.host == KITTY {
            true => &self.local_name,
            false => split_host(&key.host).0,
        }
    }

    /// Whether `host`'s last listing was an answer (until the first one: yes).
    pub fn host_online(&self, host: &str) -> bool {
        !self.offline_error.contains_key(host)
    }

    /// The session's row is what its host listed before it stopped answering.
    pub fn stale(&self, key: &SessionKey) -> bool {
        !self.host_online(self.machine(key))
    }

    /// How long ago `host` last answered, while it does not; `None` while it answers or when
    /// it never did.
    pub fn offline_for(&self, host: &str, now: Instant) -> Option<Duration> {
        if self.host_online(host) {
            return None;
        }
        Some(now.saturating_duration_since(*self.answered.get(host)?))
    }

    /// What the status line says of a host that does not answer: since when its rows are, and
    /// the last error. `None` while it answers.
    pub fn offline_note(&self, host: &str, now: Instant) -> Option<String> {
        let error = self.offline_error.get(host)?;
        let mut note = format!("{host} does not answer");
        if let Some(age) = self.offline_for(host, now) {
            note.push_str(&format!(" (its sessions as listed {} ago)", age_text(age)));
        }
        if let Some(error) = error.as_deref().map(str::trim).filter(|e| !e.is_empty()) {
            note.push_str(&format!(": {error}"));
        }
        Some(note)
    }

    pub fn is_pinned(&self, key: &SessionKey) -> bool {
        self.listed.get(key).is_some_and(|(_, pinned)| *pinned)
    }

    /// The session of a row; `None` for a header, a gap and a row that is not there.
    pub fn key_at(&self, row: usize) -> Option<&SessionKey> {
        self.rows.get(row)?.key()
    }

    /// Moves the cursor `delta` sessions down (up when negative), over headers, gaps and
    /// folded hosts, stopping at the first and the last session shown.
    pub fn move_by(&mut self, delta: i32) {
        let Some(mut at) = self.cursor.or_else(|| self.nearest_session(0)) else {
            return;
        };
        for _ in 0..delta.unsigned_abs() {
            let next = if delta > 0 {
                (at + 1..self.rows.len()).find(|&row| self.shown_session(row))
            } else {
                (0..at).rev().find(|&row| self.shown_session(row))
            };
            match next {
                Some(next) => at = next,
                None => break,
            }
        }
        self.set_cursor(at);
    }

    /// Puts the cursor on a session, unfolding its host; nothing happens when it is not
    /// listed.
    pub fn select_key(&mut self, key: &SessionKey) {
        if let Some(at) = self.position(key) {
            let host = owner(key).to_string();
            self.folded.remove(&host);
            self.set_cursor(at);
        }
    }

    /// The hosts whose sections are folded.
    pub fn folded(&self) -> &BTreeSet<String> {
        &self.folded
    }

    /// Folds these hosts' sections (a set kept from an earlier run): only those, the others
    /// open. The cursor leaves a folded one.
    pub fn set_folded(&mut self, hosts: BTreeSet<String>) {
        self.folded = hosts;
        self.leave_hidden();
    }

    /// Whether `host`'s section is folded.
    pub fn is_folded(&self, host: &str) -> bool {
        self.folded.contains(host)
    }

    /// Folds `host`'s section, or unfolds it. Returns whether it is folded now. The cursor
    /// does not stay in a folded host: it goes to the nearest session shown, if any.
    pub fn toggle_fold(&mut self, host: &str) -> bool {
        let folded = !self.folded.remove(host);
        if folded {
            self.folded.insert(host.to_string());
            self.leave_hidden();
        }
        self.follow.set(true);
        folded
    }

    /// `z`: folds the section of the cursor's session. Returns the host folded.
    pub fn fold_selected(&mut self) -> Option<String> {
        let host = owner(self.selected()?).to_string();
        self.toggle_fold(&host);
        Some(host)
    }

    /// Unfolds every host. Returns whether any was folded.
    pub fn unfold_all(&mut self) -> bool {
        let any = !self.folded.is_empty();
        self.folded.clear();
        self.follow.set(true);
        any
    }

    /// Of a folded host: its sessions, and how many of them wait. Not counted: the other
    /// tmux servers' and, for this machine, those outside tmux (the rule above them folds
    /// them apart).
    pub fn fold_counts(&self, host: &str) -> (usize, usize) {
        let Some(section) = self.section(host) else {
            return (0, 0);
        };
        let sessions = self.rows[section.start + 1..section.end].iter();
        let waiting = sessions
            .clone()
            .filter(|row| matches!(row, Row::Session(info) if info.status == Status::Waiting))
            .count();
        (sessions.len(), waiting)
    }

    /// Whether a row is hidden in a folded host: a session, or a `background` row, of a host
    /// whose section is folded. Headers and gaps never are.
    pub fn hidden(&self, row: usize) -> bool {
        let host = match self.rows.get(row) {
            Some(Row::Session(info)) => owner(&info.key),
            Some(Row::Background { host, .. }) => host,
            _ => return false,
        };
        self.folded.contains(host)
    }

    /// A session's row that is shown: where the cursor may go.
    fn shown_session(&self, row: usize) -> bool {
        self.rows[row].key().is_some() && !self.hidden(row)
    }

    /// Lines of the list a row takes: none when it is hidden.
    fn row_lines(&self, row: usize) -> usize {
        match self.hidden(row) {
            true => 0,
            false => self.rows[row].lines(),
        }
    }

    /// The cursor, in a host just folded, goes to the nearest session shown.
    fn leave_hidden(&mut self) {
        if let Some(at) = self.cursor.filter(|at| self.hidden(*at)) {
            self.cursor = self.nearest_session(at);
            self.follow.set(true);
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
            Row::Background { .. } => Some(Action::ToggleBackground),
            Row::Host { .. } | Row::Gap => None,
        }
    }

    /// A right-click on a row: a session is selected and its menu asked for.
    /// A session outside tmux has none, nor has one of another tmux server (R4).
    pub fn menu(&mut self, row: usize) -> Option<Action> {
        let key = self.key_at(row)?.clone();
        if key.host == KITTY || split_host(&key.host).1.is_some() {
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
        if key.host == KITTY || split_host(&key.host).1.is_some() {
            // Nothing to attach, or a server tmls-rs only attaches (R4): in no ring to walk.
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

    /// `host`'s sessions on its other tmux servers (`listing::parse_background`, every server's
    /// together): what its `background` row counts and, while open, lists. None: no row.
    /// Returns whether the list changed (output ages alone change nothing). The cursor stays
    /// on its session; when that is hidden or gone it goes to the nearest one.
    pub fn apply_background(&mut self, host: &str, sessions: Vec<SessionInfo>) -> bool {
        let same = |a: &[SessionInfo], b: &[SessionInfo]| {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(x, y)| (&x.key, x.status, x.attached) == (&y.key, y.status, y.attached))
        };
        let known = self
            .background
            .get(host)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if same(known, &sessions) {
            if let Some(known) = self.background.get_mut(host) {
                *known = sessions;
            }
            return false;
        }
        if sessions.is_empty() {
            self.background.remove(host);
        } else {
            self.background.insert(host.to_string(), sessions);
        }
        self.place_background(host);
        true
    }

    /// `b`: shows or hides every host's other tmux servers' sessions.
    pub fn toggle_background(&mut self) {
        self.background_open = !self.background_open;
        let hosts: Vec<String> = self.background.keys().cloned().collect();
        for host in hosts {
            self.place_background(&host);
        }
    }

    /// The other tmux servers' sessions are listed.
    pub fn background_open(&self) -> bool {
        self.background_open
    }

    /// Puts `host`'s `background` row (and, while open, its sessions) where it belongs, or
    /// takes it away.
    fn place_background(&mut self, host: &str) {
        let selected = self.selected().cloned();
        let at = self.cursor;
        if let Some(old) = self.background_rows(host) {
            self.rows.drain(old);
        }
        let sessions = self.background.get(host).cloned().unwrap_or_default();
        if !sessions.is_empty()
            && let Some(place) = self.background_place(host)
        {
            let mut rows = vec![Row::Background {
                host: host.to_string(),
                count: sessions.len(),
                open: self.background_open,
            }];
            if self.background_open {
                rows.extend(sessions.into_iter().map(Row::Session));
            }
            self.rows.splice(place..place, rows);
        }
        self.cursor = match selected {
            Some(key) => self
                .position(&key)
                .or_else(|| at.and_then(|at| self.nearest_session(at))),
            None => at.and_then(|at| self.nearest_session(at)),
        };
    }

    /// `host`'s `background` row and the sessions under it.
    fn background_rows(&self, host: &str) -> Option<Range<usize>> {
        let start = self
            .rows
            .iter()
            .position(|row| matches!(row, Row::Background { host: h, .. } if h == host))?;
        let sessions = self.rows[start + 1..]
            .iter()
            .take_while(|row| row.key().is_some())
            .count();
        Some(start..start + 1 + sessions)
    }

    /// Where `host`'s `background` row goes: after its sessions, and for this machine after
    /// its sessions outside tmux.
    fn background_place(&self, host: &str) -> Option<usize> {
        let mut at = self.section(host)?.end;
        if host == self.local_name
            && let Some(outside) = self.section(KITTY)
            && outside.start == at
        {
            at = outside.end;
        }
        Some(at)
    }

    /// What went wrong at the last `toggle_pin` or `renamed`, once; `Ok` when nothing did.
    pub fn take_pin_error(&mut self) -> anyhow::Result<()> {
        self.pin_error.take().map_or(Ok(()), Err)
    }

    /// The next session that needs the user after `after` in the list, wrapping round: one
    /// that waits (?), and only when no other does, one that finished unwatched (◆). `after`
    /// itself is never the answer, nor is a stale row.
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
                .find(|info| {
                    info.status == status && Some(&info.key) != after && !self.stale(&info.key)
                })
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
            .find(|key| Some(*key) != after && key.host != KITTY && !self.stale(key) && unread(key))
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
            .filter(|key| key.host != KITTY && !self.stale(key) && !self.is_folded(owner(key)))
            .filter(|key| in_ring(key))
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
        while fits_from > 0 && lines + self.row_lines(fits_from - 1) <= height {
            fits_from -= 1;
            lines += self.row_lines(fits_from);
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
                |from: usize| -> usize { (from..=cursor).map(|row| self.row_lines(row)).sum() };
            while top < cursor && lines(top) > height {
                top += 1;
            }
        }
        self.top.set(top);
        let mut end = top;
        let mut lines = 0;
        while end < count && lines + self.row_lines(end) <= height {
            lines += self.row_lines(end);
            end += 1;
        }
        top..end
    }

    /// The row at a line of the list (0 is the first line drawn) when `height` lines fit.
    pub fn row_at(&self, line: usize, height: usize) -> Option<usize> {
        let mut first = 0;
        self.view(height).find(|&row| {
            first += self.row_lines(row);
            line < first
        })
    }

    /// The user (or the tab in front) puts the cursor somewhere: that is where it stays.
    fn set_cursor(&mut self, row: usize) {
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

    /// The session row shown nearest to row `at`; the one above when two are as near.
    fn nearest_session(&self, at: usize) -> Option<usize> {
        (0..self.rows.len())
            .filter(|&row| self.shown_session(row))
            .min_by_key(|&row| (row.abs_diff(at), row))
    }
}
