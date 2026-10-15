//! The go-to picker (`g` in the list, Alt+Shift+G): every listed session of every host in one
//! list, narrowed by what is typed (after herdr's session picker).
//!
//! What is typed is split at spaces; a session is shown when every word is in its name, its
//! host, or the dim line the list shows under it (`sidebar::summary`: what Claude waits for,
//! its last reply or its title), or when the word's letters are in its name in order (`fz`
//! finds `fizz`). The listing has no working directory or git branch: neither is searched.
//!
//! The status filter narrows further: Tab and Shift+Tab walk all, blocked (? and ✕),
//! working (●), idle (○), done (◆); Ctrl+B / W / I / D choose one, or go back to all when it
//! is the one chosen already. Letters are the search's, so the filter has no letter keys.
//!
//! Order: sessions that wait first, then the better match (a name that is the word, starts
//! with it, has it, then the word elsewhere, then the letters in order), then the list's own
//! order. Up / Down or Ctrl+K / J move, Enter opens the session as Enter in the list does,
//! Esc closes. A session of a host that does not answer is listed dimly; the app refuses to
//! open it (`sidebar::Sidebar::stale`).

use client_core::{SessionInfo, SessionKey, Status};
use s2_core::{Key, KeyEvent, Named};
use s2_wgpu::ui::textfield::TextField;

use crate::forms::{Look, Panel, Stop, Target, edit};

/// Width of the picker, in cells.
const PICK_COLS: usize = 72;
/// Results shown at once; the view follows the cursor.
const PICK_ROWS: usize = 12;
/// Where the results start: after the title and the field.
const FIRST_ROW: usize = 3;

/// Which statuses the picker shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    /// Waits for the user (?), or failed (✕).
    Blocked,
    Working,
    Idle,
    /// Finished unwatched (◆).
    Done,
}

const FILTERS: [Filter; 5] = [
    Filter::All,
    Filter::Blocked,
    Filter::Working,
    Filter::Idle,
    Filter::Done,
];

impl Filter {
    pub fn admits(self, status: Status) -> bool {
        match self {
            Filter::All => true,
            Filter::Blocked => matches!(status, Status::Waiting | Status::Error),
            Filter::Working => status == Status::Running,
            Filter::Idle => status == Status::Idle,
            Filter::Done => status == Status::Unseen,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "all",
            Filter::Blocked => "blocked",
            Filter::Working => "working",
            Filter::Idle => "idle",
            Filter::Done => "done",
        }
    }

    /// The next filter in Tab's order (`forward`), or the one before, round the ends.
    pub fn step(self, forward: bool) -> Filter {
        let n = FILTERS.len();
        let at = FILTERS.iter().position(|f| *f == self).unwrap_or(0);
        FILTERS[if forward { at + 1 } else { at + n - 1 } % n]
    }
}

/// One session the picker can go to, as the list had it when the picker opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub key: SessionKey,
    pub status: Status,
    /// The list's dim line under the name, if any.
    pub detail: Option<String>,
    /// Its host does not answer: listed dimly, not opened.
    pub stale: bool,
}

impl Entry {
    pub fn of(info: &SessionInfo, stale: bool) -> Entry {
        Entry {
            key: info.key.clone(),
            status: info.status,
            detail: crate::sidebar::summary(info).map(|(text, _)| text.to_string()),
            stale,
        }
    }
}

/// How well one word matches a session; lower is better, `None` is no match.
fn word_score(word: &str, entry: &Entry) -> Option<u32> {
    let name = entry.key.name.to_lowercase();
    if name == word {
        return Some(0);
    }
    if name.starts_with(word) {
        return Some(1);
    }
    if name.contains(word) {
        return Some(2);
    }
    let elsewhere = [Some(&entry.key.host), entry.detail.as_ref()];
    if elsewhere
        .into_iter()
        .flatten()
        .any(|text| text.to_lowercase().contains(word))
    {
        return Some(3);
    }
    let mut letters = name.chars();
    word.chars().all(|c| letters.any(|n| n == c)).then_some(4)
}

/// How well what is typed matches a session: the sum of its words' scores; `None` when a word
/// matches nothing. Case does not count. Nothing typed matches everything equally.
pub fn score(query: &str, entry: &Entry) -> Option<u32> {
    let query = query.to_lowercase();
    query
        .split_whitespace()
        .map(|word| word_score(word, entry))
        .sum()
}

/// The entries the query and the filter let through, in the picker's order (see the module
/// comment), as indices into `entries`.
pub fn results(entries: &[Entry], query: &str, filter: Filter) -> Vec<usize> {
    let mut found: Vec<(bool, u32, usize)> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| filter.admits(entry.status))
        .filter_map(|(at, entry)| {
            let waits = entry.status == Status::Waiting;
            Some((!waits, score(query, entry)?, at))
        })
        .collect();
    found.sort();
    found.into_iter().map(|(_, _, at)| at).collect()
}

/// What a key or a click did to the picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickResult {
    Pending,
    Closed,
    Open(SessionKey),
}

/// The open picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoTo {
    entries: Vec<Entry>,
    field: TextField,
    filter: Filter,
    /// What is shown, in order (`results`), and the cursor among it.
    shown: Vec<usize>,
    cursor: usize,
}

impl GoTo {
    /// The picker over `entries`, in the list's order; nothing typed, every status.
    pub fn new(entries: Vec<Entry>) -> GoTo {
        let mut goto = GoTo {
            entries,
            field: TextField::new(),
            filter: Filter::All,
            shown: Vec::new(),
            cursor: 0,
        };
        goto.refresh();
        goto
    }

    pub fn filter(&self) -> Filter {
        self.filter
    }

    /// The sessions shown, in order.
    pub fn shown(&self) -> impl Iterator<Item = &Entry> {
        self.shown.iter().map(|at| &self.entries[*at])
    }

    /// The session under the cursor.
    pub fn selected(&self) -> Option<&Entry> {
        self.entries.get(*self.shown.get(self.cursor)?)
    }

    /// After the query or the filter changed: the results again, the cursor on the first.
    fn refresh(&mut self) {
        self.shown = results(&self.entries, self.field.text(), self.filter);
        self.cursor = 0;
    }

    fn set_filter(&mut self, filter: Filter) {
        self.filter = filter;
        self.refresh();
    }

    fn open(&self) -> PickResult {
        self.selected().map_or(PickResult::Pending, |entry| {
            PickResult::Open(entry.key.clone())
        })
    }

    /// A key while the picker is open: see the module comment. Any other key edits the field.
    pub fn key(&mut self, ev: &KeyEvent) -> PickResult {
        if !ev.pressed {
            return PickResult::Pending;
        }
        let m = ev.mods;
        let ctrl = m.ctrl && !m.alt && !m.logo && !m.shift;
        let last = self.shown.len().saturating_sub(1);
        let letter = |c: char| matches!(ev.key, Key::Char(k) if k.eq_ignore_ascii_case(&c));
        match ev.key {
            Key::Named(Named::Escape) => return PickResult::Closed,
            // A held Enter opens nothing: it may be the one that opened the picker's host.
            Key::Named(Named::Enter) if !ev.repeat => return self.open(),
            Key::Named(Named::Enter) => {}
            Key::Named(Named::Tab) if !m.ctrl && !m.alt => {
                self.set_filter(self.filter.step(!m.shift));
            }
            Key::Named(Named::Up) => self.cursor = self.cursor.saturating_sub(1),
            Key::Named(Named::Down) => self.cursor = (self.cursor + 1).min(last),
            _ if ctrl && letter('k') => self.cursor = self.cursor.saturating_sub(1),
            _ if ctrl && letter('j') => self.cursor = (self.cursor + 1).min(last),
            _ if ctrl
                && !ev.repeat
                && (letter('b') || letter('w') || letter('i') || letter('d')) =>
            {
                let chosen = match ev.key {
                    Key::Char(c) => match c.to_ascii_lowercase() {
                        'b' => Filter::Blocked,
                        'w' => Filter::Working,
                        'i' => Filter::Idle,
                        _ => Filter::Done,
                    },
                    Key::Named(_) => Filter::All,
                };
                let filter = if self.filter == chosen {
                    Filter::All
                } else {
                    chosen
                };
                self.set_filter(filter);
            }
            _ => {
                let before = self.field.text().to_string();
                if edit(&mut self.field, ev) && self.field.text() != before {
                    self.refresh();
                }
            }
        }
        PickResult::Pending
    }

    /// Ctrl+Shift+V: the clipboard's text into the field.
    pub fn paste(&mut self, text: &str) {
        self.field.insert(text);
        self.refresh();
    }

    /// The first result in view: the view follows the cursor.
    fn first(&self) -> usize {
        (self.cursor + 1).saturating_sub(PICK_ROWS)
    }

    /// The result under a position in cells.
    fn row_at(&self, x: f32, y: f32) -> Option<usize> {
        match self.panel().target_at(x, y) {
            Some((Target::Item(index), _)) => Some(index),
            _ => None,
        }
    }

    /// The pointer moved to `(x, y)` cells from the panel's corner. Returns whether the
    /// cursor moved.
    pub fn hover(&mut self, x: f32, y: f32) -> bool {
        let Some(row) = self.row_at(x, y) else {
            return false;
        };
        let changed = self.cursor != row;
        self.cursor = row;
        changed
    }

    /// A click at `(x, y)` cells from the panel's corner: opens a result, closes beside the
    /// panel.
    pub fn click(&mut self, x: f32, y: f32) -> PickResult {
        match self.row_at(x, y) {
            Some(row) => {
                self.cursor = row;
                self.open()
            }
            None if self.panel().contains(x, y) => PickResult::Pending,
            None => PickResult::Closed,
        }
    }

    /// The picker as text in cells: a title with the filter, the field, then one line per
    /// result — the mark, the name and the host, and at the right end its detail, dimmed.
    /// Always as tall as a full page of results, so that typing does not move it.
    pub fn panel(&self) -> Panel {
        let mut p = Panel::new(PICK_COLS);
        let title = format!(
            "Go to · {} of {} · Tab: {} · Esc closes",
            self.shown.len(),
            self.entries.len(),
            self.filter.label(),
        );
        p.put(0, 1, PICK_COLS - 2, &title, Look::Title);
        p.field(
            1,
            1,
            PICK_COLS - 2,
            &mut self.field.clone(),
            Stop::Name,
            Stop::Name,
        );
        if self.shown.is_empty() {
            let none = match self.entries.is_empty() {
                true => "no session listed",
                false => "no session matches",
            };
            p.put(FIRST_ROW, 1, PICK_COLS - 2, none, Look::Dim);
        }
        let shown = self
            .shown
            .iter()
            .enumerate()
            .skip(self.first())
            .take(PICK_ROWS);
        for (line, (index, at)) in shown.enumerate() {
            let entry = &self.entries[*at];
            let look = Look::Item {
                hovered: index == self.cursor,
                danger: entry.status == Status::Error,
            };
            let what = format!(
                " {} {} · {}",
                entry.status.mark(),
                entry.key.name,
                entry.key.host
            );
            let line = FIRST_ROW + line;
            let what_cols = columns(&what);
            p.put(line, 0, PICK_COLS, &what, look).target = Some(Target::Item(index));
            if entry.stale {
                p.put(line, what_cols + 1, 8, "offline", Look::Dim);
            } else if let Some(detail) = &entry.detail {
                let room = PICK_COLS.saturating_sub(what_cols + 3);
                let cols = columns(detail).min(room);
                if cols > 0 {
                    p.put(line, PICK_COLS - 1 - cols, cols, detail, Look::Dim);
                }
            }
        }
        p.lines = FIRST_ROW + PICK_ROWS;
        p
    }
}

fn columns(text: &str) -> usize {
    s2_wgpu::gfx::grid::clusters(text)
        .iter()
        .map(|(_, width)| width)
        .sum()
}
