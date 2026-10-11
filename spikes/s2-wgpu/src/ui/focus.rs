//! Keyboard focus, and where a key event goes: `route` is the one place that decides whether
//! a key is a terminal key.
//!
//! Three focus stops in a ring: list → field → terminal → list.
//!
//! Key scheme (in this order):
//! 1. An open context menu takes every key (arrows, Enter; anything else closes it).
//! 2. Ctrl+= / Ctrl++ zoom in, Ctrl+− / Ctrl+_ zoom out — everywhere.
//! 3. Ctrl+Tab / Ctrl+Shift+Tab move the focus forwards / backwards — everywhere.
//!    Plain Tab / Shift+Tab do the same **only outside the terminal**: with the terminal
//!    focused they are the program's (completion), like every other key.
//! 4. Terminal focused: `input::key_action`, unchanged — the result is passed through as it
//!    is, releases included.
//! 5. List or field focused: that widget's keys; everything else is ignored. Nothing reaches
//!    the terminal, so nothing is logged as a latency `key` (it would never get a frame).

use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};

use super::theme::ZoomDir;
use crate::input::{self, Action};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    List,
    Field,
    Terminal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Next,
    Prev,
}

impl Focus {
    /// The neighbour in the ring list → field → terminal → list.
    pub fn step(self, dir: Dir) -> Focus {
        match (self, dir) {
            (Focus::List, Dir::Next) | (Focus::Terminal, Dir::Prev) => Focus::Field,
            (Focus::Field, Dir::Next) | (Focus::List, Dir::Prev) => Focus::Terminal,
            (Focus::Terminal, Dir::Next) | (Focus::Field, Dir::Prev) => Focus::List,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKey {
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    /// Enter: use the selected item (here: go to the terminal).
    Activate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldKey {
    Insert(String),
    Backspace,
    Delete,
    /// `select`: Shift is held, the selection grows or shrinks.
    Left {
        select: bool,
    },
    Right {
        select: bool,
    },
    Home {
        select: bool,
    },
    End {
        select: bool,
    },
    SelectAll,
    Copy,
    Cut,
    Paste,
    /// Enter: Create.
    Submit,
    /// Escape: Cancel.
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKey {
    Up,
    Down,
    Activate,
    Close,
}

/// Where one key event goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    /// A terminal key: what `input::key_action` made of it.
    Terminal(Action),
    Focus(Dir),
    Zoom(ZoomDir),
    List(ListKey),
    Field(FieldKey),
    Menu(MenuKey),
    Ignore,
}

/// Routes one key event. The key parts are those of `input::key_action`.
pub fn route(
    focus: Focus,
    menu_open: bool,
    logical: &WinitKey,
    text: Option<&str>,
    mods: ModifiersState,
    pressed: bool,
    repeat: bool,
) -> Route {
    let ctrl = mods.control_key();
    let shift = mods.shift_key();
    let other_mods = mods.alt_key() || mods.super_key();
    let on_press = |to: Route| if pressed { to } else { Route::Ignore };

    if menu_open {
        return on_press(match logical {
            WinitKey::Named(NamedKey::ArrowUp) => Route::Menu(MenuKey::Up),
            WinitKey::Named(NamedKey::ArrowDown) => Route::Menu(MenuKey::Down),
            WinitKey::Named(NamedKey::Enter) => Route::Menu(MenuKey::Activate),
            WinitKey::Named(
                NamedKey::Shift
                | NamedKey::Control
                | NamedKey::Alt
                | NamedKey::AltGraph
                | NamedKey::Super
                | NamedKey::Meta
                | NamedKey::CapsLock
                | NamedKey::NumLock,
            ) => Route::Ignore,
            _ => Route::Menu(MenuKey::Close),
        });
    }

    if ctrl && !other_mods {
        let shortcut = match logical {
            WinitKey::Character(s) => match s.as_str() {
                "=" | "+" => Some(Route::Zoom(ZoomDir::In)),
                "-" | "_" => Some(Route::Zoom(ZoomDir::Out)),
                _ => None,
            },
            WinitKey::Named(NamedKey::Tab) => Some(Route::Focus(traversal(shift))),
            _ => None,
        };
        if let Some(shortcut) = shortcut {
            return on_press(shortcut);
        }
    }

    if focus == Focus::Terminal {
        return Route::Terminal(input::key_action(logical, text, mods, pressed, repeat));
    }
    if !pressed {
        return Route::Ignore;
    }
    if !ctrl && !other_mods && *logical == WinitKey::Named(NamedKey::Tab) {
        return Route::Focus(traversal(shift));
    }
    match focus {
        Focus::List => list_key(logical, ctrl || other_mods),
        Focus::Field => field_key(logical, text, ctrl, shift, other_mods),
        Focus::Terminal => Route::Ignore,
    }
}

fn traversal(shift: bool) -> Dir {
    if shift { Dir::Prev } else { Dir::Next }
}

fn list_key(logical: &WinitKey, chord: bool) -> Route {
    if chord {
        return Route::Ignore;
    }
    let key = match logical {
        WinitKey::Named(NamedKey::ArrowUp) => ListKey::Up,
        WinitKey::Named(NamedKey::ArrowDown) => ListKey::Down,
        WinitKey::Named(NamedKey::PageUp) => ListKey::PageUp,
        WinitKey::Named(NamedKey::PageDown) => ListKey::PageDown,
        WinitKey::Named(NamedKey::Home) => ListKey::Home,
        WinitKey::Named(NamedKey::End) => ListKey::End,
        WinitKey::Named(NamedKey::Enter) => ListKey::Activate,
        WinitKey::Character(s) if s == "k" => ListKey::Up,
        WinitKey::Character(s) if s == "j" => ListKey::Down,
        _ => return Route::Ignore,
    };
    Route::List(key)
}

fn field_key(
    logical: &WinitKey,
    text: Option<&str>,
    ctrl: bool,
    shift: bool,
    other_mods: bool,
) -> Route {
    if other_mods {
        return Route::Ignore;
    }
    let select = shift;
    let key = match logical {
        WinitKey::Character(s) if ctrl => match s.to_lowercase().as_str() {
            "a" => FieldKey::SelectAll,
            "c" => FieldKey::Copy,
            "x" => FieldKey::Cut,
            "v" => FieldKey::Paste,
            _ => return Route::Ignore,
        },
        _ if ctrl => return Route::Ignore,
        WinitKey::Character(s) => {
            // winit's `text` is what the key types (dead keys and layouts applied); the key
            // itself stands in when there is none.
            let typed = text.filter(|t| !t.is_empty()).unwrap_or(s.as_str());
            if typed.chars().any(char::is_control) {
                return Route::Ignore;
            }
            FieldKey::Insert(typed.to_string())
        }
        WinitKey::Named(NamedKey::Space) => FieldKey::Insert(" ".to_string()),
        WinitKey::Named(NamedKey::Backspace) => FieldKey::Backspace,
        WinitKey::Named(NamedKey::Delete) => FieldKey::Delete,
        WinitKey::Named(NamedKey::ArrowLeft) => FieldKey::Left { select },
        WinitKey::Named(NamedKey::ArrowRight) => FieldKey::Right { select },
        WinitKey::Named(NamedKey::Home) => FieldKey::Home { select },
        WinitKey::Named(NamedKey::End) => FieldKey::End { select },
        WinitKey::Named(NamedKey::Enter) => FieldKey::Submit,
        WinitKey::Named(NamedKey::Escape) => FieldKey::Cancel,
        _ => return Route::Ignore,
    };
    Route::Field(key)
}
