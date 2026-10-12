//! The window's own key bindings, and where every other key goes.
//!
//! Bindings (`global`), all on the press:
//! - Alt+Shift+Right / Left — next / previous pinned (★) session; when none is pinned, tab,
//!   or session while fewer than two tabs are open (repeats while held)
//! - Alt+Shift+Up — keyboard focus to the session list; Esc in the list — back to the terminal
//! - in the list (`list_key`): j / k or the arrows move, Enter attaches, p pins, n opens the
//!   next session that waits, c opens the new-session form for the cursor's host, m the
//!   cursor's session menu (rename, kill, open in a terminal, copy), a the alerts,
//!   y — answer the cursor's session's permission prompt (Yes / No / Cancel),
//!   s — the message form (send now, or queue while the session is busy)
//! - `?` in the list or the `?` button — the keys and mouse actions (`HELP`); Esc, `?` or a
//!   click beside it closes it
//! - Alt+Shift+N — the next session that waits for the user
//! - Alt+Shift+A — the message form for the front tab's Claude session
//! - Alt+Shift+W — close the tab (tmls has no key for this; its tab bar has the `×` only)
//! - Alt+Shift+X — close the file panel (tmls: Esc, with the keys in its viewer; here a file's
//!   panel never has the keys, so Esc stays the terminal's)
//! - Alt+Shift+F — open the panel on the list of the images the front tab's Claude session
//!   made, or close the panel
//! - in that list (`panel_key`): j / k or the arrows move, Enter opens the picture, Backspace or
//!   Esc lead from the picture back to the list, and from the list back to the terminal. The
//!   keyboard reaches the list only after a click on the panel: opening it with Alt+Shift+F or
//!   `Files` leaves the keys where they were. And only for those keys: any other key pressed
//!   there (a letter, Tab, Ctrl+C, a chord) is the terminal's — it takes the keys back and is
//!   sent to the session like any key (`leaves_panel`), so nothing typed is lost
//! - Ctrl+Q — quit; Ctrl+= / Ctrl++ and Ctrl+− — font size (repeat while held)
//! - Enter on a tab whose session ended — reattach (`route`, which knows the tab's state)
//!
//! The Alt+Shift keys work with the focus on the terminal or the list, not in a form. Quit and
//! zoom work everywhere.
//!
//! Everything else belongs to whatever has the keyboard focus (`route`): with the terminal
//! focused and a live tab it goes to that tab's session unchanged — Tab, Esc, Ctrl+C, Ctrl+Tab
//! and Alt+letters included. One exception: Ctrl+C while text is selected in the terminal
//! copies it and is not sent (`copy_instead`). A key for the list or a form reaches no session. While a form or
//! a session's menu is open the focus is `Focus::Form` and every key but quit and zoom is the
//! form's (`forms::Form::key`): typing a session's name types nothing into the terminal behind. A key for a tab
//! whose session ended, or with no tab open, is dropped: it would get a `key` line in the
//! latency log and never a frame.

use s2_core::{Key, KeyEvent, Mods, Named};

use crate::forms::Request;
use crate::tabs::{TabState, Tabs};

/// What has the keyboard focus inside the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Terminal,
    Sidebar,
    Form,
    /// The panel beside the terminal, while it shows the list of a session's images or a
    /// picture opened from it (`viewer::SidePanel::takes_keys`).
    Panel,
}

/// What a bound key asks the window to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Global {
    NextTab,
    PrevTab,
    FocusSidebar,
    FocusTerminal,
    CloseTab,
    /// Closes the file panel beside the terminal.
    ClosePanel,
    /// Opens the panel on the list of the images the front tab's session made, or closes
    /// the panel if it is open.
    ToggleFiles,
    NextWaiting,
    /// The message form for the front tab's Claude session.
    Ask,
    Quit,
    ZoomIn,
    ZoomOut,
    Reattach,
}

impl Global {
    /// Whether holding the key down does it again.
    fn repeats(self) -> bool {
        matches!(
            self,
            Global::NextTab | Global::PrevTab | Global::ZoomIn | Global::ZoomOut
        )
    }
}

const NONE: Mods = Mods {
    shift: false,
    ctrl: false,
    alt: false,
    logo: false,
};
const ALT_SHIFT: Mods = Mods {
    shift: true,
    alt: true,
    ..NONE
};
const CTRL: Mods = Mods { ctrl: true, ..NONE };
const CTRL_SHIFT: Mods = Mods {
    ctrl: true,
    shift: true,
    ..NONE
};

/// The binding a key is part of, whether this event is its press, a repeat or its release.
fn binding(ev: &KeyEvent, focus: Focus) -> Option<Global> {
    let letter = |c: char| match ev.key {
        Key::Char(key) => key.eq_ignore_ascii_case(&c),
        Key::Named(_) => false,
    };
    if ev.mods == CTRL {
        if letter('q') {
            return Some(Global::Quit);
        }
        if letter('=') || letter('+') {
            return Some(Global::ZoomIn);
        }
        // Not "_": Ctrl+_ is 0x1f, undo in readline and Emacs, and stays the terminal's.
        if letter('-') {
            return Some(Global::ZoomOut);
        }
    }
    // `+` is Shift+= on most layouts.
    if ev.mods == CTRL_SHIFT && letter('+') {
        return Some(Global::ZoomIn);
    }
    if focus == Focus::Form {
        return None;
    }
    if ev.mods == ALT_SHIFT {
        return match ev.key {
            Key::Named(Named::Right) => Some(Global::NextTab),
            Key::Named(Named::Left) => Some(Global::PrevTab),
            Key::Named(Named::Up) => Some(Global::FocusSidebar),
            Key::Char(c) if c.eq_ignore_ascii_case(&'n') => Some(Global::NextWaiting),
            Key::Char(c) if c.eq_ignore_ascii_case(&'w') => Some(Global::CloseTab),
            Key::Char(c) if c.eq_ignore_ascii_case(&'x') => Some(Global::ClosePanel),
            Key::Char(c) if c.eq_ignore_ascii_case(&'f') => Some(Global::ToggleFiles),
            Key::Char(c) if c.eq_ignore_ascii_case(&'a') => Some(Global::Ask),
            _ => None,
        };
    }
    if focus == Focus::Sidebar && ev.mods == NONE && ev.key == Key::Named(Named::Escape) {
        return Some(Global::FocusTerminal);
    }
    None
}

/// The window's own binding this key event triggers, if any. `Reattach` is never returned
/// here: it depends on the tab (`route`).
pub fn global(ev: &KeyEvent, focus: Focus) -> Option<Global> {
    let global = binding(ev, focus)?;
    (ev.pressed && (!ev.repeat || global.repeats())).then_some(global)
}

/// Where one key event goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Global(Global),
    /// To the active tab's session, unchanged.
    Terminal,
    /// To the session list.
    Sidebar,
    /// To the open form.
    Form,
    /// To the panel beside the terminal (`panel_key`).
    Panel,
    /// Nowhere: the release or a repeat of a bound key, or a key for a tab that has no live
    /// session.
    Drop,
}

/// Routes one key event. `tab` is the state of the active tab, `None` when no tab is open.
pub fn route(ev: &KeyEvent, focus: Focus, tab: Option<TabState>) -> Route {
    if binding(ev, focus).is_some() {
        return global(ev, focus).map_or(Route::Drop, Route::Global);
    }
    match (focus, tab) {
        (Focus::Sidebar, _) => Route::Sidebar,
        (Focus::Form, _) => Route::Form,
        (Focus::Panel, _) => Route::Panel,
        (Focus::Terminal, Some(TabState::Live)) => Route::Terminal,
        (Focus::Terminal, Some(TabState::Ended { .. })) => {
            let enter = ev.key == Key::Named(Named::Enter) && ev.mods == NONE;
            if enter && ev.pressed && !ev.repeat {
                Route::Global(Global::Reattach)
            } else {
                Route::Drop
            }
        }
        (Focus::Terminal, None) => Route::Drop,
    }
}

/// Routes one key event and, when it is the terminal's, sends it to the session of the tab in
/// front. No key reaches a session any other way: one for the list or a form is neither sent
/// nor given a `key` line in the latency log. Returns where the key went and, unless it was
/// sent, the event; a repeat that `gate` holds back comes back as `Route::Drop`.
pub fn dispatch(
    t_key_ns: u128,
    ev: KeyEvent,
    focus: Focus,
    gate: &mut RepeatGate,
    tabs: &mut Tabs,
) -> (Route, Option<KeyEvent>) {
    match route(&ev, focus, tabs.active_state()) {
        Route::Terminal if gate.passes(&ev) => {
            tabs.send_key(t_key_ns, ev);
            (Route::Terminal, None)
        }
        Route::Terminal => (Route::Drop, Some(ev)),
        other => (other, Some(ev)),
    }
}

/// Ctrl+C with a selection in the terminal copies instead of interrupting (as tmls does): the
/// selected text is returned for the clipboard, the selection ends, and the key is not sent —
/// call this before `dispatch`. `None` for every other key and without a selection: Ctrl+C
/// then goes to the session like any key. The repeats of a Ctrl+C that copied are held back
/// (`gate`), so holding it down does not interrupt after all.
pub fn copy_instead(
    ev: &KeyEvent,
    focus: Focus,
    gate: &mut RepeatGate,
    tabs: &mut Tabs,
) -> Option<String> {
    let ctrl_c = ev.mods == CTRL && matches!(ev.key, Key::Char('c' | 'C'));
    if !(ctrl_c && ev.pressed && !ev.repeat && focus == Focus::Terminal) {
        return None;
    }
    let session = tabs.session_mut(tabs.active()?);
    let text = session.selected_text()?;
    session.select_clear();
    gate.hold();
    Some(text)
}

/// A key of the session list (tmls `keys.py`: j/k or the arrows move, Enter attaches, p pins,
/// n jumps to the next session that waits).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKey {
    Up,
    Down,
    Open,
    Pin,
    NextWaiting,
    /// The new-session form, for the host of the session under the cursor.
    New,
    /// The menu of the session under the cursor.
    Menu,
    /// The list of alerts.
    Alerts,
    /// The keys and mouse actions (`?`).
    Help,
    /// Approve the permission prompt of the session under the cursor.
    Approve,
    /// Send or queue a message to the cursor's Claude session.
    Send,
}

/// What a key routed to the list (`Route::Sidebar`) does there; `None` for every other key.
pub fn list_key(ev: &KeyEvent) -> Option<ListKey> {
    if !ev.pressed {
        return None;
    }
    // `?` is Shift+/ on most layouts: Shift may be down.
    let plain = !ev.mods.ctrl && !ev.mods.alt && !ev.mods.logo;
    if ev.key == Key::Char('?') && plain && !ev.repeat {
        return Some(ListKey::Help);
    }
    if ev.mods != NONE {
        return None;
    }
    match ev.key {
        Key::Named(Named::Up) | Key::Char('k') => Some(ListKey::Up),
        Key::Named(Named::Down) | Key::Char('j') => Some(ListKey::Down),
        Key::Named(Named::Enter) if !ev.repeat => Some(ListKey::Open),
        Key::Char('p') if !ev.repeat => Some(ListKey::Pin),
        Key::Char('n') if !ev.repeat => Some(ListKey::NextWaiting),
        Key::Char('c') if !ev.repeat => Some(ListKey::New),
        Key::Char('m') if !ev.repeat => Some(ListKey::Menu),
        Key::Char('a') if !ev.repeat => Some(ListKey::Alerts),
        Key::Char('y') if !ev.repeat => Some(ListKey::Approve),
        Key::Char('s') if !ev.repeat => Some(ListKey::Send),
        _ => None,
    }
}

/// A key of the panel beside the terminal while it has the keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelKey {
    Up,
    Down,
    /// Shows the picture under the list's cursor.
    Open,
    /// From a picture back to the list it was opened from; from the list back to the
    /// terminal.
    Back,
}

/// What a key routed to the panel (`Route::Panel`) does there; `None` for every other key.
pub fn panel_key(ev: &KeyEvent) -> Option<PanelKey> {
    if !ev.pressed || ev.mods != NONE {
        return None;
    }
    match ev.key {
        Key::Named(Named::Up) | Key::Char('k') => Some(PanelKey::Up),
        Key::Named(Named::Down) | Key::Char('j') => Some(PanelKey::Down),
        Key::Named(Named::Enter) if !ev.repeat => Some(PanelKey::Open),
        Key::Named(Named::Backspace | Named::Escape) if !ev.repeat => Some(PanelKey::Back),
        _ => None,
    }
}

/// Whether the panel has a use for this key, pressed, held or released.
fn panel_binds(ev: &KeyEvent) -> bool {
    let key = matches!(
        ev.key,
        Key::Named(Named::Up | Named::Down | Named::Enter | Named::Backspace | Named::Escape)
            | Key::Char('j' | 'k')
    );
    key && ev.mods == NONE
}

/// Whether a key that arrives while the panel has the keys (`Focus::Panel`) takes them back to
/// the terminal, where it is then sent: the fresh press of a key that is neither the panel's
/// (`panel_binds`) nor one of the window's own bindings. Its repeats and its release follow
/// it there by themselves; those of a key the panel did take move nothing.
pub fn leaves_panel(ev: &KeyEvent) -> bool {
    ev.pressed && !ev.repeat && !panel_binds(ev) && binding(ev, Focus::Panel).is_none()
}

/// What has the keys after a tab was closed. With no tab left it is the list. When another
/// tab came to the front it is that tab's terminal. Otherwise (a tab in the back was closed)
/// nothing moves: `j` in the list must not start to type into the session in front.
pub fn focus_after_close(focus: Focus, front_changed: bool, no_tabs: bool) -> Focus {
    match (no_tabs, front_changed) {
        (true, _) => Focus::Sidebar,
        (false, true) => Focus::Terminal,
        (false, false) => focus,
    }
}

/// What has the keys after the panel changed, closed or was laid out again. The panel keeps
/// them only while what it shows takes keys (`takes_keys`: the list of images, a picture
/// opened from it) and the layout has a panel at all (`drawn`: a window too narrow for one
/// has none, though the panel is still open). Else they go to the terminal, or with no tab
/// (`tabs`) to the list of sessions: `j` must not move a cursor that is not drawn. Whoever
/// else has them keeps them.
pub fn focus_after_panel(focus: Focus, takes_keys: bool, drawn: bool, tabs: bool) -> Focus {
    match focus {
        Focus::Panel if takes_keys && drawn => Focus::Panel,
        Focus::Panel if tabs => Focus::Terminal,
        Focus::Panel => Focus::Sidebar,
        other => other,
    }
}

/// Where the keys are: `focus`, or with a form or menu open (`modal`) where they go when it
/// closes (`focus_before`).
pub fn keys_at(focus: Focus, focus_before: Focus, modal: bool) -> Focus {
    if modal { focus_before } else { focus }
}

/// Whether the list keeps the keys when the answer to `request` is handled. `keys_were`: where
/// they were when it arrived (`keys_at`). A rename or a kill asked from the list closes or
/// reopens a tab, which brings a tab to the front; that tab must not take the keys: the next
/// `j` or Enter is meant for the list, not for the session that happens to be in front. A new
/// session's tab takes them.
pub fn list_keeps_keys(request: &Request, keys_were: Focus) -> bool {
    match request {
        Request::New(_) | Request::Answer { .. } => false,
        Request::Rename(..) | Request::Kill(_) => keys_were == Focus::Sidebar,
    }
}

/// Keeps a held key from typing into a session it was not pressed for.
///
/// The key that gives a session the keys may still be down: Enter in the list (attach), Enter
/// on an ended tab (reattach), Esc in the list. Its auto-repeats would be typed into that
/// session. `hold` is called whenever the session that takes the keys changes; from then on
/// repeats do not pass until a key is pressed afresh.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RepeatGate {
    holding: bool,
}

impl RepeatGate {
    /// The session that takes the keys changed.
    pub fn hold(&mut self) {
        self.holding = true;
    }

    /// Whether a key routed to the terminal is really sent there.
    pub fn passes(&mut self, ev: &KeyEvent) -> bool {
        if ev.pressed && !ev.repeat {
            self.holding = false;
        }
        !(ev.pressed && ev.repeat && self.holding)
    }
}

/// Every key and mouse action, grouped: the help panel (`?`) and `tmls-rs keys` print this
/// one table, so neither can drift from the other. Each description fits 31 columns.
pub const HELP: &[(&str, &[(&str, &str)])] = &[
    (
        "Everywhere",
        &[
            ("Ctrl+Q", "quit; the sessions keep running"),
            ("Ctrl+= / Ctrl+−", "font size"),
            ("Alt+Shift+→ / ←", "next / previous pinned or tab"),
            ("Alt+Shift+↑", "the keys to the session list"),
            ("Alt+Shift+N", "next that waits, else unread"),
            ("Alt+Shift+A", "send a message to this tab"),
            ("Alt+Shift+W", "close the tab (session lives)"),
            ("Alt+Shift+F", "images this session made"),
            ("Alt+Shift+X", "close the file panel"),
        ],
    ),
    (
        "List",
        &[
            ("j / k  ↑ / ↓", "move"),
            ("Enter", "attach"),
            ("Esc", "keys back to the terminal"),
            ("p", "pin or unpin (★)"),
            ("n", "next that waits, else unread"),
            ("c", "new session on this host"),
            ("m", "the session's menu"),
            ("a", "alerts"),
            ("?", "this help"),
            ("y", "approve a permission prompt"),
            ("s", "send or queue a message"),
            ("click", "attach; on + a new session"),
            ("right-click", "the session's menu"),
            ("middle-click", "pin or unpin"),
        ],
    ),
    (
        "Terminal",
        &[
            ("Ctrl+Shift+C", "copy the selection"),
            ("Ctrl+Shift+V", "paste"),
            ("Ctrl+C", "copies when text is selected"),
            ("drag", "select (Shift+drag in tmux)"),
            ("Ctrl+click", "open a link or a file"),
            ("Shift+wheel", "scroll here, not in tmux"),
            ("Enter", "reattach a tab that ended"),
        ],
    ),
    (
        "Panel",
        &[
            ("j / k", "move in the image list"),
            ("Enter", "open the picture"),
            ("Backspace / Esc", "back"),
            ("drag the divider", "the panel's width"),
        ],
    ),
    (
        "Forms",
        &[
            ("Tab / Shift+Tab", "next / previous field"),
            ("Enter", "the focused button"),
            ("Esc", "cancel"),
            ("↑ / ↓", "choose; a saved message"),
            ("n", "No, in an approval"),
            ("Ctrl+Shift+V", "paste into the field"),
        ],
    ),
];

/// Columns of a chord and of its description in the help panel.
const HELP_CHORD_COLS: usize = 20;
const HELP_WHAT_COLS: usize = 32;
/// The groups of the left column; the rest go right.
const HELP_LEFT_GROUPS: usize = 2;

/// `HELP` as plain text, for `tmls-rs keys`.
pub fn help_text() -> String {
    let mut out = String::new();
    for (group, rows) in HELP {
        out.push_str(group);
        out.push('\n');
        for (chord, what) in *rows {
            out.push_str(&format!("  {chord:<w$}{what}\n", w = HELP_CHORD_COLS));
        }
        out.push('\n');
    }
    out
}

/// `HELP` as a panel: a title line, then the groups in two columns.
pub fn help_panel() -> crate::forms::Panel {
    use crate::forms::{Look, Panel};
    let column = HELP_CHORD_COLS + HELP_WHAT_COLS;
    let cols = 2 + column + 2 + column + 2;
    let mut p = Panel::new(cols);
    p.put(
        0,
        2,
        cols - 4,
        "Keys and mouse · Esc or ? closes",
        Look::Title,
    );
    let mut bottom = 0;
    for (index, groups) in [&HELP[..HELP_LEFT_GROUPS], &HELP[HELP_LEFT_GROUPS..]]
        .into_iter()
        .enumerate()
    {
        let x = 2 + index * (column + 2);
        let mut line = 2;
        for (group, rows) in groups {
            p.put(line, x, column, group, Look::Label);
            line += 1;
            for (chord, what) in *rows {
                p.put(line, x, HELP_CHORD_COLS - 1, chord, Look::Text);
                p.put(line, x + HELP_CHORD_COLS, HELP_WHAT_COLS, what, Look::Dim);
                line += 1;
            }
            line += 1;
        }
        bottom = bottom.max(line);
    }
    p.lines = bottom;
    p
}
