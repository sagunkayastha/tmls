//! Mouse reports to the PTY (`InputMsg::Mouse`) and `Emu::mouse_reporting`.
//!
//! The child is `cat -v` on a tty without echo or line buffering: what it reads comes back
//! with ESC shown as `^[`, so the screen shows exactly the bytes the program received.

use std::time::{Duration, Instant};

use s2_core::{Emu, Frame, InputMsg, Key, KeyEvent, Mods, MouseButton, MouseKind, Pty};

const WAIT: Duration = Duration::from_secs(5);
const NONE: Mods = Mods { shift: false, ctrl: false, alt: false, logo: false };

/// Starts `printf <modes>ready; cat -v` and waits for "ready".
fn cat_with(modes: &str) -> Emu {
    let script = format!("stty -echo -icanon min 1 time 0; printf '{modes}ready\\n'; exec cat -v");
    let pty = Pty::spawn(&["sh".to_string(), "-c".to_string(), script], 40, 6).expect("spawn sh");
    let emu = Emu::start(pty, 40, 6, None).unwrap();
    screen_where(&emu, |text| text.contains("ready"));
    emu
}

fn text_of(frame: &Frame) -> String {
    (0..frame.rows).map(|y| frame.row_text(y)).collect::<Vec<_>>().join("\n")
}

/// The screen once it satisfies `done`; fails after `WAIT` with the last screen seen.
fn screen_where(emu: &Emu, done: impl Fn(&str) -> bool) -> String {
    let start = Instant::now();
    let mut last = String::new();
    loop {
        if let Some(frame) = emu.frames.take() {
            last = text_of(&frame);
            if done(&last) {
                return last;
            }
        }
        assert!(start.elapsed() < WAIT, "no matching screen within {WAIT:?}; last:\n{last}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn mouse(emu: &Emu, kind: MouseKind, button: Option<MouseButton>, (col, row): (u16, u16), mods: Mods) {
    emu.input_tx.send(InputMsg::Mouse { kind, button, col, row, mods }).unwrap();
}

/// Types `c` and returns the screen that shows it: everything sent before is on it too.
fn after_typing(emu: &Emu, c: char) -> String {
    let ev = KeyEvent { key: Key::Char(c), mods: NONE, text: Some(c.to_string()), repeat: false, pressed: true };
    emu.input_tx.send(InputMsg::Key(ev, 1)).unwrap();
    screen_where(emu, |text| text.contains(c))
}

#[test]
fn sgr_reports_reach_the_program_when_it_asked_for_them() {
    let emu = cat_with("\\033[?1002h\\033[?1006h");
    assert!(emu.mouse_reporting());
    let left = Some(MouseButton::Left);
    // Cells are 0-based here and 1-based on the wire.
    mouse(&emu, MouseKind::Press, left, (2, 1), NONE);
    mouse(&emu, MouseKind::Motion, left, (3, 1), NONE);
    mouse(&emu, MouseKind::Release, left, (3, 1), NONE);
    mouse(&emu, MouseKind::Press, Some(MouseButton::WheelUp), (0, 0), NONE);
    mouse(&emu, MouseKind::Press, Some(MouseButton::WheelDown), (39, 5), NONE);
    mouse(&emu, MouseKind::Press, Some(MouseButton::Right), (4, 4), Mods { ctrl: true, ..NONE });
    let screen = after_typing(&emu, 'z').replace('\n', "");
    let expected = "^[[<0;3;2M^[[<32;4;2M^[[<0;4;2m^[[<64;1;1M^[[<65;40;6M^[[<18;5;5Mz";
    assert!(screen.contains(expected), "expected {expected}, screen:\n{screen}");
}

#[test]
fn motion_is_reported_only_under_a_mode_that_asks_for_it() {
    // 1000: presses and releases, no motion.
    let emu = cat_with("\\033[?1000h\\033[?1006h");
    assert!(emu.mouse_reporting());
    let left = Some(MouseButton::Left);
    mouse(&emu, MouseKind::Press, left, (0, 0), NONE);
    mouse(&emu, MouseKind::Motion, left, (5, 0), NONE);
    mouse(&emu, MouseKind::Release, left, (5, 0), NONE);
    let screen = after_typing(&emu, 'z').replace('\n', "");
    assert!(screen.contains("^[[<0;1;1M^[[<0;6;1mz"), "screen:\n{screen}");
}

#[test]
fn x10_format_without_1006() {
    let emu = cat_with("\\033[?1000h");
    assert!(emu.mouse_reporting());
    mouse(&emu, MouseKind::Press, Some(MouseButton::Left), (2, 1), NONE);
    mouse(&emu, MouseKind::Press, Some(MouseButton::WheelUp), (2, 1), NONE);
    let screen = after_typing(&emu, 'z').replace('\n', "");
    // ESC [ M, then button + 32, column + 33, row + 33 as bytes: ' ' '#' '"', wheel up '`'.
    assert!(screen.contains("^[[M #\"^[[M`#\"z"), "screen:\n{screen}");
}

#[test]
fn nothing_is_sent_when_the_program_did_not_ask() {
    let emu = cat_with("");
    assert!(!emu.mouse_reporting());
    let left = Some(MouseButton::Left);
    mouse(&emu, MouseKind::Press, left, (2, 1), NONE);
    mouse(&emu, MouseKind::Motion, left, (3, 1), NONE);
    mouse(&emu, MouseKind::Release, left, (3, 1), NONE);
    mouse(&emu, MouseKind::Press, Some(MouseButton::WheelUp), (0, 0), NONE);
    let screen = after_typing(&emu, 'z');
    assert_eq!(screen.trim_end(), "ready\nz", "only the key reached the program");
}

#[test]
fn mouse_reporting_follows_the_mode() {
    let emu = cat_with("");
    assert!(!emu.mouse_reporting());
    emu.input_tx.send(InputMsg::Bytes(b"\x1b[?1003hon".to_vec(), s2_core::now_ns())).unwrap();
    screen_where(&emu, |text| text.contains("on"));
    assert!(emu.mouse_reporting());
    // Any-event tracking: motion with no button held is reported (button 3 + 32, X10 form).
    mouse(&emu, MouseKind::Motion, None, (0, 0), NONE);
    let screen = after_typing(&emu, 'z').replace('\n', "");
    assert!(screen.contains("^[[MC!!z"), "screen:\n{screen}");
    emu.input_tx.send(InputMsg::Bytes(b"\x1b[?1003loff".to_vec(), s2_core::now_ns())).unwrap();
    screen_where(&emu, |text| text.contains("off"));
    assert!(!emu.mouse_reporting());
}
