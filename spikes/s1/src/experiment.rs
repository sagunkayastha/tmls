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
use crate::carry::{self, Tracker};
use crate::snap::{self, SnapOpts};
use crate::splitter;

/// Default cap on split points per stream.
pub const MAX_SPLITS: usize = 400;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, PartialOrd, Ord)]
pub enum Strategy {
    Naive,
    Aligned,
    /// Aligned, but the snapshot uses the Formatter's `unwrap` option (soft wraps preserved).
    AlignedUnwrap,
    AlignedCarry,
}

impl Strategy {
    pub const ALL: [Strategy; 4] = [Strategy::Naive, Strategy::Aligned, Strategy::AlignedUnwrap, Strategy::AlignedCarry];
}

impl fmt::Display for Strategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Strategy::Naive => "Naive",
            Strategy::Aligned => "Aligned",
            Strategy::AlignedUnwrap => "AlignedUnwrap",
            Strategy::AlignedCarry => "AlignedCarry",
        })
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
    /// Size of the carry snapshot (zero for baseline strategies).
    pub carry_bytes: usize,
    /// Linked viewport runs overlaid by the carry snapshot.
    pub carry_overlay_runs: usize,
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
    /// Mismatch introduced only by continuation after a faithful snapshot.
    pub post_tail_first_mismatch: Option<String>,
    pub alt_mode_at_split: Option<u16>,
    pub a_scrollback_rows: usize,
    /// Fully plain-blank rows at the bottom of A's viewport when snapshotted.
    pub a_trailing_blank_rows: u16,
    /// Cell mismatches (C vs D) that are visually invisible: both texts blank (`""`/`" "`), same
    /// bg, no inverse/underline/strike difference, not wide (only fg/bold/italic/faint on a blank
    /// or `""` vs `" "` under a background colour).
    pub cells_blank_equivalent: usize,
    /// First cell mismatch (C vs D) that is not blank-equivalent, i.e. visible.
    pub first_visible_cell_mismatch: Option<String>,
    /// Every mismatching field group (see [`mismatch_groups`]), not just the first-match class.
    pub secondary_classes: Vec<String>,
    /// Parser state at `split` ([`splitter::CutState`]: Ground / Utf8 / Esc / Csi / String).
    pub cut_state: String,
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
    blank(a) && blank(b) && a.bg == b.bg && a.hyperlink == b.hyperlink
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
        Strategy::Aligned | Strategy::AlignedUnwrap | Strategy::AlignedCarry => splitter::last_boundary_at_or_before(bounds, p),
    };
    let mut a = new_term(cols, rows, scrollback);
    let mut tracker = Tracker::default();
    if case.strategy == Strategy::AlignedCarry {
        tracker.feed(&mut a, &bytes[..q]);
    } else {
        a.vt_write(&bytes[..q]);
    }
    let a_alt_screen = a.active_screen().expect("active_screen") == Screen::Alternate;
    let (s, snap_d) = {
        let t0 = std::time::Instant::now();
        let s = if case.strategy == Strategy::AlignedCarry {
            carry::snapshot_full(&mut a, &tracker, SnapOpts { palette: false })
        } else {
            snap::snapshot_unwrap(&a, SnapOpts { palette: false }, case.strategy == Strategy::AlignedUnwrap)
        };
        (s, t0.elapsed())
    };
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
        carry_bytes: if case.strategy == Strategy::AlignedCarry { s.len() } else { 0 },
        carry_overlay_runs: if case.strategy == Strategy::AlignedCarry { carry::overlay_run_count(&mut a) } else { 0 },
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
        post_tail_first_mismatch: if snap_faithful { first_mismatch(&diff, &d.history, &c_hist) } else { None },
        alt_mode_at_split: tracker.alt_entry.as_ref().map(|entry| entry.mode),
        a_scrollback_rows: ca.1.scrollback_rows,
        a_trailing_blank_rows: trailing_blank_rows(&ca.0),
        cells_blank_equivalent: diff.cell_mismatches.iter().filter(|(_, _, a, b)| blank_equivalent(a, b)).count(),
        first_visible_cell_mismatch: diff.cell_mismatches.iter().find(|(_, _, a, b)| !blank_equivalent(a, b))
            .map(|(x, y, a, b)| format!("cell ({x},{y}): D={} C={}", cell_str(a), cell_str(b))),
        secondary_classes: mismatch_groups(&diff, &d.history, &c_hist),
        cut_state: format!("{:?}", splitter::cut_states(&bytes[..p]).last().copied().unwrap_or(splitter::CutState::Ground)),
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
    if let Some(h) = &c.hyperlink { s += &format!(" link={h:?}"); }
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

/// `trailing_row_drop` test: cells equal with C shifted by `dy`, the D row that falls outside the
/// overlap is plain-blank (the dropped trailing row), and D has exactly `dy` more history rows.
fn row_drop_shift(d: &(Grid, TermState), c: &(Grid, TermState), dy: i32) -> bool {
    let (g, cols) = (&d.0, d.0.cols as usize);
    let excluded_blank = (0..g.rows as i32).filter(|y| y + dy < 0 || y + dy >= c.0.rows as i32).all(|y| {
        let y = y as usize;
        g.cells[y * cols..(y + 1) * cols].iter().all(grid::is_plain_blank)
    });
    equal_shifted(&d.0, &c.0, dy)
        && excluded_blank
        && d.1.scrollback_rows as i64 - c.1.scrollback_rows as i64 == dy as i64
}

/// Mismatch groups names used for `secondary_classes`, in report order.
pub const GROUPS: &[&str] = &[
    "alt_screen", "scrollback", "modes", "cursor", "title_pwd", "kitty_kbd", "grid_size", "text",
    "style", "hyperlink",
];

/// Every mismatching field group of a D-vs-C comparison: `alt_screen` (alt_screen /
/// mode.alt_screen), `scrollback` (scrollback_rows / total_rows / history text), `modes` (other
/// mode.*), `cursor`, `title_pwd`, `kitty_kbd`, `grid_size`, `text` (a cell's text differs),
/// `style` (same text, colours/attrs differ), `hyperlink` (same text, hyperlink differs).
pub fn mismatch_groups(diff: &Diff, d_hist: &[String], c_hist: &[String]) -> Vec<String> {
    let has = |pred: &dyn Fn(&str) -> bool| diff.state_mismatches.iter().any(|(f, _, _)| pred(f));
    let mut g: Vec<&str> = Vec::new();
    if has(&|f| f == "alt_screen" || f == "mode.alt_screen") { g.push("alt_screen"); }
    if has(&|f| f == "scrollback_rows" || f == "total_rows") || d_hist != c_hist { g.push("scrollback"); }
    if has(&|f| f.starts_with("mode.") && f != "mode.alt_screen") { g.push("modes"); }
    if has(&|f| CURSOR_FIELDS.contains(&f)) { g.push("cursor"); }
    if has(&|f| f == "title" || f == "pwd") { g.push("title_pwd"); }
    if has(&|f| f == "kitty_flags") { g.push("kitty_kbd"); }
    if has(&|f| f == "grid_size") { g.push("grid_size"); }
    let cm = &diff.cell_mismatches;
    if cm.iter().any(|(_, _, a, b)| a.text != b.text) { g.push("text"); }
    if cm.iter().any(|(_, _, a, b)| {
        a.text == b.text && Cell { hyperlink: None, ..a.clone() } != Cell { hyperlink: None, ..b.clone() }
    }) { g.push("style"); }
    if cm.iter().any(|(_, _, a, b)| a.text == b.text && a.hyperlink != b.hyperlink) { g.push("hyperlink"); }
    g.into_iter().map(String::from).collect()
}

fn state_detail(diff: &Diff, pred: impl Fn(&str) -> bool) -> Option<String> {
    diff.state_mismatches.iter().find(|(f, _, _)| pred(f)).map(|(f, a, b)| format!("{f}: D={a} C={b}"))
}

/// Taxonomy, first matching class in priority order:
/// `parser_state_split` (Naive, split not a boundary), `alt_screen` (alt_screen differs, or A was
/// on the alt screen and the tail leaves it), `trailing_row_drop` (cells equal after shifting C by
/// ±1 row, the D row left out is blank, and D has exactly that many more history rows), `scrollback` (scrollback_rows/total_rows/history text differ), `modes`, `cursor` (only
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
            if row_drop_shift(x.d, x.c, dy) {
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

/// The taxonomy ([`classify`]) for an Aligned (boundary) comparison of D vs C outside
/// [`run_case_with`] (used by the resize experiment). `diff` direction: `a` = D, `b` = C.
pub fn classify_aligned(
    a_alt_screen: bool,
    tail: &[u8],
    diff: &Diff,
    d: &(Grid, TermState),
    c: &(Grid, TermState),
    d_hist: &[String],
    c_hist: &[String],
) -> (String, String) {
    let tail_leaves_alt = contains_any(tail, &[b"\x1b[?1049l", b"\x1b[?1047l", b"\x1b[?47l"]);
    let (cl, det) = classify(&Ctx {
        strategy: Strategy::Aligned, split_is_boundary: true, a_alt_screen, tail_leaves_alt,
        diff, d, c, d_hist, c_hist,
    });
    (cl.to_string(), det)
}

/// First mismatch text for a D-vs-C diff plus history (see [`run_case_with`]).
pub fn describe_first_mismatch(diff: &Diff, d_hist: &[String], c_hist: &[String]) -> Option<String> {
    first_mismatch(diff, d_hist, c_hist)
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
    split_plan(bytes, boundaries, seed, MAX_SPLITS).points
}

pub fn split_points_capped(bytes: &[u8], boundaries: &[usize], seed: u64, cap: usize) -> Vec<usize> {
    split_plan(bytes, boundaries, seed, cap).points
}

/// The split points of one stream plus how they were chosen (written to `streams.json`).
/// Anchor counts are distinct offsets in the ±2 windows: `*_candidates` found, `*_kept` in `points`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct SplitPlan {
    pub points: Vec<usize>,
    pub cap: usize,
    pub alt_anchor_candidates: usize,
    pub alt_anchor_kept: usize,
    pub osc8_anchor_candidates: usize,
    pub osc8_anchor_kept: usize,
    pub wide_anchor_candidates: usize,
    pub wide_anchor_kept: usize,
    pub random_boundary: usize,
    pub random_non_boundary: usize,
    /// Non-boundary points in `points` by cut state.
    pub non_boundary_csi: usize,
    pub non_boundary_string: usize,
    pub non_boundary_utf8: usize,
    pub non_boundary_esc: usize,
}

fn windows(anchors: &[usize], len: usize) -> Vec<usize> {
    let mut v: Vec<usize> = anchors.iter().flat_map(|&a| a.saturating_sub(2)..=(a + 2).min(len)).collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Selection order, each stage only adding points not yet chosen and never exceeding `cap`:
/// 1. 0 and len;
/// 2. ALL offsets within ±2 of every `ESC[?1049h`, `ESC[?1049l` and `ESC]8;` (only subsampled,
///    seeded, if they alone exceed the cap — then `*_kept < *_candidates` says so);
/// 3. 64 random boundary offsets, then 64 random non-boundary offsets;
/// 4. shuffled offsets within ±2 of wide-character lead bytes, filling the remaining slots.
pub fn split_plan(bytes: &[u8], boundaries: &[usize], seed: u64, cap: usize) -> SplitPlan {
    let len = bytes.len();
    let cap = cap.max(2);
    let mut rng = StdRng::seed_from_u64(seed);
    let bset: HashSet<usize> = boundaries.iter().copied().collect();
    let mut chosen: Vec<usize> = vec![0];
    if len > 0 { chosen.push(len); }
    let mut have: HashSet<usize> = chosen.iter().copied().collect();
    let take = |cands: Vec<usize>, limit: usize, chosen: &mut Vec<usize>, have: &mut HashSet<usize>| {
        let mut n = 0;
        for p in cands {
            if n >= limit || chosen.len() >= cap { break; }
            if have.insert(p) { chosen.push(p); n += 1; }
        }
        n
    };

    let mut alt_a = find_all(bytes, b"\x1b[?1049h");
    alt_a.extend(find_all(bytes, b"\x1b[?1049l"));
    let alt_w = windows(&alt_a, len);
    let osc_w = windows(&find_all(bytes, b"\x1b]8;"), len);
    let wide_w = windows(&wide_char_offsets(bytes), len);

    let mut prio: Vec<usize> = alt_w.iter().chain(osc_w.iter()).copied().collect::<HashSet<_>>().into_iter().collect();
    prio.sort_unstable();
    if prio.len() + chosen.len() > cap { prio.shuffle(&mut rng); }
    take(prio, usize::MAX, &mut chosen, &mut have);

    let mut bs: Vec<usize> = boundaries.iter().copied().filter(|&b| b != 0 && b != len).collect();
    bs.shuffle(&mut rng);
    let random_boundary = take(bs, 64, &mut chosen, &mut have);
    let mut nb: Vec<usize> = (1..len).filter(|p| !bset.contains(p)).collect();
    nb.shuffle(&mut rng);
    let random_non_boundary = take(nb, 64, &mut chosen, &mut have);

    let mut wide = wide_w.clone();
    wide.shuffle(&mut rng);
    take(wide, usize::MAX, &mut chosen, &mut have);

    chosen.sort_unstable();
    let kept = |w: &[usize]| w.iter().filter(|p| have.contains(p)).count();
    let states = splitter::cut_states(bytes);
    let count = |st: splitter::CutState| chosen.iter().filter(|&&p| !bset.contains(&p) && states[p] == st).count();
    SplitPlan {
        cap,
        alt_anchor_candidates: alt_w.len(),
        alt_anchor_kept: kept(&alt_w),
        osc8_anchor_candidates: osc_w.len(),
        osc8_anchor_kept: kept(&osc_w),
        wide_anchor_candidates: wide_w.len(),
        wide_anchor_kept: kept(&wide_w),
        random_boundary,
        random_non_boundary,
        non_boundary_csi: count(splitter::CutState::Csi),
        non_boundary_string: count(splitter::CutState::String),
        non_boundary_utf8: count(splitter::CutState::Utf8),
        non_boundary_esc: count(splitter::CutState::Esc),
        points: chosen,
    }
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
        let au = run_case(b, &case(3, Strategy::AlignedUnwrap), 20, 3, 100);
        assert!(a.identical, "aligned: {a:?}");
        assert!(au.identical, "aligned_unwrap: {au:?}");
        assert_eq!(a.effective_split, 2);
        // Recorded outcome: the fresh parser in C sees "[31mcd" as text.
        assert!(!n.identical, "naive: {n:?}");
        assert_eq!(n.class.as_deref(), Some("parser_state_split"), "naive: {n:?}");
    }

    #[test]
    fn aligned_unwrap_identical_on_plain_text() {
        let b = b"hello\r\nworld";
        let r = run_case(b, &case(7, Strategy::AlignedUnwrap), 20, 3, 100);
        assert!(r.identical, "{r:?}");
    }

    #[test]
    fn alt_screen_split_roundtrip() {
        let b = b"primary one\r\nprimary two\r\n\x1b[?1049halt text\x1b[?1049lback\r\n";
        let split = b.windows(8).position(|w| w == b"\x1b[?1049l").unwrap();
        let r = run_case(b, &case(split, Strategy::Aligned), 20, 5, 100);
        assert!(r.a_alt_screen);
        assert!(!r.secondary_classes.is_empty(), "{r:?}");
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
    fn alt_exit_after_restore_identical() {
        let b = b"primary one\r\nprimary two\x1b[?1049halt text\x1b[?1049lback";
        let split = b.windows(8).position(|w| w == b"\x1b[?1049l").unwrap();
        let baseline = run_case(b, &case(split, Strategy::Aligned), 20, 5, 2000);
        assert_eq!(baseline.class.as_deref(), Some("alt_screen"));
        let carry = run_case(b, &case(split, Strategy::AlignedCarry), 20, 5, 2000);
        assert!(carry.identical, "{carry:?}");
    }

    #[test]
    fn saved_cursor_restored() {
        let b = b"\x1b[1;31m\x1b[2;5H\x1b7\x1b[0m\x1b[4;2Hmid\x1b8after";
        let split = b.windows(2).position(|w| w == b"\x1b8").unwrap();
        let carry = run_case(b, &case(split, Strategy::AlignedCarry), 20, 5, 2000);
        assert!(carry.identical, "{carry:?}");
    }

    #[test]
    fn active_hyperlink_continues_after_restore() {
        let b = b"\x1b]8;;https://example.org\x1b\\before after\x1b]8;;\x1b\\";
        let split = b.windows(6).position(|w| w == b" after").unwrap();
        let result = run_case(b, &case(split, Strategy::AlignedCarry), 40, 3, 2000);
        assert!(result.identical, "{result:?}");
    }

    #[test]
    fn alt_mouse_mode_reconciled() {
        let b = b"\x1b[?1000hprimary\x1b[?1049halt\x1b[?1000l\x1b[?1049l";
        let split = b.windows(8).position(|w| w == b"\x1b[?1049l").unwrap();
        let r = run_case(b, &case(split, Strategy::AlignedCarry), 20, 4, 2000);
        assert!(r.identical, "{r:?}");
    }

    #[test]
    fn wide_pending_wrap_roundtrip() {
        let b = "00:abcｗxyz\r\n01:abcｗxyz\r\n02:abcｗxyz\r\n03:abcｗxyz\r\n";
        let r = run_case(b.as_bytes(), &case(9, Strategy::AlignedCarry), 4, 3, 2000);
        assert!(r.identical, "{r:?}");
    }

    /// ECH restores the erased background at (0,0).
    #[test]
    fn residual_style_only_repro() {
        let prefix = b"\x1b[1;1Habcdefgh\x1b[2;1Hijklmnop\x1b[48;2;181;189;104m\x1b[2;4H\x1b[2J\x1b[0m";
        let mut stream = prefix.to_vec();
        stream.extend_from_slice(b"X");
        assert!(stream.len() <= 200);
        let r = run_case(&stream, &case(prefix.len(), Strategy::AlignedCarry), 8, 3, 2000);
        assert!(r.identical, "{r:?}");
    }

    /// ECH restores empty-cell identity and background at (4,1).
    #[test]
    fn residual_text_repro() {
        let prefix = b"\x1b[1;1Habcdefgh\x1b[2;1Hijklmnop\x1b[48;2;181;189;104m\x1b[2;4H\x1b[0K\x1b[0m";
        let mut stream = prefix.to_vec();
        stream.extend_from_slice(b"X");
        assert!(stream.len() <= 200);
        let r = run_case(&stream, &case(prefix.len(), Strategy::AlignedCarry), 8, 3, 2000);
        assert!(r.identical, "{r:?}");
    }

    /// The carried primary snapshot preserves empty-cell identity through a 47-mode exit.
    #[test]
    fn residual_alt_screen_repro() {
        let prefix = b"\n\n\x1b[?1049h\x1b[?1049l\n\n\n\n\n\nprimary cycle 1 line 5 \x1b[35mcolour\x1b[?1047hs\x1b[?1047lb\x1b[?47h";
        let mut stream = prefix.to_vec();
        stream.extend_from_slice(b"\x1b[?47lback");
        assert!(stream.len() <= 200);
        let r = run_case(&stream, &case(prefix.len(), Strategy::AlignedCarry), 80, 24, 2000);
        assert!(r.identical, "{r:?}");
    }

    #[test]
    fn strategy_matrix_includes_carry() {
        let b = b"ab\x1b[31mcd";
        assert_eq!(Strategy::ALL.len(), 4);
        for st in Strategy::ALL {
            let result = run_case(b, &case(b.len(), st), 20, 3, 2000);
            assert!(result.identical, "{st}: {result:?}");
        }
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
        // Formatter dropping that row. C has one history row fewer: its history is D's minus the
        // last line, so history text is NOT equal; the screen is shifted by one row.
        let s2: String = (0..300).map(|i| format!("line {i:03}\r\n")).collect();
        let b2 = s2.as_bytes();
        let r2 = run_case(b2, &case(b2.len(), Strategy::Aligned), 80, 24, 2000);
        assert!(r2.scrollback_compared);
        assert!(!r2.identical, "{r2:?}");
        assert!(r2.state_mismatches.iter().any(|f| f == "scrollback_rows"), "{r2:?}");
        assert_eq!(r2.class.as_deref(), Some("trailing_row_drop"), "{r2:?}");
        assert_eq!(r2.scrollback_text_equal, Some(false), "{r2:?}");
        let d = reference(b2, 80, 24, 2000);
        let c = snap::restore(80, 24, 2000, &snap::snapshot(&{
            let mut a = new_term(80, 24, 2000);
            a.vt_write(b2);
            a
        }, SnapOpts { palette: false }));
        let ch = history_text(&c);
        assert_eq!(ch.len() + 1, d.history.len());
        assert_eq!(&d.history[..ch.len()], &ch[..], "C history = D history minus its last line");
        assert!(r2.secondary_classes.iter().any(|g| g == "scrollback"), "{r2:?}");
    }

    #[test]
    fn trailing_row_drop_is_tight() {
        let row = |t: &str| (0..4).map(|i| Cell { text: t.chars().nth(i).map(String::from).unwrap_or_default(), ..Cell::default() }).collect::<Vec<_>>();
        let g = |rows: &[&str]| Grid { cols: 4, rows: rows.len() as u16, cells: rows.iter().flat_map(|r| row(r)).collect() };
        let st = |sb: usize| TermState { scrollback_rows: sb, ..TermState::default() };
        // D: a b <blank>, 2 history rows; C: x a b, 1 history row -> C = D shifted down by one.
        let d = (g(&["aaaa", "bbbb", ""]), st(2));
        let c = (g(&["xxxx", "aaaa", "bbbb"]), st(1));
        assert!(row_drop_shift(&d, &c, 1));
        // D's excluded row not blank -> not a dropped trailing row.
        let d2 = (g(&["aaaa", "bbbb", "zzzz"]), st(2));
        assert!(!row_drop_shift(&d2, &c, 1));
        // History difference must equal the shift.
        assert!(!row_drop_shift(&(d.0.clone(), st(1)), &c, 1));
        assert!(!row_drop_shift(&(d.0.clone(), st(4)), &c, 1));
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

        // Alt-screen / OSC 8 anchors are never dropped by the cap in favour of wide-char anchors.
        let mut s = String::from("\x1b[?1049hA\x1b]8;;u\x1b\\L\x1b]8;;\x1b\\\x1b[?1049l");
        for i in 0..1500 { s += &format!("日{i}"); }
        s += "\x1b[?1049hB\x1b[?1049l";
        let b = s.as_bytes();
        let plan = split_plan(b, &splitter::boundaries(b), 1, MAX_SPLITS);
        assert!(plan.points.len() <= MAX_SPLITS);
        assert_eq!(plan.alt_anchor_kept, plan.alt_anchor_candidates, "{plan:?}");
        assert_eq!(plan.osc8_anchor_kept, plan.osc8_anchor_candidates, "{plan:?}");
        assert!(plan.wide_anchor_kept < plan.wide_anchor_candidates, "{plan:?}");
        assert_eq!((plan.random_boundary, plan.random_non_boundary), (64, 64));
        let nb = plan.non_boundary_csi + plan.non_boundary_string + plan.non_boundary_utf8 + plan.non_boundary_esc;
        let bset = splitter::boundaries(b);
        assert_eq!(nb, plan.points.iter().filter(|p| bset.binary_search(p).is_err()).count());
    }
}
