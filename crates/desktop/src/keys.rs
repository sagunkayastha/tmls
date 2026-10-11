//! The window's own key bindings, and where every other key goes.
//!
//! Bindings (`global`), all on the press:
//! - Alt+Shift+Right / Left — next / previous pinned (★) session; when none is pinned, tab,
//!   or session while fewer than two tabs are open (repeats while held)
//! - Alt+Shift+Up — keyboard focus to the session list; Esc in the list — back to the terminal
//! - in the list (`list_key`): j / k or the arrows move, Enter attaches, p pins, n opens the
//!   next session that waits, c opens the new-session form for the cursor's host, m the
//!   cursor's session menu (rename, kill, open in a terminal, copy), a the alerts
//! - Alt+Shift+N — the next session that waits for the user
//! - Alt+Shift+W — close the tab (tmls has no key for this; its tab bar has the `×` only)
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
}

/// What a bound key asks the window to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Global {
    NextTab,
    PrevTab,
    FocusSidebar,
    FocusTerminal,
    CloseTab,
    NextWaiting,
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
}

/// What a key routed to the list (`Route::Sidebar`) does there; `None` for every other key.
pub fn list_key(ev: &KeyEvent) -> Option<ListKey> {
    if !ev.pressed || ev.mods != NONE {
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
        _ => None,
    }
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
        Request::New(_) => false,
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
