//! The shared ssh connection: where its socket goes, and that every ssh tmls-rs starts uses it
//! once `use_control_master` ran. A test binary of its own: that switch is for the whole process.

use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use client_core::listing::{SessionKey, listing_command};
use client_core::tmux::{
    attach_argv, control_dir, control_options, control_wanted, kill_argv, ssh_base, use_control_master,
};

fn key(host: &str, name: &str) -> SessionKey {
    SessionKey { host: host.to_string(), name: name.to_string() }
}

#[test]
fn the_socket_directory_is_the_runtime_dir_else_the_cache() {
    let os = |s: &'static str| Some(OsStr::new(s));
    assert_eq!(control_dir(os("/run/user/1000"), os("/home/u")), Some(PathBuf::from("/run/user/1000/tmls-rs")));
    assert_eq!(control_dir(None, os("/home/u")), Some(PathBuf::from("/home/u/.cache/tmls-rs")));
    assert_eq!(control_dir(os(""), os("/home/u")), Some(PathBuf::from("/home/u/.cache/tmls-rs")));
    assert_eq!(control_dir(None, None), None);
    // `/cm-` and the 40 characters of %C would not fit a Unix socket's path: no shared connection.
    let long = format!("/{}", "d".repeat(70));
    assert_eq!(control_dir(Some(OsStr::new(&long)), None), None);
}

#[test]
fn the_socket_directory_leaves_room_for_the_suffix_ssh_binds() {
    // ssh binds `<ControlPath>.` and 16 random characters into a 107-byte sun_path: the directory,
    // `/cm-`, the 40 of %C and those 17 fit only while the directory has 46 bytes or fewer.
    // `/tmls-rs` is 8 of them.
    let runtime = |len: usize| format!("/{}", "r".repeat(len - 1));
    let at_46 = runtime(38);
    assert_eq!(control_dir(Some(OsStr::new(&at_46)), None).map(|d| d.as_os_str().len()), Some(46));
    let at_47 = runtime(39);
    assert_eq!(control_dir(Some(OsStr::new(&at_47)), None), None);
}

#[test]
fn no_socket_directory_with_a_space_or_a_percent() {
    // `-o ControlPath=...` is not quoted, and `%` starts one of ssh's tokens.
    let os = |s: &'static str| Some(OsStr::new(s));
    assert_eq!(control_dir(os("/run/user/my dir"), None), None);
    assert_eq!(control_dir(os("/run/user/tab\there"), None), None);
    assert_eq!(control_dir(None, os("/home/a b")), None);
    assert_eq!(control_dir(os("/run/user/100%"), None), None);
    assert_eq!(control_dir(None, os("/home/%u")), None);
}

#[test]
fn the_switch_is_off_only_for_1() {
    assert!(control_wanted(None));
    assert!(control_wanted(Some(OsStr::new(""))));
    assert!(control_wanted(Some(OsStr::new("0"))));
    assert!(!control_wanted(Some(OsStr::new("1"))));
}

#[test]
fn control_options_name_the_socket_in_the_directory() {
    assert_eq!(
        control_options(Path::new("/run/user/1000/tmls-rs")),
        ["-o", "ControlMaster=auto", "-o", "ControlPersist=10m", "-o", "ControlPath=/run/user/1000/tmls-rs/cm-%C"]
    );
}

#[test]
fn once_switched_on_every_ssh_shares_the_connection() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("ssh-control-{}", std::process::id()))
        .join("tmls-rs");
    std::fs::create_dir_all(&dir).unwrap();
    // An existing directory with loose permissions is made private.
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    use_control_master(&dir).expect("the socket directory");
    assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);

    let path = format!("ControlPath={}/cm-%C", dir.display());
    let base = ssh_base("ph", &["-t"]);
    assert_eq!(base[..5], ["ssh", "-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3"]);
    assert!(base.contains(&path), "{base:?}");
    assert_eq!(base[base.len() - 2..], ["-t", "ph"]);
    for argv in [attach_argv(&key("ph", "w"), "here"), kill_argv(&key("ph", "w"), "here")] {
        assert!(argv.contains(&path), "{argv:?}");
    }
    assert!(listing_command("ph", "here").get_args().any(|arg| arg == OsStr::new(&path)));
    // This machine's commands are no ssh at all.
    assert_eq!(attach_argv(&key("here", "w"), "here")[0], "tmux");
}
