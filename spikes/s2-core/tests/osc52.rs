//! Clipboard writes from the program (OSC 52) reach `FrameSlot::take_copied`.

use std::time::{Duration, Instant};

use s2_core::{Emu, Pty};

const WAIT: Duration = Duration::from_secs(5);

fn run(script: &str) -> Emu {
    let pty = Pty::spawn(&["sh".to_string(), "-c".to_string(), script.to_string()], 40, 6)
        .expect("spawn sh");
    Emu::start(pty, 40, 6, None).unwrap()
}

/// Everything copied until `n` texts have arrived; fails after `WAIT`.
fn copied(emu: &Emu, n: usize) -> Vec<String> {
    let start = Instant::now();
    let mut all = Vec::new();
    while all.len() < n {
        all.extend(emu.frames.take_copied());
        assert!(start.elapsed() < WAIT, "{n} copies expected within {WAIT:?}; got {all:?}");
        std::thread::sleep(Duration::from_millis(1));
    }
    all
}

#[test]
fn a_program_that_writes_osc_52_is_heard() {
    // What tmux sends when a mouse selection ends: empty `Pc`, BEL at the end.
    let emu = run("printf 'before\\033]52;;aGVsbG8gd29ybGQ=\\007after\\n'; sleep 5");
    assert_eq!(copied(&emu, 1), ["hello world"]);
}

#[test]
fn two_writes_arrive_in_order_and_once() {
    let emu = run("printf '\\033]52;c;b25l\\033\\\\'; printf '\\033]52;c;dHdv\\007'; sleep 5");
    assert_eq!(copied(&emu, 2), ["one", "two"]);
    std::thread::sleep(Duration::from_millis(50));
    assert!(emu.frames.take_copied().is_empty(), "taken once");
}

#[test]
fn a_query_is_not_a_write() {
    let emu = run("printf '\\033]52;c;?\\007\\033]52;c;eWVz\\007'; sleep 5");
    assert_eq!(copied(&emu, 1), ["yes"]);
}
