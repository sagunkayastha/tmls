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
fn physical(c: char) -> key::Key {
    let lower = c.to_ascii_lowercase();
    let code = match lower {
        'a'..='z' => key::Key::A as u32 + (lower as u32 - 'a' as u32),
        '0'..='9' => key::Key::Digit0 as u32 + (lower as u32 - '0' as u32),
        ' ' => key::Key::Space as u32,
        _ => key::Key::Unidentified as u32,
    };
    key::Key::try_from(code).unwrap_or(key::Key::Unidentified)
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
            .set_utf8(ev.text.clone())
            .set_unshifted_codepoint(unshifted);
        self.enc.set_options_from_terminal(term);
        let mut out = Vec::with_capacity(16);
        self.enc.encode_to_vec(&self.ev, &mut out)?;
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
