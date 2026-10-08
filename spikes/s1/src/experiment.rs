//! Snapshot/restore fidelity experiment. For one stream and one split offset `p`:
//!
//! - D (reference): fresh terminal fed the whole stream.
//! - Naive: A <- `bytes[..p]`; S <- snapshot(A); C <- restore(S); C <- `bytes[p..]`.
//! - Aligned: `q = last_boundary_at_or_before(p)`; same with `q` (the unparsed tail `bytes[q..p]`
//!   is carried over and fed to C).
//!
//! C is compared against D after [`grid::normalize`] on both grids (diff direction: `a` = D,
//! `b` = C). Scrollback text is compared separately through the Formatter's `Format::Plain`
//! output, which covers the whole PageList (history + active screen, oldest row first, trailing
//! blank rows trimmed): the first `scrollback_rows` lines are the history.

use std::collections::HashSet;
use std::fmt;

use libghostty_vt::fmt::{Format, Formatter, FormatterOptions};
use libghostty_vt::screen::Screen;
use libghostty_vt::terminal::{Options, Terminal};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use serde::Serialize;

use crate::grid::{self, Cell, Diff, Grid, TermState};
use crate::snap::{self, SnapOpts};
use crate::splitter;

/// Default cap on split points per stream.
pub const MAX_SPLITS: usize = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, PartialOrd, Ord)]
pub enum Strategy {
    Naive,
    Aligned,
}

impl fmt::Display for Strategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self { Strategy::Naive => "Naive", Strategy::Aligned => "Aligned" })
    }
}

#[derive(Clone, Debug)]
pub struct Case {
    pub stream: String,
    pub split: usize,
    pub strategy: Strategy,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct CaseResult {
    /// Normalised grids equal, no state mismatch, and scrollback text equal (when compared).
    pub identical: bool,
    pub cell_mismatches: usize,
    /// Field names of the state mismatches (`cursor_x`, `mode.origin`, ...).
    pub state_mismatches: Vec<String>,
    pub snapshot_bytes: usize,
    pub snapshot_us: u128,
    pub restore_us: u128,
    /// Human-readable first mismatch: first state mismatch, else first cell, else scrollback text.
    pub first_mismatch: Option<String>,
    /// Offset actually snapshotted (`p` for Naive, `q` for Aligned).
    pub effective_split: usize,
    /// `split` is in `splitter::boundaries`.
    pub split_is_boundary: bool,
    /// A was on the alternate screen when snapshotted.
    pub a_alt_screen: bool,
    pub scrollback_compared: bool,
    /// History text (Plain, right-trimmed lines) of C equals D's. `None` when not compared.
    pub scrollback_text_equal: Option<bool>,
    /// Why scrollback text was not compared (empty when it was).
    pub scrollback_note: String,
    /// Taxonomy class (see [`classify`]); `None` when identical.
    pub class: Option<String>,
    /// The mismatch that put the case in its class.
    pub class_detail: Option<String>,
    // --- diagnostics ---
    /// restore(S) (before the tail) equals A (normalised grid, state, history text).
    pub snap_faithful: bool,
    /// First A-vs-restore(S) mismatch when `!snap_faithful`.
    pub snap_first_mismatch: Option<String>,
    pub a_scrollback_rows: usize,
    /// Fully plain-blank rows at the bottom of A's viewport when snapshotted.
    pub a_trailing_blank_rows: u16,
    /// Cell mismatches (C vs D) that are visually invisible: both texts blank (`""`/`" "`), same
    /// bg, no inverse/underline/strike difference, not wide (only fg/bold/italic/faint on a blank
    /// or `""` vs `" "` under a background colour).
    pub cells_blank_equivalent: usize,
    /// First cell mismatch (C vs D) that is not blank-equivalent, i.e. visible.
    pub first_visible_cell_mismatch: Option<String>,
}

fn trailing_blank_rows(g: &Grid) -> u16 {
    let cols = g.cols as usize;
    (0..g.rows)
        .rev()
        .take_while(|&y| g.cells[y as usize * cols..(y as usize + 1) * cols].iter().all(grid::is_plain_blank))
        .count() as u16
}

/// A mismatched cell pair that renders the same.
pub fn blank_equivalent(a: &Cell, b: &Cell) -> bool {
    let blank = |c: &Cell| (c.text.is_empty() || c.text == " ") && !c.wide && !c.inverse && !c.underline && !c.strike;
    blank(a) && blank(b) && a.bg == b.bg
}

/// The reference side of a stream (D), computed once per stream.
pub struct Reference {
    pub cap: (Grid, TermState),
    pub history: Vec<String>,
}

fn new_term(cols: u16, rows: u16, scrollback: u32) -> Terminal<'static, 'static> {
    Terminal::new(Options { cols, rows, max_scrollback: scrollback as usize }).expect("terminal new")
}

/// Plain-text history lines: Formatter `Format::Plain`, `unwrap(false)`, first `scrollback_rows`
/// lines, each right-trimmed. Row `i` of the PageList is line `i` (unwrap off), so this lines up
/// history rows between two terminals.
pub fn history_text(t: &Terminal) -> Vec<String> {
    let o = FormatterOptions::new().with_format(Format::Plain).with_unwrap(false).with_trim(true);
    let mut f = Formatter::new(t, o).expect("formatter new");
    let bytes = f.format_alloc(None).expect("format_alloc").to_vec();
    let s = String::from_utf8_lossy(&bytes);
    let sb = t.scrollback_rows().expect("scrollback_rows");
    s.split('\n').take(sb).map(|l| l.trim_end().to_string()).collect()
}

fn capture_norm(t: &mut Terminal) -> (Grid, TermState) {
    let mut c = grid::capture(t).expect("capture");
    grid::normalize(&mut c.0);
    c
}

pub fn reference(bytes: &[u8], cols: u16, rows: u16, scrollback: u32) -> Reference {
    let mut d = new_term(cols, rows, scrollback);
    d.vt_write(bytes);
    let cap = capture_norm(&mut d);
    let history = history_text(&d);
    Reference { cap, history }
}

/// Run one case (computes the reference D itself).
pub fn run_case(bytes: &[u8], case: &Case, cols: u16, rows: u16, scrollback: u32) -> CaseResult {
    let bounds = splitter::boundaries(bytes);
    let r = reference(bytes, cols, rows, scrollback);
    run_case_with(bytes, &bounds, &r, case, cols, rows, scrollback)
}

/// Run one case against a precomputed reference and boundary list.
pub fn run_case_with(
    bytes: &[u8],
    bounds: &[usize],
    d: &Reference,
    case: &Case,
    cols: u16,
    rows: u16,
    scrollback: u32,
) -> CaseResult {
    let p = case.split.min(bytes.len());
    let split_is_boundary = bounds.binary_search(&p).is_ok();
    let q = match case.strategy {
        Strategy::Naive => p,
        Strategy::Aligned => splitter::last_boundary_at_or_before(bounds, p),
    };
    let mut a = new_term(cols, rows, scrollback);
    a.vt_write(&bytes[..q]);
    let a_alt_screen = a.active_screen().expect("active_screen") == Screen::Alternate;
    let (s, snap_d) = snap::timed_snapshot(&a, SnapOpts { palette: false });
    let (mut c, rest_d) = snap::timed_restore(cols, rows, scrollback as usize, &s);
    let ca = capture_norm(&mut a);
    let a_hist = history_text(&a);
    let c0 = capture_norm(&mut c);
    let c0_hist = history_text(&c);
    let d0 = grid::diff(&ca, &c0);
    let snap_faithful = d0.is_identical() && a_hist == c0_hist;
    c.vt_write(&bytes[q..]);
    let cc = capture_norm(&mut c);
    let c_hist = history_text(&c);
    let diff = grid::diff(&d.cap, &cc);
    let sb_equal = c_hist == d.history;

    let mut res = CaseResult {
        identical: diff.is_identical() && sb_equal,
        cell_mismatches: diff.cell_mismatches.len(),
        state_mismatches: diff.state_mismatches.iter().map(|(f, _, _)| f.clone()).collect(),
        snapshot_bytes: s.len(),
        snapshot_us: snap_d.as_micros(),
        restore_us: rest_d.as_micros(),
        first_mismatch: first_mismatch(&diff, &d.history, &c_hist),
        effective_split: q,
        split_is_boundary,
        a_alt_screen,
        scrollback_compared: true,
        scrollback_text_equal: Some(sb_equal),
        scrollback_note: String::new(),
        class: None,
        class_detail: None,
        snap_faithful,
        snap_first_mismatch: if snap_faithful { None } else { first_mismatch(&d0, &a_hist, &c0_hist) },
        a_scrollback_rows: ca.1.scrollback_rows,
        a_trailing_blank_rows: trailing_blank_rows(&ca.0),
        cells_blank_equivalent: diff.cell_mismatches.iter().filter(|(_, _, a, b)| blank_equivalent(a, b)).count(),
        first_visible_cell_mismatch: diff.cell_mismatches.iter().find(|(_, _, a, b)| !blank_equivalent(a, b))
            .map(|(x, y, a, b)| format!("cell ({x},{y}): D={} C={}", cell_str(a), cell_str(b))),
    };
    if !res.identical {
        let tail_leaves_alt = contains_any(&bytes[q..], &[b"\x1b[?1049l", b"\x1b[?1047l", b"\x1b[?47l"]);
        let (class, detail) = classify(&Ctx {
            strategy: case.strategy,
            split_is_boundary,
            a_alt_screen,
            tail_leaves_alt,
            diff: &diff,
            d: &d.cap,
            c: &cc,
            d_hist: &d.history,
            c_hist: &c_hist,
        });
        res.class = Some(class.to_string());
        res.class_detail = Some(detail);
    }
    res
}

fn contains_any(hay: &[u8], needles: &[&[u8]]) -> bool {
    needles.iter().any(|n| hay.windows(n.len()).any(|w| w == *n))
}

fn cell_str(c: &Cell) -> String {
    let mut s = format!("{:?}", c.text);
    if let Some(fg) = c.fg { s += &format!(" fg={fg:?}"); }
    if let Some(bg) = c.bg { s += &format!(" bg={bg:?}"); }
    for (on, n) in [(c.bold, "bold"), (c.italic, "italic"), (c.underline, "ul"), (c.inverse, "inv"),
                    (c.strike, "strike"), (c.faint, "faint"), (c.wide, "wide")] {
        if on { s += " "; s += n; }
    }
    s
}

fn first_hist_diff(d: &[String], c: &[String]) -> String {
    let i = d.iter().zip(c).position(|(a, b)| a != b).unwrap_or(d.len().min(c.len()));
    format!("history line {i}: D={:?} C={:?} (D {} lines, C {})",
        d.get(i).map(String::as_str).unwrap_or("<none>"),
        c.get(i).map(String::as_str).unwrap_or("<none>"), d.len(), c.len())
}

fn first_mismatch(diff: &Diff, d_hist: &[String], c_hist: &[String]) -> Option<String> {
    if let Some((f, a, b)) = diff.state_mismatches.first() {
        return Some(format!("{f}: D={a} C={b}"));
    }
    if let Some((x, y, a, b)) = diff.cell_mismatches.first() {
        return Some(format!("cell ({x},{y}): D={} C={}", cell_str(a), cell_str(b)));
    }
    if d_hist != c_hist {
        return Some(first_hist_diff(d_hist, c_hist));
    }
    None
}

struct Ctx<'a> {
    strategy: Strategy,
    split_is_boundary: bool,
    a_alt_screen: bool,
    tail_leaves_alt: bool,
    diff: &'a Diff,
    d: &'a (Grid, TermState),
    c: &'a (Grid, TermState),
    d_hist: &'a [String],
    c_hist: &'a [String],
}

const CURSOR_FIELDS: &[&str] = &["cursor_x", "cursor_y", "cursor_visible", "cursor_style", "pending_wrap"];

/// All cells equal when C is shifted by `dy` rows relative to D (over the overlapping rows).
fn equal_shifted(d: &Grid, c: &Grid, dy: i32) -> bool {
    if (d.cols, d.rows) != (c.cols, c.rows) { return false; }
    let cols = d.cols as usize;
    (0..d.rows as i32).all(|y| {
        let yc = y + dy;
        if yc < 0 || yc >= c.rows as i32 { return true; }
        let (y, yc) = (y as usize, yc as usize);
        d.cells[y * cols..(y + 1) * cols] == c.cells[yc * cols..(yc + 1) * cols]
    })
}

fn state_detail(diff: &Diff, pred: impl Fn(&str) -> bool) -> Option<String> {
    diff.state_mismatches.iter().find(|(f, _, _)| pred(f)).map(|(f, a, b)| format!("{f}: D={a} C={b}"))
}

/// Taxonomy, first matching class in priority order:
/// `parser_state_split` (Naive, split not a boundary), `alt_screen` (alt_screen differs, or A was
/// on the alt screen and the tail leaves it), `trailing_row_drop` (cells equal after shifting C by
/// ±1 row), `scrollback` (scrollback_rows/total_rows/history text differ), `modes`, `cursor` (only
/// cursor fields differ, no cell mismatch), `title_pwd`, `kitty_kbd`, `style_only` (cells differ
/// but text is equal everywhere), `text`, `other`.
fn classify(x: &Ctx) -> (&'static str, String) {
    let diff = x.diff;
    let fm = || first_mismatch(diff, x.d_hist, x.c_hist).unwrap_or_default();
    if x.strategy == Strategy::Naive && !x.split_is_boundary {
        return ("parser_state_split", fm());
    }
    if let Some(d) = state_detail(diff, |f| f == "alt_screen") {
        return ("alt_screen", d);
    }
    if x.a_alt_screen && x.tail_leaves_alt {
        return ("alt_screen", format!("snapshot taken on alt screen, tail leaves it; {}", fm()));
    }
    if !diff.cell_mismatches.is_empty() {
        for dy in [1, -1] {
            if equal_shifted(&x.d.0, &x.c.0, dy) {
                return ("trailing_row_drop", format!("C equals D shifted by {dy:+} row(s); {}", fm()));
            }
        }
    }
    if let Some(d) = state_detail(diff, |f| f == "scrollback_rows" || f == "total_rows") {
        return ("scrollback", d);
    }
    if x.d_hist != x.c_hist {
        return ("scrollback", first_hist_diff(x.d_hist, x.c_hist));
    }
    if let Some(d) = state_detail(diff, |f| f.starts_with("mode.")) {
        return ("modes", d);
    }
    if diff.cell_mismatches.is_empty()
        && !diff.state_mismatches.is_empty()
        && diff.state_mismatches.iter().all(|(f, _, _)| CURSOR_FIELDS.contains(&f.as_str()))
    {
        return ("cursor", fm());
    }
    if let Some(d) = state_detail(diff, |f| f == "title" || f == "pwd") {
        return ("title_pwd", d);
    }
    if let Some(d) = state_detail(diff, |f| f == "kitty_flags") {
        return ("kitty_kbd", d);
    }
    if let Some((cx, cy, a, b)) = diff.cell_mismatches.iter().find(|(_, _, a, b)| a.text != b.text) {
        return ("text", format!("cell ({cx},{cy}): D={} C={}", cell_str(a), cell_str(b)));
    }
    if let Some((cx, cy, a, b)) = diff.cell_mismatches.first() {
        return ("style_only", format!("cell ({cx},{cy}): D={} C={}", cell_str(a), cell_str(b)));
    }
    ("other", fm())
}

/// Ordered taxonomy class names (for reports).
pub const CLASSES: &[&str] = &[
    "parser_state_split", "alt_screen", "trailing_row_drop", "scrollback", "modes", "cursor",
    "title_pwd", "kitty_kbd", "style_only", "text", "other",
];

fn is_wide_cp(cp: u32) -> bool {
    matches!(cp,
        0x1100..=0x115F | 0x2E80..=0x303E | 0x3041..=0x33FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF
        | 0xA000..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6 | 0x1F300..=0x1F64F | 0x1F900..=0x1F9FF | 0x20000..=0x3FFFD)
}

/// Byte offsets of the lead byte of every (approximately) width-2 character.
fn wide_char_offsets(bytes: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let n = match b { 0xe0..=0xef => 3, 0xf0..=0xf4 => 4, _ => 0 };
        if b >= 0xe2
            && n > 0
            && i + n <= bytes.len()
            && std::str::from_utf8(&bytes[i..i + n])
                .is_ok_and(|s| s.chars().next().is_some_and(|c| is_wide_cp(c as u32)))
        {
            out.push(i);
            i += n;
            continue;
        }
        i += 1;
    }
    out
}

fn find_all(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    hay.windows(needle.len()).enumerate().filter(|(_, w)| *w == needle).map(|(i, _)| i).collect()
}

/// Split points with the default cap ([`MAX_SPLITS`]).
pub fn split_points(bytes: &[u8], boundaries: &[usize], seed: u64) -> Vec<usize> {
    split_points_capped(bytes, boundaries, seed, MAX_SPLITS)
}

/// 64 random boundary offsets + 64 random non-boundary offsets, plus every offset within ±2 of
/// each `ESC[?1049h`, `ESC[?1049l`, `ESC]8;` and wide-character lead byte; always 0 and len;
/// sorted, deduplicated, at most `cap` points (the targeted points are subsampled to fit, the
/// random ones and 0/len are kept).
pub fn split_points_capped(bytes: &[u8], boundaries: &[usize], seed: u64, cap: usize) -> Vec<usize> {
    let len = bytes.len();
    let mut rng = StdRng::seed_from_u64(seed);
    let bset: HashSet<usize> = boundaries.iter().copied().collect();
    let mut base: Vec<usize> = vec![0, len];

    let mut bs: Vec<usize> = boundaries.iter().copied().filter(|&b| b != 0 && b != len).collect();
    bs.shuffle(&mut rng);
    base.extend(bs.into_iter().take(64));
    let mut nb: Vec<usize> = (1..len).filter(|p| !bset.contains(p)).collect();
    nb.shuffle(&mut rng);
    base.extend(nb.into_iter().take(64));
    base.sort_unstable();
    base.dedup();
    base.truncate(cap.max(2));
    if base.last() != Some(&len) {
        // Keep 0 and len even under a tiny cap.
        base.pop();
        base.push(len);
    }

    let mut anchors: Vec<usize> = Vec::new();
    for pat in [&b"\x1b[?1049h"[..], b"\x1b[?1049l", b"\x1b]8;"] {
        anchors.extend(find_all(bytes, pat));
    }
    anchors.extend(wide_char_offsets(bytes));
    let have: HashSet<usize> = base.iter().copied().collect();
    let mut targeted: Vec<usize> = anchors
        .iter()
        .flat_map(|&a| a.saturating_sub(2)..=(a + 2).min(len))
        .filter(|p| !have.contains(p))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    targeted.sort_unstable();
    targeted.shuffle(&mut rng);
    targeted.truncate(cap.saturating_sub(base.len()));

    base.extend(targeted);
    base.sort_unstable();
    base.dedup();
    base
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case(split: usize, strategy: Strategy) -> Case {
        Case { stream: "t".into(), split, strategy }
    }

    #[test]
    fn split_inside_csi_naive_differs_aligned_identical() {
        let b = b"ab\x1b[31mcd";
        let n = run_case(b, &case(3, Strategy::Naive), 20, 3, 100);
        let a = run_case(b, &case(3, Strategy::Aligned), 20, 3, 100);
        assert!(a.identical, "aligned: {a:?}");
        assert_eq!(a.effective_split, 2);
        // Recorded outcome: the fresh parser in C sees "[31mcd" as text.
        assert!(!n.identical, "naive: {n:?}");
        assert_eq!(n.class.as_deref(), Some("parser_state_split"), "naive: {n:?}");
    }

    #[test]
    fn alt_screen_split_roundtrip() {
        let b = b"primary one\r\nprimary two\r\n\x1b[?1049halt text\x1b[?1049lback\r\n";
        let split = b.windows(8).position(|w| w == b"\x1b[?1049l").unwrap();
        let r = run_case(b, &case(split, Strategy::Aligned), 20, 5, 100);
        assert!(r.a_alt_screen);
        // Recorded outcome: the Formatter emits only the active (alt) screen, so the primary
        // content behind it is lost; after 1049l, C's primary screen lacks "primary one/two".
        assert!(!r.identical, "{r:?}");
        assert_eq!(r.class.as_deref(), Some("alt_screen"), "{r:?}");
        // While still on the alt screen the restore is faithful.
        let mid = b.windows(8).position(|w| w == b"alt text").unwrap() + 8;
        let r2 = run_case(&b[..split], &case(mid, Strategy::Aligned), 20, 5, 100);
        assert!(r2.identical, "{r2:?}");
    }

    #[test]
    fn scrollback_preserved_or_documented() {
        // 300 lines, split at the end, no trailing newline: history fully preserved.
        let mut s: String = (0..299).map(|i| format!("line {i:03}\r\n")).collect();
        s += "line 299";
        let b = s.as_bytes();
        let r = run_case(b, &case(b.len(), Strategy::Aligned), 80, 24, 2000);
        assert!(r.scrollback_compared && r.scrollback_note.is_empty());
        assert_eq!(r.scrollback_text_equal, Some(true), "{r:?}");
        assert!(!r.state_mismatches.iter().any(|f| f == "scrollback_rows"), "{r:?}");
        assert!(r.identical, "{r:?}");

        // Same with a trailing CRLF (the cursor sits on an empty last row): recorded outcome is the
        // Formatter dropping that row; history text is still equal and the screen is shifted.
        let s2: String = (0..300).map(|i| format!("line {i:03}\r\n")).collect();
        let b2 = s2.as_bytes();
        let r2 = run_case(b2, &case(b2.len(), Strategy::Aligned), 80, 24, 2000);
        assert!(r2.scrollback_compared);
        assert!(!r2.identical, "{r2:?}");
        assert!(r2.state_mismatches.iter().any(|f| f == "scrollback_rows"), "{r2:?}");
        assert_eq!(r2.class.as_deref(), Some("trailing_row_drop"), "{r2:?}");
    }

    #[test]
    fn modes_roundtrip() {
        let b = crate::r#gen::adversarial().into_iter().find(|(n, _)| n == "modes_mix").unwrap().1;
        let split = b.windows(14).position(|w| w == b"final mode set").unwrap();
        let r = run_case(&b, &case(split, Strategy::Aligned), 80, 24, 2000);
        let modes: Vec<_> = r.state_mismatches.iter().filter(|f| f.starts_with("mode.")).collect();
        assert!(modes.is_empty(), "{r:?}");
    }

    #[test]
    fn split_points_bounded() {
        let mut streams = crate::r#gen::adversarial();
        // A big stream with many targeted anchors (wide chars, alt screen, OSC 8).
        let big: String = (0..2000).map(|i| format!("日本 {i} \x1b[?1049h\x1b]8;;u\x1b\\x\x1b[?1049l\r\n")).collect();
        streams.push(("big".into(), big.into_bytes()));
        streams.push(("empty".into(), Vec::new()));
        for (name, b) in &streams {
            let bounds = splitter::boundaries(b);
            let v = split_points(b, &bounds, 1);
            assert!(v.len() <= MAX_SPLITS, "{name}: {}", v.len());
            assert_eq!(v.first(), Some(&0), "{name}");
            assert_eq!(v.last(), Some(&b.len()), "{name}");
            assert!(v.windows(2).all(|w| w[0] < w[1]), "{name}: sorted+dedup");
            assert_eq!(v, split_points(b, &bounds, 1), "{name}: deterministic");
            let c = split_points_capped(b, &bounds, 1, 200);
            assert!(c.len() <= 200 && c.first() == Some(&0) && c.last() == Some(&b.len()), "{name}");
        }
        let bounds = splitter::boundaries(b"");
        assert_eq!(split_points(b"", &bounds, 1), vec![0]);
    }
}
