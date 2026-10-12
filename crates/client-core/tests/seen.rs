//! The ◆ mark: a session that finished since the user last looked at it.

use client_core::listing::{HostSnapshot, SessionInfo, SessionKey, Status, Tool};
use client_core::seen::Seen;

fn key(host: &str, name: &str) -> SessionKey {
    SessionKey { host: host.to_string(), name: name.to_string() }
}

/// A snapshot of `host` as the listing parser gives it (it never says `Unseen`).
fn snap(host: &str, sessions: &[(&str, Status)]) -> HostSnapshot {
    let sessions = sessions
        .iter()
        .map(|(name, status)| SessionInfo {
            key: key(host, name),
            status: *status,
            reason: None,
            tool: Tool::Other,
            title: None,
            context_pct: None,
            attached: 0,
            last_output_age_s: Some(0),
            pid: None,
        })
        .collect();
    HostSnapshot { host: host.to_string(), online: true, error: None, sessions }
}

fn offline(host: &str) -> HostSnapshot {
    HostSnapshot { host: host.to_string(), online: false, error: Some("timed out".to_string()), sessions: Vec::new() }
}

/// The marks of `host`'s sessions after `seen` has looked at this listing.
fn marks(seen: &mut Seen, host: &str, sessions: &[(&str, Status)]) -> Vec<Status> {
    let mut snapshot = snap(host, sessions);
    seen.apply(&mut snapshot);
    seen.forget_missing(&snapshot);
    snapshot.sessions.iter().map(|s| s.status).collect()
}

use Status::{Error, Idle, Running, Unseen, Waiting};

#[test]
fn running_then_idle_while_not_opened_is_unseen() {
    let mut seen = Seen::new();
    assert_eq!(marks(&mut seen, "ph", &[("job", Running), ("quiet", Idle)]), [Running, Idle]);
    assert_eq!(marks(&mut seen, "ph", &[("job", Idle), ("quiet", Idle)]), [Unseen, Idle]);
    // it stays unseen while nothing changes, however long
    assert_eq!(marks(&mut seen, "ph", &[("job", Idle), ("quiet", Idle)]), [Unseen, Idle]);
    assert_eq!(marks(&mut seen, "ph", &[("job", Idle), ("quiet", Idle)]), [Unseen, Idle]);
}

#[test]
fn a_session_first_seen_idle_is_not_unseen() {
    let mut seen = Seen::new();
    assert_eq!(marks(&mut seen, "ph", &[("old", Idle)]), [Idle]);
    assert_eq!(marks(&mut seen, "ph", &[("old", Idle), ("new", Idle)]), [Idle, Idle]);
}

#[test]
fn opened_clears_it() {
    let mut seen = Seen::new();
    marks(&mut seen, "ph", &[("job", Running)]);
    assert_eq!(marks(&mut seen, "ph", &[("job", Idle)]), [Unseen]);
    seen.opened(&key("ph", "job"));
    assert_eq!(marks(&mut seen, "ph", &[("job", Idle)]), [Idle]);
    assert_eq!(marks(&mut seen, "ph", &[("job", Idle)]), [Idle], "and it stays cleared");
    // the next run that finishes is unseen again
    assert_eq!(marks(&mut seen, "ph", &[("job", Running)]), [Running]);
    assert_eq!(marks(&mut seen, "ph", &[("job", Idle)]), [Unseen]);
}

#[test]
fn opening_a_session_that_is_not_unseen_changes_nothing() {
    let mut seen = Seen::new();
    seen.opened(&key("ph", "never listed"));
    assert_eq!(marks(&mut seen, "ph", &[("job", Running)]), [Running]);
    seen.opened(&key("ph", "job")); // opened while it runs: it still finishes unseen later
    assert_eq!(marks(&mut seen, "ph", &[("job", Idle)]), [Unseen]);
}

#[test]
fn working_again_clears_it() {
    let mut seen = Seen::new();
    marks(&mut seen, "ph", &[("job", Running)]);
    assert_eq!(marks(&mut seen, "ph", &[("job", Idle)]), [Unseen]);
    assert_eq!(marks(&mut seen, "ph", &[("job", Running)]), [Running]);
    assert_eq!(marks(&mut seen, "ph", &[("job", Idle)]), [Unseen]);
}

#[test]
fn waiting_and_error_are_never_overwritten() {
    let mut seen = Seen::new();
    marks(&mut seen, "ph", &[("asks", Running), ("fails", Running)]);
    assert_eq!(marks(&mut seen, "ph", &[("asks", Waiting), ("fails", Error)]), [Waiting, Error]);
    assert_eq!(marks(&mut seen, "ph", &[("asks", Waiting), ("fails", Error)]), [Waiting, Error]);

    // nor when the session was unseen before it came to need the user
    marks(&mut seen, "ph", &[("asks", Running), ("fails", Running)]);
    assert_eq!(marks(&mut seen, "ph", &[("asks", Idle), ("fails", Idle)]), [Unseen, Unseen]);
    assert_eq!(marks(&mut seen, "ph", &[("asks", Waiting), ("fails", Error)]), [Waiting, Error]);
}

#[test]
fn what_follows_waiting_and_error() {
    let mut seen = Seen::new();
    marks(&mut seen, "ph", &[("asks", Waiting), ("fails", Error)]);
    // answered and finished between two listings: it finished unseen. An error that goes idle did not finish.
    assert_eq!(marks(&mut seen, "ph", &[("asks", Idle), ("fails", Idle)]), [Unseen, Idle]);
}

#[test]
fn a_session_that_disappears_is_forgotten() {
    let mut seen = Seen::new();
    marks(&mut seen, "ph", &[("job", Running), ("other", Running)]);
    assert_eq!(marks(&mut seen, "ph", &[("job", Idle), ("other", Idle)]), [Unseen, Unseen]);
    assert_eq!(marks(&mut seen, "ph", &[("other", Idle)]), [Unseen], "job was killed");
    // a new session of the same name starts clean, not with the old one's mark
    assert_eq!(marks(&mut seen, "ph", &[("job", Idle), ("other", Idle)]), [Idle, Unseen]);

    // and one that was running when it went is not "finished" when its name comes back
    marks(&mut seen, "ph", &[("job", Running), ("other", Idle)]);
    marks(&mut seen, "ph", &[("other", Idle)]);
    assert_eq!(marks(&mut seen, "ph", &[("job", Idle), ("other", Idle)]), [Idle, Unseen]);
}

#[test]
fn hosts_do_not_share_sessions() {
    let mut seen = Seen::new();
    marks(&mut seen, "ph", &[("job", Running)]);
    marks(&mut seen, "nano", &[("job", Idle)]);
    assert_eq!(marks(&mut seen, "nano", &[("job", Idle)]), [Idle], "ph's job ran, not nano's");
    assert_eq!(marks(&mut seen, "ph", &[("job", Idle)]), [Unseen]);
    // nano listing nothing forgets nothing of ph
    assert_eq!(marks(&mut seen, "nano", &[]), []);
    assert_eq!(marks(&mut seen, "ph", &[("job", Idle)]), [Unseen]);
    seen.opened(&key("nano", "job"));
    assert_eq!(marks(&mut seen, "ph", &[("job", Idle)]), [Unseen], "opening nano's job is not opening ph's");
}

#[test]
fn an_offline_host_forgets_nothing() {
    let mut seen = Seen::new();
    marks(&mut seen, "ph", &[("done", Running), ("busy", Running)]);
    assert_eq!(marks(&mut seen, "ph", &[("done", Idle), ("busy", Running)]), [Unseen, Running]);
    let mut down = offline("ph");
    seen.apply(&mut down);
    seen.forget_missing(&down);
    // back: the mark is still there, and what finished while the host was away is unseen
    assert_eq!(marks(&mut seen, "ph", &[("done", Idle), ("busy", Idle)]), [Unseen, Unseen]);
}

#[test]
fn a_renamed_session_is_a_new_identity() {
    // the documented limit: the mark does not follow a rename
    let mut seen = Seen::new();
    marks(&mut seen, "ph", &[("job", Running)]);
    assert_eq!(marks(&mut seen, "ph", &[("job", Idle)]), [Unseen]);
    assert_eq!(marks(&mut seen, "ph", &[("job 2", Idle)]), [Idle]);
    // renamed while running: the new name has no "running" behind it
    marks(&mut seen, "ph", &[("build", Running)]);
    assert_eq!(marks(&mut seen, "ph", &[("build 2", Idle)]), [Idle]);
}

#[test]
fn applying_a_snapshot_twice_keeps_the_mark() {
    let mut seen = Seen::new();
    marks(&mut seen, "ph", &[("job", Running), ("quiet", Idle)]);
    let mut kept = snap("ph", &[("job", Idle), ("quiet", Idle)]);
    let statuses = |s: &HostSnapshot| s.sessions.iter().map(|s| s.status).collect::<Vec<_>>();
    seen.apply(&mut kept);
    assert_eq!(statuses(&kept), [Unseen, Idle]);
    for _ in 0..3 {
        seen.apply(&mut kept); // the same snapshot, already marked
        seen.forget_missing(&kept);
        assert_eq!(statuses(&kept), [Unseen, Idle]);
    }
    // and a fresh listing after that is still unseen
    assert_eq!(marks(&mut seen, "ph", &[("job", Idle), ("quiet", Idle)]), [Unseen, Idle]);

    // once opened, applying the kept snapshot again takes the mark off it
    seen.opened(&key("ph", "job"));
    seen.apply(&mut kept);
    assert_eq!(statuses(&kept), [Idle, Idle]);
    seen.apply(&mut kept);
    assert_eq!(statuses(&kept), [Idle, Idle]);
}
