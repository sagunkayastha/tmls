//! State the Formatter does not emit. The worker owns the PTY byte stream and
//! calls [`Tracker::feed`] for every chunk; incomplete VT units stay buffered.

use libghostty_vt::screen::Screen;
use libghostty_vt::style::{Style, StyleColor, Underline};
use libghostty_vt::terminal::{Mode, Terminal};

use crate::snap::{self, SnapOpts};
use crate::splitter::{self, CutState};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tracker {
    pub title: Option<String>,
    pub pwd: Option<String>,
    pub kitty_stack: Vec<u32>,
    pub saved_cursor: Option<SavedCursor>,
    pub alt_entry: Option<AltEntry>,
    pending: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedCursor {
    pub x: u16,
    pub y: u16,
    pub pen_sgr: Vec<u8>,
    pub origin_mode: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AltEntry {
    pub mode: u16,
    pub primary_snapshot: Vec<u8>,
    pub primary_cursor: (u16, u16),
}

impl Tracker {
    pub fn feed(&mut self, term: &mut Terminal, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        let states = splitter::cut_states(&self.pending);
        let end = (1..=self.pending.len())
            .rev()
            .find(|&p| states[p] == CutState::Ground)
            .unwrap_or(0);
        if end == 0 { return; }
        let ready: Vec<u8> = self.pending.drain(..end).collect();
        let bounds = splitter::boundaries(&ready);
        for w in bounds.windows(2) {
            let unit = &ready[w[0]..w[1]];
            self.before_unit(term, unit);
            term.vt_write(unit);
        }
    }

    fn before_unit(&mut self, term: &Terminal, unit: &[u8]) {
        if unit == b"\x1b7" || unit == b"\x1b[s" {
            self.saved_cursor = Some(SavedCursor {
                x: term.cursor_x().expect("cursor x"),
                y: term.cursor_y().expect("cursor y"),
                pen_sgr: sgr_from_style(&term.cursor_style().expect("cursor style")),
                origin_mode: term.mode(Mode::ORIGIN).expect("origin mode"),
            });
        }
        if let Some(body) = unit.strip_prefix(b"\x1b[?") {
            if let Some((&final_byte, params)) = body.split_last() {
                if let Ok(mode) = std::str::from_utf8(params).unwrap_or("").parse::<u16>() {
                    if [47, 1047, 1049].contains(&mode) {
                        if final_byte == b'h' && term.active_screen().expect("screen") == Screen::Primary {
                            self.alt_entry = Some(AltEntry {
                                mode,
                                primary_snapshot: snap::snapshot(term, SnapOpts { palette: true }),
                                primary_cursor: (term.cursor_x().expect("x"), term.cursor_y().expect("y")),
                            });
                        } else if final_byte == b'l' {
                            self.alt_entry = None;
                        }
                    }
                }
            }
        }
        if unit.starts_with(b"\x1b[") && unit.last() == Some(&b'u') {
            let body = &unit[2..unit.len()-1];
            match body.first() {
                Some(b'>') => self.kitty_stack.push(parse_number(&body[1..], 0)),
                Some(b'<') => {
                    let n = parse_number(&body[1..], 1) as usize;
                    for _ in 0..n { self.kitty_stack.pop(); }
                }
                Some(b'=') => {
                    let flags = parse_number(&body[1..], 0);
                    if let Some(top) = self.kitty_stack.last_mut() { *top = flags; }
                }
                _ => {}
            }
        }
        if let Some(body) = unit.strip_prefix(b"\x1b]") {
            let body = body.strip_suffix(b"\x07")
                .or_else(|| body.strip_suffix(b"\x1b\\"))
                .unwrap_or(body);
            if let Some((kind, value)) = body.split_once_byte(b';') {
                let value = String::from_utf8_lossy(value).trim_end_matches('\0').to_string();
                match kind {
                    b"0" | b"2" => self.title = Some(value),
                    b"7" => self.pwd = Some(value),
                    _ => {}
                }
            }
        }
    }
}

trait SplitOnceByte {
    fn split_once_byte(&self, needle: u8) -> Option<(&[u8], &[u8])>;
}
impl SplitOnceByte for [u8] {
    fn split_once_byte(&self, needle: u8) -> Option<(&[u8], &[u8])> {
        let i = self.iter().position(|&b| b == needle)?;
        Some((&self[..i], &self[i + 1..]))
    }
}

fn parse_number(bytes: &[u8], default: u32) -> u32 {
    let first = bytes.split(|&b| b == b';').next().unwrap_or_default();
    if first.is_empty() { return default; }
    std::str::from_utf8(first).ok().and_then(|s| s.parse().ok()).unwrap_or(default)
}

pub fn sgr_from_style(style: &Style) -> Vec<u8> {
    let mut params: Vec<String> = vec!["0".into()];
    for (on, code) in [
        (style.bold, 1), (style.faint, 2), (style.italic, 3),
        (style.blink, 5), (style.inverse, 7), (style.invisible, 8),
        (style.strikethrough, 9), (style.overline, 53),
    ] {
        if on { params.push(code.to_string()); }
    }
    if style.underline != Underline::None { params.push("4".into()); }
    for (color, prefix) in [(style.fg_color, 38), (style.bg_color, 48), (style.underline_color, 58)] {
        match color {
            StyleColor::None => {},
            StyleColor::Palette(i) if prefix == 38 && i.0 < 8 => params.push((30 + i.0).to_string()),
            StyleColor::Palette(i) if prefix == 48 && i.0 < 8 => params.push((40 + i.0).to_string()),
            StyleColor::Palette(i) if prefix == 38 && i.0 < 16 => params.push((90 + i.0 - 8).to_string()),
            StyleColor::Palette(i) if prefix == 48 && i.0 < 16 => params.push((100 + i.0 - 8).to_string()),
            StyleColor::Palette(i) => params.push(format!("{prefix};5;{}", i.0)),
            StyleColor::Rgb(c) => params.push(format!("{prefix};2;{};{};{}", c.r, c.g, c.b)),
        }
    }
    format!("\x1b[{}m", params.join(";")).into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use libghostty_vt::terminal::Options;

    fn term() -> Terminal<'static, 'static> {
        Terminal::new(Options { cols: 20, rows: 4, max_scrollback: 2000 }).unwrap()
    }

    #[test]
    fn tracker_records_title_pwd() {
        let mut t = term();
        let mut tr = Tracker::default();
        tr.feed(&mut t, b"\x1b]0;hello\x07\x1b]7;file:///home/sgkayast\x1b\\");
        assert_eq!(tr.title.as_deref(), Some("hello"));
        assert_eq!(tr.pwd.as_deref(), Some("file:///home/sgkayast"));
    }

    #[test]
    fn tracker_kitty_stack_push_pop() {
        let mut t = term();
        let mut tr = Tracker::default();
        tr.feed(&mut t, b"\x1b[>1u\x1b[>5u\x1b[<u");
        assert_eq!(tr.kitty_stack, vec![1]);
    }

    #[test]
    fn tracker_saved_cursor_position() {
        let mut t = term();
        let mut tr = Tracker::default();
        tr.feed(&mut t, b"\x1b[1;31m\x1b[3;6H\x1b7");
        let saved = tr.saved_cursor.unwrap();
        assert_eq!((saved.x, saved.y), (5, 2));
        let sgr = String::from_utf8(saved.pen_sgr).unwrap();
        assert!(sgr.contains('1') && sgr.contains("31"), "{sgr:?}");
    }

    #[test]
    fn tracker_alt_entry_takes_primary_snapshot() {
        let mut t = term();
        let mut tr = Tracker::default();
        tr.feed(&mut t, b"primary\x1b[?1049h");
        let entry = tr.alt_entry.unwrap();
        assert_eq!(entry.mode, 1049);
        let mut restored = crate::snap::restore(20, 4, 2000, &entry.primary_snapshot);
        let (grid, _) = crate::grid::capture(&mut restored).unwrap();
        assert_eq!(grid.cells[0].text, "p");
    }

    #[test]
    fn tracker_chunking_matches_boundaries() {
        let bytes = b"primary\x1b]0;title\x1b\\\x1b[1;31m\x1b7\x1b[?1049halt\x1b[>1u";
        let mut whole = Tracker::default();
        let mut wt = term();
        whole.feed(&mut wt, bytes);
        let mut chopped = Tracker::default();
        let mut ct = term();
        for b in bytes { chopped.feed(&mut ct, std::slice::from_ref(b)); }
        assert_eq!(chopped, whole);
    }
}
