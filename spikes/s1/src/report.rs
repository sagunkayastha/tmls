//! Per-case records (`cases.jsonl`) and the markdown reports (`summary.md`, `taxonomy.md`).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::Serialize;

use crate::experiment::{CaseResult, SplitPlan, Strategy, CLASSES, GROUPS};

#[derive(Clone, Debug, Serialize)]
pub struct Record {
    pub stream: String,
    /// `recorded` or `gen`.
    pub source: String,
    pub cols: u16,
    pub rows: u16,
    pub stream_bytes: usize,
    pub split: usize,
    pub strategy: Strategy,
    #[serde(flatten)]
    pub result: CaseResult,
}

/// Per-stream run metadata for the summary header.
#[derive(Clone, Debug, Serialize)]
pub struct StreamInfo {
    pub name: String,
    pub source: String,
    pub cols: u16,
    pub rows: u16,
    pub bytes: usize,
    pub splits: usize,
    pub split_cap: usize,
    pub seconds: f64,
    /// Reference terminal D after the whole stream.
    pub d_scrollback_rows: usize,
    pub d_alt_screen: bool,
    /// How the split points were chosen (anchor counts kept/candidates, non-boundary cut states).
    pub plan: SplitPlan,
}

/// What `grid::diff` does and does not compare (stated in both reports).
pub const COMPARED: &str = "Compared per cell: text (grapheme), fg/bg (palette-resolved RGB), bold, italic, \
underline on/off, inverse, strike, faint, wide, **OSC 8 hyperlink URI** (via `GridRef::hyperlink_uri`). \
**Not compared:** underline style (curly/double/…) and underline colour, blink, invisible, overline, \
palette index vs equal RGB, hyperlink *id* (only the URI), protection, semantic-prompt marks; \
DECSC saved cursor and kitty keyboard push stack (only the current flags) are not state fields either.";

fn pct(v: &mut [u128], p: f64) -> u128 {
    if v.is_empty() { return 0; }
    v.sort_unstable();
    let i = ((v.len() as f64 - 1.0) * p).round() as usize;
    v[i]
}

fn trunc(s: &str, n: usize) -> String {
    let s = s.replace('|', "\\|").replace('\n', " ");
    if s.chars().count() <= n { s } else { s.chars().take(n).collect::<String>() + "…" }
}

pub fn summary_md(header: &str, streams: &[StreamInfo], recs: &[Record]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# S1 snapshot/restore summary\n\n{header}\n\n{COMPARED}\n");
    let _ = writeln!(out, "## Streams\n\nAnchor columns are kept/candidate offsets in the ±2 windows (alt = `?1049h/l`, osc8 = `ESC]8;`, wide = wide-char lead bytes); alt and osc8 anchors are kept before random points, wide anchors fill the remaining slots. Non-boundary columns count the chosen split points that are not boundaries, by parser state at the cut (inside CSI / inside an OSC-DCS-APC-PM-SOS string / mid-UTF-8 / after ESC or in an nF escape).\n");
    let _ = writeln!(out, "| stream | source | size | bytes | splits (cap) | anchors alt / osc8 / wide | non-boundary csi / string / utf8 / esc | D history rows | D on alt | wall s |\n|---|---|---|---|---|---|---|---|---|---|");
    let mut nb = [0usize; 4];
    for s in streams {
        let p = &s.plan;
        nb[0] += p.non_boundary_csi; nb[1] += p.non_boundary_string; nb[2] += p.non_boundary_utf8; nb[3] += p.non_boundary_esc;
        let _ = writeln!(out, "| {} | {} | {}x{} | {} | {} ({}) | {}/{} / {}/{} / {}/{} | {} / {} / {} / {} | {} | {} | {:.1} |",
            s.name, s.source, s.cols, s.rows, s.bytes, s.splits, s.split_cap,
            p.alt_anchor_kept, p.alt_anchor_candidates, p.osc8_anchor_kept, p.osc8_anchor_candidates, p.wide_anchor_kept, p.wide_anchor_candidates,
            p.non_boundary_csi, p.non_boundary_string, p.non_boundary_utf8, p.non_boundary_esc,
            s.d_scrollback_rows, s.d_alt_screen, s.seconds);
    }
    let _ = writeln!(out, "\nNon-boundary split points, all streams: {} inside CSI, {} inside strings, {} mid-UTF-8, {} after ESC / in nF ({} total).",
        nb[0], nb[1], nb[2], nb[3], nb.iter().sum::<usize>());
    let _ = writeln!(out, "\n## Per stream x strategy\n");
    let _ = writeln!(out, "Identical = normalised grids equal, no state mismatch, history text equal. Mean cells = mean cell mismatches over non-identical cases. State fields = top 3 mismatching state fields (case count).\n");
    let _ = writeln!(out, "| stream | strategy | cases | identical % | mean cells (non-id) | top-3 state fields | snap B p50 | snap B max | snap µs p50/p95 | restore µs p50/p95 |\n|---|---|---|---|---|---|---|---|---|---|");
    let mut groups: Vec<(String, Strategy)> = Vec::new();
    for s in streams {
        for st in Strategy::ALL { groups.push((s.name.clone(), st)); }
    }
    let mut totals: BTreeMap<Strategy, (usize, usize)> = BTreeMap::new();
    for (name, st) in groups {
        let g: Vec<&Record> = recs.iter().filter(|r| r.stream == name && r.strategy == st).collect();
        if g.is_empty() { continue; }
        let n = g.len();
        let ident = g.iter().filter(|r| r.result.identical).count();
        let t = totals.entry(st).or_default();
        t.0 += n;
        t.1 += ident;
        let non: Vec<&&Record> = g.iter().filter(|r| !r.result.identical).collect();
        let mean_cells = if non.is_empty() { "-".to_string() } else {
            format!("{:.1}", non.iter().map(|r| r.result.cell_mismatches).sum::<usize>() as f64 / non.len() as f64)
        };
        let mut fields: BTreeMap<&str, usize> = BTreeMap::new();
        for r in &g { for f in &r.result.state_mismatches { *fields.entry(f.as_str()).or_default() += 1; } }
        let mut fv: Vec<(&str, usize)> = fields.into_iter().collect();
        fv.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let top = if fv.is_empty() { "-".to_string() } else {
            fv.iter().take(3).map(|(f, c)| format!("{f} ({c})")).collect::<Vec<_>>().join(", ")
        };
        let mut sb: Vec<u128> = g.iter().map(|r| r.result.snapshot_bytes as u128).collect();
        let mut su: Vec<u128> = g.iter().map(|r| r.result.snapshot_us).collect();
        let mut ru: Vec<u128> = g.iter().map(|r| r.result.restore_us).collect();
        let sb_max = sb.iter().copied().max().unwrap_or(0);
        let _ = writeln!(out, "| {name} | {st} | {n} | {:.1} | {mean_cells} | {top} | {} | {sb_max} | {}/{} | {}/{} |",
            100.0 * ident as f64 / n as f64, pct(&mut sb, 0.5), pct(&mut su, 0.5), pct(&mut su, 0.95), pct(&mut ru, 0.5), pct(&mut ru, 0.95));
    }
    let _ = writeln!(out, "\n## Totals\n\n| strategy | cases | identical | identical % |\n|---|---|---|---|");
    for (st, (n, i)) in totals {
        let _ = writeln!(out, "| {st} | {n} | {i} | {:.1} |", 100.0 * i as f64 / n.max(1) as f64);
    }
    out
}

pub fn taxonomy_md(header: &str, recs: &[Record]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# S1 mismatch taxonomy\n\n{header}\n\n{COMPARED}\n");
    let _ = writeln!(out, "Each non-identical case gets the first matching class in this priority order: {}.\n", CLASSES.join(" > "));
    let mut counts: BTreeMap<(&str, Strategy), usize> = BTreeMap::new();
    let mut streams: BTreeMap<(&str, Strategy), BTreeMap<&str, usize>> = BTreeMap::new();
    let mut example: BTreeMap<(&str, Strategy), &Record> = BTreeMap::new();
    for r in recs {
        let Some(c) = r.result.class.as_deref() else { continue };
        let c = CLASSES.iter().copied().find(|k| *k == c).unwrap_or("other");
        *counts.entry((c, r.strategy)).or_default() += 1;
        *streams.entry((c, r.strategy)).or_default().entry(r.stream.as_str()).or_default() += 1;
        example.entry((c, r.strategy)).or_insert(r);
    }
    let total = |st: Strategy| recs.iter().filter(|r| r.strategy == st).count();
    let ident = |st: Strategy| recs.iter().filter(|r| r.strategy == st && r.result.identical).count();
    let _ = writeln!(out, "| class | Naive | Aligned | AlignedUnwrap | AlignedCarry | example (stream @ split [strategy]: first mismatch) |\n|---|---|---|---|---|---|");
    let _ = writeln!(out, "| (identical) | {} | {} | {} | {} | |", ident(Strategy::Naive), ident(Strategy::Aligned), ident(Strategy::AlignedUnwrap), ident(Strategy::AlignedCarry));
    for c in CLASSES {
        let n = counts.get(&(*c, Strategy::Naive)).copied().unwrap_or(0);
        let a = counts.get(&(*c, Strategy::Aligned)).copied().unwrap_or(0);
        let u = counts.get(&(*c, Strategy::AlignedUnwrap)).copied().unwrap_or(0);
        let k = counts.get(&(*c, Strategy::AlignedCarry)).copied().unwrap_or(0);
        let ex = example.get(&(*c, Strategy::Aligned)).or_else(|| example.get(&(*c, Strategy::Naive))).or_else(|| example.get(&(*c, Strategy::AlignedUnwrap)));
        let ex = ex.map_or(String::new(), |r| format!("{} @ {} [{}]: {}", r.stream, r.split, r.strategy,
            trunc(r.result.class_detail.as_deref().unwrap_or(""), 160)));
        let _ = writeln!(out, "| {c} | {n} | {a} | {u} | {k} | {ex} |");
    }
    let _ = writeln!(out, "| **total** | {} | {} | {} | {} | |", total(Strategy::Naive), total(Strategy::Aligned), total(Strategy::AlignedUnwrap), total(Strategy::AlignedCarry));

    let _ = writeln!(out, "\n## All mismatching groups\n\nNon-identical cases with each mismatch group present (`secondary_classes`; a case counts in every group it has, so columns do not sum to the totals).\n");
    let _ = writeln!(out, "| group | Naive | Aligned | AlignedUnwrap | AlignedCarry |\n|---|---|---|---|---|");
    for gname in GROUPS {
        let n = |st: Strategy| recs.iter().filter(|r| r.strategy == st && !r.result.identical && r.result.secondary_classes.iter().any(|x| x == gname)).count();
        let _ = writeln!(out, "| {gname} | {} | {} | {} | {} |", n(Strategy::Naive), n(Strategy::Aligned), n(Strategy::AlignedUnwrap), n(Strategy::AlignedCarry));
    }
    let _ = writeln!(out, "\n## Streams per class\n\n| class | strategy | streams (cases) |\n|---|---|---|");
    for c in CLASSES {
        for st in Strategy::ALL {
            if let Some(m) = streams.get(&(*c, st)) {
                let mut v: Vec<(&&str, &usize)> = m.iter().collect();
                v.sort_by(|a, b| b.1.cmp(a.1));
                let s = v.iter().map(|(k, n)| format!("{k} ({n})")).collect::<Vec<_>>().join(", ");
                let _ = writeln!(out, "| {c} | {st} | {s} |");
            }
        }
    }
    let _ = writeln!(out, "\n## Diagnostics per class x strategy\n");
    let _ = writeln!(out, "- *snap lossy* = restore(S) already differs from A before any tail byte (else the loss only shows once the tail runs).");
    let _ = writeln!(out, "- *A trailing blank* = A's viewport ended in >= 1 fully blank row when snapshotted (and had history).");
    let _ = writeln!(out, "- *invisible-only* = every cell mismatch is blank-equivalent (`\"\"` vs `\" \"` under the same bg, or fg/bold/italic/faint on a blank) and there is no state/history mismatch.\n");
    let _ = writeln!(out, "| class | strategy | cases | snap lossy | A trailing blank (+history) | invisible-only |\n|---|---|---|---|---|---|");
    for c in CLASSES {
        for st in Strategy::ALL {
            let g: Vec<&Record> = recs.iter().filter(|r| r.strategy == st && r.result.class.as_deref() == Some(*c)).collect();
            if g.is_empty() { continue; }
            let lossy = g.iter().filter(|r| !r.result.snap_faithful).count();
            let tb = g.iter().filter(|r| r.result.a_trailing_blank_rows > 0 && r.result.a_scrollback_rows > 0).count();
            let inv = g.iter().filter(|r| r.result.cell_mismatches > 0 && r.result.cells_blank_equivalent == r.result.cell_mismatches
                && r.result.state_mismatches.is_empty() && r.result.scrollback_text_equal == Some(true)).count();
            let _ = writeln!(out, "| {c} | {st} | {} | {lossy} | {tb} | {inv} |", g.len());
        }
    }
    let _ = writeln!(out, "\n## Examples per class x strategy\n");
    for c in CLASSES {
        for st in Strategy::ALL {
            if let Some(r) = example.get(&(*c, st)) {
                let _ = writeln!(out, "- **{c} / {st}**: `{}` @ {} (effective {}, boundary={}): {}{}", r.stream, r.split,
                    r.result.effective_split, r.result.split_is_boundary, trunc(r.result.class_detail.as_deref().unwrap_or(""), 300),
                    r.result.snap_first_mismatch.as_deref().map_or(String::new(), |m| format!(" — snap lossy at split: {}", trunc(m, 160))));
            }
        }
    }
    out
}
