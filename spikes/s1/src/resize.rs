//! Resize-at-split experiment: does a snapshot restored into a terminal of a *different* size
//! (e.g. a client attaching at 100×30) end up where a live terminal resized the same way does?
//!
//! `q` = the splitter boundary nearest the middle of the stream; A = fresh terminal (original
//! size) fed `bytes[..q]`; S = [`snap::snapshot`] of A. Variants (D vs C, `grid::diff` after
//! `normalize`, plus history text):
//!
//! - `baseline`: no resize. D = all bytes; C = restore(S) at orig, tail. (= Task 4 Aligned case.)
//! - `restore_small`: D = `[..q]`, resize(R), `[q..]`, resize(orig);
//!   C = restore(S) into R, `[q..]`, resize(orig).
//! - `restore_orig_resize_before_tail`: same D; C = restore(S) at orig, resize(R), `[q..]`,
//!   resize(orig). Separates "replaying S into a different size" from "reflowing restored rows".
//! - `restore_orig_resize_after_tail`: D = all bytes, resize(R), resize(orig);
//!   C = restore(S) at orig, `[q..]`, resize(R), resize(orig).
//!
//! - `restore_small_unwrap`: as `restore_small`, but S is taken with the Formatter's `unwrap`
//!   option on ([`snap::snapshot_unwrap`]), so soft-wrapped rows are emitted as one logical line.
//! - `restore_orig_resize_before_tail_unwrap`: as `restore_orig_resize_before_tail` with that S.
//!
//! `d_resize_roundtrip_identical` records whether the resize pair itself is lossless on D
//! (D with resizes vs D without).

use libghostty_vt::screen::Screen;
use libghostty_vt::terminal::{Options, Terminal};
use serde::Serialize;

use crate::experiment::{classify_aligned, describe_first_mismatch, history_text};
use crate::grid::{self, Grid, TermState};
use crate::snap::{self, SnapOpts};
use crate::splitter;

pub const CELL_W: u32 = 8;
pub const CELL_H: u32 = 16;

pub const VARIANTS: &[&str] = &[
    "baseline", "restore_small", "restore_orig_resize_before_tail", "restore_orig_resize_after_tail",
    "restore_small_unwrap", "restore_orig_resize_before_tail_unwrap",
];

#[derive(Clone, Debug, Default, Serialize)]
pub struct ResizeRecord {
    pub stream: String,
    pub cols: u16,
    pub rows: u16,
    pub to_cols: u16,
    pub to_rows: u16,
    pub stream_bytes: usize,
    pub split: usize,
    /// A (the snapshotted terminal at `split`) was on the alternate screen.
    pub a_alt_screen: bool,
    pub variant: String,
    pub identical: bool,
    pub cell_mismatches: usize,
    pub state_mismatches: Vec<String>,
    pub history_equal: bool,
    pub d_history_rows: usize,
    pub c_history_rows: usize,
    pub class: Option<String>,
    pub class_detail: Option<String>,
    pub first_mismatch: Option<String>,
    pub d_resize_roundtrip_identical: bool,
}

fn term(cols: u16, rows: u16, sb: u32) -> Terminal<'static, 'static> {
    Terminal::new(Options { cols, rows, max_scrollback: sb as usize }).expect("terminal new")
}

fn rs(t: &mut Terminal, cols: u16, rows: u16) {
    t.resize(cols, rows, CELL_W, CELL_H).expect("resize");
}

fn cap(t: &mut Terminal) -> ((Grid, TermState), Vec<String>) {
    let mut c = grid::capture(t).expect("capture");
    grid::normalize(&mut c.0);
    let h = history_text(t);
    (c, h)
}

/// Splitter boundary nearest `len / 2` (ties: the earlier one).
pub fn middle_boundary(bytes: &[u8]) -> usize {
    let mid = bytes.len() / 2;
    splitter::boundaries(bytes).into_iter().min_by_key(|&b| (b.abs_diff(mid), b)).unwrap_or(0)
}

/// Run all [`VARIANTS`] for one stream at `q` with the intermediate size `(to_cols, to_rows)`.
pub fn run_stream(name: &str, bytes: &[u8], cols: u16, rows: u16, to_cols: u16, to_rows: u16, sb: u32, q: usize) -> Vec<ResizeRecord> {
    let (head, tail) = bytes.split_at(q);
    let mut a = term(cols, rows, sb);
    a.vt_write(head);
    let a_alt = a.active_screen().expect("active_screen") == Screen::Alternate;
    let s = snap::snapshot(&a, SnapOpts { palette: false });
    let su = snap::snapshot_unwrap(&a, SnapOpts { palette: false }, true);

    // References.
    let mut d0 = term(cols, rows, sb);
    d0.vt_write(bytes);
    let d0c = cap(&mut d0);
    let mut d1 = term(cols, rows, sb);
    d1.vt_write(head);
    rs(&mut d1, to_cols, to_rows);
    d1.vt_write(tail);
    rs(&mut d1, cols, rows);
    let d1c = cap(&mut d1);
    let mut d2 = term(cols, rows, sb);
    d2.vt_write(bytes);
    rs(&mut d2, to_cols, to_rows);
    rs(&mut d2, cols, rows);
    let d2c = cap(&mut d2);

    let mut out = Vec::new();
    for &v in VARIANTS {
        let (d, c, d_rt) = match v {
            "baseline" => {
                let mut c = snap::restore(cols, rows, sb as usize, &s);
                c.vt_write(tail);
                (&d0c, cap(&mut c), true)
            }
            "restore_small" => {
                let mut c = snap::restore(to_cols, to_rows, sb as usize, &s);
                c.vt_write(tail);
                rs(&mut c, cols, rows);
                (&d1c, cap(&mut c), grid::diff(&d0c.0, &d1c.0).is_identical() && d0c.1 == d1c.1)
            }
            "restore_orig_resize_before_tail" => {
                let mut c = snap::restore(cols, rows, sb as usize, &s);
                rs(&mut c, to_cols, to_rows);
                c.vt_write(tail);
                rs(&mut c, cols, rows);
                (&d1c, cap(&mut c), grid::diff(&d0c.0, &d1c.0).is_identical() && d0c.1 == d1c.1)
            }
            "restore_orig_resize_after_tail" => {
                let mut c = snap::restore(cols, rows, sb as usize, &s);
                c.vt_write(tail);
                rs(&mut c, to_cols, to_rows);
                rs(&mut c, cols, rows);
                (&d2c, cap(&mut c), grid::diff(&d0c.0, &d2c.0).is_identical() && d0c.1 == d2c.1)
            }
            "restore_small_unwrap" => {
                let mut c = snap::restore(to_cols, to_rows, sb as usize, &su);
                c.vt_write(tail);
                rs(&mut c, cols, rows);
                (&d1c, cap(&mut c), grid::diff(&d0c.0, &d1c.0).is_identical() && d0c.1 == d1c.1)
            }
            "restore_orig_resize_before_tail_unwrap" => {
                let mut c = snap::restore(cols, rows, sb as usize, &su);
                rs(&mut c, to_cols, to_rows);
                c.vt_write(tail);
                rs(&mut c, cols, rows);
                (&d1c, cap(&mut c), grid::diff(&d0c.0, &d1c.0).is_identical() && d0c.1 == d1c.1)
            }
            _ => unreachable!(),
        };
        let diff = grid::diff(&d.0, &c.0);
        let history_equal = d.1 == c.1;
        let identical = diff.is_identical() && history_equal;
        let (class, detail) = if identical { (None, None) } else {
            let (cl, det) = classify_aligned(a_alt, tail, &diff, &d.0, &c.0, &d.1, &c.1);
            (Some(cl), Some(det))
        };
        out.push(ResizeRecord {
            stream: name.to_string(), cols, rows, to_cols, to_rows, stream_bytes: bytes.len(), split: q, a_alt_screen: a_alt,
            variant: v.to_string(), identical,
            cell_mismatches: diff.cell_mismatches.len(),
            state_mismatches: diff.state_mismatches.iter().map(|(f, _, _)| f.clone()).collect(),
            history_equal, d_history_rows: d.1.len(), c_history_rows: c.1.len(),
            class, class_detail: detail,
            first_mismatch: describe_first_mismatch(&diff, &d.1, &c.1),
            d_resize_roundtrip_identical: d_rt,
        });
    }
    out
}

fn md(s: &str) -> String {
    let mut t: String = s.chars().take(110).collect();
    if s.chars().count() > 110 { t.push('…'); }
    t.replace('|', "\\|").replace('`', "'")
}

/// `resize/summary.md`.
pub fn summary_md(header: &str, recs: &[ResizeRecord]) -> String {
    let mut s = format!("# S1 resize-at-split experiment\n\n{header}\n\n");
    s += "| stream | size → via | split/len (alt) | variant | identical | cells | state fields | history (D/C rows) | D resize round-trip lossless | class | first mismatch |\n";
    s += "|---|---|---|---|---|---|---|---|---|---|---|\n";
    for r in recs {
        s += &format!(
            "| {} | {}x{} → {}x{} | {}/{}{} | {} | {} | {} | {} | {} ({}/{}) | {} | {} | {} |\n",
            r.stream, r.cols, r.rows, r.to_cols, r.to_rows, r.split, r.stream_bytes,
            if r.a_alt_screen { " (alt)" } else { "" }, r.variant,
            if r.identical { "**yes**" } else { "no" }, r.cell_mismatches,
            if r.state_mismatches.is_empty() { "-".into() } else { r.state_mismatches.join(", ") },
            if r.history_equal { "equal" } else { "differs" }, r.d_history_rows, r.c_history_rows,
            if r.variant == "baseline" { "-" } else if r.d_resize_roundtrip_identical { "yes" } else { "no" },
            r.class.as_deref().unwrap_or("-"),
            md(r.first_mismatch.as_deref().unwrap_or("-")),
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn middle_boundary_is_a_boundary() {
        let b = b"abc\x1b[31mdef\x1b[0mghi";
        let q = middle_boundary(b);
        assert!(splitter::boundaries(b).contains(&q));
        assert!(q.abs_diff(b.len() / 2) <= 3);
    }

    #[test]
    fn plain_text_resize_variants_run() {
        let b = b"one\r\ntwo\r\nthree\r\nfour";
        let recs = run_stream("t", b, 20, 4, 10, 3, 1000, middle_boundary(b));
        assert_eq!(recs.len(), VARIANTS.len());
        let base = &recs[0];
        assert!(base.identical, "{base:?}");
    }
}

#[cfg(test)]
mod probe {
    //! `cargo test -p s1 probe_resize -- --ignored --nocapture`: row texts for the report.
    use super::*;
    use crate::portability::grid_text_lines;

    fn rows(t: &mut Terminal) -> Vec<String> {
        grid_text_lines(&grid::capture(t).unwrap().0)
    }

    fn show(name: &str, bytes: &[u8], cols: u16, rows_: u16, sb: u32) {
        let q = middle_boundary(bytes);
        let (head, tail) = bytes.split_at(q);
        let mut a = term(cols, rows_, sb);
        a.vt_write(head);
        let s = snap::snapshot(&a, SnapOpts { palette: false });
        let mut d1 = term(cols, rows_, sb);
        d1.vt_write(head);
        rs(&mut d1, 100, 30);
        let d1_mid = rows(&mut d1);
        d1.vt_write(tail);
        rs(&mut d1, cols, rows_);
        let mut c = snap::restore(100, 30, sb as usize, &s);
        let c_mid = rows(&mut c);
        c.vt_write(tail);
        rs(&mut c, cols, rows_);
        println!("== {name} q={q} D1 cursor=({},{}) C cursor=({},{}) D1 sb={} C sb={}",
            d1.cursor_x().unwrap(), d1.cursor_y().unwrap(), c.cursor_x().unwrap(), c.cursor_y().unwrap(),
            d1.scrollback_rows().unwrap(), c.scrollback_rows().unwrap());
        for (i, (x, y)) in d1_mid.iter().zip(&c_mid).enumerate().take(8) {
            println!("  mid {i:2} D1={x:?}\n         C ={y:?}");
        }
        let (dr, cr) = (rows(&mut d1), rows(&mut c));
        for (i, (x, y)) in dr.iter().zip(&cr).enumerate().filter(|(_, (x, y))| x != y).take(4) {
            println!("  end {i:2} D1={x:?}\n         C ={y:?}");
        }
        let (dh, ch) = (history_text(&d1), history_text(&c));
        let i = dh.iter().zip(&ch).position(|(x, y)| x != y);
        println!("  history first diff at {i:?}: D1={:?} C={:?}", i.and_then(|i| dh.get(i)), i.and_then(|i| ch.get(i)));
        if let Some(i) = i {
            for j in i.saturating_sub(1)..(i + 3) {
                println!("    hist {j}: D1={:?}\n              C ={:?}", dh.get(j), ch.get(j));
            }
        }
    }

    #[test]
    #[ignore]
    fn probe_resize() {
        for (n, b) in crate::r#gen::adversarial() {
            match n.as_str() {
                "wrap_pending" => show(&n, &b, 120, 24, 2000),
                "scrollback_200" => show(&n, &b, 80, 24, 2000),
                _ => {}
            }
        }
    }
}
