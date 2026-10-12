//! When `tmls-rs` leaves the shell at once and runs on as its own process, and how.

use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

use desktop::launch::{self, detach_command, log_path, wants_foreground};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|arg| arg.to_string()).collect()
}

#[test]
fn no_args_detach() {
    assert!(!wants_foreground(&args(&[]), None));
}

#[test]
fn other_flags_detach() {
    assert!(!wants_foreground(&args(&["--some-flag", "host"]), None));
}

#[test]
fn bench_stays_in_the_foreground() {
    assert!(wants_foreground(&args(&["--bench", "x"]), None));
    assert!(wants_foreground(
        &args(&["--some-flag", "--bench", "x"]),
        None
    ));
}

#[test]
fn foreground_flags_stay_in_the_foreground() {
    assert!(wants_foreground(&args(&["--foreground"]), None));
    assert!(wants_foreground(&args(&["--fg"]), None));
    assert!(wants_foreground(&args(&["--some-flag", "--fg"]), None));
}

#[test]
fn env_one_stays_in_the_foreground() {
    assert!(wants_foreground(&args(&[]), Some("1")));
    assert!(!wants_foreground(&args(&[]), Some("0")));
    assert!(!wants_foreground(&args(&[]), Some("")));
}

#[test]
fn a_subcommand_stays_in_the_foreground() {
    assert!(wants_foreground(&args(&["keys"]), None));
}

#[test]
fn a_subcommand_takes_no_more_words() {
    assert_eq!(launch::subcommand(&args(&["keys"])), Some(Ok("keys")));
    assert_eq!(
        launch::subcommand(&args(&["keys", "--fg"])),
        Some(Ok("keys"))
    );
    // Not the app with the hosts `keys` and `extra`: a usage error.
    assert!(matches!(
        launch::subcommand(&args(&["keys", "extra"])),
        Some(Err(e)) if e.contains("extra")
    ));
    assert_eq!(launch::subcommand(&args(&["ph", "keys"])), None);
    assert_eq!(launch::subcommand(&args(&[])), None);
}

#[test]
fn hosts_detach() {
    assert!(!wants_foreground(&args(&["ph"]), None));
    assert!(!wants_foreground(&args(&["archbox", "ph"]), None));
    assert!(!wants_foreground(&args(&["ph", "keys"]), None));
}

#[test]
fn log_under_xdg_cache_home() {
    assert_eq!(
        log_path(Some("/c"), "/home/u"),
        PathBuf::from("/c/tmls-rs/tmls-rs.log")
    );
}

#[test]
fn log_under_home_cache_without_xdg() {
    let expected = PathBuf::from("/home/u/.cache/tmls-rs/tmls-rs.log");
    assert_eq!(log_path(None, "/home/u"), expected);
    // An empty or relative XDG_CACHE_HOME is invalid by the spec and ignored.
    assert_eq!(log_path(Some(""), "/home/u"), expected);
    assert_eq!(log_path(Some("rel"), "/home/u"), expected);
}

fn sink() -> std::fs::File {
    OpenOptions::new().write(true).open("/dev/null").unwrap()
}

fn argv(cmd: &std::process::Command) -> Vec<&OsStr> {
    cmd.get_args().collect()
}

#[test]
fn detach_command_reruns_self_with_foreground_once() {
    let exe = Path::new("/opt/tmls-rs");
    let cmd = detach_command(exe, &args(&["a", "b"]), sink()).unwrap();
    assert_eq!(cmd.get_program(), OsStr::new("/opt/tmls-rs"));
    assert_eq!(argv(&cmd), ["a", "b", "--foreground"]);

    let cmd = detach_command(exe, &args(&["--foreground", "a"]), sink()).unwrap();
    assert_eq!(argv(&cmd), ["--foreground", "a"]);
    let cmd = detach_command(exe, &args(&["--fg"]), sink()).unwrap();
    assert_eq!(argv(&cmd), ["--fg"]);
    let cmd = detach_command(exe, &args(&[]), sink()).unwrap();
    assert_eq!(argv(&cmd), ["--foreground"]);
}

#[test]
fn foreground_flags_are_named() {
    assert!(launch::is_foreground_flag("--foreground"));
    assert!(launch::is_foreground_flag("--fg"));
    assert!(!launch::is_foreground_flag("--bench"));
}
