//! Integration: `s0 send` drives `s0 recv` over a local PTY at 200 Hz; nothing may be lost.
#[test]
fn local_path_zero_loss() {
    let dir = std::path::PathBuf::from("/data/scratch/s0/test"); std::fs::create_dir_all(&dir).unwrap();
    let pid = std::process::id();
    let send_log = dir.join(format!("send-{pid}.jsonl"));
    let recv_log = dir.join(format!("recv-local-{pid}.jsonl"));
    let bin = env!("CARGO_BIN_EXE_s0");
    let st = std::process::Command::new(bin)
        .args(["send", "--log", send_log.to_str().unwrap(), "--events", "300", "--rate", "200", "--settle-ms", "2000", "--",
               bin, "recv", "--log", recv_log.to_str().unwrap(), "--quit-after", "300"])
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
}

fn read_jsonl(p: &std::path::Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(p).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

/// The child inherits send's cwd: a relative program path and a relative recv --log both resolve
/// against it (portable-pty would otherwise use $HOME).
#[test]
fn child_inherits_cwd() {
    let dir = std::path::PathBuf::from("/data/scratch/s0/test"); std::fs::create_dir_all(&dir).unwrap();
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
    let dir = std::path::PathBuf::from("/data/scratch/s0/test"); std::fs::create_dir_all(&dir).unwrap();
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
