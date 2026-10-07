//! Integration: `s0 send` drives `s0 recv` over a local PTY at 200 Hz; nothing may be lost.

/// Cargo's per-target scratch dir (under `target/`), so the tests run on any host/container.
fn test_dir() -> std::path::PathBuf {
    let d = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("s0-test");
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn local_path_zero_loss() {
    let dir = test_dir();
    let pid = std::process::id();
    let send_log = dir.join(format!("send-{pid}.jsonl"));
    let recv_log = dir.join(format!("recv-local-{pid}.jsonl"));
    let bin = env!("CARGO_BIN_EXE_s0");
    let st = std::process::Command::new(bin)
        .args(["send", "--log", send_log.to_str().unwrap(), "--events", "300", "--rate", "200", "--settle-ms", "2000",
               "--ready-token", "t0k3n42", "--",
               bin, "recv", "--log", recv_log.to_str().unwrap(), "--quit-after", "300", "--ready-token", "t0k3n42"])
        .status().unwrap();
    assert!(st.success());
    let send: Vec<serde_json::Value> = std::fs::read_to_string(&send_log).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let acked = send.iter().filter(|v| v.get("ack_ns").map_or(false, |a| !a.is_null())).count();
    assert_eq!(acked, 300, "every event acked");
    let recv_n = std::fs::read_to_string(&recv_log).unwrap().lines().filter(|l| l.contains("\"seq\"")).count();
    assert_eq!(recv_n, 300);
    let done = send.last().unwrap();
    assert_eq!(done["done"], true);
    assert_eq!(done["acked"], 300);
    assert_eq!(done["saw_done"], true);
    assert_eq!(done["events"], 300);
    assert!(done["effective_rate"].as_f64().unwrap() > 50.0, "{done}");
    assert_eq!(done["marker_gap_ms"], 0);
    assert_eq!(done["ready_token"], true);
    let seen_ms = done["done_seen_ms"].as_f64().expect("done_seen_ms set when S0DONE was seen");
    assert!((0.0..2000.0).contains(&seen_ms), "{done}");
}

/// `--marker-gap-ms`: the marker follows the payload after a pause; nothing is lost and the gap
/// is recorded in the done line.
#[test]
fn marker_gap_run() {
    let dir = test_dir();
    let pid = std::process::id();
    let send_log = dir.join(format!("send-gap-{pid}.jsonl"));
    let recv_log = dir.join(format!("recv-gap-{pid}.jsonl"));
    let bin = env!("CARGO_BIN_EXE_s0");
    let st = std::process::Command::new(bin)
        .args(["send", "--log", send_log.to_str().unwrap(), "--events", "16", "--rate", "50", "--settle-ms", "2000",
               "--marker-gap-ms", "10", "--", bin, "recv", "--log", recv_log.to_str().unwrap(), "--quit-after", "16"])
        .status().unwrap();
    assert!(st.success());
    let send = read_jsonl(&send_log);
    let done = send.last().unwrap();
    assert_eq!((done["acked"].as_u64(), done["saw_done"].as_bool(), done["marker_gap_ms"].as_u64()), (Some(16), Some(true), Some(10)), "{done}");
    // Every event's write took at least the gap.
    for r in send.iter().filter(|v| v.get("seq").is_some()) {
        let d = r["write_end_ns"].as_u64().unwrap() - r["write_start_ns"].as_u64().unwrap();
        assert!(d >= 10_000_000, "{r}");
    }
    let recv = read_jsonl(&recv_log);
    assert_eq!(recv.iter().filter(|v| v["intact"] == true).count(), 16);
}

fn read_jsonl(p: &std::path::Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(p).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

/// The child inherits send's cwd: a relative program path and a relative recv --log both resolve
/// against it (portable-pty would otherwise use $HOME).
#[test]
fn child_inherits_cwd() {
    let dir = test_dir();
    let pid = std::process::id();
    let send_log = dir.join(format!("send-cwd-{pid}.jsonl"));
    let rel_recv = format!("recv-cwd-{pid}.jsonl");
    let bin = std::path::Path::new(env!("CARGO_BIN_EXE_s0"));
    // Relative path from `dir` to the binary: "../" per component of dir, then bin minus its "/".
    let ups = "../".repeat(dir.components().count() - 1);
    let rel_bin = format!("{ups}{}", bin.strip_prefix("/").unwrap().display());
    assert!(!rel_bin.starts_with('/'));
    let st = std::process::Command::new(bin).current_dir(&dir)
        .args(["send", "--log", send_log.to_str().unwrap(), "--events", "20", "--rate", "200", "--settle-ms", "2000", "--",
               &rel_bin, "recv", "--log", &rel_recv, "--quit-after", "20"])
        .status().unwrap();
    assert!(st.success());
    let recv_n = std::fs::read_to_string(dir.join(&rel_recv)).expect("recv log under send's cwd")
        .lines().filter(|l| l.contains("\"seq\"")).count();
    assert_eq!(recv_n, 20);
    assert_eq!(read_jsonl(&send_log).last().unwrap()["acked"], 20);
}

/// A path that goes away part-way (recv quits after 50 of 300): the log still holds the records
/// written so far and a done line carrying the error and/or saw_done.
#[test]
fn partial_run_still_logged() {
    let dir = test_dir();
    let pid = std::process::id();
    let send_log = dir.join(format!("send-partial-{pid}.jsonl"));
    let recv_log = dir.join(format!("recv-partial-{pid}.jsonl"));
    let bin = env!("CARGO_BIN_EXE_s0");
    let st = std::process::Command::new(bin)
        .args(["send", "--log", send_log.to_str().unwrap(), "--events", "300", "--rate", "200", "--settle-ms", "2000", "--",
               bin, "recv", "--log", recv_log.to_str().unwrap(), "--quit-after", "50"])
        .status().unwrap();
    let v = read_jsonl(&send_log);
    let done = v.last().unwrap();
    assert_eq!(done["done"], true, "{done}");
    let recs = v.iter().filter(|x| x.get("seq").is_some()).count();
    assert!(recs >= 50 && recs < 300, "records {recs}: {done}");
    assert_eq!(done["sent"], recs);
    assert!(done["acked"].as_u64().unwrap() >= 50, "{done}");
    assert!(done["saw_done"] == true || done.get("error").is_some(), "{done}");
    assert!(!st.success(), "a path that closed mid-run must fail: {done}");
    // Usually the reader sees EOF first ("PTY closed ..."); an EIO on write ("write seq N") is also valid.
    let err = done["error"].as_str().unwrap();
    assert!(err.contains("PTY closed") || err.contains("write seq"), "{done}");
}

/// The ready gate with a token: a receiver that prints plain `S0READY` (no token) or another
/// token never opens it, so no event is sent and send fails on the ready timeout.
#[test]
fn ready_token_mismatch_sends_nothing() {
    let dir = test_dir();
    let pid = std::process::id();
    let bin = env!("CARGO_BIN_EXE_s0");
    for (name, recv_tok) in [("none", None), ("other", Some("someoneelse"))] {
        let send_log = dir.join(format!("send-tok-{name}-{pid}.jsonl"));
        let recv_log = dir.join(format!("recv-tok-{name}-{pid}.jsonl"));
        let mut args = vec!["send", "--log", send_log.to_str().unwrap(), "--events", "5", "--rate", "200",
                            "--settle-ms", "500", "--ready-timeout-ms", "1500", "--ready-token", "mine123", "--",
                            bin, "recv", "--log", recv_log.to_str().unwrap(), "--quit-after", "5"];
        if let Some(t) = recv_tok { args.extend(["--ready-token", t]); }
        let st = std::process::Command::new(bin).args(&args).status().unwrap();
        assert!(!st.success(), "{name}: gate opened on the wrong marker");
        let v = read_jsonl(&send_log);
        let done = v.last().unwrap();
        assert_eq!(done["sent"], 0, "{name}: {done}");
        assert!(done["error"].as_str().unwrap().contains("S0READY:mine123 not seen"), "{name}: {done}");
    }
}

/// A ready marker that was on screen only before the last nav key does not open the gate (it might
/// be a preview of the receiver while the cursor is still elsewhere); one printed after it does.
#[test]
fn ready_before_last_nav_key_ignored() {
    let dir = test_dir();
    let pid = std::process::id();
    let bin = env!("CARGO_BIN_EXE_s0");
    let run = |name: &str, script: &str| {
        let send_log = dir.join(format!("send-navcut-{name}-{pid}.jsonl"));
        let st = std::process::Command::new(bin)
            .args(["send", "--log", send_log.to_str().unwrap(), "--events", "0", "--settle-ms", "200",
                   "--ready-timeout-ms", "1500", "--ready-token", "navcut1",
                   "--nav", "expect:navcut1:5000,wait:300,key:j", "--", "sh", "-c", script])
            .status().unwrap();
        (st.success(), read_jsonl(&send_log).last().unwrap().clone())
    };
    // Marker printed once, before the nav key; nothing after it.
    let (ok, done) = run("before", "printf 'S0READY:navcut1\\r\\n'; sleep 5");
    assert!(!ok, "gate opened on a pre-nav marker: {done}");
    assert!(done["error"].as_str().unwrap().contains("not seen"), "{done}");
    // Marker printed again after the key arrives (raw mode so `head -c1` returns on the bare `j`;
    // the nav waits 300 ms after the first marker so `stty` has run before the key is sent).
    let (ok, done) = run("after", "printf 'S0READY:navcut1\\r\\n'; stty raw -echo; head -c1 >/dev/null; printf 'S0READY:navcut1\\r\\n'; sleep 5");
    assert!(ok, "gate did not open on a post-nav marker: {done}");
}
