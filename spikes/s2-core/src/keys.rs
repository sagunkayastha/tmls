//! Toolkit-neutral key events and their encoding to PTY bytes via libghostty-vt's key encoder.
//! Front-ends map winit/gpui events into `KeyEvent`.

use libghostty_vt::terminal::Terminal;
use libghostty_vt::{key, paste};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub mods: Mods,
    /// Text the key produced, if any (already layout- and shift-translated).
    pub text: Option<String>,
    pub repeat: bool,
    pub pressed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Named(Named),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Named {
    Enter,
    Tab,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    /// F1–F12.
    F(u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub logo: bool,
}

/// Physical key for `c` on a US layout; `Unidentified` leaves the encoder to use the text.
/// Punctuation and shifted digits are mapped too: the legacy encoding of Ctrl + `[ \ ] / _ @ ^`
/// is chosen by physical key, and without it those keys send nothing.
fn physical(c: char) -> key::Key {
    use key::Key as K;
    const DIGITS: [K; 10] =
        [K::Digit0, K::Digit1, K::Digit2, K::Digit3, K::Digit4, K::Digit5, K::Digit6, K::Digit7, K::Digit8, K::Digit9];
    let lower = c.to_ascii_lowercase();
    match lower {
        'a'..='z' => K::try_from(K::A as u32 + (lower as u32 - 'a' as u32)).unwrap_or(K::Unidentified),
        '0'..='9' => DIGITS[lower as usize - '0' as usize],
        ' ' => K::Space,
        '`' | '~' => K::Backquote,
        '-' | '_' => K::Minus,
        '=' | '+' => K::Equal,
        '[' | '{' => K::BracketLeft,
        ']' | '}' => K::BracketRight,
        '\\' | '|' => K::Backslash,
        ';' | ':' => K::Semicolon,
        '\'' | '"' => K::Quote,
        ',' | '<' => K::Comma,
        '.' | '>' => K::Period,
        '/' | '?' => K::Slash,
        _ => ")!@#$%^&*(".find(c).map_or(K::Unidentified, |digit| DIGITS[digit]),
    }
}

/// Text the encoder may use. A front-end can report the control character a chord produced
/// (winit gives `"\x03"` for Ctrl+C); the encoder would treat that as printable text and emit a
/// CSI-u sequence, so C0 and DEL text is dropped and the key and modifiers decide.
fn printable(text: Option<&str>) -> Option<&str> {
    text.filter(|t| !t.is_empty() && !t.chars().any(|c| c.is_ascii_control()))
}

fn named(n: Named) -> key::Key {
    match n {
        Named::Enter => key::Key::Enter,
        Named::Tab => key::Key::Tab,
        Named::Backspace => key::Key::Backspace,
        Named::Escape => key::Key::Escape,
        Named::Up => key::Key::ArrowUp,
        Named::Down => key::Key::ArrowDown,
        Named::Left => key::Key::ArrowLeft,
        Named::Right => key::Key::ArrowRight,
        Named::Home => key::Key::Home,
        Named::End => key::Key::End,
        Named::PageUp => key::Key::PageUp,
        Named::PageDown => key::Key::PageDown,
        Named::Insert => key::Key::Insert,
        Named::Delete => key::Key::Delete,
        Named::F(n) => key::Key::try_from(key::Key::F1 as u32 + u32::from(n.clamp(1, 12)) - 1)
            .unwrap_or(key::Key::Unidentified),
    }
}

/// The C0 byte every terminal sends for Ctrl + `c`, for the chords libghostty-vt's legacy encoder
/// leaves empty when the event has no text: it follows the fixterms proposal and keeps
/// Ctrl+`[`, Ctrl+`i` and Ctrl+`m` out of its control table, and it does not resolve Ctrl+Shift
/// chords (`_`, `@`, …) from the key alone. Used only when the encoder emitted nothing for a
/// press, so kitty-protocol output is never replaced. (With Alt the encoder always emits its
/// ESC-prefixed form, so Alt chords never get here.)
fn classic_ctrl(ev: &KeyEvent) -> Option<u8> {
    let Key::Char(c) = ev.key else { return None };
    if !ev.mods.ctrl || ev.mods.alt || ev.mods.logo {
        return None;
    }
    match c {
        '@'..='_' | 'a'..='z' => Some(c as u8 & 0x1f),
        ' ' => Some(0x00),
        '?' => Some(0x7f),
        _ => None,
    }
}

/// Owns the libghostty-vt encoder; not `Send`, so it lives on the emulator thread.
pub struct Encoder {
    enc: key::Encoder<'static>,
    ev: key::Event<'static>,
}

impl Encoder {
    pub fn new() -> anyhow::Result<Encoder> {
        Ok(Encoder { enc: key::Encoder::new()?, ev: key::Event::new()? })
    }

    /// Bytes for `ev` under the terminal's current modes (kitty flags, DECCKM, …).
    /// Empty when the key sends nothing (a bare release in legacy mode, for example).
    pub fn encode(&mut self, ev: &KeyEvent, term: &Terminal) -> anyhow::Result<Vec<u8>> {
        let action = match (ev.pressed, ev.repeat) {
            (false, _) => key::Action::Release,
            (true, true) => key::Action::Repeat,
            (true, false) => key::Action::Press,
        };
        let mut mods = key::Mods::empty();
        mods.set(key::Mods::SHIFT, ev.mods.shift);
        mods.set(key::Mods::CTRL, ev.mods.ctrl);
        mods.set(key::Mods::ALT, ev.mods.alt);
        mods.set(key::Mods::SUPER, ev.mods.logo);
        let (code, unshifted) = match ev.key {
            Key::Char(c) => (physical(c), c.to_lowercase().next().unwrap_or(c)),
            Key::Named(n) => (named(n), '\0'),
        };
        self.ev
            .set_action(action)
            .set_key(code)
            .set_mods(mods)
            .set_consumed_mods(key::Mods::empty())
            .set_composing(false)
            .set_utf8(printable(ev.text.as_deref()))
            .set_unshifted_codepoint(unshifted);
        self.enc.set_options_from_terminal(term);
        let mut out = Vec::with_capacity(16);
        self.enc.encode_to_vec(&self.ev, &mut out)?;
        if out.is_empty() && ev.pressed {
            out.extend(classic_ctrl(ev));
        }
        Ok(out)
    }
}

/// Paste bytes: bracketed when the terminal has mode 2004 set; unsafe control bytes replaced.
pub fn paste_bytes(text: &str, bracketed: bool) -> anyhow::Result<Vec<u8>> {
    let mut data = text.as_bytes().to_vec();
    let mut buf = vec![0u8; data.len() + 16];
    loop {
        match paste::encode(&mut data, bracketed, &mut buf) {
            Ok(n) => {
                buf.truncate(n);
                return Ok(buf);
            }
            Err(libghostty_vt::error::Error::OutOfSpace { required }) => buf.resize(required, 0),
            Err(e) => return Err(e.into()),
        }
    }
}
