//! Shutdown, back-pressure and large writes. These tests look at this process's threads, so they
//! are a test binary of their own and take a lock to run one at a time.

use std::sync::mpsc::channel;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use s2_core::{Emu, Frame, InputMsg, Key, KeyEvent, Mods, Pty};

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn alone() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn sh(script: &str, cols: u16, rows: u16) -> Pty {
    Pty::spawn(&["sh".to_string(), "-c".to_string(), script.to_string()], cols, rows).expect("spawn sh")
}

/// Names of this process's `s2-core` threads (`s2-emu`, `s2-pty-read`, `s2-pty-write`).
fn core_threads() -> Vec<String> {
    let mut names = Vec::new();
    for task in std::fs::read_dir("/proc/self/task").unwrap() {
        // A thread can end between the listing and the read.
        if let Ok(comm) = std::fs::read_to_string(task.unwrap().path().join("comm"))
            && comm.starts_with("s2-")
        {
            names.push(comm.trim().to_string());
        }
    }
    names.sort();
    names
}

/// Drops `emu` on another thread; false if that takes longer than `limit`. A drop that hangs
/// must fail the test, not hang it.
fn drops_within(emu: Emu, limit: Duration) -> bool {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        drop(emu);
        let _ = tx.send(());
    });
    rx.recv_timeout(limit).is_ok()
}

fn assert_no_core_threads() {
    let start = Instant::now();
    while !core_threads().is_empty() {
        assert!(start.elapsed() < Duration::from_secs(2), "threads left after drop: {:?}", core_threads());
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Takes frames until `done` accepts one. On timeout the emulator is dropped off-thread (its
/// drop is what may be broken) and the test fails.
fn frame_within(emu: Emu, limit: Duration, done: impl Fn(&Frame) -> bool) -> Emu {
    let start = Instant::now();
    let mut last = None;
    while start.elapsed() < limit {
        match emu.frames.take() {
            Some(frame) if done(&frame) => return emu,
            Some(frame) => last = Some(frame),
            None => std::thread::sleep(Duration::from_millis(1)),
        }
    }
    let shown = last.map(|f| (f.seq, (0..f.rows).map(|y| f.row_text(y)).collect::<Vec<_>>()));
    drops_within(emu, Duration::from_secs(1));
    panic!("no matching frame within {limit:?}; last frame: {shown:?}");
}

fn has_row(frame: &Frame, text: &str) -> bool {
    (0..frame.rows).any(|y| frame.row_text(y).contains(text))
}

const LINE: &str = "0123456789abcdefghijklmnopqrstuvwxyz the quick brown fox\n";

/// B1: a 4 MiB paste into a child that copies it back to the terminal, as `cat` does. The child
/// stops reading while its own output is not drained, so the paste only completes if the
/// emulator keeps draining output while the write is in progress. The byte count the child
/// prints at the end proves every byte arrived.
///
/// The child writes each line with a carriage return instead of a line feed (and the tty echo is
/// off): 4 MiB of scrolling output takes minutes in a debug build, where libghostty-vt is built
/// with Zig's `Debug` checks, and the back-pressure this test needs does not depend on it.
#[test]
fn large_paste_into_a_child_that_copies_it_back_completes() {
    let _alone = alone();
    let text = LINE.repeat(4 * 1024 * 1024 / LINE.len() + 1);
    assert!(text.len() >= 4 * 1024 * 1024);
    let child = "stty -echo; echo ready; n=$(tr '\\n' '\\r' | tee /dev/tty | wc -c); echo; echo received $n bytes; sleep 1000";
    let emu = Emu::start(sh(child, 80, 24), 80, 24, None).unwrap();
    let emu = frame_within(emu, Duration::from_secs(5), |f| has_row(f, "ready"));
    emu.input_tx.send(InputMsg::Paste(text.clone())).unwrap();
    // Ctrl+D on an empty line ends the child's input; it is queued behind the paste.
    let end_of_input =
        KeyEvent { key: Key::Char('d'), mods: Mods { ctrl: true, ..Mods::default() }, text: None, repeat: false, pressed: true };
    emu.input_tx.send(InputMsg::Key(end_of_input, 1)).unwrap();
    let received = format!("received {} bytes", text.len());
    let emu = frame_within(emu, Duration::from_secs(60), |f| has_row(f, &received));
    assert!(drops_within(emu, Duration::from_secs(2)), "drop after the paste hung");
    assert_no_core_threads();
}

/// B1 as the review states it: 4 MiB pasted into plain `cat`, tty echo on, so 8 MiB of scrolling
/// output comes back. `cat` only ends after it has read and written everything up to the end of
/// input. Ignored by default because a debug build of libghostty-vt parses about 30 KiB/s of
/// such output; run it with
/// `cargo test --release -p s2-core --test shutdown -- --ignored`.
#[test]
#[ignore = "minutes in a debug build; run with --release"]
fn large_paste_into_cat_completes() {
    let _alone = alone();
    let text = LINE.repeat(4 * 1024 * 1024 / LINE.len() + 1);
    let emu = Emu::start(sh("cat; echo; echo cat is done; sleep 1000", 80, 24), 80, 24, None).unwrap();
    emu.input_tx.send(InputMsg::Paste(text)).unwrap();
    let end_of_input =
        KeyEvent { key: Key::Char('d'), mods: Mods { ctrl: true, ..Mods::default() }, text: None, repeat: false, pressed: true };
    emu.input_tx.send(InputMsg::Key(end_of_input, 1)).unwrap();
    let emu = frame_within(emu, Duration::from_secs(120), |f| has_row(f, "cat is done"));
    assert!(drops_within(emu, Duration::from_secs(2)), "drop after the paste hung");
    assert_no_core_threads();
}

/// B1: input for a child that never reads it stays queued without blocking anything; the
/// emulator keeps working and drop still returns.
#[test]
fn drop_returns_while_input_is_stuck_behind_a_child_that_never_reads() {
    let _alone = alone();
    let emu = Emu::start(sh("sleep 1000", 80, 24), 80, 24, None).unwrap();
    emu.input_tx.send(InputMsg::Paste(LINE.repeat(4 * 1024 * 1024 / LINE.len()))).unwrap();
    for id in 0..100 {
        let key = KeyEvent { key: Key::Char('x'), mods: Mods::default(), text: Some("x".into()), repeat: false, pressed: true };
        emu.input_tx.send(InputMsg::Key(key, id)).unwrap();
    }
    // The emulator thread is not stuck in a write: it still handles what comes after. A resize
    // shows in every later frame, whatever the tty echo of the accepted input scrolls away.
    emu.input_tx.send(InputMsg::Resize(100, 30)).unwrap();
    let emu = frame_within(emu, Duration::from_secs(5), |f| (f.cols, f.rows) == (100, 30));
    assert!(drops_within(emu, Duration::from_secs(2)), "drop hung with input queued");
    assert_no_core_threads();
}

/// B2: under a flood the reader waits on the backlog; drop must end it and join it.
#[test]
fn no_thread_is_left_after_dropping_a_flooded_emulator() {
    let _alone = alone();
    for round in 0..3u64 {
        let emu = Emu::start(sh("cat /dev/zero | tr '\\0' a", 300, 100), 300, 100, None).unwrap();
        let emu = frame_within(emu, Duration::from_secs(5), |f| f.row_text(0).starts_with("aaaa"));
        // Long enough for the backlog to fill, so the reader is waiting on it when drop comes.
        std::thread::sleep(Duration::from_millis(60 + 40 * round));
        assert!(drops_within(emu, Duration::from_secs(2)), "drop hung under flood");
    }
    assert_no_core_threads();
}

/// A quiet emulator and one whose child already exited shut down the same way.
#[test]
fn no_thread_is_left_after_dropping_a_quiet_or_exited_emulator() {
    let _alone = alone();
    let quiet = Emu::start(sh("sleep 1000", 80, 24), 80, 24, None).unwrap();
    let exited = Emu::start(sh("true", 80, 24), 80, 24, None).unwrap();
    let start = Instant::now();
    while !exited.exited() {
        assert!(start.elapsed() < Duration::from_secs(5), "the child did not exit");
        std::thread::sleep(Duration::from_millis(2));
    }
    // The exited emulator's reader ends by itself, a moment after it reported the exit; the
    // quiet one's keeps waiting for output.
    let readers = || core_threads().iter().filter(|name| *name == "s2-pty-read").count();
    let start = Instant::now();
    while readers() != 1 {
        assert!(start.elapsed() < Duration::from_secs(2), "expected one reader thread: {:?}", core_threads());
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(drops_within(quiet, Duration::from_secs(2)));
    assert!(drops_within(exited, Duration::from_secs(2)));
    assert_no_core_threads();
}
