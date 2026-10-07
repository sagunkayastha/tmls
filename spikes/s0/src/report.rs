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
    /// Non-empty log lines (either log) that were not JSON or not a known record/done line.
    pub unparsed_lines: u32,
    /// `yes` or `NO:<reasons>`; see [`Report::complete_status`]. Filled by `main`.
    pub complete: String,
    /// Receiver-only report: send-side columns are unknown, shown as `–`.
    pub recv_only: bool,
}

impl Report {
    /// A row is a gate result only when the run finished: every event was sent, the receiver
    /// stopped on `--quit-after` (not EOF/error) and both logs parsed cleanly. `saw_done` is not
    /// part of the rule: the receiver writes and flushes its done line (`exit=quit_after`) *before*
    /// it prints `S0DONE`, so `recv_exit == quit_after` already proves its log is complete; whether
    /// `S0DONE` then reached the sender's screen (often not, on tmls paths) is reported in its own
    /// `saw_done` column.
    pub fn complete_status(&self) -> String {
        let mut why = Vec::new();
        if !self.recv_only && self.sent < self.events { why.push("sent<events".to_string()); }
        if self.recv_exit.as_deref() != Some("quit_after") {
            why.push(format!("recv_exit={}", self.recv_exit.as_deref().unwrap_or("–")));
        }
        if self.unparsed_lines > 0 { why.push(format!("unparsed={}", self.unparsed_lines)); }
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

/// Non-empty lines parsed as JSON, plus the count of lines that were not JSON.
fn load_lines(path: &Path) -> anyhow::Result<(Vec<serde_json::Value>, u32)> {
    let (mut vals, mut bad) = (Vec::new(), 0);
    for l in std::fs::read_to_string(path)?.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str(l) { Ok(v) => vals.push(v), Err(_) => bad += 1 }
    }
    Ok((vals, bad))
}

/// What the sender log holds besides its records.
#[derive(Debug, Default, PartialEq)]
pub struct SendMeta { pub saw_done: Option<bool>, pub events: Option<u32>, pub unparsed: u32 }

/// Unparsable = not JSON, or JSON that is neither a done line nor a valid record.
pub fn load_send(path: &Path) -> anyhow::Result<(Vec<SendRecord>, SendMeta)> {
    let (vals, mut unparsed) = load_lines(path)?;
    let (mut recs, mut m) = (Vec::new(), SendMeta::default());
    for v in vals {
        if v.get("done").is_some() {
            m.saw_done = v.get("saw_done").and_then(|x| x.as_bool());
            m.events = v.get("events").and_then(|x| x.as_u64()).and_then(|x| u32::try_from(x).ok());
            continue;
        }
        match serde_json::from_value(v) { Ok(r) => recs.push(r), Err(_) => unparsed += 1 }
    }
    m.unparsed = unparsed;
    Ok((recs, m))
}

/// (records, dropped_acks, exit, unparsable lines)
pub fn load_recv(path: &Path) -> anyhow::Result<(Vec<RecvRecord>, Option<u64>, Option<String>, u32)> {
    let (vals, mut unparsed) = load_lines(path)?;
    let (mut recs, mut dropped, mut exit) = (Vec::new(), None, None);
    for v in vals {
        if v.get("done").is_some() {
            dropped = v.get("dropped_acks").and_then(|x| x.as_u64());
            exit = v.get("exit").and_then(|x| x.as_str()).map(String::from);
            continue;
        }
        match serde_json::from_value(v) { Ok(r) => recs.push(r), Err(_) => unparsed += 1 }
    }
    Ok((recs, dropped, exit, unparsed))
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
    if r.recv_only {
        return format!("| {} | {} | {} | {} | {} | {} | {} | {} | – | – | {} | – | {} | – | – | {} |", r.path, r.load, r.events,
            r.arrived, r.missing, r.dup, r.corrupt, r.reorder, opt(&r.recv_exit), opt(&r.dropped_acks), r.complete_status());
    }
    format!("| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |", r.path, r.load, r.events,
        r.arrived, r.missing, r.dup, r.corrupt, r.reorder, r.acked,
        r.sent, opt(&r.recv_exit), opt(&r.saw_done), opt(&r.dropped_acks),
        fmt_quad([r.p50_ms, r.p95_ms, r.p99_ms, r.max_ms]),
        fmt_quad([r.ack_p50_ms, r.ack_p95_ms, r.ack_p99_ms, r.ack_max_ms]),
        r.complete_status())
}

pub struct Opts {
    pub send: Option<PathBuf>, pub recv: PathBuf, pub path: String, pub load: String,
    pub json: Option<PathBuf>, pub header: bool, pub skew_note: Option<String>, pub events: Option<u32>,
    /// Skip send-side fields (sent/acked/latencies = `–`); `events` is then required.
    pub recv_only: bool,
}

pub fn main(o: Opts) -> anyhow::Result<()> {
    let (send, meta) = match (&o.send, o.recv_only) {
        (Some(p), false) => load_send(p)?,
        (_, true) => (Vec::new(), SendMeta::default()),
        (None, false) => anyhow::bail!("--send is required unless --recv-only"),
    };
    if o.recv_only { anyhow::ensure!(o.events.is_some(), "--recv-only needs --events"); }
    let (recv, dropped, exit, recv_unparsed) = load_recv(&o.recv)?;
    // Default: what the sender was asked to send (its done line), so a run cut short is not
    // silently judged against only the events it managed to send.
    let events = o.events.or(meta.events).unwrap_or(send.len() as u32);
    let mut rep = build(&send, &recv, events, &o.path, &o.load);
    rep.recv_only = o.recv_only;
    rep.dropped_acks = dropped; rep.recv_exit = exit; rep.saw_done = meta.saw_done; rep.skew_note = o.skew_note;
    rep.unparsed_lines = meta.unparsed + recv_unparsed;
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
    /// `S0_TEST_DIR` if set, else the system temp dir (archbox runs tests with TMPDIR=/data/scratch/s0).
    fn test_dir(name: &str) -> std::path::PathBuf {
        let base = std::env::var_os("S0_TEST_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let d = base.join(format!("s0-report-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    #[test]
    fn load_skips_done_lines() {
        let d = test_dir("load");
        let sp = d.join("send.jsonl"); let rp = d.join("recv.jsonl");
        std::fs::write(&sp, "{\"seq\":0,\"kind\":\"Letter\",\"write_start_ns\":1,\"write_end_ns\":2,\"ack_ns\":null}\n{\"done\":true,\"acked\":0,\"saw_done\":true,\"events\":7,\"rate\":50.0,\"cmd\":[]}\n").unwrap();
        std::fs::write(&rp, "{\"seq\":0,\"intact\":true,\"got_hex\":\"\",\"wall_ns\":5,\"idx\":0}\n{\"done\":true,\"arrivals\":1,\"dropped_acks\":3,\"exit\":\"quit_after\"}\n").unwrap();
        let (sv, meta) = load_send(&sp).unwrap();
        let (rv, dropped, exit, bad) = load_recv(&rp).unwrap();
        assert_eq!((sv.len(), rv.len(), dropped, exit.as_deref(), bad), (1, 1, Some(3), Some("quit_after"), 0));
        assert_eq!(meta, SendMeta { saw_done: Some(true), events: Some(7), unparsed: 0 });
    }
    #[test]
    fn unparsable_lines_counted_and_events_default_from_done_line() {
        let d = test_dir("unparsed");
        let sp = d.join("send.jsonl"); let rp = d.join("recv.jsonl"); let jp = d.join("rep.json");
        // send: 1 record, a truncated line, a JSON line that is not a record, done line asks for 3.
        std::fs::write(&sp, "{\"seq\":0,\"kind\":\"Letter\",\"write_start_ns\":1,\"write_end_ns\":2,\"ack_ns\":null}\n{\"seq\":1,\"ki\n{\"x\":1}\n{\"done\":true,\"saw_done\":false,\"events\":3}\n").unwrap();
        std::fs::write(&rp, "{\"seq\":0,\"intact\":true,\"got_hex\":\"\",\"wall_ns\":5,\"idx\":0}\ngarbage\n{\"done\":true,\"arrivals\":1,\"dropped_acks\":0,\"exit\":\"quit_after\"}\n").unwrap();
        let (_, meta) = load_send(&sp).unwrap();
        assert_eq!(meta.unparsed, 2);
        assert_eq!(load_recv(&rp).unwrap().3, 1);
        main(Opts { send: Some(sp), recv: rp, path: "p".into(), load: "q".into(), json: Some(jp.clone()),
                    header: false, skew_note: None, events: None, recv_only: false }).unwrap();
        let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&jp).unwrap()).unwrap();
        assert_eq!(j["events"], 3, "events default = send done line's events, not send.len()");
        assert_eq!(j["sent"], 1);
        assert_eq!(j["unparsed_lines"], 3);
        assert_eq!(j["complete"], "NO:sent<events,unparsed=3");
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
        // Complete without S0DONE on screen (typical tmls row): saw_done is reported, not gated.
        rep.saw_done = Some(false);
        assert_eq!(rep.complete_status(), "yes");
        assert!(markdown_row(&rep).contains("| 5 | 5 | quit_after | false | 0 |"));
        // Incomplete: 3 of 5 sent, recv hit EOF, no S0DONE.
        let mut rep = build(&send[..3], &recv[..3], 5, "p", "quiet");
        rep.recv_exit = Some("eof".into()); rep.saw_done = Some(false); rep.dropped_acks = None;
        assert_eq!(rep.complete_status(), "NO:sent<events,recv_exit=eof");
        let row = markdown_row(&rep);
        assert!(row.contains("| 3 | 3 | eof | false | – |"), "{row}");
        assert!(row.trim_end().ends_with("| NO:sent<events,recv_exit=eof |"), "{row}");
        // Missing done lines entirely.
        let rep = build(&send, &recv, 5, "p", "quiet");
        assert_eq!(rep.complete_status(), "NO:recv_exit=–");
        // Header has the same number of columns as a row.
        let cols = |l: &str| l.matches('|').count();
        assert_eq!(cols(HEADER.lines().next().unwrap()), cols(&markdown_row(&rep)));
        assert_eq!(cols(HEADER.lines().nth(1).unwrap()), cols(&markdown_row(&rep)));
    }
    #[test]
    fn recv_only_row_has_dashes_and_same_columns() {
        let recv: Vec<_> = (0..4).map(|i| r(i, 1, true, i as u64)).collect();
        let mut rep = build(&[], &recv, 5, "k1", "quiet");
        rep.recv_only = true; rep.recv_exit = Some("quit_after".into());
        assert_eq!((rep.arrived, rep.missing), (4, 1));
        let row = markdown_row(&rep);
        assert!(row.contains("| 5 | 4 | 1 | 0 | 0 | 0 | – | – | quit_after | – |"), "{row}");
        assert!(row.trim_end().ends_with("| – | – | yes |"), "{row}");
        assert_eq!(row.matches('|').count(), HEADER.lines().next().unwrap().matches('|').count());
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
