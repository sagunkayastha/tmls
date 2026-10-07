//! Integration tests: run `s0 recv` on a real PTY, type numbered events at it, check log + acks.
//!
//! Deviations from the brief's sketch (behaviour under test is unchanged):
//! - The sender waits for the receiver's first output (`ESC[?2004h`, written only after raw mode
//!   is set) before typing. Otherwise bytes written while the PTY is still in cooked mode go
//!   through the line discipline (^U kill-line, ^W werase, ^V lnext, echo...) and get mangled.
//! - No unbounded `wait()`/`join()`: a 60 s deadline kills the child and fails the test.
use portable_pty::{native_pty_system, Child, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const READY: &[u8] = b"\x1b[?2004h";
const DEADLINE: Duration = Duration::from_secs(60);

fn s0_bin() -> std::path::PathBuf { std::path::PathBuf::from(env!("CARGO_BIN_EXE_s0")) }

fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::path::PathBuf::from("/data/scratch/s0/test");
    std::fs::create_dir_all(&d).unwrap();
    d.join(format!("{name}-{}.jsonl", std::process::id()))
}

fn contains(hay: &[u8], needle: &[u8]) -> bool { hay.windows(needle.len()).any(|w| w == needle) }

/// Reader thread: reads until READY is seen and reports it, then waits for `go` before draining
/// the rest of the PTY output to EOF/EIO. Returns everything read.
fn spawn_reader(mut reader: Box<dyn Read + Send>, ready_tx: mpsc::Sender<bool>, go_rx: mpsc::Receiver<()>,
                done_tx: mpsc::Sender<Vec<u8>>) {
    std::thread::spawn(move || {
        let mut v = Vec::new();
        let mut buf = [0u8; 4096];
        while !contains(&v, READY) {
            match reader.read(&mut buf) { Ok(0) | Err(_) => break, Ok(n) => v.extend_from_slice(&buf[..n]) }
        }
        let _ = ready_tx.send(contains(&v, READY));
        let _ = go_rx.recv();
        let _ = reader.read_to_end(&mut v); // EIO once the child exits; bytes so far are kept
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
    spawn_reader(reader, ready_tx, go_rx, done_tx);
    let deadline = Instant::now() + DEADLINE;
    if ready_rx.recv_timeout(Duration::from_secs(10)) != Ok(true) {
        let _ = child.kill();
        panic!("s0 recv never signalled ready");
    }
    Run { child, writer, go_tx, done_rx, deadline, _master: pair.master }
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
}

#[test]
fn recv_keeps_parsing_when_stdout_slow() {
    // Receiver with flood on; we do not read its output for a while, then check all events logged.
    let log = scratch("slow");
    let mut r = start(&["recv", "--log", log.to_str().unwrap(), "--quit-after", "500", "--flood", "200000"]);
    for seq in 0..500u32 {
        for w in s0::event::encode(seq) { r.writer.write_all(&w).unwrap(); }
        std::thread::sleep(Duration::from_millis(2));
    }
    // Only now start draining the PTY (finish sends `go`).
    let out = r.finish();
    assert!(contains(&out, b"S0DONE"));
    let text = std::fs::read_to_string(&log).unwrap();
    let n = text.lines().filter(|l| l.contains("\"seq\"")).count();
    assert_eq!(n, 500);
    let done: serde_json::Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    eprintln!("slow-stdout run: {done}");
}
