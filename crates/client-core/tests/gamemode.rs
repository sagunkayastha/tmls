//! `game-mode status` (the PC's script): what is asked and what counts as on.

use std::time::Duration;

use client_core::gamemode::{PROGRAM, REMOTE_PERIOD, TIMEOUT, parse, status_argv};

#[test]
fn the_status_is_asked_and_only_on_is_on() {
    assert_eq!(PROGRAM, "game-mode");
    assert_eq!(status_argv(), ["game-mode", "status"]);
    for on in ["on", "on\n", " ON \n", "On"] {
        assert!(parse(on), "{on:?}");
    }
    for off in ["off\n", "", "\n", "on and off", "unknown", "1"] {
        assert!(!parse(off), "{off:?}");
    }
    assert_eq!(REMOTE_PERIOD, Duration::from_secs(300));
    assert_eq!(TIMEOUT, Duration::from_secs(2));
}
