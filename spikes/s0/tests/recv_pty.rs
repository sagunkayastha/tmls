//! Integration tests: run `s0 recv` on a real PTY, type numbered events at it, check log + acks.
//!
//! Deviations from the brief's sketch (behaviour under test is unchanged):
//! - The sender waits for the receiver's plain-text readiness marker `S0READY` (written only after
//!   raw mode is set) before typing. Otherwise bytes written while the PTY is still in cooked mode go
//!   through the line discipline (^U kill-line, ^W werase, ^V lnext, echo...) and get mangled.
//! - No unbounded `wait()`/`join()`: a 60 s deadline kills the child and fails the test.
use portable_pty::{native_pty_system, Child, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

const READY: &[u8] = b"S0READY";
const DEADLINE: Duration = Duration::from_secs(60);

fn s0_bin() -> std::path::PathBuf { std::path::PathBuf::from(env!("CARGO_BIN_EXE_s0")) }

fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("s0-test");
    std::fs::create_dir_all(&d).unwrap();
    d.join(format!("{name}-{}.jsonl", std::process::id()))
}

fn contains(hay: &[u8], needle: &[u8]) -> bool { hay.windows(needle.len()).any(|w| w == needle) }

/// Reader thread: reads until READY is seen and reports it, then waits for `go` before draining
/// the rest of the PTY output to EOF/EIO. Returns everything read; `seen` mirrors it live.
fn spawn_reader(mut reader: Box<dyn Read + Send>, ready_tx: mpsc::Sender<bool>, go_rx: mpsc::Receiver<()>,
                done_tx: mpsc::Sender<Vec<u8>>, seen: Arc<Mutex<Vec<u8>>>) {
    std::thread::spawn(move || {
        let mut v = Vec::new();
        let mut buf = [0u8; 4096];
        while !contains(&v, READY) {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => { v.extend_from_slice(&buf[..n]); seen.lock().unwrap().extend_from_slice(&buf[..n]); }
            }
        }
        let _ = ready_tx.send(contains(&v, READY));
        let _ = go_rx.recv();
        loop { // EIO once the child exits; bytes so far are kept
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => { v.extend_from_slice(&buf[..n]); seen.lock().unwrap().extend_from_slice(&buf[..n]); }
            }
        }
        let _ = done_tx.send(v);
    });
}

/// Poll the child until it exits or `deadline`; on timeout kill it and panic.
fn wait_with_deadline(child: &mut Box<dyn Child + Send + Sync>, deadline: Instant) {
    loop {
        if let Some(st) = child.try_wait().unwrap() { assert!(st.success(), "s0 recv exited with {st:?}"); return; }
        if Instant::now() > deadline { let _ = child.kill(); panic!("s0 recv did not exit within {DEADLINE:?}"); }
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct Run { child: Box<dyn Child + Send + Sync>, writer: Box<dyn Write + Send>,
             go_tx: mpsc::Sender<()>, done_rx: mpsc::Receiver<Vec<u8>>, deadline: Instant,
             seen: Arc<Mutex<Vec<u8>>>,
             _master: Box<dyn portable_pty::MasterPty + Send> }

fn start(args: &[&str]) -> Run {
    let pty = native_pty_system();
    let pair = pty.openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 }).unwrap();
    let mut cmd = CommandBuilder::new(s0_bin());
    cmd.args(args);
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let writer = pair.master.take_writer().unwrap();
    let reader = pair.master.try_clone_reader().unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (go_tx, go_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let seen = Arc::new(Mutex::new(Vec::new()));
    spawn_reader(reader, ready_tx, go_rx, done_tx, seen.clone());
    let deadline = Instant::now() + DEADLINE;
    if ready_rx.recv_timeout(Duration::from_secs(10)) != Ok(true) {
        let _ = child.kill();
        panic!("s0 recv never signalled ready");
    }
    Run { child, writer, go_tx, done_rx, deadline, seen, _master: pair.master }
}

impl Run {
    fn finish(mut self) -> Vec<u8> {
        let _ = self.go_tx.send(());
        wait_with_deadline(&mut self.child, self.deadline);
        let left = self.deadline.saturating_duration_since(Instant::now()).max(Duration::from_secs(5));
        self.done_rx.recv_timeout(left).expect("PTY reader did not finish")
    }
}

#[test]
fn recv_logs_every_event_and_acks() {
    let log = scratch("recv");
    let mut r = start(&["recv", "--log", log.to_str().unwrap(), "--quit-after", "100"]);
    let _ = r.go_tx.send(()); // drain output concurrently
    for seq in 0..100u32 {
        for w in s0::event::encode(seq) { r.writer.write_all(&w).unwrap(); }
    }
    let out = r.finish();
    let text = String::from_utf8_lossy(&out);
    for seq in 0..100u32 { assert!(text.contains(&format!("A{seq:06}")), "no ack for {seq}"); }
    assert!(text.contains("S0DONE"));
    let lines: Vec<serde_json::Value> = std::fs::read_to_string(&log).unwrap()
        .lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let seqs: Vec<u64> = lines.iter().filter_map(|v| v.get("seq").and_then(|s| s.as_u64())).collect();
    assert_eq!(seqs, (0..100).collect::<Vec<_>>());
    assert!(lines.iter().all(|v| v.get("intact").map_or(true, |i| i.as_bool() == Some(true))));
    let done = lines.last().unwrap();
    assert_eq!(done["done"], true);
    assert_eq!(done["arrivals"], 100);
    assert_eq!(done["exit"], "quit_after");
    // Readiness: ESC[?2004h then S0READY, before any ack.
    let ready = text.find("S0READY").unwrap();
    assert!(text[..ready].contains("\x1b[?2004h"));
    assert!(ready < text.find("A000000").unwrap());
}

/// Type 500 events at a flooding receiver without reading its output, then drain.
/// Returns (seq line count, done record).
fn slow_stdout_run(name: &str, extra: &[&str]) -> (usize, serde_json::Value) {
    let log = scratch(name);
    let mut args = vec!["recv", "--log", log.to_str().unwrap(), "--quit-after", "500"];
    args.extend_from_slice(extra);
    let mut r = start(&args);
    for seq in 0..500u32 {
        for w in s0::event::encode(seq) { r.writer.write_all(&w).unwrap(); }
        std::thread::sleep(Duration::from_millis(2));
    }
    // Only now start draining the PTY (finish sends `go`).
    let out = r.finish();
    assert!(contains(&out, b"S0DONE"));
    let text = std::fs::read_to_string(&log).unwrap();
    let n = text.lines().filter(|l| l.contains("\"seq\"")).count();
    let done: serde_json::Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    eprintln!("{name}: {done}");
    (n, done)
}

#[test]
fn recv_keeps_parsing_when_stdout_slow() {
    // Receiver with flood on; we do not read its output for a while, then check all events logged.
    let (n, done) = slow_stdout_run("slow", &["--flood", "200000"]);
    assert_eq!(n, 500);
    assert_eq!(done["arrivals"], 500);
    assert_eq!(done["exit"], "quit_after");
}

#[test]
fn recv_drops_acks_not_arrivals_when_queue_full() {
    // Tiny stdout queue + flood + no reading: the queue is full almost at once, so acks must be
    // dropped (counted) while every arrival is still logged.
    let (n, done) = slow_stdout_run("drop", &["--flood", "200000", "--ack-queue", "4"]);
    assert_eq!(n, 500);
    assert_eq!(done["arrivals"], 500);
    assert!(done["dropped_acks"].as_u64().unwrap() > 0, "no acks dropped: {done}");
}

fn send_seq(r: &mut Run, seq: u32) { for w in s0::event::encode(seq) { r.writer.write_all(&w).unwrap(); } }

fn recv_seqs(log: &std::path::Path) -> (Vec<u64>, serde_json::Value) {
    let lines: Vec<serde_json::Value> = std::fs::read_to_string(log).unwrap()
        .lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    let seqs = lines.iter().filter_map(|v| v.get("seq").and_then(|s| s.as_u64())).collect();
    (seqs, lines.last().unwrap().clone())
}

/// The receiver is still running `ms` after the last write.
fn still_running(r: &mut Run, ms: u64) -> bool {
    std::thread::sleep(Duration::from_millis(ms));
    r.child.try_wait().unwrap().is_none()
}

/// `--quit-after 5`: duplicates do not count; it stops only once 0..5 have all arrived.
#[test]
fn recv_quit_after_counts_distinct_seqs() {
    let log = scratch("distinct");
    let mut r = start(&["recv", "--log", log.to_str().unwrap(), "--quit-after", "5"]);
    let _ = r.go_tx.send(());
    for seq in [0, 0, 1, 1, 4, 4] { send_seq(&mut r, seq); } // 6 markers, 3 distinct
    assert!(still_running(&mut r, 500), "recv quit on duplicates");
    send_seq(&mut r, 2);
    assert!(still_running(&mut r, 300), "recv quit with 4 of 5 distinct");
    send_seq(&mut r, 3);
    let out = r.finish();
    assert!(contains(&out, b"S0DONE"));
    let (seqs, done) = recv_seqs(&log);
    assert_eq!(seqs, vec![0, 0, 1, 1, 4, 4, 2, 3]);
    assert_eq!(done["exit"], "quit_after");
    assert_eq!(done["arrivals"], 8);
    assert_eq!(done["distinct_in_range"], 5);
}

/// `--quit-after 5`: seqs outside 0..5 (the stray `#999999`, seq 5) do not count either.
#[test]
fn recv_quit_after_ignores_out_of_range() {
    let log = scratch("oob");
    let mut r = start(&["recv", "--log", log.to_str().unwrap(), "--quit-after", "5"]);
    let _ = r.go_tx.send(());
    for seq in 0..4 { send_seq(&mut r, seq); }
    r.writer.write_all(b"#999999").unwrap();
    send_seq(&mut r, 5);
    send_seq(&mut r, 5);
    assert!(still_running(&mut r, 500), "recv counted out-of-range seqs");
    send_seq(&mut r, 4);
    let out = r.finish();
    assert!(contains(&out, b"S0DONE"));
    let (seqs, done) = recv_seqs(&log);
    assert_eq!(seqs, vec![0, 1, 2, 3, 999_999, 5, 5, 4]);
    assert_eq!(done["exit"], "quit_after");
    assert_eq!(done["distinct_in_range"], 5);
}

/// `--flood` starts only at the first arrival, so S0READY stays on a quiet screen until then.
#[test]
fn recv_flood_starts_after_first_arrival() {
    let log = scratch("lazyflood");
    let mut r = start(&["recv", "--log", log.to_str().unwrap(), "--quit-after", "2", "--flood", "200000"]);
    let _ = r.go_tx.send(());
    std::thread::sleep(Duration::from_millis(500));
    assert!(!contains(&r.seen.lock().unwrap(), b"F0"), "flood started before any event");
    send_seq(&mut r, 0);
    let t = Instant::now();
    while !contains(&r.seen.lock().unwrap(), b"F0") {
        assert!(t.elapsed() < Duration::from_secs(5), "no flood after the first event");
        std::thread::sleep(Duration::from_millis(10));
    }
    send_seq(&mut r, 1);
    let out = r.finish();
    let text = String::from_utf8_lossy(&out);
    let ready = text.find("S0READY").unwrap();
    assert!(ready < text.find("F0").unwrap());
}

/// `--ready-token T`: the receiver prints `S0READY:T` (and not a bare `S0READY` line).
#[test]
fn recv_ready_token_printed() {
    let log = scratch("token");
    let mut r = start(&["recv", "--log", log.to_str().unwrap(), "--quit-after", "1", "--ready-token", "abc123"]);
    let _ = r.go_tx.send(());
    send_seq(&mut r, 0);
    let out = r.finish();
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("\x1b[?2004hS0READY:abc123\r\n"), "{text:?}");
    assert!(!text.contains("S0READY\r\n"), "{text:?}");
}
