//! `s0 report`: join sender and receiver logs into counts and latency percentiles.
use crate::event::{self, Kind};
use crate::log::{hex, RecvRecord, SendRecord};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

#[derive(Serialize, Debug, Clone)]
pub struct CorruptExample { pub seq: u32, pub kind: Kind, pub expected_hex: String, pub got_hex: String }

#[derive(Serialize, Debug, Clone, Default)]
pub struct Report {
    pub path: String, pub load: String,
    /// `sent` = send records; `arrived` = distinct seqs in `0..events`; `out_of_range` = receiver
    /// records whose seq is `>= events` (not counted as arrived, dup, corrupt or reorder).
    pub events: u32, pub sent: u32, pub arrived: u32, pub out_of_range: u32, pub missing: u32, pub dup: u32, pub corrupt: u32, pub reorder: u32, pub acked: u32,
    pub p50_ms: Option<f64>, pub p95_ms: Option<f64>, pub p99_ms: Option<f64>, pub max_ms: Option<f64>,
    pub ack_p50_ms: Option<f64>, pub ack_p95_ms: Option<f64>, pub ack_p99_ms: Option<f64>, pub ack_max_ms: Option<f64>,
    pub corrupt_by_kind: BTreeMap<Kind, u32>, pub missing_by_kind: BTreeMap<Kind, u32>,
    pub corrupt_examples: Vec<CorruptExample>,
    pub dropped_acks: Option<u64>, pub recv_exit: Option<String>, pub saw_done: Option<bool>,
    pub skew_note: Option<String>,
    /// `yes` or `NO:<reasons>`; see [`Report::complete_status`]. Filled by `main`.
    pub complete: String,
}

impl Report {
    /// A row is a gate result only when the run finished: every event was sent, the receiver
    /// stopped on `--quit-after` (not EOF/error) and the sender saw `S0DONE`.
    pub fn complete_status(&self) -> String {
        let mut why = Vec::new();
        if self.sent < self.events { why.push("sent<events".to_string()); }
        if self.recv_exit.as_deref() != Some("quit_after") {
            why.push(format!("recv_exit={}", self.recv_exit.as_deref().unwrap_or("–")));
        }
        if self.saw_done != Some(true) { why.push("no_done".into()); }
        if why.is_empty() { "yes".into() } else { format!("NO:{}", why.join(",")) }
    }
}

/// Nearest-rank percentile of an ascending-sorted slice.
pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    sorted[((p / 100.0 * sorted.len() as f64).ceil() as usize).max(1) - 1]
}

fn quad(mut v: Vec<f64>) -> [Option<f64>; 4] {
    if v.is_empty() { return [None; 4]; }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    [Some(percentile(&v, 50.0)), Some(percentile(&v, 95.0)), Some(percentile(&v, 99.0)), v.last().copied()]
}

fn ms(later: u128, earlier: u128) -> f64 { (later as f64 - earlier as f64) / 1e6 }

pub fn build(send: &[SendRecord], recv: &[RecvRecord], events: u32, path: &str, load: &str) -> Report {
    let mut rep = Report { path: path.into(), load: load.into(), events, sent: send.len() as u32, ..Default::default() };
    let sent: HashMap<u32, &SendRecord> = send.iter().map(|s| (s.seq, s)).collect();
    let kind_for = |seq: u32| sent.get(&seq).map(|s| s.kind).unwrap_or_else(|| event::kind_of(seq));
    let mut order: Vec<&RecvRecord> = recv.iter().collect();
    order.sort_by_key(|r| r.idx);
    let mut seen = BTreeSet::new();
    let mut prev: Option<u32> = None;
    let (mut one_way, mut acks) = (Vec::new(), Vec::new());
    for r in order {
        if r.seq >= events { rep.out_of_range += 1; continue; }
        if !seen.insert(r.seq) { rep.dup += 1; continue; }
        // reorder = first arrival whose seq is lower than the previous first arrival's seq
        if prev.is_some_and(|p| r.seq < p) { rep.reorder += 1; }
        prev = Some(r.seq);
        if !r.intact {
            rep.corrupt += 1;
            *rep.corrupt_by_kind.entry(kind_for(r.seq)).or_default() += 1;
            if rep.corrupt_examples.len() < 10 {
                // Payload only: the receiver's got_hex never includes the marker.
                let exp = event::expected_payload(r.seq);
                rep.corrupt_examples.push(CorruptExample { seq: r.seq, kind: kind_for(r.seq), expected_hex: hex(&exp), got_hex: r.got_hex.clone() });
            }
        }
        if let Some(s) = sent.get(&r.seq) { one_way.push(ms(r.wall_ns, s.write_end_ns)); }
    }
    rep.arrived = seen.len() as u32;
    rep.missing = events.saturating_sub(rep.arrived);
    for seq in 0..events {
        if !seen.contains(&seq) { *rep.missing_by_kind.entry(kind_for(seq)).or_default() += 1; }
    }
    for s in send {
        if let Some(a) = s.ack_ns { rep.acked += 1; acks.push(ms(a, s.write_end_ns)); }
    }
    [rep.p50_ms, rep.p95_ms, rep.p99_ms, rep.max_ms] = quad(one_way);
    [rep.ack_p50_ms, rep.ack_p95_ms, rep.ack_p99_ms, rep.ack_max_ms] = quad(acks);
    rep
}

fn load_lines(path: &Path) -> anyhow::Result<Vec<serde_json::Value>> {
    Ok(std::fs::read_to_string(path)?.lines().filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok()).collect())
}

pub fn load_send(path: &Path) -> anyhow::Result<(Vec<SendRecord>, Option<bool>)> {
    let (mut recs, mut saw) = (Vec::new(), None);
    for v in load_lines(path)? {
        if v.get("done").is_some() { saw = v.get("saw_done").and_then(|x| x.as_bool()); continue; }
        if let Ok(r) = serde_json::from_value(v) { recs.push(r); }
    }
    Ok((recs, saw))
}

pub fn load_recv(path: &Path) -> anyhow::Result<(Vec<RecvRecord>, Option<u64>, Option<String>)> {
    let (mut recs, mut dropped, mut exit) = (Vec::new(), None, None);
    for v in load_lines(path)? {
        if v.get("done").is_some() {
            dropped = v.get("dropped_acks").and_then(|x| x.as_u64());
            exit = v.get("exit").and_then(|x| x.as_str()).map(String::from);
            continue;
        }
        if let Ok(r) = serde_json::from_value(v) { recs.push(r); }
    }
    Ok((recs, dropped, exit))
}

fn fmt_quad(q: [Option<f64>; 4]) -> String {
    match q {
        [Some(a), Some(b), Some(c), Some(d)] => format!("{a:.1}/{b:.1}/{c:.1}/{d:.1}"),
        _ => "–".into(),
    }
}

pub const HEADER: &str = "| path | load | events | arrived | missing | dup | corrupt | reorder | acked | sent | recv_exit | saw_done | dropped_acks | one-way p50/p95/p99/max ms | ack p50/p95/p99/max ms | complete |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|";

fn opt<T: std::fmt::Display>(v: &Option<T>) -> String { v.as_ref().map_or("–".into(), |x| x.to_string()) }

pub fn markdown_row(r: &Report) -> String {
    format!("| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |", r.path, r.load, r.events,
        r.arrived, r.missing, r.dup, r.corrupt, r.reorder, r.acked,
        r.sent, opt(&r.recv_exit), opt(&r.saw_done), opt(&r.dropped_acks),
        fmt_quad([r.p50_ms, r.p95_ms, r.p99_ms, r.max_ms]),
        fmt_quad([r.ack_p50_ms, r.ack_p95_ms, r.ack_p99_ms, r.ack_max_ms]),
        r.complete_status())
}

pub struct Opts {
    pub send: PathBuf, pub recv: PathBuf, pub path: String, pub load: String,
    pub json: Option<PathBuf>, pub header: bool, pub skew_note: Option<String>, pub events: Option<u32>,
}

pub fn main(o: Opts) -> anyhow::Result<()> {
    let (send, saw_done) = load_send(&o.send)?;
    let (recv, dropped, exit) = load_recv(&o.recv)?;
    let events = o.events.unwrap_or(send.len() as u32);
    let mut rep = build(&send, &recv, events, &o.path, &o.load);
    rep.dropped_acks = dropped; rep.recv_exit = exit; rep.saw_done = saw_done; rep.skew_note = o.skew_note;
    rep.complete = rep.complete_status();
    if let Some(j) = &o.json {
        if let Some(p) = j.parent() { std::fs::create_dir_all(p)?; }
        std::fs::write(j, serde_json::to_string_pretty(&rep)?)?;
    }
    if o.header { println!("{HEADER}"); }
    println!("{}", markdown_row(&rep));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{event::Kind, log::{RecvRecord, SendRecord}};
    fn s(seq: u32, t: u128) -> SendRecord { SendRecord { seq, kind: Kind::Letter, write_start_ns: t, write_end_ns: t, ack_ns: Some(t + 4_000_000) } }
    fn r(seq: u32, t: u128, intact: bool, idx: u64) -> RecvRecord { RecvRecord { seq, intact, got_hex: String::new(), wall_ns: t, idx } }
    #[test]
    fn report_classifies() {
        let send: Vec<_> = (0..5).map(|i| s(i, 1_000_000 * i as u128)).collect();
        // seq 1 missing, seq 2 twice, seq 3 corrupt, seq 4 arrives before seq 0 (reorder)
        let recv = vec![r(4, 10_000_000, true, 0), r(0, 11_000_000, true, 1), r(2, 12_000_000, true, 2),
                        r(2, 12_500_000, true, 3), r(3, 13_000_000, false, 4)];
        let rep = build(&send, &recv, 5, "p", "quiet");
        assert_eq!((rep.arrived, rep.missing, rep.dup, rep.corrupt, rep.reorder), (4, 1, 1, 1, 1));
    }
    #[test]
    fn percentiles() {
        let v: Vec<f64> = (1..=100).map(|x| x as f64).collect();
        assert_eq!(percentile(&v, 50.0), 50.0);
        assert_eq!(percentile(&v, 99.0), 99.0);
        assert_eq!(percentile(&v, 100.0), 100.0);
    }
    #[test]
    fn load_skips_done_lines() {
        let d = std::path::PathBuf::from("/data/scratch/s0/report_test");
        std::fs::create_dir_all(&d).unwrap();
        let sp = d.join("send.jsonl"); let rp = d.join("recv.jsonl");
        std::fs::write(&sp, "{\"seq\":0,\"kind\":\"Letter\",\"write_start_ns\":1,\"write_end_ns\":2,\"ack_ns\":null}\n{\"done\":true,\"acked\":0,\"saw_done\":true,\"events\":1,\"rate\":50.0,\"cmd\":[]}\n").unwrap();
        std::fs::write(&rp, "{\"seq\":0,\"intact\":true,\"got_hex\":\"\",\"wall_ns\":5,\"idx\":0}\n{\"done\":true,\"arrivals\":1,\"dropped_acks\":3,\"exit\":\"quit_after\"}\n").unwrap();
        let (sv, saw) = load_send(&sp).unwrap();
        let (rv, dropped, exit) = load_recv(&rp).unwrap();
        assert_eq!((sv.len(), saw, rv.len(), dropped, exit.as_deref()), (1, Some(true), 1, Some(3), Some("quit_after")));
    }
    #[test]
    fn report_marks_incomplete_runs() {
        let send: Vec<_> = (0..5).map(|i| s(i, 1_000_000 * i as u128)).collect();
        let recv: Vec<_> = (0..5).map(|i| r(i, 2_000_000 * i as u128, true, i as u64)).collect();
        // Complete: all sent, recv quit_after, S0DONE seen.
        let mut rep = build(&send, &recv, 5, "p", "quiet");
        rep.recv_exit = Some("quit_after".into()); rep.saw_done = Some(true); rep.dropped_acks = Some(0);
        assert_eq!(rep.sent, 5);
        assert_eq!(rep.complete_status(), "yes");
        let row = markdown_row(&rep);
        assert!(row.contains("| 5 | 5 | quit_after | true | 0 |"), "{row}");
        assert!(row.trim_end().ends_with("| yes |"), "{row}");
        // Incomplete: 3 of 5 sent, recv hit EOF, no S0DONE.
        let mut rep = build(&send[..3], &recv[..3], 5, "p", "quiet");
        rep.recv_exit = Some("eof".into()); rep.saw_done = Some(false); rep.dropped_acks = None;
        assert_eq!(rep.complete_status(), "NO:sent<events,recv_exit=eof,no_done");
        let row = markdown_row(&rep);
        assert!(row.contains("| 3 | 3 | eof | false | – |"), "{row}");
        assert!(row.trim_end().ends_with("| NO:sent<events,recv_exit=eof,no_done |"), "{row}");
        // Missing done lines entirely.
        let rep = build(&send, &recv, 5, "p", "quiet");
        assert_eq!(rep.complete_status(), "NO:recv_exit=–,no_done");
        // Header has the same number of columns as a row.
        let cols = |l: &str| l.matches('|').count();
        assert_eq!(cols(HEADER.lines().next().unwrap()), cols(&markdown_row(&rep)));
        assert_eq!(cols(HEADER.lines().nth(1).unwrap()), cols(&markdown_row(&rep)));
    }
    #[test]
    fn out_of_range_seqs_not_counted_as_arrived() {
        let send: Vec<_> = (0..3).map(|i| s(i, 0)).collect();
        let recv = vec![r(0, 1, true, 0), r(999_999, 2, false, 1), r(1, 3, true, 2), r(7, 4, true, 3)];
        let rep = build(&send, &recv, 3, "p", "quiet");
        assert_eq!((rep.arrived, rep.missing, rep.out_of_range, rep.corrupt, rep.reorder), (2, 1, 2, 0, 0));
    }
    #[test]
    fn corrupt_example_expected_excludes_marker() {
        let send = vec![s(2, 0)];
        // seq 2 is AltLetter: ESC + letter. Got: the letter without ESC.
        let exp = crate::event::expected_payload(2);
        let mut rec = r(2, 1, false, 0); rec.got_hex = hex(&exp[1..]);
        let rep = build(&send, &[rec], 3, "p", "quiet");
        let ex = &rep.corrupt_examples[0];
        assert_eq!(ex.expected_hex, hex(&exp));
        assert!(!ex.expected_hex.contains(&hex(&crate::event::marker(2))));
        assert_eq!(&ex.expected_hex[2..], ex.got_hex); // byte-for-byte comparable: only the ESC differs
    }
    #[test]
    fn markdown_row_and_negative_latency() {
        let send = vec![s(0, 10_000_000)];
        let recv = vec![r(0, 9_000_000, true, 0)];
        let rep = build(&send, &recv, 1, "p", "quiet");
        assert_eq!(rep.p50_ms, Some(-1.0));
        assert!(markdown_row(&rep).contains("-1.0/-1.0/-1.0/-1.0"));
        assert!(markdown_row(&build(&[], &[], 0, "p", "q")).contains("–"));
    }
}
