//! Snapshot = libghostty-vt `Formatter` VT output of the active screen with every extra-state
//! option on (modes, scrolling region, tab stops, pwd, keyboard, cursor, style, hyperlink,
//! protection, kitty keyboard, charsets; palette optional). Restore = fresh `Terminal` +
//! `vt_write(snapshot)`.
//!
//! Deviation from the plan: the 0.2.2 Formatter emits the cursor CUP inside the screen section and
//! *then* DECSTBM / DECSLRM (which home the cursor) and the tab-stop rebuild (`CSI n G` + HTS),
//! so a raw snapshot restores the cursor at the last tab stop. [`snapshot`] therefore appends a
//! final absolute CUP (skipped when DECOM origin mode is set, since CUP is then region-relative
//! and the region is not readable through the API). [`snapshot_raw`] is the untouched Formatter
//! output, for measuring the Formatter itself.

use std::time::{Duration, Instant};

use libghostty_vt::fmt::{Format, Formatter, FormatterOptions};
use libghostty_vt::terminal::{Mode, Options, Terminal};

pub fn cup(x: u16, y: u16) -> Vec<u8> {
    format!("\x1b[{};{}H", y + 1, x + 1).into_bytes()
}

/// Plain blank viewport rows following the last content row.
pub fn trailing_blank_rows(term: &mut Terminal) -> u16 {
    let (grid, _) = crate::grid::capture(term).expect("capture");
    let cols = grid.cols as usize;
    (0..grid.rows)
        .rev()
        .take_while(|&y| grid.cells[y as usize * cols..(y as usize + 1) * cols]
            .iter().all(crate::grid::is_plain_blank))
        .count() as u16
}

#[derive(Clone, Copy, Debug)]
pub struct SnapOpts {
    /// Emit the colour palette (OSC 4) in the snapshot.
    pub palette: bool,
}

/// VT snapshot of `term`: [`snapshot_raw`] + a trailing cursor-position fixup (see module docs).
/// Panics on FFI error (spike code).
pub fn snapshot(term: &Terminal, o: SnapOpts) -> Vec<u8> {
    snapshot_unwrap(term, o, false)
}

/// [`snapshot`] with the Formatter's `unwrap` option chosen by the caller. `unwrap = true` joins
/// soft-wrapped rows into one logical line (no CRLF at the soft wrap), so a restoring terminal
/// re-wraps them itself and keeps the soft-wrap flag; used by the resize experiment.
pub fn snapshot_unwrap(term: &Terminal, o: SnapOpts, unwrap: bool) -> Vec<u8> {
    let mut s = snapshot_raw_unwrap(term, o, unwrap);
    if !term.mode(Mode::ORIGIN).expect("mode") {
        let (x, y) = (term.cursor_x().expect("cursor_x"), term.cursor_y().expect("cursor_y"));
        s.extend_from_slice(&cup(x, y));
    }
    s
}

/// The Formatter's VT output with every extra-state option on except unwrap/trim.
pub fn snapshot_raw(term: &Terminal, o: SnapOpts) -> Vec<u8> {
    snapshot_raw_unwrap(term, o, false)
}

fn snapshot_raw_unwrap(term: &Terminal, o: SnapOpts, unwrap: bool) -> Vec<u8> {
    let opts = FormatterOptions::new()
        .with_format(Format::Vt)
        .with_unwrap(unwrap)
        .with_trim(false)
        .with_palette(o.palette)
        .with_modes(true)
        .with_scrolling_region(true)
        .with_tabstops(true)
        .with_pwd(true)
        .with_keyboard(true)
        .with_cursor(true)
        .with_style(true)
        .with_hyperlink(true)
        .with_protection(true)
        .with_kitty_keyboard(true)
        .with_charsets(true);
    let mut f = Formatter::new(term, opts).expect("formatter new");
    f.format_alloc(None).expect("format_alloc").to_vec()
}

/// Fresh terminal of the given size fed with `snapshot`.
pub fn restore(cols: u16, rows: u16, max_scrollback: usize, snapshot: &[u8]) -> Terminal<'static, 'static> {
    let mut t = Terminal::new(Options { cols, rows, max_scrollback }).expect("terminal new");
    t.vt_write(snapshot);
    t
}

pub fn timed_snapshot(term: &Terminal, o: SnapOpts) -> (Vec<u8>, Duration) {
    let t0 = Instant::now();
    let s = snapshot(term, o);
    (s, t0.elapsed())
}

pub fn timed_restore(cols: u16, rows: u16, max_scrollback: usize, snapshot: &[u8]) -> (Terminal<'static, 'static>, Duration) {
    let t0 = Instant::now();
    let t = restore(cols, rows, max_scrollback, snapshot);
    (t, t0.elapsed())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_restore_plain_text_identical() {
        let mut t = Terminal::new(Options { cols: 20, rows: 3, max_scrollback: 100 }).unwrap();
        t.vt_write(b"hello\r\n\x1b[1;34mworld\x1b[0m");
        let s = snapshot(&t, SnapOpts { palette: true });
        let mut r = restore(20, 3, 100, &s);
        assert!(crate::grid::diff(&crate::grid::capture(&mut t).unwrap(), &crate::grid::capture(&mut r).unwrap()).is_identical());
    }

    #[test]
    fn timed_variants_return_same_bytes() {
        let mut t = Terminal::new(Options { cols: 20, rows: 3, max_scrollback: 100 }).unwrap();
        t.vt_write(b"abc");
        let (s, _d) = timed_snapshot(&t, SnapOpts { palette: false });
        assert_eq!(s, snapshot(&t, SnapOpts { palette: false }));
        let (mut r, _d) = timed_restore(20, 3, 100, &s);
        assert!(crate::grid::diff(&crate::grid::capture(&mut t).unwrap(), &crate::grid::capture(&mut r).unwrap()).is_identical());
    }
}

#[cfg(test)]
mod probe {
    //! `cargo test -p s1 probe_ -- --ignored --nocapture`: observations for the report.
    use super::*;
    use crate::grid::{capture, diff};

    fn show(name: &str, cols: u16, rows: u16, input: &[u8]) {
        let mut t = Terminal::new(Options { cols, rows, max_scrollback: 10_000 }).unwrap();
        t.vt_write(input);
        let raw = snapshot_raw(&t, SnapOpts { palette: false });
        let s = snapshot(&t, SnapOpts { palette: false });
        let mut r = restore(cols, rows, 10_000, &s);
        let mut rr = restore(cols, rows, 10_000, &raw);
        let (a, b, c) = (capture(&mut t).unwrap(), capture(&mut r).unwrap(), capture(&mut rr).unwrap());
        let d = diff(&a, &b);
        let draw = diff(&a, &c);
        println!("== {name}: in={} snap={} sb_orig={} sb_rest={} cells_diff={} raw_cells_diff={}",
            input.len(), s.len(), a.1.scrollback_rows, b.1.scrollback_rows,
            d.cell_mismatches.len(), draw.cell_mismatches.len());
        println!("   state: {:?}", d.state_mismatches);
        println!("   raw state: {:?}", draw.state_mismatches);
        for m in d.cell_mismatches.iter().take(2) { println!("   cell: {m:?}"); }
        println!("   tail: {:?}", String::from_utf8_lossy(&raw[raw.len().saturating_sub(160)..]));
    }

    #[test]
    #[ignore]
    fn probe_gen_streams() {
        show("tiny", 20, 3, b"hello\r\n\x1b[1;34mworld\x1b[0m");
        show("alt_active", 20, 3, b"primary\x1b[?1049halt text");
        show("pending_wrap", 5, 3, b"abcde");
        for (n, b) in crate::r#gen::adversarial() {
            show(&n, 120, 24, &b);
        }
    }
}
