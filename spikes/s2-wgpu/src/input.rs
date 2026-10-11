//! winit input → `s2-core` input: keys (and the two shortcuts the app keeps for itself), IME
//! commits, the mouse wheel.
//!
//! The functions take the parts of a winit event, not the event: `winit::event::KeyEvent` has a
//! private field and cannot be built in a test.

use s2_core::{Key, KeyEvent, Mods, Named};
use winit::event::MouseScrollDelta;
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};

/// What a key event means for the app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Goes to the emulator (`InputMsg::Key`).
    Send(KeyEvent),
    /// Ctrl+Shift+C: copy the selection.
    Copy,
    /// Ctrl+Shift+V: paste the clipboard.
    Paste,
    /// A modifier on its own, a key the contract has no name for, or the repeat / release of a
    /// shortcut.
    Ignore,
}

/// Maps one key event. `logical` is winit's `logical_key`, `text` its `text`, `mods` the state
/// from the last `ModifiersChanged`.
///
/// `text` rules (contract, A1 fix wave): with Ctrl held no text is passed — the core then
/// encodes from key and modifiers, where printable text would make it emit CSI-u for chords like
/// Ctrl+`[`. Named keys pass none either (winit attaches `"\r"`, `"\t"`, … to some). A release
/// never has text.
pub fn key_action(
    logical: &WinitKey,
    text: Option<&str>,
    mods: ModifiersState,
    pressed: bool,
    repeat: bool,
) -> Action {
    let (key, own_text) = match logical {
        WinitKey::Character(s) => match s.chars().next() {
            Some(first) => (Key::Char(first), Some(s.as_str())),
            None => return Action::Ignore,
        },
        WinitKey::Named(NamedKey::Space) => (Key::Char(' '), Some(" ")),
        WinitKey::Named(named) => match contract_name(*named) {
            Some(named) => (Key::Named(named), None),
            None => return Action::Ignore,
        },
        WinitKey::Unidentified(_) | WinitKey::Dead(_) => return Action::Ignore,
    };

    let chord = ModifiersState::CONTROL | ModifiersState::SHIFT;
    if mods == chord {
        let shortcut = match key {
            Key::Char('c' | 'C') => Some(Action::Copy),
            Key::Char('v' | 'V') => Some(Action::Paste),
            _ => None,
        };
        if let Some(shortcut) = shortcut {
            return if pressed && !repeat {
                shortcut
            } else {
                Action::Ignore
            };
        }
    }

    let text = match (pressed, mods.control_key(), key) {
        (false, ..) | (_, true, _) | (.., Key::Named(_)) => None,
        // winit has no text for some chords (Alt on some platforms): the character stands in.
        (true, false, Key::Char(_)) => text.filter(|t| !t.is_empty()).or(own_text),
    };
    Action::Send(KeyEvent {
        key,
        mods: Mods {
            shift: mods.shift_key(),
            ctrl: mods.control_key(),
            alt: mods.alt_key(),
            logo: mods.super_key(),
        },
        text: text.map(str::to_string),
        repeat: pressed && repeat,
        pressed,
    })
}

fn contract_name(named: NamedKey) -> Option<Named> {
    Some(match named {
        NamedKey::Enter => Named::Enter,
        NamedKey::Tab => Named::Tab,
        NamedKey::Backspace => Named::Backspace,
        NamedKey::Escape => Named::Escape,
        NamedKey::ArrowUp => Named::Up,
        NamedKey::ArrowDown => Named::Down,
        NamedKey::ArrowLeft => Named::Left,
        NamedKey::ArrowRight => Named::Right,
        NamedKey::Home => Named::Home,
        NamedKey::End => Named::End,
        NamedKey::PageUp => Named::PageUp,
        NamedKey::PageDown => Named::PageDown,
        NamedKey::Insert => Named::Insert,
        NamedKey::Delete => Named::Delete,
        NamedKey::F1 => Named::F(1),
        NamedKey::F2 => Named::F(2),
        NamedKey::F3 => Named::F(3),
        NamedKey::F4 => Named::F(4),
        NamedKey::F5 => Named::F(5),
        NamedKey::F6 => Named::F(6),
        NamedKey::F7 => Named::F(7),
        NamedKey::F8 => Named::F(8),
        NamedKey::F9 => Named::F(9),
        NamedKey::F10 => Named::F(10),
        NamedKey::F11 => Named::F(11),
        NamedKey::F12 => Named::F(12),
        _ => return None,
    })
}

/// `Ime::Commit(text)` as one key event: the core has no "raw bytes" input, and a press whose
/// `text` is the whole composed string is encoded as exactly that string (so it gets a key id
/// and a latency line like any key). `Paste` would wrap it in bracketed-paste markers.
pub fn ime_commit(text: &str) -> Option<KeyEvent> {
    let first = text.chars().next()?;
    Some(KeyEvent {
        key: Key::Char(first),
        mods: Mods::default(),
        text: Some(text.to_string()),
        repeat: false,
        pressed: true,
    })
}

/// Rows per wheel notch.
pub const WHEEL_LINES: f32 = 3.0;

/// Wheel delta → rows for `InputMsg::Scroll`: negative is up, into the scrollback (the wheel
/// turned away from the user, winit's positive y). `carry` holds the fraction of a row not yet
/// scrolled (touchpads report pixels); keep one per window.
pub fn wheel_rows(delta: &MouseScrollDelta, cell_height: f32, carry: &mut f32) -> i32 {
    let rows = match delta {
        MouseScrollDelta::LineDelta(_, y) => y * WHEEL_LINES,
        MouseScrollDelta::PixelDelta(position) if cell_height >= 1.0 => {
            position.y as f32 / cell_height
        }
        MouseScrollDelta::PixelDelta(_) => 0.0,
    };
    if !rows.is_finite() {
        return 0;
    }
    *carry += rows;
    let whole = carry.trunc();
    *carry -= whole;
    -(whole.clamp(-10_000.0, 10_000.0) as i32)
}
