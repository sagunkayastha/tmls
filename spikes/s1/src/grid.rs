//! Capture of a terminal's visible grid + non-grid state, and a structural diff.

use libghostty_vt::render::{CellIterator, RenderState, RowIterator};
use libghostty_vt::screen::{CellWide, Screen};
use libghostty_vt::style::{RgbColor, Underline};
use libghostty_vt::terminal::{Mode, Point, PointCoordinate, Terminal};
use serde::Serialize;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Cell {
    pub text: String,
    pub fg: Option<(u8, u8, u8)>,
    pub bg: Option<(u8, u8, u8)>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub strike: bool,
    pub faint: bool,
    pub wide: bool,
    /// OSC 8 hyperlink URI (via `GridRef::hyperlink_uri`); `None` = no link.
    pub hyperlink: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Grid {
    pub cols: u16,
    pub rows: u16,
    /// Row-major, `cells.len() == cols * rows`.
    pub cells: Vec<Cell>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TermState {
    pub cursor_x: u16,
    pub cursor_y: u16,
    pub cursor_visible: bool,
    /// `Terminal::is_cursor_pending_wrap` (the next print soft-wraps first).
    pub pending_wrap: bool,
    pub cursor_style: String,
    pub alt_screen: bool,
    pub modes: Vec<(String, bool)>,
    pub title: String,
    pub pwd: String,
    pub kitty_flags: u32,
    pub scrollback_rows: usize,
    pub total_rows: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Diff {
    /// (x, y, a, b)
    pub cell_mismatches: Vec<(u16, u16, Cell, Cell)>,
    /// (field, a, b)
    pub state_mismatches: Vec<(String, String, String)>,
}

impl Diff {
    pub fn is_identical(&self) -> bool {
        self.cell_mismatches.is_empty() && self.state_mismatches.is_empty()
    }
}

/// The fixed mode list captured into `TermState::modes`, in this order.
/// All 13 names from the S1 plan exist as `Mode` consts in libghostty-vt 0.2.2.
pub const MODES: &[(&str, Mode)] = &[
    ("cursor_keys", Mode::DECCKM),            // DEC 1
    ("origin", Mode::ORIGIN),                 // DEC 6
    ("autowrap", Mode::WRAPAROUND),           // DEC 7
    ("cursor_visible", Mode::CURSOR_VISIBLE), // DEC 25
    ("alt_screen", Mode::ALT_SCREEN_SAVE),    // DEC 1049
    ("bracketed_paste", Mode::BRACKETED_PASTE), // DEC 2004
    ("mouse_normal", Mode::NORMAL_MOUSE),     // DEC 1000
    ("mouse_button", Mode::BUTTON_MOUSE),     // DEC 1002
    ("mouse_any", Mode::ANY_MOUSE),           // DEC 1003
    ("mouse_sgr", Mode::SGR_MOUSE),           // DEC 1006
    ("focus_events", Mode::FOCUS_EVENT),      // DEC 1004
    ("synchronized_output", Mode::SYNC_OUTPUT), // DEC 2026
    ("grapheme_cluster", Mode::GRAPHEME_CLUSTER), // DEC 2027
];

fn rgb(c: RgbColor) -> (u8, u8, u8) {
    (c.r, c.g, c.b)
}

/// Capture the visible viewport (via the render state) and the terminal's
/// non-grid state. `&mut` is not strictly required by the 0.2.2 API
/// (`RenderState::update` takes `&Terminal`) but render-state update consumes
/// the terminal's dirty flags, so exclusive access is the honest signature.
pub fn capture(term: &mut Terminal) -> anyhow::Result<(Grid, TermState)> {
    let mut rs = RenderState::new()?;
    let mut rows_it = RowIterator::new()?;
    let mut cells_it = CellIterator::new()?;

    let snap = rs.update(term)?;
    let cols = snap.cols()?;
    let rows = snap.rows()?;
    let visual = snap.cursor_visual_style()?;
    let blinking = snap.cursor_blinking()?;

    let mut cells = Vec::with_capacity(cols as usize * rows as usize);
    let mut linked: Vec<usize> = Vec::new();
    let mut buf = String::new();
    let mut row_iter = rows_it.update(&snap)?;
    let mut y: u16 = 0;
    while let Some(row) = row_iter.next() {
        if y >= rows {
            break;
        }
        let mut cell_iter = cells_it.update(row)?;
        let mut x: u16 = 0;
        while let Some(c) = cell_iter.next() {
            if x >= cols {
                break;
            }
            buf.clear();
            if c.graphemes_len()? > 0 {
                c.graphemes_utf8(&mut buf)?;
            }
            let st = c.style()?;
            let raw = c.raw_cell()?;
            let wide = matches!(raw.wide()?, CellWide::Wide);
            if raw.has_hyperlink()? {
                linked.push(cells.len());
            }
            cells.push(Cell {
                text: buf.clone(),
                fg: c.fg_color()?.map(rgb),
                bg: c.bg_color()?.map(rgb),
                bold: st.bold,
                italic: st.italic,
                underline: st.underline != Underline::None,
                inverse: st.inverse,
                strike: st.strikethrough,
                faint: st.faint,
                wide,
                hyperlink: None,
            });
            x += 1;
        }
        // Defensive: pad a short row so the grid stays rectangular.
        while x < cols {
            cells.push(Cell::default());
            x += 1;
        }
        y += 1;
    }
    while y < rows {
        cells.extend(std::iter::repeat_n(Cell::default(), cols as usize));
        y += 1;
    }
    drop(row_iter);
    drop(snap);
    // Hyperlink URIs are not in the render state; resolve them per linked cell via GridRef.
    let mut ubuf = vec![0u8; 4096];
    for i in linked {
        let (x, y) = ((i % cols as usize) as u16, (i / cols as usize) as u32);
        let gr = term.grid_ref(Point::Viewport(PointCoordinate { x, y }))?;
        let n = loop {
            match gr.hyperlink_uri(&mut ubuf) {
                Ok(n) => break n,
                Err(libghostty_vt::error::Error::OutOfSpace { required }) => {
                    ubuf.resize(required.max(ubuf.len() * 2), 0)
                }
                Err(e) => return Err(e.into()),
            }
        };
        if n > 0 {
            cells[i].hyperlink = Some(String::from_utf8_lossy(&ubuf[..n]).into_owned());
        }
    }

    let mut modes = Vec::with_capacity(MODES.len());
    for (name, m) in MODES {
        modes.push((name.to_string(), term.mode(*m)?));
    }
    let pen = term.cursor_style()?;
    let state = TermState {
        cursor_x: term.cursor_x()?,
        cursor_y: term.cursor_y()?,
        cursor_visible: term.is_cursor_visible()?,
        pending_wrap: term.is_cursor_pending_wrap()?,
        // Shape/blink come from the render state (DECSCUSR); `Terminal::cursor_style`
        // is the SGR pen for newly printed chars, so both are folded in.
        cursor_style: format!("{visual:?};blink={blinking};pen={pen:?}"),
        alt_screen: term.active_screen()? == Screen::Alternate,
        modes,
        title: term.title()?.to_string(),
        pwd: term.pwd()?.to_string(),
        kitty_flags: u32::from(term.kitty_keyboard_flags()?.bits()),
        scrollback_rows: term.scrollback_rows()?,
        total_rows: term.total_rows()?,
    };
    Ok((Grid { cols, rows, cells }, state))
}

/// True for a cell that renders as plain default blank: text `""` or `" "`, default fg/bg, no
/// attributes, not wide.
pub fn is_plain_blank(c: &Cell) -> bool {
    (c.text.is_empty() || c.text == " ")
        && c.fg.is_none()
        && c.bg.is_none()
        && c.hyperlink.is_none()
        && !(c.bold || c.italic || c.underline || c.inverse || c.strike || c.faint || c.wide)
}

/// Map every plain blank cell ([`is_plain_blank`]) to the canonical blank (`Cell::default()`,
/// text `""`). The Formatter restores never-written cells as `" "`; visually identical, so the
/// harness compares normalised grids. Any other difference (incl. a coloured `""` vs `" "`)
/// stays a mismatch.
pub fn normalize(g: &mut Grid) {
    for c in &mut g.cells {
        if is_plain_blank(c) && !c.text.is_empty() {
            c.text.clear();
        }
    }
}

/// Structural diff. Cells are compared over the overlapping region; a size
/// mismatch is reported as a `grid_size` state mismatch.
pub fn diff(a: &(Grid, TermState), b: &(Grid, TermState)) -> Diff {
    let (ga, sa) = a;
    let (gb, sb) = b;
    let mut d = Diff::default();

    if (ga.cols, ga.rows) != (gb.cols, gb.rows) {
        d.state_mismatches.push((
            "grid_size".into(),
            format!("{}x{}", ga.cols, ga.rows),
            format!("{}x{}", gb.cols, gb.rows),
        ));
    }
    for y in 0..ga.rows.min(gb.rows) {
        for x in 0..ga.cols.min(gb.cols) {
            let ca = &ga.cells[y as usize * ga.cols as usize + x as usize];
            let cb = &gb.cells[y as usize * gb.cols as usize + x as usize];
            if ca != cb {
                d.cell_mismatches.push((x, y, ca.clone(), cb.clone()));
            }
        }
    }

    let mut field = |name: &str, va: String, vb: String| {
        if va != vb {
            d.state_mismatches.push((name.to_string(), va, vb));
        }
    };
    field("cursor_x", sa.cursor_x.to_string(), sb.cursor_x.to_string());
    field("cursor_y", sa.cursor_y.to_string(), sb.cursor_y.to_string());
    field("cursor_visible", sa.cursor_visible.to_string(), sb.cursor_visible.to_string());
    field("pending_wrap", sa.pending_wrap.to_string(), sb.pending_wrap.to_string());
    field("cursor_style", sa.cursor_style.clone(), sb.cursor_style.clone());
    field("alt_screen", sa.alt_screen.to_string(), sb.alt_screen.to_string());
    field("title", sa.title.clone(), sb.title.clone());
    field("pwd", sa.pwd.clone(), sb.pwd.clone());
    field("kitty_flags", sa.kitty_flags.to_string(), sb.kitty_flags.to_string());
    field("scrollback_rows", sa.scrollback_rows.to_string(), sb.scrollback_rows.to_string());
    field("total_rows", sa.total_rows.to_string(), sb.total_rows.to_string());
    // Modes compared by name so differing lists still line up.
    let lookup = |ms: &[(String, bool)], n: &str| {
        ms.iter().find(|(k, _)| k == n).map_or("absent".to_string(), |(_, v)| v.to_string())
    };
    let mut names: Vec<&str> = sa.modes.iter().map(|(n, _)| n.as_str()).collect();
    for (n, _) in &sb.modes {
        if !names.contains(&n.as_str()) {
            names.push(n);
        }
    }
    for n in names {
        field(&format!("mode.{n}"), lookup(&sa.modes, n), lookup(&sb.modes, n));
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use libghostty_vt::terminal::{Options, Terminal};
    fn term(cols: u16, rows: u16) -> Terminal<'static, 'static> {
        Terminal::new(Options { cols, rows, max_scrollback: 1000 }).unwrap()
    }
    #[test]
    fn capture_reads_text_and_colour() {
        let mut t = term(10, 2);
        t.vt_write(b"ab\x1b[31mc\x1b[0m\r\nxy");
        let (g, s) = capture(&mut t).unwrap();
        assert_eq!((g.cols, g.rows, g.cells.len()), (10, 2, 20));
        assert_eq!(g.cells[0].text, "a");
        assert_eq!(g.cells[2].text, "c");
        assert!(g.cells[2].fg.is_some(), "red fg captured");
        assert_eq!(g.cells[10].text, "x");
        assert_eq!((s.cursor_x, s.cursor_y), (2, 1));
    }
    #[test]
    fn wide_char_marks_spacer() {
        let mut t = term(10, 1);
        t.vt_write("日本".as_bytes());
        let (g, _) = capture(&mut t).unwrap();
        assert_eq!(g.cells[0].text, "日");
        assert!(g.cells[0].wide);
        assert_eq!(g.cells[2].text, "本");
    }
    #[test]
    fn diff_identical_and_mismatch() {
        let mut a = term(5, 1); a.vt_write(b"abc");
        let mut b = term(5, 1); b.vt_write(b"abd");
        let ca = capture(&mut a).unwrap(); let cb = capture(&mut b).unwrap();
        assert!(diff(&ca, &ca).is_identical());
        let d = diff(&ca, &cb);
        assert_eq!(d.cell_mismatches.len(), 1);
        assert_eq!(d.cell_mismatches[0].0, 2);
    }
    #[test]
    fn normalize_blank_cells() {
        let blank_empty = Cell::default();
        let blank_space = Cell { text: " ".into(), ..Cell::default() };
        let coloured_space = Cell { text: " ".into(), bg: Some((1, 2, 3)), ..Cell::default() };
        let bold_space = Cell { text: " ".into(), bold: true, ..Cell::default() };
        let x = Cell { text: "x".into(), ..Cell::default() };
        let mut a = Grid { cols: 5, rows: 1, cells: vec![blank_empty.clone(), blank_space.clone(), coloured_space.clone(), bold_space.clone(), x.clone()] };
        let mut b = Grid { cols: 5, rows: 1, cells: vec![blank_space.clone(), blank_empty.clone(), coloured_space.clone(), bold_space.clone(), x.clone()] };
        let st = TermState::default();
        assert_eq!(diff(&(a.clone(), st.clone()), &(b.clone(), st.clone())).cell_mismatches.len(), 2);
        normalize(&mut a);
        normalize(&mut b);
        assert!(diff(&(a.clone(), st.clone()), &(b.clone(), st.clone())).is_identical());
        // Non-plain blanks are untouched.
        assert_eq!(a.cells[2], coloured_space);
        assert_eq!(a.cells[3], bold_space);
        // A coloured "" vs coloured " " still differs.
        let mut c = Grid { cols: 1, rows: 1, cells: vec![Cell { bg: Some((1, 2, 3)), ..Cell::default() }] };
        normalize(&mut c);
        assert_ne!(c.cells[0], coloured_space);
    }

    #[test]
    fn pending_wrap_in_state() {
        let mut t = term(5, 2);
        t.vt_write(b"abcde");
        let (_, s) = capture(&mut t).unwrap();
        assert!(s.pending_wrap);
        let mut u = term(5, 2);
        u.vt_write(b"abcde\x1b[1;5H");
        let cu = capture(&mut u).unwrap();
        assert!(!cu.1.pending_wrap);
        let d = diff(&capture(&mut t).unwrap(), &cu);
        assert!(d.state_mismatches.iter().any(|(f, _, _)| f == "pending_wrap"), "{d:?}");
    }

    #[test]
    fn hyperlink_uri_captured() {
        let mut t = term(10, 1);
        t.vt_write(b"a\x1b]8;;http://x.example/p\x1b\\bc\x1b]8;;\x1b\\d");
        let (g, _) = capture(&mut t).unwrap();
        assert_eq!(g.cells[0].hyperlink, None);
        assert_eq!(g.cells[1].hyperlink.as_deref(), Some("http://x.example/p"));
        assert_eq!(g.cells[2].hyperlink.as_deref(), Some("http://x.example/p"));
        assert_eq!(g.cells[3].hyperlink, None);
        let mut u = term(10, 1);
        u.vt_write(b"abcd");
        assert_eq!(diff(&capture(&mut t).unwrap(), &capture(&mut u).unwrap()).cell_mismatches.len(), 2);
    }

    #[test]
    fn modes_and_alt_screen_in_state() {
        let mut t = term(5, 2);
        t.vt_write(b"\x1b[?1049h\x1b[?2004h\x1b[?25l");
        let (_, s) = capture(&mut t).unwrap();
        assert!(s.alt_screen);
        assert!(!s.cursor_visible);
        assert!(s.modes.iter().any(|(n, v)| n == "bracketed_paste" && *v));
    }
}
