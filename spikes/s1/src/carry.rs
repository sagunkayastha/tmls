//! State the Formatter does not emit. The worker owns the PTY byte stream and
//! calls [`Tracker::feed`] for every chunk; incomplete VT units stay buffered.

use libghostty_vt::screen::Screen;
use libghostty_vt::style::{Style, StyleColor, Underline};
use libghostty_vt::terminal::{Mode, Terminal};

use crate::grid::{self, Cell};
use crate::snap::{self, SnapOpts};
use crate::splitter::{self, CutState};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tracker {
    pub title: Option<String>,
    pub pwd: Option<String>,
    pub kitty_stack: Vec<u32>,
    pub saved_cursor: Option<SavedCursor>,
    pub alt_entry: Option<AltEntry>,
    pub active_hyperlink: Option<String>,
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
                let alt_modes = params.split(|&b| b == b';')
                    .filter_map(|p| std::str::from_utf8(p).ok()?.parse::<u16>().ok())
                    .filter(|mode| [47, 1047, 1049].contains(mode))
                    .collect::<Vec<_>>();
                if final_byte == b'h' && term.active_screen().expect("screen") == Screen::Primary {
                    if let Some(&mode) = alt_modes.first() {
                        self.alt_entry = Some(AltEntry {
                            mode,
                            primary_snapshot: snap::snapshot(term, SnapOpts { palette: true }),
                            primary_cursor: (term.cursor_x().expect("x"), term.cursor_y().expect("y")),
                        });
                    }
                } else if final_byte == b'l' && self.alt_entry.as_ref()
                    .is_some_and(|entry| alt_modes.contains(&entry.mode)) {
                    self.alt_entry = None;
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
                    b"8" => {
                        let uri = value.split_once(';').map(|(_, uri)| uri).unwrap_or("");
                        self.active_hyperlink = if uri.is_empty() { None } else { Some(uri.to_string()) };
                    }
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

fn sgr_from_cell(cell: &Cell) -> Vec<u8> {
    let mut p = vec!["0".to_string()];
    for (on, code) in [
        (cell.bold, 1), (cell.faint, 2), (cell.italic, 3),
        (cell.underline, 4), (cell.inverse, 7), (cell.strike, 9),
    ] { if on { p.push(code.to_string()); } }
    if let Some((r, g, b)) = cell.fg { p.push(format!("38;2;{r};{g};{b}")); }
    if let Some((r, g, b)) = cell.bg { p.push(format!("48;2;{r};{g};{b}")); }
    format!("\x1b[{}m", p.join(";")).into_bytes()
}

/// Formatter VT plus state the Formatter drops. All overlays use absolute
/// screen positions; the final sequence restores the live pen and cursor.
pub fn snapshot_full(term: &mut Terminal, tr: &Tracker, opts: SnapOpts) -> Vec<u8> {
    let on_alt = term.active_screen().expect("screen") == Screen::Alternate;
    let mut out = Vec::new();
    if on_alt {
        if let Some(entry) = &tr.alt_entry {
            out.extend_from_slice(&entry.primary_snapshot);
            out.extend_from_slice(format!("\x1b[?{}h", entry.mode).as_bytes());
        }
    }
    out.extend_from_slice(&snap::snapshot(term, opts));
    // A primary-screen prefix may enable modes which the active-screen
    // Formatter does not explicitly disable. Reconcile the compared modes.
    for (code, mode) in [
        (1, Mode::DECCKM), (7, Mode::WRAPAROUND), (25, Mode::CURSOR_VISIBLE),
        (1000, Mode::NORMAL_MOUSE), (1002, Mode::BUTTON_MOUSE),
        (1003, Mode::ANY_MOUSE), (1004, Mode::FOCUS_EVENT),
        (1006, Mode::SGR_MOUSE), (2004, Mode::BRACKETED_PASTE),
        (2026, Mode::SYNC_OUTPUT), (2027, Mode::GRAPHEME_CLUSTER),
    ] {
        let suffix = if term.mode(mode).expect("mode") { 'h' } else { 'l' };
        out.extend_from_slice(format!("\x1b[?{code}{suffix}").as_bytes());
    }
    // The Formatter can omit blank viewport rows, but replaying every blank
    // row also scrolls a fresh empty terminal. Measure the row deficit before
    // adding only the rows that the reconstruction actually dropped.
    let baseline = snap::restore(
        term.cols().expect("cols"), term.rows().expect("rows"), 2000, &out,
    );
    let deficit = term.scrollback_rows().expect("scrollback").saturating_sub(
        baseline.scrollback_rows().expect("restored scrollback"),
    );
    let pad = snap::trailing_blank_rows(term).min(deficit as u16);
    for _ in 0..pad { out.extend_from_slice(b"\r\n"); }

    let (grid, _) = grid::capture(term).expect("capture");
    let origin = term.mode(Mode::ORIGIN).expect("origin mode");
    if origin { out.extend_from_slice(b"\x1b[?6l"); }
    let mut i = 0usize;
    while i < grid.cells.len() {
        let Some(uri) = grid.cells[i].hyperlink.as_deref() else { i += 1; continue };
        let row = i / grid.cols as usize;
        let mut end = i + 1;
        while end < grid.cells.len()
            && end / grid.cols as usize == row
            && grid.cells[end].hyperlink.as_deref() == Some(uri) { end += 1; }
        out.extend_from_slice(&snap::cup((i % grid.cols as usize) as u16, row as u16));
        out.extend_from_slice(b"\x1b]8;;");
        out.extend_from_slice(uri.as_bytes());
        out.extend_from_slice(b"\x1b\\");
        for cell in &grid.cells[i..end] {
            if cell.wide && cell.text.is_empty() { continue; }
            out.extend_from_slice(&sgr_from_cell(cell));
            out.extend_from_slice(if cell.text.is_empty() { b" " } else { cell.text.as_bytes() });
        }
        out.extend_from_slice(b"\x1b]8;;\x1b\\");
        i = end;
    }
    erase_overlay(&mut out, &grid);
    if let Some(title) = &tr.title {
        out.extend_from_slice(b"\x1b]0;"); out.extend_from_slice(title.as_bytes()); out.extend_from_slice(b"\x1b\\");
    }
    if let Some(pwd) = &tr.pwd {
        out.extend_from_slice(b"\x1b]7;"); out.extend_from_slice(pwd.as_bytes()); out.extend_from_slice(b"\x1b\\");
    }
    if !tr.kitty_stack.is_empty() {
        // Formatter emits the current flags as a set operation on the base
        // entry. Reset that base before rebuilding the worker's push stack.
        out.extend_from_slice(b"\x1b[=0;1u");
    }
    for &flags in &tr.kitty_stack { out.extend_from_slice(format!("\x1b[>{flags}u").as_bytes()); }
    if let Some(saved) = &tr.saved_cursor {
        out.extend_from_slice(&snap::cup(saved.x, saved.y));
        out.extend_from_slice(&saved.pen_sgr);
        out.extend_from_slice(b"\x1b7\x1b[0m");
    }
    let x = term.cursor_x().expect("cursor x");
    let y = term.cursor_y().expect("cursor y");
    out.extend_from_slice(&snap::cup(x, y));
    if term.is_cursor_pending_wrap().expect("pending wrap") {
        let mut replay_x = x;
        if x > 0 && grid.cells[y as usize * grid.cols as usize + (x - 1) as usize].wide {
            replay_x -= 1;
            out.extend_from_slice(&snap::cup(replay_x, y));
        }
        let cell = &grid.cells[y as usize * grid.cols as usize + replay_x as usize];
        out.extend_from_slice(&sgr_from_cell(cell));
        out.extend_from_slice(cell.text.as_bytes());
    }
    out.extend_from_slice(&sgr_from_style(&term.cursor_style().expect("cursor style")));
    if origin {
        out.extend_from_slice(b"\x1b[?6h");
        let top = scroll_top_from_vt(&out);
        out.extend_from_slice(&snap::cup(x, y.saturating_sub(top)));
    }
    if let Some(uri) = &tr.active_hyperlink {
        out.extend_from_slice(b"\x1b]8;;");
        out.extend_from_slice(uri.as_bytes());
        out.extend_from_slice(b"\x1b\\");
    }
    out
}

fn erase_overlay(out: &mut Vec<u8>, grid: &grid::Grid) {
    let cols = grid.cols as usize;
    for row in 0..grid.rows as usize {
        let cells = &grid.cells[row * cols..(row + 1) * cols];
        let erasable = |x: usize| cells[x].text.is_empty() && !cells[x].wide
            && (x == 0 || !cells[x - 1].wide);
        let mut col = 0;
        while col < cols {
            if !erasable(col) { col += 1; continue; }
            let start = col;
            let bg = cells[col].bg;
            while col < cols && erasable(col) && cells[col].bg == bg {
                col += 1;
            }
            if bg.is_none() && !cells[col..].iter().any(|c| !c.text.is_empty()) { continue; }
            out.extend_from_slice(&snap::cup(start as u16, row as u16));
            out.extend_from_slice(b"\x1b[0m");
            if let Some((r, g, b)) = bg {
                out.extend_from_slice(format!("\x1b[48;2;{r};{g};{b}m").as_bytes());
            }
            out.extend_from_slice(format!("\x1b[{}X\x1b[0m", col - start).as_bytes());
        }
    }
}

pub fn overlay_run_count(term: &mut Terminal) -> usize {
    let (grid, _) = grid::capture(term).expect("capture");
    grid.cells.iter().enumerate().filter(|(i, c)| {
        c.hyperlink.is_some() && (*i == 0 || *i % grid.cols as usize == 0
            || grid.cells[*i - 1].hyperlink != c.hyperlink)
    }).count()
}

fn scroll_top_from_vt(bytes: &[u8]) -> u16 {
    let mut top = 0;
    for chunk in bytes.split(|&b| b == 0x1b) {
        if let Some(body) = chunk.strip_prefix(b"[") {
            if let Some(pos) = body.iter().position(|&b| b == b'r') {
                let n = std::str::from_utf8(&body[..pos]).ok().and_then(|s| s.split(';').next())
                    .and_then(|s| s.parse::<u16>().ok()).unwrap_or(1);
                top = n.saturating_sub(1);
            }
        }
    }
    top
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
    fn combined_private_modes_preserve_primary_on_alt_exit() {
        let mut t = term();
        let mut tr = Tracker::default();
        tr.feed(&mut t, b"primary\x1b[?1049;25halt");
        assert_eq!(tr.alt_entry.as_ref().map(|entry| entry.mode), Some(1049));
        let snapshot = snapshot_full(&mut t, &tr, SnapOpts { palette: true });
        tr.feed(&mut t, b"\x1b[?1049;25lmore");
        assert!(tr.alt_entry.is_none());
        let mut restored = crate::snap::restore(20, 4, 2000, &snapshot);
        restored.vt_write(b"\x1b[?1049;25lmore");
        let mut a = crate::grid::capture(&mut t).unwrap();
        let mut b = crate::grid::capture(&mut restored).unwrap();
        crate::grid::normalize(&mut a.0);
        crate::grid::normalize(&mut b.0);
        assert!(crate::grid::diff(&a, &b).is_identical());
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

    fn roundtrip(bytes: &[u8], cols: u16, rows: u16) -> (Terminal<'static, 'static>, Terminal<'static, 'static>) {
        let mut original = Terminal::new(Options { cols, rows, max_scrollback: 2000 }).unwrap();
        let mut tr = Tracker::default();
        tr.feed(&mut original, bytes);
        let snapshot = snapshot_full(&mut original, &tr, SnapOpts { palette: true });
        let restored = crate::snap::restore(cols, rows, 2000, &snapshot);
        (original, restored)
    }

    #[test]
    fn trailing_blank_rows_padded() {
        let mut stream = Vec::new();
        for i in 0..300 { stream.extend_from_slice(format!("line{i:03}\r\n").as_bytes()); }
        let (original, restored) = roundtrip(&stream, 20, 24);
        assert_eq!(restored.scrollback_rows().unwrap(), original.scrollback_rows().unwrap());
        assert_eq!(crate::experiment::history_text(&restored), crate::experiment::history_text(&original));
    }

    #[test]
    fn empty_viewport_does_not_gain_history() {
        let (original, restored) = roundtrip(b"", 20, 24);
        assert_eq!(restored.scrollback_rows().unwrap(), original.scrollback_rows().unwrap());
    }

    #[test]
    fn hyperlink_overlay_restores_uris() {
        let bytes = b"plain \x1b]8;;https://one.example\x1b\\first\x1b]8;;\x1b\\ gap \x1b]8;;https://two.example\x1b\\second\x1b]8;;\x1b\\";
        let (mut original, mut restored) = roundtrip(bytes, 40, 4);
        let (a, _) = crate::grid::capture(&mut original).unwrap();
        let (b, _) = crate::grid::capture(&mut restored).unwrap();
        assert_eq!(a.cells.iter().map(|c| &c.hyperlink).collect::<Vec<_>>(), b.cells.iter().map(|c| &c.hyperlink).collect::<Vec<_>>());
        assert!(a.cells.iter().any(|c| c.hyperlink.as_deref() == Some("https://one.example")));
    }

    #[test]
    fn pending_wrap_reemitted() {
        let (mut original, mut restored) = roundtrip(b"12345", 5, 3);
        assert!(restored.is_cursor_pending_wrap().unwrap());
        original.vt_write(b"x");
        restored.vt_write(b"x");
        assert!(crate::grid::diff(&crate::grid::capture(&mut original).unwrap(), &crate::grid::capture(&mut restored).unwrap()).is_identical());
    }

    #[test]
    fn title_pwd_restored() {
        let (original, restored) = roundtrip(b"\x1b]0;my title\x07\x1b]7;file:///home/sgkayast\x1b\\", 20, 4);
        assert_eq!(restored.title().unwrap(), original.title().unwrap());
        assert_eq!(restored.pwd().unwrap(), original.pwd().unwrap());
    }

    #[test]
    fn kitty_stack_restored() {
        let (mut original, mut restored) = roundtrip(b"\x1b[>1u\x1b[>5u", 20, 4);
        for step in 0..3 {
            assert_eq!(restored.kitty_keyboard_flags().unwrap(), original.kitty_keyboard_flags().unwrap(), "after {step} pops");
            original.vt_write(b"\x1b[<u");
            restored.vt_write(b"\x1b[<u");
        }
    }

    #[test]
    fn decom_cup_ok() {
        let (original, restored) = roundtrip(b"\x1b[2;4r\x1b[?6h\x1b[2;5H", 20, 5);
        assert_eq!((restored.cursor_x().unwrap(), restored.cursor_y().unwrap()), (original.cursor_x().unwrap(), original.cursor_y().unwrap()));
    }
    #[test]
    fn ech_overlay_restores_erased_bg_after_ed() {
        let (mut original, mut restored) = roundtrip(b"\x1b[1;1Habcdefgh\x1b[2;1Hijklmnop\x1b[48;2;181;189;104m\x1b[2;4H\x1b[2J\x1b[0m", 8, 3);
        assert!(crate::grid::diff(&crate::grid::capture(&mut original).unwrap(), &crate::grid::capture(&mut restored).unwrap()).is_identical());
    }

    #[test]
    fn ech_overlay_restores_empty_identity_after_el() {
        let (mut original, mut restored) = roundtrip(b"\x1b[1;1Habcdefgh\x1b[2;1Hijklmnop\x1b[48;2;181;189;104m\x1b[2;4H\x1b[0K\x1b[0m", 8, 3);
        assert!(crate::grid::diff(&crate::grid::capture(&mut original).unwrap(), &crate::grid::capture(&mut restored).unwrap()).is_identical());
    }

    #[test]
    fn ech_overlay_leaves_trailing_default_blanks_alone() {
        let mut original = term();
        let mut tr = Tracker::default();
        tr.feed(&mut original, b"short");
        let snapshot = snapshot_full(&mut original, &tr, SnapOpts { palette: true });
        assert!(!snapshot.windows(2).any(|w| w == b"[X"));
        assert!(!snapshot.windows(2).any(|w| w == b"1X"));
    }


}
