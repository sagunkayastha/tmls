use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::{Duration, Instant};

use libghostty_vt::terminal::{Options, Terminal};
use s2_core::keys::{paste_bytes, Encoder};
use s2_core::latency::{self, Event, Log, Pcts};
use s2_core::{now_ns, Emu, Frame, InputMsg, Key, KeyEvent, Mods, Named, Pty};

const WAIT: Duration = Duration::from_secs(5);

fn cat(cols: u16, rows: u16) -> Pty {
    Pty::spawn(&["cat".to_string()], cols, rows).expect("spawn cat")
}

fn sh(script: &str, cols: u16, rows: u16) -> Pty {
    Pty::spawn(&["sh".to_string(), "-c".to_string(), script.to_string()], cols, rows).expect("spawn sh")
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Takes frames until `done` accepts one; returns it and how many frames were taken.
fn frames_until(emu: &Emu, done: impl Fn(&Frame) -> bool) -> (Frame, usize) {
    let start = Instant::now();
    let mut taken = 0;
    loop {
        if let Some(frame) = emu.frames.take() {
            taken += 1;
            if done(&frame) {
                return (frame, taken);
            }
        }
        assert!(start.elapsed() < WAIT, "no matching frame within {WAIT:?}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn wait_for(what: &str, cond: impl Fn() -> bool) {
    let start = Instant::now();
    while !cond() {
        assert!(start.elapsed() < WAIT, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn press(key: Key, mods: Mods, text: Option<&str>) -> KeyEvent {
    KeyEvent { key, mods, text: text.map(str::to_string), repeat: false, pressed: true }
}

fn typed(c: char) -> KeyEvent {
    press(Key::Char(c), Mods::default(), Some(&c.to_string()))
}

fn repeated(c: char) -> KeyEvent {
    KeyEvent { repeat: true, ..typed(c) }
}

fn has_row(frame: &Frame, text: &str) -> bool {
    (0..frame.rows).any(|y| frame.row_text(y).contains(text))
}

/// A waker that counts its calls.
fn count_wakes(emu: &Emu) -> Arc<AtomicUsize> {
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = wakes.clone();
    emu.frames.set_waker(Box::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }));
    wakes
}

/// The `frame` lines of a latency log: (seq, keys).
fn logged_frames(dir: &std::path::Path) -> Vec<(u64, Vec<u64>)> {
    let text = std::fs::read_to_string(dir.join("latency.jsonl")).unwrap();
    let mut frames = Vec::new();
    for line in text.lines() {
        if let Event::Frame { seq, keys, .. } = serde_json::from_str(line).unwrap() {
            frames.push((seq, keys));
        }
    }
    frames
}

#[test]
fn pty_echo_roundtrip() {
    let pty = cat(80, 24);
    let mut reader = pty.reader().unwrap();
    pty.writer().write_all(b"hi\n").unwrap();
    // `read` waits for data, so it runs on its own thread and the wait here can time out.
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 256];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut seen = Vec::new();
    let start = Instant::now();
    // The tty echoes "hi" and cat prints it again.
    while seen.iter().filter(|&&b| b == b'h').count() < 2 {
        let left = WAIT.checked_sub(start.elapsed()).unwrap_or_default();
        let chunk = rx.recv_timeout(left);
        seen.extend(chunk.unwrap_or_else(|_| panic!("read back only {:?}", String::from_utf8_lossy(&seen))));
    }
    assert!(contains(&seen, b"hi"));
    // Once the child is gone the reader reports end of file instead of waiting forever.
    drop(pty);
    let start = Instant::now();
    while rx.recv_timeout(WAIT.checked_sub(start.elapsed()).expect("the reader to end")).is_ok() {}
}

#[test]
fn emu_frame_after_output() {
    let emu = Emu::start(cat(80, 24), 80, 24, None).unwrap();
    let (first, _) = frames_until(&emu, |_| true);
    assert_eq!((first.cols, first.rows, first.cells.len()), (80, 24, 80 * 24));
    assert!(first.dirty_rows.iter().all(|&d| d), "the first frame draws every row");

    emu.input_tx.send(InputMsg::Bytes(b"ok".to_vec(), now_ns())).unwrap();
    let (frame, _) = frames_until(&emu, |f| f.row_text(0) == "ok");
    assert!(frame.dirty_rows[0]);
    assert!(frame.dirty_rows[1..].iter().all(|&d| !d), "only row 0 changed: {:?}", frame.dirty_rows);
    assert!(frame.seq > first.seq);
}

/// The slot holds one frame: 100 frames published while the front-end is busy leave only the
/// last one to take, and it redraws every row the 100 changed.
#[test]
fn emu_slot_is_latest_wins() {
    let emu = Emu::start(cat(80, 24), 80, 24, None).unwrap();
    emu.input_tx.send(InputMsg::Bytes(b"top".to_vec(), now_ns())).unwrap();
    let (before, _) = frames_until(&emu, |f| f.row_text(0) == "top");
    let wakes = count_wakes(&emu);
    // One feed at a time, each waited for, so each is its own publish; none is taken.
    for i in 0..100 {
        let published = wakes.load(Ordering::SeqCst);
        emu.input_tx.send(InputMsg::Bytes(format!("\r\nline {i}").into_bytes(), now_ns())).unwrap();
        wait_for("a frame for the feed", || wakes.load(Ordering::SeqCst) > published);
    }
    let last = emu.frames.take().expect("the latest frame");
    assert_eq!(last.seq, before.seq + 100, "every feed was published");
    assert_eq!((last.row_text(22), last.row_text(23)), ("line 98".to_string(), "line 99".to_string()));
    assert!(last.dirty_rows.iter().all(|&d| d), "rows dirtied by replaced frames stay dirty");
    let cursor = last.cursor.expect("cursor in viewport");
    assert_eq!((cursor.col, cursor.row), (7, 23));
    assert!(emu.frames.take().is_none(), "one slot, not a queue");
}

#[test]
fn emu_reports_wide_cells_and_colors() {
    let emu = Emu::start(cat(20, 4), 20, 4, None).unwrap();
    emu.input_tx.send(InputMsg::Bytes("\x1b[1;31m你a".as_bytes().to_vec(), now_ns())).unwrap();
    let (frame, _) = frames_until(&emu, |f| f.row_text(0) == "你a");
    assert_eq!(frame.cells[0].wide, s2_core::Wide::Lead);
    assert_eq!(frame.cells[1].wide, s2_core::Wide::Spacer);
    assert_eq!(frame.cells[2].text, "a");
    assert!(frame.cells[0].flags.bold);
    assert!(frame.cells[0].fg.is_some());
}

#[test]
fn keys_encode_letter_and_ctrl() {
    let mut term = Terminal::new(Options { cols: 80, rows: 24, max_scrollback: 0 }).unwrap();
    let mut enc = Encoder::new().unwrap();
    let ctrl = Mods { ctrl: true, ..Mods::default() };
    assert_eq!(enc.encode(&press(Key::Char('a'), Mods::default(), Some("a")), &term).unwrap(), b"a");
    assert_eq!(enc.encode(&press(Key::Char('c'), ctrl, None), &term).unwrap(), b"\x03");
    assert_eq!(enc.encode(&press(Key::Named(Named::Up), Mods::default(), None), &term).unwrap(), b"\x1b[A");
    term.vt_write(b"\x1b[?1h"); // DECCKM
    assert_eq!(enc.encode(&press(Key::Named(Named::Up), Mods::default(), None), &term).unwrap(), b"\x1bOA");
    assert_eq!(enc.encode(&press(Key::Named(Named::Enter), Mods::default(), None), &term).unwrap(), b"\r");
}

/// B4: in legacy mode Ctrl + punctuation sends its C0 byte (it used to send nothing).
#[test]
fn keys_ctrl_punctuation_sends_control_bytes() {
    let term = Terminal::new(Options { cols: 80, rows: 24, max_scrollback: 0 }).unwrap();
    let mut enc = Encoder::new().unwrap();
    let ctrl = Mods { ctrl: true, ..Mods::default() };
    let ctrl_shift = Mods { ctrl: true, shift: true, ..Mods::default() };
    let cases: [(char, Mods, &[u8]); 12] = [
        ('[', ctrl, b"\x1b"),
        ('\\', ctrl, b"\x1c"),
        (']', ctrl, b"\x1d"),
        ('/', ctrl, b"\x1f"),
        ('_', ctrl_shift, b"\x1f"),
        ('@', ctrl_shift, b"\x00"),
        ('^', ctrl_shift, b"\x1e"),
        ('?', ctrl_shift, b"\x7f"),
        (' ', ctrl, b"\x00"),
        // The letters libghostty-vt leaves out of its control table.
        ('i', ctrl, b"\t"),
        ('m', ctrl, b"\r"),
        ('C', ctrl_shift, b"\x03"),
    ];
    for (c, mods, want) in cases {
        let got = enc.encode(&press(Key::Char(c), mods, None), &term).unwrap();
        assert_eq!(got.escape_ascii().to_string(), want.escape_ascii().to_string(), "Ctrl+{c:?}");
    }
    // Alt adds the ESC prefix; a release and a key with no control meaning still send nothing.
    let ctrl_alt = Mods { ctrl: true, alt: true, ..Mods::default() };
    assert_eq!(enc.encode(&press(Key::Char('c'), ctrl_alt, None), &term).unwrap(), b"\x1b\x03");
    assert_eq!(enc.encode(&press(Key::Char('\\'), ctrl_alt, None), &term).unwrap(), b"\x1b\x1c");
    let release = KeyEvent { pressed: false, ..press(Key::Char('['), ctrl, None) };
    assert_eq!(enc.encode(&release, &term).unwrap(), b"");
    assert_eq!(enc.encode(&press(Key::Char('-'), ctrl, None), &term).unwrap(), b"");
    // Without Ctrl the punctuation is still plain text.
    for c in ['[', '\\', '/', '_', '@', ';'] {
        let got = enc.encode(&press(Key::Char(c), Mods::default(), Some(&c.to_string())), &term).unwrap();
        assert_eq!(got, c.to_string().as_bytes(), "plain {c:?}");
    }
}

/// B5: a front-end may pass the control character as `text` (winit does for Ctrl chords and
/// for Enter, Tab, Backspace, Escape). It must not be encoded as if it were printable.
#[test]
fn keys_control_text_is_not_text() {
    let term = Terminal::new(Options { cols: 80, rows: 24, max_scrollback: 0 }).unwrap();
    let mut enc = Encoder::new().unwrap();
    let ctrl = Mods { ctrl: true, ..Mods::default() };
    let none = Mods::default();
    assert_eq!(enc.encode(&press(Key::Char('c'), ctrl, Some("\x03")), &term).unwrap(), b"\x03");
    assert_eq!(enc.encode(&press(Key::Char('['), ctrl, Some("\x1b")), &term).unwrap(), b"\x1b");
    assert_eq!(enc.encode(&press(Key::Char('i'), ctrl, Some("\t")), &term).unwrap(), b"\t");
    assert_eq!(enc.encode(&press(Key::Named(Named::Enter), none, Some("\r")), &term).unwrap(), b"\r");
    assert_eq!(enc.encode(&press(Key::Named(Named::Tab), none, Some("\t")), &term).unwrap(), b"\t");
    assert_eq!(enc.encode(&press(Key::Named(Named::Backspace), none, Some("\x08")), &term).unwrap(), b"\x7f");
    assert_eq!(enc.encode(&press(Key::Named(Named::Escape), none, Some("\x1b")), &term).unwrap(), b"\x1b");
    assert_eq!(enc.encode(&press(Key::Named(Named::Delete), none, Some("\x7f")), &term).unwrap(), b"\x1b[3~");
    // Printable text is untouched, multi-byte too.
    assert_eq!(enc.encode(&press(Key::Char('é'), none, Some("é")), &term).unwrap(), "é".as_bytes());
}

/// With the kitty keyboard protocol on, the same keys take their CSI-u form.
#[test]
fn keys_encode_kitty_protocol() {
    let mut term = Terminal::new(Options { cols: 80, rows: 24, max_scrollback: 0 }).unwrap();
    let mut enc = Encoder::new().unwrap();
    let ctrl = Mods { ctrl: true, ..Mods::default() };
    let none = Mods::default();
    assert_eq!(enc.encode(&press(Key::Named(Named::Escape), none, None), &term).unwrap(), b"\x1b");
    term.vt_write(b"\x1b[>1u"); // disambiguate escape codes
    assert_eq!(enc.encode(&press(Key::Char('a'), none, Some("a")), &term).unwrap(), b"a");
    assert_eq!(enc.encode(&press(Key::Named(Named::Escape), none, None), &term).unwrap(), b"\x1b[27u");
    assert_eq!(enc.encode(&press(Key::Char('c'), ctrl, None), &term).unwrap(), b"\x1b[99;5u");
    assert_eq!(enc.encode(&press(Key::Char('c'), ctrl, Some("\x03")), &term).unwrap(), b"\x1b[99;5u");
    assert_eq!(enc.encode(&press(Key::Char('['), ctrl, None), &term).unwrap(), b"\x1b[91;5u");
    assert_eq!(enc.encode(&press(Key::Char('/'), ctrl, None), &term).unwrap(), b"\x1b[47;5u");
    assert_eq!(enc.encode(&press(Key::Char('i'), ctrl, None), &term).unwrap(), b"\x1b[105;5u");
    assert_eq!(enc.encode(&press(Key::Named(Named::Enter), none, Some("\r")), &term).unwrap(), b"\r");
    let release = KeyEvent { pressed: false, ..press(Key::Char('a'), none, Some("a")) };
    assert_eq!(enc.encode(&release, &term).unwrap(), b"");
    term.vt_write(b"\x1b[>3u"); // … and report event types
    assert_eq!(enc.encode(&release, &term).unwrap(), b"\x1b[97;1:3u");
}

#[test]
fn key_reaches_pty_and_is_attributed_to_a_frame() {
    let dir = scratch("key_attribution");
    let log = Log::create(&dir).unwrap();
    let emu = Emu::start(cat(80, 24), 80, 24, Some(log.clone())).unwrap();
    log.key(7, now_ns());
    emu.input_tx.send(InputMsg::Key(typed('x'), 7)).unwrap();
    // The tty echoes the key; the frame showing the echo carries the key id.
    let (frame, _) = frames_until(&emu, |f| f.keys.contains(&7));
    assert_eq!(frame.row_text(0), "x");
    assert_eq!(frame.keys, [7]);
    log.frame(frame.seq, now_ns(), now_ns(), "present_return");
    log.flush();
    assert_eq!(logged_frames(&dir), [(frame.seq, vec![7])]);
    let report = latency::report(&dir.join("latency.jsonl")).unwrap();
    assert_eq!(report.key_to_present.n, 1);
    assert!(report.key_to_present.p50 > 0);
    // A key is reported once: no later frame carries it again.
    emu.input_tx.send(InputMsg::Bytes(b"more".to_vec(), now_ns())).unwrap();
    let (later, _) = frames_until(&emu, |f| f.row_text(0).contains("more"));
    assert!(later.keys.is_empty());
}

/// B3: a frame made without PTY output (here a resize) cannot show a key's echo, so it must not
/// take the key; the next frame that has output does.
#[test]
fn frame_without_output_takes_no_key() {
    let pty = sh("stty -echo; echo ready; sleep 1000", 80, 24);
    let writer = pty.writer();
    writer.start_recording();
    let emu = Emu::start(pty, 80, 24, None).unwrap();
    frames_until(&emu, |f| has_row(f, "ready"));
    emu.input_tx.send(InputMsg::Key(typed('x'), 1)).unwrap();
    // Echo is off and the child prints nothing: the key is written and nothing comes back.
    wait_for("the key to be written", || writer.recorded() == b"x");
    emu.input_tx.send(InputMsg::Resize(90, 30)).unwrap();
    let (resized, _) = frames_until(&emu, |f| f.cols == 90);
    assert!(resized.keys.is_empty(), "a resize frame took keys {:?}", resized.keys);
    assert!(!has_row(&resized, "x"));

    emu.input_tx.send(InputMsg::Bytes(b"out".to_vec(), now_ns())).unwrap();
    let (with_output, _) = frames_until(&emu, |f| has_row(f, "out"));
    assert_eq!(with_output.keys, [1], "the first frame with output after the write takes the key");
}

/// B8: a frame that was taken but never presented must not lose its keys; they belong to the
/// next frame the front-end does log.
#[test]
fn keys_of_a_taken_frame_that_was_not_presented_are_logged() {
    let dir = scratch("unpresented_frame");
    let log = Log::create(&dir).unwrap();
    let emu = Emu::start(cat(80, 24), 80, 24, Some(log.clone())).unwrap();
    log.key(1, now_ns());
    emu.input_tx.send(InputMsg::Key(typed('x'), 1)).unwrap();
    let (first, _) = frames_until(&emu, |f| f.keys.contains(&1));
    // The front-end takes a newer frame before it renders, and presents only that one.
    emu.input_tx.send(InputMsg::Bytes(b"more".to_vec(), now_ns())).unwrap();
    let (second, _) = frames_until(&emu, |f| f.row_text(0).contains("more"));
    assert!(second.seq > first.seq && second.keys.is_empty());
    log.frame(second.seq, now_ns(), now_ns(), "present_return");
    log.flush();
    assert_eq!(logged_frames(&dir), [(second.seq, vec![1])]);
    assert_eq!(latency::report(&dir.join("latency.jsonl")).unwrap().key_to_present.n, 1);
}

/// The repeat filter on the emulator thread: no repeat reaches the PTY from a focus loss until
/// a fresh press, whatever batches the messages arrive in.
#[test]
fn no_key_repeat_reaches_the_pty_after_focus_loss() {
    let pty = cat(80, 24);
    let writer = pty.writer();
    writer.start_recording();
    let emu = Emu::start(pty, 80, 24, None).unwrap();
    let send = |msg| emu.input_tx.send(msg).unwrap();
    send(InputMsg::Key(typed('a'), 1));
    send(InputMsg::Key(repeated('b'), 2));
    wait_for("the press and its repeat", || writer.recorded() == b"ab");
    send(InputMsg::Focus(false));
    send(InputMsg::Key(repeated('x'), 3));
    send(InputMsg::Focus(true));
    send(InputMsg::Key(repeated('y'), 4));
    send(InputMsg::Key(typed('c'), 5));
    send(InputMsg::Key(repeated('d'), 6));
    wait_for("the fresh press and its repeat", || writer.recorded().ends_with(b"cd"));
    assert_eq!(String::from_utf8_lossy(&writer.recorded()), "abcd");
}

#[test]
fn paste_bracketed_when_mode_set() {
    assert_eq!(paste_bytes("a\nb", false).unwrap(), b"a\rb");
    assert_eq!(paste_bytes("hi", true).unwrap(), b"\x1b[200~hi\x1b[201~");

    let pty = cat(80, 24);
    let writer = pty.writer();
    writer.start_recording();
    let emu = Emu::start(pty, 80, 24, None).unwrap();
    emu.input_tx.send(InputMsg::Paste("one".into())).unwrap();
    wait_for("the plain paste", || contains(&writer.recorded(), b"one"));
    assert!(!contains(&writer.recorded(), b"\x1b[200~"));

    emu.input_tx.send(InputMsg::Bytes(b"\x1b[?2004hset".to_vec(), now_ns())).unwrap();
    // The tty echo of "one" may land before or after "set" on the row.
    frames_until(&emu, |f| f.row_text(0).contains("set"));
    emu.input_tx.send(InputMsg::Paste("two".into())).unwrap();
    wait_for("the bracketed paste", || contains(&writer.recorded(), b"\x1b[200~two\x1b[201~"));
}

#[test]
fn reply_goes_through_writer() {
    let pty = cat(80, 24);
    let writer = pty.writer();
    writer.start_recording();
    let emu = Emu::start(pty, 80, 24, None).unwrap();
    assert!(writer.recorded().is_empty());
    // A cursor position query: the emulator's reply must appear on the one PTY writer.
    emu.input_tx.send(InputMsg::Bytes(b"\x1b[6n".to_vec(), now_ns())).unwrap();
    wait_for("the CPR reply", || contains(&writer.recorded(), b"\x1b[1;1R"));
}

#[test]
fn resize_changes_frame_and_pty_size() {
    // The child reports the size of its tty when it gets a line.
    let emu = Emu::start(sh("read line; stty size; sleep 1000", 80, 24), 80, 24, None).unwrap();
    emu.input_tx.send(InputMsg::Resize(100, 30)).unwrap();
    let (frame, _) = frames_until(&emu, |f| f.cols == 100);
    assert_eq!((frame.rows, frame.cells.len()), (30, 3000));
    assert!(frame.dirty_rows.iter().all(|&d| d));
    emu.input_tx.send(InputMsg::Key(press(Key::Named(Named::Enter), Mods::default(), None), 1)).unwrap();
    frames_until(&emu, |f| has_row(f, "30 100"));
}

#[test]
fn exit_is_reported() {
    // Echo is off and the child prints nothing more, so the only wake after "ready" is the exit.
    let emu = Emu::start(sh("stty -echo; echo ready; read line", 80, 24), 80, 24, None).unwrap();
    frames_until(&emu, |f| has_row(f, "ready"));
    let (tx, rx) = channel();
    let tx = std::sync::Mutex::new(tx);
    emu.frames.set_waker(Box::new(move || {
        let _ = tx.lock().unwrap().send(());
    }));
    assert!(rx.recv_timeout(Duration::from_millis(200)).is_err(), "nothing is pending");
    assert!(!emu.exited());
    emu.input_tx.send(InputMsg::Key(press(Key::Named(Named::Enter), Mods::default(), None), 1)).unwrap();
    rx.recv_timeout(WAIT).expect("the waker to fire when the child exits");
    assert!(emu.exited(), "exited is true by the time the waker runs");
}

/// B7: a waker installed after the first frame was published, or after the child exited, is
/// called at once instead of waiting for a wake-up that already happened.
#[test]
fn waker_set_late_is_called_at_once() {
    let emu = Emu::start(cat(80, 24), 80, 24, None).unwrap();
    emu.input_tx.send(InputMsg::Bytes(b"x".to_vec(), now_ns())).unwrap();
    let early = count_wakes(&emu);
    wait_for("a frame to be published", || early.load(Ordering::SeqCst) > 0);
    // A frame is waiting in the slot and its wake-up went to the old waker.
    assert_eq!(count_wakes(&emu).load(Ordering::SeqCst), 1, "a frame was pending");

    let emu = Emu::start(Pty::spawn(&["true".to_string()], 80, 24).unwrap(), 80, 24, None).unwrap();
    wait_for("the child to exit", || emu.exited());
    while emu.frames.take().is_some() {}
    assert_eq!(count_wakes(&emu).load(Ordering::SeqCst), 1, "the child had exited");
}

#[test]
fn latency_report_percentiles() {
    assert_eq!(Pcts::of((1..=100).collect()), Pcts { n: 100, p50: 50, p95: 95, p99: 99 });
    assert_eq!(Pcts::of(vec![30, 10, 20]), Pcts { n: 3, p50: 20, p95: 30, p99: 30 });
    assert_eq!(Pcts::of(vec![]), Pcts::default());

    // Ten keys, one per frame: key at i ms, write +10 us, output +200 us, submit +1 ms, present +(i+1) ms.
    let dir = scratch("latency_report");
    let log = Log::create(&dir).unwrap();
    for i in 0..10u64 {
        let key = i * 1_000_000;
        log.event(&Event::Key { id: i, t_key_ns: key });
        log.event(&Event::Write { id: i, t_write_ns: key + 10_000 });
        log.event(&Event::Frame {
            seq: i,
            t_output_ns: key + 210_000,
            t_submit_ns: key + 1_210_000,
            t_present_ns: key + 1_210_000 + (i + 1) * 1_000_000,
            present_kind: "present_return".into(),
            keys: vec![i],
            t_work_done_ns: None,
        });
    }
    // A key that was never written and a frame with an unknown key are ignored.
    log.event(&Event::Key { id: 99, t_key_ns: 0 });
    log.event(&Event::Frame { seq: 10, t_output_ns: 1, t_submit_ns: 2, t_present_ns: 3, present_kind: "present_return".into(), keys: vec![99, 1234], t_work_done_ns: None });
    log.flush();

    let report = latency::report(&dir.join("latency.jsonl")).unwrap();
    assert_eq!(report.key_to_write, Pcts { n: 10, p50: 10_000, p95: 10_000, p99: 10_000 });
    assert_eq!(report.write_to_output, Pcts { n: 10, p50: 200_000, p95: 200_000, p99: 200_000 });
    assert_eq!(report.output_to_submit, Pcts { n: 10, p50: 1_000_000, p95: 1_000_000, p99: 1_000_000 });
    assert_eq!(report.submit_to_present, Pcts { n: 10, p50: 5_000_000, p95: 10_000_000, p99: 10_000_000 });
    assert_eq!(report.key_to_present, Pcts { n: 10, p50: 6_210_000, p95: 11_210_000, p99: 11_210_000 });
}

/// The checked-in pair Track B's `analyze.py` is tested against (see `data/make-golden.py`).
#[test]
fn golden_log_matches_report() {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data");
    let expected: latency::Report =
        serde_json::from_str(&std::fs::read_to_string(data.join("golden-report.json")).unwrap()).unwrap();
    assert_eq!(expected.key_to_present.n, 49);
    assert_eq!(latency::report(&data.join("golden-latency.jsonl")).unwrap(), expected);
}

#[test]
fn sampler_writes_process_samples() {
    let dir = scratch("sampler");
    let log = Log::create(&dir).unwrap();
    let sampler = s2_core::bench::Sampler::start(log.clone());
    std::thread::sleep(Duration::from_millis(650));
    drop(sampler);
    log.flush();
    let text = std::fs::read_to_string(dir.join("latency.jsonl")).unwrap();
    let samples: Vec<Event> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert!(!samples.is_empty());
    for sample in samples {
        let Event::Sample { t_ns, cpu_pct, rss_kb, fps, .. } = sample else { panic!("not a sample: {sample:?}") };
        assert!(t_ns > 0 && rss_kb > 0 && cpu_pct >= 0.0 && fps == 0.0);
    }
}
