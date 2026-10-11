//! Viewport scrolling (`InputMsg::Scroll` / `ScrollToBottom`).

use std::time::{Duration, Instant};

use s2_core::{now_ns, Emu, Frame, InputMsg, Key, KeyEvent, Mods, Pty};

const WAIT: Duration = Duration::from_secs(5);

fn sh(script: &str, cols: u16, rows: u16) -> Pty {
    Pty::spawn(&["sh".to_string(), "-c".to_string(), script.to_string()], cols, rows).expect("spawn sh")
}

fn frame_where(emu: &Emu, done: impl Fn(&Frame) -> bool) -> Frame {
    let start = Instant::now();
    loop {
        if let Some(frame) = emu.frames.take()
            && done(&frame)
        {
            return frame;
        }
        assert!(start.elapsed() < WAIT, "no matching frame within {WAIT:?}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn rows(frame: &Frame) -> Vec<String> {
    (0..frame.rows).map(|y| frame.row_text(y)).collect()
}

/// 30 lines on a 5-row screen: the live screen shows 26–29 and an empty cursor row, and the
/// scrollback starts with the child's "ready". The child prints that once the tty echo is off,
/// so a key typed after this returns is not echoed.
fn emu_with_scrollback() -> (Emu, Frame) {
    let emu = Emu::start(sh("stty -echo; echo ready; sleep 1000", 20, 5), 20, 5, None).unwrap();
    // The whole line, break included: a late "\r\n" would land in the middle of the lines below.
    frame_where(&emu, |f| f.row_text(0) == "ready" && f.cursor.is_some_and(|c| (c.col, c.row) == (0, 1)));
    let mut text = String::new();
    for i in 0..30 {
        text.push_str(&format!("line {i}\r\n"));
    }
    emu.input_tx.send(InputMsg::Bytes(text.into_bytes(), now_ns())).unwrap();
    let live = frame_where(&emu, |f| f.row_text(3) == "line 29");
    (emu, live)
}

#[test]
fn scroll_moves_the_viewport_and_publishes_a_frame() {
    let (emu, live) = emu_with_scrollback();
    assert_eq!(rows(&live), ["line 26", "line 27", "line 28", "line 29", ""]);
    assert!(live.cursor.is_some());

    // Negative is up, into the scrollback.
    emu.input_tx.send(InputMsg::Scroll(-3)).unwrap();
    let up = frame_where(&emu, |f| f.seq > live.seq);
    assert_eq!(rows(&up), ["line 23", "line 24", "line 25", "line 26", "line 27"]);
    assert!(up.dirty_rows.iter().all(|&d| d), "every row shows other text: {:?}", up.dirty_rows);
    assert_eq!(up.cursor, None, "the cursor row is below the viewport");
    assert!(up.keys.is_empty());

    // Positive is back down; the scroll is clamped at the live screen.
    emu.input_tx.send(InputMsg::Scroll(1)).unwrap();
    let down = frame_where(&emu, |f| f.seq > up.seq);
    assert_eq!(down.row_text(0), "line 24");
    emu.input_tx.send(InputMsg::Scroll(100)).unwrap();
    let bottom = frame_where(&emu, |f| f.seq > down.seq);
    assert_eq!(rows(&bottom), rows(&live));
    assert!(bottom.cursor.is_some());

    // … and at the top of the scrollback.
    emu.input_tx.send(InputMsg::Scroll(-1000)).unwrap();
    let top = frame_where(&emu, |f| f.seq > bottom.seq);
    assert_eq!((top.row_text(0), top.row_text(1)), ("ready".to_string(), "line 0".to_string()));

    emu.input_tx.send(InputMsg::ScrollToBottom).unwrap();
    let back = frame_where(&emu, |f| f.seq > top.seq);
    assert_eq!(rows(&back), rows(&live));
}

#[test]
fn output_leaves_a_scrolled_viewport_in_place_and_typing_returns_to_the_bottom() {
    let (emu, live) = emu_with_scrollback();
    emu.input_tx.send(InputMsg::Scroll(-10)).unwrap();
    let up = frame_where(&emu, |f| f.seq > live.seq);
    assert_eq!(up.row_text(0), "line 16");

    emu.input_tx.send(InputMsg::Bytes(b"new output".to_vec(), now_ns())).unwrap();
    let still_up = frame_where(&emu, |f| f.seq > up.seq);
    assert_eq!(rows(&still_up), rows(&up), "output must not move a scrolled viewport");

    // A key that sends bytes returns to the live screen. Echo is off and the child prints
    // nothing, so that frame has no PTY output and takes no key (B3).
    let key = KeyEvent { key: Key::Char('x'), mods: Mods::default(), text: Some("x".into()), repeat: false, pressed: true };
    emu.input_tx.send(InputMsg::Key(key, 1)).unwrap();
    let back = frame_where(&emu, |f| f.seq > still_up.seq);
    assert_eq!(rows(&back), ["line 26", "line 27", "line 28", "line 29", "new output"]);
    assert!(back.keys.is_empty());
}
