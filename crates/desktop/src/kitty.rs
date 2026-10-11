//! Claude sessions that run outside tmux: bringing their terminal window to the front.
//!
//! As tmls (`local.py`): such a session cannot be attached in a tab, so Enter or a click on it
//! focuses the kitty window it runs in. kitty is asked over its remote-control sockets
//! (`$XDG_RUNTIME_DIR/kitty-*`; needs `allow_remote_control` and `listen_on` in kitty.conf):
//! `kitty @ ls` names every window's processes, the session's window is the one whose process
//! is Claude's nearest ancestor, and `kitty @ focus-window` raises it. A session in another
//! terminal is not found; the caller says so.
//!
//! `focus` runs programs and waits for them (two seconds each at most): call it off the
//! window's thread.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::Value;

/// Every process kitty knows of in the windows `kitty @ ls` printed (each window's own process
/// and its foreground ones), with the id of its window.
pub fn windows(ls: &str) -> HashMap<u32, u64> {
    let mut out = HashMap::new();
    let list = |value: &Value, key: &str| -> Vec<Value> {
        value
            .get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let pid = |value: &Value| {
        value
            .get("pid")
            .and_then(Value::as_u64)
            .and_then(|pid| u32::try_from(pid).ok())
    };
    let Ok(Value::Array(os_windows)) = serde_json::from_str(ls) else {
        return out;
    };
    for os_window in &os_windows {
        for tab in list(os_window, "tabs") {
            for window in list(&tab, "windows") {
                let Some(id) = window.get("id").and_then(Value::as_u64) else {
                    continue;
                };
                let foreground = list(&window, "foreground_processes");
                for pid in pid(&window)
                    .into_iter()
                    .chain(foreground.iter().filter_map(pid))
                {
                    out.insert(pid, id);
                }
            }
        }
    }
    out
}

/// Each process's parent, from `ps -eo pid=,ppid=`.
pub fn parents(ps: &str) -> HashMap<u32, u32> {
    ps.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some((fields.next()?.parse().ok()?, fields.next()?.parse().ok()?))
        })
        .collect()
}

/// The window of `pid`'s nearest ancestor (itself included) that is a window's process.
pub fn window_of(
    pid: u32,
    parents: &HashMap<u32, u32>,
    windows: &HashMap<u32, u64>,
) -> Option<u64> {
    let mut pid = pid;
    // A process table is a tree; the bound only guards against a damaged one.
    for _ in 0..64 {
        if let Some(id) = windows.get(&pid) {
            return Some(*id);
        }
        pid = *parents.get(&pid).filter(|parent| **parent > 1)?;
    }
    None
}

/// What `program arguments…` printed, when it ended well within two seconds.
fn output(program: &str, arguments: &[&str]) -> Option<String> {
    let out = Command::new("timeout")
        .arg("2")
        .arg(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn sockets() -> Vec<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    let Some(entries) = runtime.and_then(|dir| std::fs::read_dir(dir).ok()) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("kitty-"))
        })
        .collect();
    found.sort();
    found
}

/// Brings the kitty window that runs process `pid` to the front.
pub fn focus(pid: u32) -> Result<(), String> {
    let parents = parents(&output("ps", &["-eo", "pid=,ppid="]).unwrap_or_default());
    let sockets = sockets();
    if sockets.is_empty() {
        return Err("no kitty remote-control socket (listen_on in kitty.conf)".to_string());
    }
    for socket in sockets {
        let to = format!("unix:{}", socket.display());
        let Some(ls) = output("kitty", &["@", "--to", &to, "ls"]) else {
            continue;
        };
        if let Some(id) = window_of(pid, &parents, &windows(&ls)) {
            let matching = format!("id:{id}");
            return output(
                "kitty",
                &["@", "--to", &to, "focus-window", "--match", &matching],
            )
            .map(drop)
            .ok_or_else(|| "kitty did not focus the window".to_string());
        }
    }
    Err("its window is not a kitty window this app can reach".to_string())
}
