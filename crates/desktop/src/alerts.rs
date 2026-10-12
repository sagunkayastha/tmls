//! Alerts: which sessions started to need the user, the bell's count, the list behind it and
//! the desktop notification (tmls README "Alerts", `app.py` `_alert`, `notifications.py`).
//! No window and no process here: `observe` says what to notify, the app runs the command.
//!
//! **What alerts.** A session whose mark *changes to* `?` (waiting), `◆` (finished unwatched)
//! or `✕` (error) at a listing of its host. As in tmls, a session that was not known before
//! is not news: nothing alerts at the first listing after the start or for a session that
//! appears already waiting. A listing of a host that did not answer changes nothing here
//! (unlike tmls): what its sessions did meanwhile alerts when the host is back, as the list
//! then shows it (`Seen` forgets nothing for an offline host either).
//!
//! **Silence focused** (on by default): no alert for the session of the tab in front while
//! the window has the keyboard focus — the user is looking at it. With the window *not*
//! focused that session does alert; the list never gives it a `◆` (`Sidebar::deliver` counts
//! the tab in front as watched), so its going from working to idle is taken as `◆` here.
//!
//! **Settings** are tmls's `~/.config/tmls/notifications.json`, read once and never written:
//! `desktop` (default on) and `silence_focused` (default on). `sound` is tmls's; this app
//! plays none. A missing or damaged file, or a value that is not a boolean, is the default.
//!
//! **Approvals** (tmls's Yes / No on a permission prompt): `y` on an alert asks the app to
//! answer its session's prompt (`PanelResult::Approve`); nothing here sends a key to a session.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use client_core::{SessionInfo, SessionKey, Status};
use s2_core::{Key, KeyEvent, Named};

use crate::forms::{Look, Panel, Target};

/// How many alerts are kept (tmls `KEEP_ALERTS`).
pub const MAX_ALERTS: usize = 50;
/// The alerts panel: its width (tmls: 60) and how many alerts it shows at once.
const PANEL_COLS: usize = 60;
const PANEL_ROWS: usize = 12;

/// The switches of tmls's alerts panel that this app follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Desktop notifications through `notify-send`.
    pub desktop: bool,
    /// No alert for the session the user is looking at.
    pub silence_focused: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            desktop: true,
            silence_focused: true,
        }
    }
}

impl Settings {
    /// Reads `notifications.json` in `config_dir`. Never fails, never writes.
    pub fn load(config_dir: &Path) -> Settings {
        std::fs::read_to_string(config_dir.join("notifications.json"))
            .map_or_else(|_| Settings::default(), |text| Settings::parse(&text))
    }

    /// The settings a `notifications.json` with this text gives.
    pub fn parse(text: &str) -> Settings {
        let defaults = Settings::default();
        let json: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
        let switch =
            |name: &str, default: bool| json.get(name).and_then(|v| v.as_bool()).unwrap_or(default);
        Settings {
            desktop: switch("desktop", defaults.desktop),
            silence_focused: switch("silence_focused", defaults.silence_focused),
        }
    }
}

/// One line of the alerts list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alert {
    pub key: SessionKey,
    /// `Waiting`, `Unseen` or `Error`.
    pub status: Status,
    /// What a waiting session waits for, when the listing says.
    pub reason: Option<String>,
    pub at: Instant,
}

impl Alert {
    /// What happened, in a word or the session's own: tmls `ALERT_TEXT`.
    fn detail(&self) -> &str {
        match (self.status, self.reason.as_deref()) {
            (Status::Waiting, Some(reason)) if !reason.is_empty() => reason,
            (Status::Waiting, _) => "waiting",
            (Status::Error, _) => "error",
            _ => "done",
        }
    }
}

/// A desktop notification to send.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    pub title: String,
    pub body: String,
}

/// The command that shows `note`: no shell, title and body one argument each. After `--`
/// neither can be read as an option, whatever a session or a host is called.
pub fn notify_argv(note: &Notification) -> Vec<String> {
    ["notify-send", "--", &note.title, &note.body]
        .map(str::to_string)
        .to_vec()
}

/// Sends `notes` through `spawn` (the app: `forms::spawn_detached`). A machine without
/// `notify-send`, or one where it cannot start, shows none: that is not an error to report
/// with every alert.
pub fn send(notes: &[Notification], spawn: &mut dyn FnMut(&[String]) -> std::io::Result<()>) {
    for note in notes {
        let _ = spawn(&notify_argv(note));
    }
}

/// Who is looking at what, when a listing arrives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Watching<'a> {
    /// The session of the tab in front.
    pub front: Option<&'a SessionKey>,
    /// The window has the keyboard focus.
    pub window_focused: bool,
}

/// The alerts so far, newest first, and how many are new since the list was last opened.
#[derive(Debug, Default)]
pub struct Alerts {
    settings: Settings,
    entries: Vec<Alert>,
    unread: usize,
    /// Each listed session's mark at its host's last listing.
    marks: HashMap<SessionKey, Status>,
}

impl Alerts {
    pub fn new(settings: Settings) -> Alerts {
        Alerts {
            settings,
            ..Alerts::default()
        }
    }

    /// A host's new listing: `listed` are its sessions as the list shows them now (after
    /// `Seen` marked the unseen ones). Adds an alert for every session that started to need
    /// the user, and returns the notifications to send. A listing of a host that did not
    /// answer (`online` false) says nothing about its sessions: their marks are kept, so what
    /// changed while it was away alerts at its next listing. A session that is not in an
    /// online listing is forgotten.
    pub fn observe<'a>(
        &mut self,
        host: &str,
        online: bool,
        listed: impl IntoIterator<Item = &'a SessionInfo>,
        watching: Watching,
        now: Instant,
    ) -> Vec<Notification> {
        if !online {
            return Vec::new();
        }
        let mut before = HashMap::new();
        self.marks.retain(|key, status| {
            let of_host = key.host == host;
            if of_host {
                before.insert(key.clone(), *status);
            }
            !of_host
        });
        let mut notes = Vec::new();
        for info in listed {
            let old = before.get(&info.key).copied();
            self.marks.insert(info.key.clone(), info.status);
            let front = watching.front == Some(&info.key);
            let watched = front && watching.window_focused;
            // The tab in front never gets a ◆ in the list; unwatched, it finished all the same.
            let finished = matches!(old, Some(Status::Running | Status::Waiting));
            let status = match info.status {
                Status::Idle if front && !watched && finished => Status::Unseen,
                status => status,
            };
            let needs_user = matches!(status, Status::Waiting | Status::Unseen | Status::Error);
            // Unknown before: not news.
            let news = needs_user && old.is_some_and(|old| old != status);
            if !news || (watched && self.settings.silence_focused) {
                continue;
            }
            let alert = Alert {
                key: info.key.clone(),
                status,
                reason: info.reason.clone().filter(|_| status == Status::Waiting),
                at: now,
            };
            if self.settings.desktop && status != Status::Error {
                notes.push(Notification {
                    title: format!("tmls · {}", alert.key.host),
                    body: format!("{} {} · {}", status.mark(), alert.key.name, alert.detail()),
                });
            }
            self.entries.insert(0, alert);
            self.entries.truncate(MAX_ALERTS);
            self.unread = (self.unread + 1).min(MAX_ALERTS);
        }
        notes
    }

    /// The alerts, newest first.
    pub fn entries(&self) -> &[Alert] {
        &self.entries
    }

    /// How many alerts came since the list was last opened.
    pub fn unread(&self) -> usize {
        self.unread
    }

    /// Opens the list as it is now; every alert in it counts as read.
    pub fn open(&mut self, now: Instant) -> AlertsPanel {
        self.unread = 0;
        AlertsPanel {
            entries: self.entries.clone(),
            now,
            cursor: 0,
        }
    }
}

/// What a key or a click did to the alerts panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelResult {
    Pending,
    Closed,
    /// Go to this alert's session.
    Jump(SessionKey),
    /// Answer this alert's session's permission prompt.
    Approve(SessionKey),
}

/// What a jump to an alert's session does: opens its tab, or says in the status line why
/// not — its host does not answer now, or the session was renamed or killed since. `listed`:
/// the list has the session now; `has_tab`: a tab of it is open (ended, perhaps);
/// `host_online`: the host answered its last listing.
pub fn jump(
    key: &SessionKey,
    listed: bool,
    has_tab: bool,
    host_online: bool,
) -> Result<(), String> {
    if listed || has_tab {
        Ok(())
    } else if !host_online {
        Err(format!("{} is offline", key.host))
    } else {
        Err(format!(
            "{} on {} is gone (renamed or ended)",
            key.name, key.host
        ))
    }
}

/// The open list of alerts, as it was when it was opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlertsPanel {
    entries: Vec<Alert>,
    now: Instant,
    /// The alert the arrows or the pointer are on.
    cursor: usize,
}

/// How long ago, in at most three columns: `now`, `5m`, `3h`, `2d`.
fn age(age: Duration) -> String {
    match age.as_secs() {
        0..60 => "now".to_string(),
        secs @ 60..3600 => format!("{}m", secs / 60),
        secs @ 3600..86_400 => format!("{}h", secs / 3600),
        secs => format!("{}d", (secs / 86_400).min(99)),
    }
}

impl AlertsPanel {
    /// The alert the cursor is on.
    pub fn selected(&self) -> Option<&Alert> {
        self.entries.get(self.cursor)
    }

    /// Up / Down or k / j move, Enter jumps to the session, Esc closes; nothing else does
    /// anything.
    pub fn key(&mut self, ev: &KeyEvent) -> PanelResult {
        if !ev.pressed {
            return PanelResult::Pending;
        }
        let last = self.entries.len().saturating_sub(1);
        match ev.key {
            Key::Named(Named::Up) | Key::Char('k') => self.cursor = self.cursor.saturating_sub(1),
            Key::Named(Named::Down) | Key::Char('j') => self.cursor = (self.cursor + 1).min(last),
            Key::Named(Named::Enter) if !ev.repeat => {
                return self.selected().map_or(PanelResult::Pending, |alert| {
                    PanelResult::Jump(alert.key.clone())
                });
            }
            Key::Char('y') if !ev.repeat => {
                return self.selected().map_or(PanelResult::Pending, |alert| {
                    PanelResult::Approve(alert.key.clone())
                });
            }
            Key::Named(Named::Escape) => return PanelResult::Closed,
            _ => {}
        }
        PanelResult::Pending
    }

    fn alert_at(&self, x: f32, y: f32) -> Option<usize> {
        match self.panel().target_at(x, y) {
            Some((Target::Item(index), _)) => Some(index),
            _ => None,
        }
    }

    /// The pointer moved to `(x, y)` cells from the panel's corner. Returns whether the
    /// cursor moved.
    pub fn hover(&mut self, x: f32, y: f32) -> bool {
        let Some(index) = self.alert_at(x, y) else {
            return false;
        };
        let changed = self.cursor != index;
        self.cursor = index;
        changed
    }

    /// A click at `(x, y)` cells from the panel's corner: jumps to an alert's session, closes
    /// beside the panel.
    pub fn click(&mut self, x: f32, y: f32) -> PanelResult {
        match self.alert_at(x, y) {
            Some(index) => PanelResult::Jump(self.entries[index].key.clone()),
            None if self.panel().contains(x, y) => PanelResult::Pending,
            None => PanelResult::Closed,
        }
    }

    /// The first alert shown: the view follows the cursor.
    fn first(&self) -> usize {
        (self.cursor + 1).saturating_sub(PANEL_ROWS)
    }

    /// The panel as text in cells: a title, then one line per alert — how long ago, the mark,
    /// the session, its host and what happened.
    pub fn panel(&self) -> Panel {
        let mut p = Panel::new(PANEL_COLS);
        let title = "Alerts · Enter jumps · Esc closes";
        p.put(0, 1, PANEL_COLS - 2, title, Look::Title);
        if self.entries.is_empty() {
            p.put(1, 1, PANEL_COLS - 2, "nothing yet", Look::Dim);
        }
        let shown = self
            .entries
            .iter()
            .enumerate()
            .skip(self.first())
            .take(PANEL_ROWS);
        for (line, (index, alert)) in shown.enumerate() {
            let look = Look::Item {
                hovered: index == self.cursor,
                danger: alert.status == Status::Error,
            };
            let text = format!(
                " {:>3}  {} {} · {}  {}",
                age(self.now.saturating_duration_since(alert.at)),
                alert.status.mark(),
                alert.key.name,
                alert.key.host,
                alert.detail(),
            );
            p.put(line + 1, 0, PANEL_COLS, &text, look).target = Some(Target::Item(index));
        }
        p.lines = 1 + self.entries.len().clamp(1, PANEL_ROWS);
        p
    }
}
