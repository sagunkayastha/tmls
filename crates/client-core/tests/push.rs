//! Status pushed by Claude's hooks through tmls's events.jsonl: the line parser, the mapping
//! onto a listing (tmls `push.apply`), the replies kept across listings, and the reader process.

use std::collections::HashMap;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use client_core::listing::{HostSnapshot, SessionInfo, SessionKey, Status, Tool};
use client_core::push::{
    Applied, Kind, PushEvent, PushLine, REPLY_CHARS, Tail, apply, carry_replies, one_line, parse_line, tail_argv,
};

const SID: &str = "0b5c1d2e-3f4a-4b6c-8d7e-9f0a1b2c3d4e";

fn key(name: &str) -> SessionKey {
    SessionKey { host: "nas".to_string(), name: name.to_string() }
}

/// A listed Claude session with session id `sid`, pane `pane`, status time `since`.
fn claude(name: &str, status: Status, sid: Option<&str>, pane: Option<&str>, since: u64) -> SessionInfo {
    SessionInfo {
        key: key(name),
        status,
        reason: None,
        tool: Tool::Claude,
        title: None,
        context_pct: None,
        attached: 0,
        last_output_age_s: Some(0),
        pid: None,
        session_id: sid.map(str::to_string),
        pane: pane.map(str::to_string),
        activity: Some(since),
        last_reply: None,
    }
}

fn listing(sessions: Vec<SessionInfo>) -> HostSnapshot {
    HostSnapshot { host: "nas".to_string(), online: true, error: None, sessions }
}

fn event(line: &str) -> PushEvent {
    match parse_line(line) {
        Some(PushLine::Event(event)) => event,
        other => panic!("{line}: {other:?}"),
    }
}

#[test]
fn hook_lines_parse_as_tmls_writes_them() {
    let e = event(&format!(r#"{{"t":1760200000.25,"ev":"PermissionRequest","sid":"{SID}","pane":"%7","cwd":"/w","tool":"Bash"}}"#));
    assert_eq!(e, PushEvent { t: 1760200000.25, sid: Some(SID.into()), pane: Some("%7".into()), kind: Kind::Waiting("permission prompt".into()) });
    let kind = |line: &str| event(line).kind;
    assert_eq!(kind(r#"{"t":1,"ev":"Notification","nt":"permission_prompt","msg":"Claude needs your permission"}"#), Kind::Waiting("permission prompt".into()));
    assert_eq!(kind(r#"{"t":1,"ev":"Notification","nt":"idle_prompt"}"#), Kind::Other);
    assert_eq!(kind(r#"{"t":1,"ev":"UserPromptSubmit","sid":"x"}"#), Kind::Busy);
    assert_eq!(kind(r#"{"t":1,"ev":"PreToolUse","tool":"Edit"}"#), Kind::Busy);
    assert_eq!(kind(r#"{"t":1,"ev":"Stop","msg":"All 12 tests pass.","bg":2}"#), Kind::Stop { msg: Some("All 12 tests pass.".into()), bg: 2 });
    assert_eq!(kind(r#"{"t":1,"ev":"Stop","msg":"","bg":0}"#), Kind::Stop { msg: None, bg: 0 });
    assert_eq!(kind(r#"{"t":1,"ev":"StopFailure","err":"rate_limit","msg":"429 Too Many Requests"}"#), Kind::Failed { err: "rate_limit".into(), msg: Some("429 Too Many Requests".into()) });
    assert_eq!(kind(r#"{"t":1,"ev":"SessionStart","src":"startup"}"#), Kind::SessionStart);
    assert_eq!(kind(r#"{"t":1,"ev":"SessionEnd","reason":"exit"}"#), Kind::SessionEnd);
    assert_eq!(kind(r#"{"t":1,"ev":"StatusLine","sid":"x","ctx":40}"#), Kind::Other);
    // A pane that is no pane id is dropped, the event kept.
    assert_eq!(event(r#"{"t":1,"ev":"Stop","pane":"%7;reboot"}"#).pane, None);
    assert_eq!(parse_line("tmls-push on"), Some(PushLine::Available(true)));
    assert_eq!(parse_line("tmls-push off"), Some(PushLine::Available(false)));
}

#[test]
fn junk_lines_are_ignored() {
    for junk in ["", "not json", r#"{"t":1,"ev":"Stop""#, "[1,2]", r#"{"ev":"Stop","sid":"x"}"#, r#"{"t":"soon","ev":"Stop"}"#, r#"{"t":1}"#, "tmls-push maybe"] {
        assert_eq!(parse_line(junk), None, "{junk:?}");
    }
    // A huge message parses; its reply is one line of REPLY_CHARS.
    let huge = format!(r#"{{"t":5,"ev":"Stop","sid":"{SID}","msg":"{}"}}"#, "x".repeat(100_000));
    let mut snap = listing(vec![claude("w", Status::Running, Some(SID), None, 1)]);
    assert_eq!(apply(&mut snap, &event(&huge)), Applied::Changed);
    assert_eq!(snap.sessions[0].last_reply.as_ref().map(|r| r.chars().count()), Some(REPLY_CHARS));
}

#[test]
fn events_move_a_session_as_tmls_does() {
    let mut snap = listing(vec![claude("w", Status::Idle, Some(SID), Some("%3"), 100)]);
    let at = |t: f64, kind: Kind| PushEvent { t, sid: Some(SID.into()), pane: None, kind };
    assert_eq!(apply(&mut snap, &at(101.0, Kind::Busy)), Applied::Changed);
    assert_eq!((snap.sessions[0].status, snap.sessions[0].activity), (Status::Running, Some(101)));
    assert_eq!(apply(&mut snap, &at(101.5, Kind::Busy)), Applied::Unchanged, "nothing new");
    assert_eq!(apply(&mut snap, &at(102.0, Kind::Waiting("permission prompt".into()))), Applied::Changed);
    assert_eq!((snap.sessions[0].status, snap.sessions[0].reason.as_deref()), (Status::Waiting, Some("permission prompt")));
    assert_eq!(apply(&mut snap, &at(103.0, Kind::Stop { msg: Some("Done.\nDetails below".into()), bg: 0 })), Applied::Changed);
    let s = &snap.sessions[0];
    assert_eq!((s.status, s.reason.as_deref(), s.last_reply.as_deref()), (Status::Idle, None, Some("Done.")));
    // A background task still runs: running, the reply kept.
    assert_eq!(apply(&mut snap, &at(104.0, Kind::Stop { msg: None, bg: 1 })), Applied::Changed);
    assert_eq!((snap.sessions[0].status, snap.sessions[0].last_reply.as_deref()), (Status::Running, Some("Done.")));
    assert_eq!(apply(&mut snap, &at(105.0, Kind::Failed { err: "rate_limit".into(), msg: None })), Applied::Changed);
    let s = &snap.sessions[0];
    assert_eq!((s.status, s.reason.as_deref(), s.last_reply.as_deref()), (Status::Error, Some("rate_limit"), Some("rate_limit")));
    // By pane when the session id is not listed.
    let mut snap = listing(vec![claude("p", Status::Idle, None, Some("%9"), 100)]);
    let by_pane = PushEvent { t: 200.0, sid: Some("other".into()), pane: Some("%9".into()), kind: Kind::Busy };
    assert_eq!(apply(&mut snap, &by_pane), Applied::Changed);
    assert_eq!(snap.sessions[0].status, Status::Running);
}

#[test]
fn older_or_unknown_events_change_nothing() {
    let mut snap = listing(vec![claude("w", Status::Running, Some(SID), Some("%3"), 500)]);
    let before = snap.clone();
    let older = PushEvent { t: 499.0, sid: Some(SID.into()), pane: None, kind: Kind::Stop { msg: Some("old".into()), bg: 0 } };
    assert_eq!(apply(&mut snap, &older), Applied::Unchanged, "the listing saw something newer");
    let stranger = PushEvent { t: 900.0, sid: Some("nobody".into()), pane: Some("%77".into()), kind: Kind::Busy };
    assert_eq!(apply(&mut snap, &stranger), Applied::Unchanged);
    let other = PushEvent { t: 900.0, sid: Some(SID.into()), pane: None, kind: Kind::Other };
    assert_eq!(apply(&mut snap, &other), Applied::Unchanged);
    let mut offline = HostSnapshot { online: false, sessions: Vec::new(), ..snap.clone() };
    assert_eq!(apply(&mut offline, &PushEvent { kind: Kind::Busy, ..stranger.clone() }), Applied::Unchanged);
    assert_eq!(snap, before);
}

#[test]
fn session_start_and_end_ask_for_a_listing() {
    let mut snap = listing(vec![]);
    for kind in [Kind::SessionStart, Kind::SessionEnd] {
        assert_eq!(apply(&mut snap, &PushEvent { t: 1.0, sid: None, pane: None, kind }), Applied::Relist);
    }
}

#[test]
fn the_reply_is_its_first_line_cut_to_120_chars() {
    assert_eq!(one_line("\n\n  Fixed it.  \nMore."), Some("Fixed it.".to_string()));
    assert_eq!(one_line("   \n"), None);
    assert_eq!(one_line(&"é".repeat(300)).map(|r| r.chars().count()), Some(REPLY_CHARS));
}

#[test]
fn replies_are_carried_across_listings_until_the_session_goes() {
    let mut replies = HashMap::new();
    let mut first = listing(vec![SessionInfo { last_reply: Some("done".into()), ..claude("w", Status::Idle, Some(SID), None, 1) }]);
    carry_replies(&mut first, &mut replies);
    assert_eq!(replies.get(&key("w")).map(String::as_str), Some("done"));
    let mut next = listing(vec![claude("w", Status::Idle, Some(SID), None, 1)]);
    carry_replies(&mut next, &mut replies);
    assert_eq!(next.sessions[0].last_reply.as_deref(), Some("done"));
    let mut offline = HostSnapshot { online: false, sessions: Vec::new(), ..listing(vec![]) };
    carry_replies(&mut offline, &mut replies);
    assert_eq!(replies.len(), 1, "a host away forgets nothing");
    carry_replies(&mut listing(vec![]), &mut replies);
    assert!(replies.is_empty(), "gone from an online listing");
}

#[test]
fn the_reader_only_reads_events_jsonl() {
    let argv = tail_argv("ph", "here");
    let script = argv.last().unwrap();
    assert!(script.contains("tail -n0 -F \"$f\""), "{script}");
    assert!(script.contains("$HOME/.cache/tmls/events.jsonl"), "{script}");
    assert!(!script.contains('>') || script.contains("2>/dev/null"), "writes nothing: {script}");
    assert_eq!(tail_argv("here", "here")[..2], ["sh", "-c"]);
}

#[test]
fn a_reader_reads_lines_restarts_after_its_end_and_stops_when_dropped() {
    let (tx, rx) = channel();
    let script = r#"echo tmls-push on; echo '{"t":5,"ev":"UserPromptSubmit","sid":null,"pane":"%1"}'; echo garbage; exit 0"#;
    let tail = Tail::start_with(
        vec!["sh".into(), "-c".into(), script.into()],
        Duration::from_millis(100),
        Box::new(move |line| {
            let _ = tx.send(line);
        }),
    );
    let got: Vec<PushLine> = (0..3).map(|_| rx.recv_timeout(Duration::from_secs(5)).expect("a line")).collect();
    assert_eq!(got[0], PushLine::Available(true));
    assert!(matches!(&got[1], PushLine::Event(e) if e.kind == Kind::Busy && e.pane.as_deref() == Some("%1")));
    assert_eq!(got[2], PushLine::Available(true), "started again after it ended; garbage skipped");
    drop(tail);
    // A reader that never ends is killed when dropped, at once.
    let tail = Tail::start_with(
        vec!["sh".into(), "-c".into(), "echo tmls-push off; exec sleep 30".into()],
        Duration::from_millis(100),
        Box::new(|_| {}),
    );
    std::thread::sleep(Duration::from_millis(300));
    let dropping = Instant::now();
    drop(tail);
    assert!(dropping.elapsed() < Duration::from_secs(2));
}
