//! `game-mode status` (the PC's script): what is asked and what counts as on.

use std::time::Duration;

use client_core::gamemode::{
    PROGRAM, REMOTE_PERIOD, TIMEOUT, answer, parse, remote_period, remote_scan, status_argv,
};

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

#[test]
fn remote_hosts_are_scanned_only_once_game_mode_is_known_to_be_off() {
    assert!(!remote_scan(false, false), "not before the first probe is over: it may say on");
    assert!(!remote_scan(true, true), "not in game mode");
    assert!(remote_scan(true, false));
    assert!(!remote_scan(false, true));
}

#[test]
fn a_remote_host_the_user_is_signing_in_to_is_not_slowed() {
    assert_eq!(remote_period(true, false), Some(REMOTE_PERIOD));
    assert_eq!(remote_period(true, true), None, "the list is to follow the sign-in within seconds");
    assert_eq!(remote_period(false, false), None);
    assert_eq!(remote_period(false, true), None);
}

/// A stand-in for the script, run as the app runs it: never the real `game-mode`.
fn probe(script: &str, timeout: Duration) -> Option<bool> {
    let argv = ["sh", "-c", script].map(str::to_string);
    answer(client_core::tmux::run(&argv, timeout))
}

#[test]
fn only_a_probe_that_answered_says_on_or_off() {
    assert_eq!(probe("echo on", Duration::from_secs(5)), Some(true));
    assert_eq!(probe("echo off", Duration::from_secs(5)), Some(false));
    // Under a game's load the script can be slow or fail: that is no answer, and not "off".
    assert_eq!(probe("sleep 5", Duration::from_millis(50)), None, "a probe that timed out");
    assert_eq!(probe("echo on; exit 1", Duration::from_secs(5)), None, "a probe that failed");
}
