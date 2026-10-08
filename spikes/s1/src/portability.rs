//! Portability probe: is the libghostty-vt `Format::Vt` snapshot a portable VT stream, i.e. does a
//! *different* emulator (pyte 0.8.2, through `tools/pyte_restore.py`) render it to the same visible
//! text? Per stream: D = fresh libghostty terminal fed the whole stream; S = [`snap::snapshot`] of
//! D (the Aligned snapshot at `len`). pyte(S) and pyte(original bytes) are each compared
//! line-by-line (right-trimmed) with the Formatter `Format::Plain` text of D's visible screen.
//! A libghostty control (restore(S) Plain vs D Plain) is recorded alongside.

use std::path::{Path, PathBuf};
use std::process::Command;

use libghostty_vt::fmt::{Format, Formatter, FormatterOptions};
use libghostty_vt::terminal::Terminal;
use serde::{Deserialize, Serialize};

use crate::grid::Grid;

/// The tmls venv interpreter that has pyte 0.8.2 (used read-only).
pub const DEFAULT_PYTHON: &str = "/home/sgkayast/Others/tmls/.venv/bin/python";

/// `spikes/s1/tools/pyte_restore.py`.
pub fn helper_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tools").join("pyte_restore.py")
}

/// Result of a line-by-line comparison of two screens.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct LineCmp {
    /// Number of rows compared (the longer of the two inputs).
    pub rows: usize,
    pub lines_equal: usize,
    /// First differing row: (row, a escaped, b escaped).
    pub first_diff: Option<(usize, String, String)>,
}

impl LineCmp {
    pub fn all_equal(&self) -> bool {
        self.lines_equal == self.rows
    }
}

/// Compare two screens line by line after right-trimming every line; a missing line counts as
/// `""` (so a screen whose trailing blank rows were dropped compares equal to a padded one).
/// Lines in `first_diff` are `{:?}`-escaped.
pub fn compare_lines(a: &[String], b: &[String]) -> LineCmp {
    let rows = a.len().max(b.len());
    let get = |v: &[String], i: usize| v.get(i).map(|s| s.trim_end().to_string()).unwrap_or_default();
    let mut out = LineCmp { rows, ..Default::default() };
    for i in 0..rows {
        let (x, y) = (get(a, i), get(b, i));
        if x == y {
            out.lines_equal += 1;
        } else if out.first_diff.is_none() {
            out.first_diff = Some((i, format!("{x:?}"), format!("{y:?}")));
        }
    }
    out
}

/// `Some(dy)` when `b[y + dy] == a[y]` for every overlapping row (dy = ±1) but `a != b`, i.e. `b`
/// is `a` shifted vertically by one row (the Formatter's trailing-blank-row drop, see Task 4).
pub fn shifted_by_one(a: &[String], b: &[String]) -> Option<i32> {
    if compare_lines(a, b).all_equal() { return None; }
    let t = |v: &[String], i: i64| -> String {
        if i < 0 { return String::new(); }
        v.get(i as usize).map(|s| s.trim_end().to_string()).unwrap_or_default()
    };
    let n = a.len().max(b.len()) as i64;
    [1i32, -1].into_iter().find(|&dy| {
        (0..n).filter(|&y| (0..n).contains(&(y + dy as i64))).all(|y| t(a, y) == t(b, y + dy as i64))
    })
}

/// Formatter `Format::Plain` (unwrap off, trim on) of the *visible* screen of `t`: the PageList
/// output minus the first `scrollback_rows` lines, padded/truncated to `rows` lines, right-trimmed.
pub fn visible_plain(t: &Terminal, rows: u16) -> Vec<String> {
    let o = FormatterOptions::new().with_format(Format::Plain).with_unwrap(false).with_trim(true);
    let mut f = Formatter::new(t, o).expect("formatter new");
    let bytes = f.format_alloc(None).expect("format_alloc").to_vec();
    let s = String::from_utf8_lossy(&bytes);
    let sb = t.scrollback_rows().expect("scrollback_rows");
    let mut v: Vec<String> = s.split('\n').skip(sb).take(rows as usize).map(|l| l.trim_end().to_string()).collect();
    v.resize(rows as usize, String::new());
    v
}

/// Visible text from a captured [`Grid`] (cross-check for [`visible_plain`]): blank cells become
/// `' '`, the spacer cell after a wide char is skipped, lines right-trimmed.
pub fn grid_text_lines(g: &Grid) -> Vec<String> {
    let cols = g.cols as usize;
    (0..g.rows as usize)
        .map(|y| {
            let mut s = String::new();
            let mut skip = false;
            for c in &g.cells[y * cols..(y + 1) * cols] {
                if skip { skip = false; continue; }
                if c.text.is_empty() { s.push(' ') } else { s.push_str(&c.text) }
                skip = c.wide;
            }
            s.trim_end().to_string()
        })
        .collect()
}

/// What `pyte_restore.py` reports.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PyteOut {
    #[serde(skip_deserializing)]
    pub lines: Vec<String>,
    pub cursor_x: i64,
    pub cursor_y: i64,
    /// Events pyte routed to `Screen.debug` (sequences it does not implement).
    pub ignored: u64,
    pub ignored_top: Vec<(String, u64)>,
}

/// Run `python helper <vt> <cols> <rows>` and parse its output.
pub fn run_pyte(python: &str, vt: &Path, cols: u16, rows: u16) -> anyhow::Result<PyteOut> {
    let out = Command::new(python)
        .arg("-I")
        .arg(helper_path())
        .arg(vt)
        .arg(cols.to_string())
        .arg(rows.to_string())
        .output()?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    anyhow::ensure!(out.status.success(), "pyte helper failed ({}): {stderr}", out.status);
    let info_line = stderr.lines().rev().find(|l| l.starts_with('{')).unwrap_or("{}");
    let mut p: PyteOut = serde_json::from_str(info_line)?;
    p.lines = String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect();
    Ok(p)
}

/// One side (pyte fed the snapshot, or pyte fed the original stream) vs libghostty D.
#[derive(Clone, Debug, Default, Serialize)]
pub struct PyteSide {
    pub cmp: LineCmp,
    pub pyte_cursor: (i64, i64),
    pub cursor_match: bool,
    pub ignored: u64,
    pub ignored_top: Vec<(String, u64)>,
    pub error: Option<String>,
}

impl PyteSide {
    pub fn from(p: anyhow::Result<PyteOut>, d_lines: &[String], d_cursor: (u16, u16)) -> Self {
        match p {
            Ok(p) => PyteSide {
                cmp: compare_lines(d_lines, &p.lines),
                pyte_cursor: (p.cursor_x, p.cursor_y),
                cursor_match: (p.cursor_x, p.cursor_y) == (d_cursor.0 as i64, d_cursor.1 as i64),
                ignored: p.ignored,
                ignored_top: p.ignored_top,
                error: None,
            },
            Err(e) => PyteSide { error: Some(e.to_string()), ..Default::default() },
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct PortRecord {
    pub stream: String,
    pub source: String,
    pub cols: u16,
    pub rows: u16,
    pub stream_bytes: usize,
    pub snapshot_bytes: usize,
    pub d_alt_screen: bool,
    pub d_cursor: (u16, u16),
    pub d_pending_wrap: bool,
    /// [`visible_plain`] of D equals [`grid_text_lines`] of D's capture (sanity check).
    pub plain_matches_grid: bool,
    /// libghostty control: Plain of restore(S) vs Plain of D.
    pub ghostty_restore: LineCmp,
    /// restore(S) is D shifted by one row (`b[y+dy] == a[y]`).
    pub ghostty_restore_shift: Option<i32>,
    /// pyte fed S vs Plain of D.
    pub pyte_snapshot: PyteSide,
    /// pyte fed S vs Plain of libghostty restore(S): the emulator-portability measure proper
    /// (the Formatter's own losses are on both sides).
    pub pyte_vs_restore: LineCmp,
    /// pyte fed the original stream vs Plain of D.
    pub pyte_original: PyteSide,
}

fn pct(a: usize, b: usize) -> String {
    if b == 0 { "-".into() } else { format!("{:.1}", 100.0 * a as f64 / b as f64) }
}

fn md_cell(s: &str) -> String {
    let mut t: String = s.chars().take(70).collect();
    if s.chars().count() > 70 { t.push('…'); }
    t.replace('|', "\\|").replace('`', "'")
}

fn first_diff_md(c: &LineCmp) -> String {
    match &c.first_diff {
        None => "-".into(),
        Some((i, a, b)) => format!("row {i}: ghostty=`{}` pyte=`{}`", md_cell(a), md_cell(b)),
    }
}

/// `portability/summary.md`.
pub fn summary_md(header: &str, recs: &[PortRecord]) -> String {
    let mut s = format!("# S1 portability probe (pyte 0.8.2)\n\n{header}\n\n");
    s += "| stream | size | snap B | ghostty restore vs D | pyte(snapshot) vs ghostty restore | pyte(snapshot) vs D | % | cursor | pyte ignored | pyte(original) vs D | % | cursor | first diff pyte(snapshot) vs ghostty restore |\n";
    s += "|---|---|---|---|---|---|---|---|---|---|---|---|---|\n";
    let (mut ts, mut to, mut tr, mut tp) = (0usize, 0usize, 0usize, 0usize);
    let mut rows_total = 0usize;
    for r in recs {
        let cur = |p: &PyteSide| if p.error.is_some() { "err".to_string() }
            else if p.cursor_match { "ok".into() }
            else { format!("({},{}) vs ({},{})", p.pyte_cursor.0, p.pyte_cursor.1, r.d_cursor.0, r.d_cursor.1) };
        s += &format!(
            "| {} | {}x{} | {} | {}/{}{} | {}/{} | {}/{} | {} | {} | {} | {}/{} | {} | {} | {} |\n",
            r.stream, r.cols, r.rows, r.snapshot_bytes,
            r.ghostty_restore.lines_equal, r.ghostty_restore.rows,
            r.ghostty_restore_shift.map(|d| format!(" (shift {d:+})")).unwrap_or_default(),
            r.pyte_vs_restore.lines_equal, r.pyte_vs_restore.rows,
            r.pyte_snapshot.cmp.lines_equal, r.pyte_snapshot.cmp.rows,
            pct(r.pyte_snapshot.cmp.lines_equal, r.pyte_snapshot.cmp.rows), cur(&r.pyte_snapshot),
            r.pyte_snapshot.ignored,
            r.pyte_original.cmp.lines_equal, r.pyte_original.cmp.rows,
            pct(r.pyte_original.cmp.lines_equal, r.pyte_original.cmp.rows), cur(&r.pyte_original),
            first_diff_md(&r.pyte_vs_restore),
        );
        ts += r.pyte_snapshot.cmp.lines_equal;
        to += r.pyte_original.cmp.lines_equal;
        tr += r.ghostty_restore.lines_equal;
        tp += r.pyte_vs_restore.lines_equal;
        rows_total += r.rows as usize;
    }
    let n = |f: &dyn Fn(&PortRecord) -> bool| recs.iter().filter(|r| f(r)).count();
    s += &format!(
        "\n**Totals** ({} streams, {rows_total} rows), lines equal: ghostty restore vs D {tr} ({}%), pyte(snapshot) vs ghostty restore {tp} ({}%), \
         pyte(snapshot) vs D {ts} ({}%), pyte(original) vs D {to} ({}%).  \n\
         Streams fully equal: ghostty restore vs D {}, pyte(snapshot) vs ghostty restore {}, pyte(snapshot) vs D {}, pyte(original) vs D {}. \
         Cursor match: pyte(snapshot) {}, pyte(original) {}. Plain/grid cross-check failures: {}.\n",
        recs.len(), pct(tr, rows_total), pct(tp, rows_total), pct(ts, rows_total), pct(to, rows_total),
        n(&|r| r.ghostty_restore.all_equal()), n(&|r| r.pyte_vs_restore.all_equal()), n(&|r| r.pyte_snapshot.cmp.all_equal()), n(&|r| r.pyte_original.cmp.all_equal()),
        n(&|r| r.pyte_snapshot.cursor_match), n(&|r| r.pyte_original.cursor_match),
        n(&|r| !r.plain_matches_grid),
    );
    s += "\nOutcome per stream (P = pyte(snapshot) vs ghostty restore, O = pyte(original) vs D, G = ghostty restore vs D):\n\n\
          | stream | P | O | G | reading |\n|---|---|---|---|---|\n";
    for r in recs {
        let (p, o, g) = (r.pyte_vs_restore.all_equal(), r.pyte_original.cmp.all_equal(), r.ghostty_restore.all_equal());
        let reading = match (p, o) {
            (true, true) => "portable; pyte also renders the raw stream right",
            (true, false) => "portable; pyte gets the raw stream wrong but renders the snapshot right (snapshot is simpler VT)",
            (false, true) => "snapshot NOT portable: pyte renders the raw stream right but not the snapshot",
            (false, false) => "pyte renders this content differently anyway",
        };
        let gnote = if g { String::new() } else {
            format!(" (Formatter loss vs D{})", r.ghostty_restore_shift.map(|d| format!(": restore = D shifted {d:+} row")).unwrap_or_default())
        };
        let eq = |b: bool| if b { "equal" } else { "differs" };
        s += &format!("| {} | {} | {} | {} | {reading}{gnote} |\n", r.stream, eq(p), eq(o), eq(g));
    }
    s += "\nMost frequent sequences pyte ignored (`Screen.debug` args; pyte passes neither the CSI final byte nor the ESC final, so an unknown `ESC x` shows as `(no args)`; `CSI ? … (handler)` = a private CSI the stock handler would have crashed on):\n\n";
    for r in recs {
        if r.pyte_snapshot.ignored + r.pyte_original.ignored == 0 { continue; }
        let top = |p: &PyteSide| p.ignored_top.iter().take(4).map(|(k, n)| format!("`{}`×{n}", md_cell(k))).collect::<Vec<_>>().join(", ");
        s += &format!("- **{}**: snapshot {} [{}]; original {} [{}]\n", r.stream,
            r.pyte_snapshot.ignored, top(&r.pyte_snapshot), r.pyte_original.ignored, top(&r.pyte_original));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use libghostty_vt::terminal::Options;

    fn v(x: &[&str]) -> Vec<String> { x.iter().map(|s| s.to_string()).collect() }

    #[test]
    fn compare_lines_trims_pads_and_reports_first_diff() {
        let c = compare_lines(&v(&["ab  ", "x", ""]), &v(&["ab", "x  "]));
        assert_eq!(c, LineCmp { rows: 3, lines_equal: 3, first_diff: None });
        let c = compare_lines(&v(&["ab", "cd", "e\t"]), &v(&["ab", "c", "f"]));
        assert_eq!(c.rows, 3);
        assert_eq!(c.lines_equal, 1);
        assert_eq!(c.first_diff, Some((1, "\"cd\"".into(), "\"c\"".into())));
        assert!(!c.all_equal());
        let c = compare_lines(&v(&["a"]), &v(&["a", "late"]));
        assert_eq!((c.lines_equal, c.rows), (1, 2));
        assert_eq!(c.first_diff.unwrap().0, 1);
    }

    #[test]
    fn shifted_by_one_detects_row_shift() {
        assert_eq!(shifted_by_one(&v(&["a", "b", "c"]), &v(&["", "a", "b"])), Some(1));
        assert_eq!(shifted_by_one(&v(&["a", "b", "c"]), &v(&["b", "c", ""])), Some(-1));
        assert_eq!(shifted_by_one(&v(&["a", "b"]), &v(&["a", "b"])), None);
        assert_eq!(shifted_by_one(&v(&["a", "b"]), &v(&["x", "y"])), None);
    }

    #[test]
    fn visible_plain_matches_grid_text() {
        let mut t = Terminal::new(Options { cols: 10, rows: 3, max_scrollback: 1000 }).unwrap();
        t.vt_write("l1\r\nl2\r\nl3\r\n\x1b[31m漢x\x1b[0m  y".as_bytes());
        let p = visible_plain(&t, 3);
        assert_eq!(p, v(&["l2", "l3", "漢x  y"]));
        let g = crate::grid::capture(&mut t).unwrap().0;
        assert_eq!(grid_text_lines(&g), p);
    }

    /// Shells out to `tools/pyte_restore.py`; only with `S1_PYTE=1` (needs the tmls venv).
    #[test]
    fn pyte_renders_snapshot() {
        if std::env::var("S1_PYTE").as_deref() != Ok("1") {
            eprintln!("pyte_renders_snapshot: skipped (set S1_PYTE=1)");
            return;
        }
        let mut t = Terminal::new(Options { cols: 20, rows: 4, max_scrollback: 1000 }).unwrap();
        t.vt_write(b"hello\r\n\x1b[1;34mworld\x1b[0m\x1b[?2004h\r\n  indented");
        let s = crate::snap::snapshot(&t, crate::snap::SnapOpts { palette: false });
        let dir = PathBuf::from(std::env::var("TMPDIR").unwrap_or_else(|_| "/data/scratch/s1".into()));
        let vt = dir.join("s1_pyte_test.vt");
        std::fs::write(&vt, &s).unwrap();
        let p = run_pyte(DEFAULT_PYTHON, &vt, 20, 4).unwrap();
        let d = visible_plain(&t, 4);
        let c = compare_lines(&d, &p.lines);
        eprintln!("pyte lines={:?} cursor=({},{}) ignored={} top={:?}", p.lines, p.cursor_x, p.cursor_y, p.ignored, p.ignored_top);
        eprintln!("D lines={d:?} cmp={c:?} D cursor=({},{})", t.cursor_x().unwrap(), t.cursor_y().unwrap());
        assert_eq!(p.lines.len(), 4);
        assert!(c.all_equal(), "{c:?}");
        assert_eq!((p.cursor_x, p.cursor_y), (t.cursor_x().unwrap() as i64, t.cursor_y().unwrap() as i64));
    }
}
