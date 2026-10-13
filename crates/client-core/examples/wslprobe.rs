//! A console check of "WSL is this machine" on a real Windows, with the functions the app uses:
//! `wslprobe CONFIG_DIR` (a folder with a `wsl` file) lists the WSL machine, makes sure of the
//! keep-alive holder, creates two tmux sessions named `tmls-probe…` (one with a space and both
//! kinds of quote in its name, one that types `claude --version`), lists again, reads a file,
//! attaches on a PTY and follows the events for a moment. It leaves the sessions and the holder.
//! `wslprobe CONFIG_DIR status` only lists and checks; `wslprobe CONFIG_DIR cleanup` kills the
//! `tmls-probe…` sessions and nothing else.

use std::io::Read;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use client_core::config::Config;
use client_core::listing::{SessionKey, listing_command};
use client_core::local;
use client_core::poll::list_once;
use client_core::tmux::{self, NewSession, Start};
use client_core::{files, push};

const T: Duration = Duration::from_secs(60);

fn names(cfg: &Config) -> Vec<String> {
    let me = &cfg.local_name;
    let snap = list_once(me, listing_command(me, me), T);
    println!("list {me}: online {}, error {:?}, {} sessions", snap.online, snap.error, snap.sessions.len());
    for s in &snap.sessions {
        println!("  session {:?} (host {:?}) status {} tool {:?}", s.key.name, s.key.host, s.status.mark(), s.tool.letter());
    }
    snap.sessions.into_iter().map(|s| s.key.name).collect()
}

fn sh(script: &str) -> String {
    match tmux::run(&local::here().script(script.to_string()), T) {
        Ok(out) => out.trim_end().to_string(),
        Err(e) => format!("ERROR {e:#}"),
    }
}

fn show(what: &str, ran: anyhow::Result<String>) {
    match ran {
        Ok(out) => println!("{what}: ok {:?}", out.trim_end()),
        Err(e) => println!("{what}: ERROR {e:#}"),
    }
}

/// Whether words arrive inside WSL as they were sent.
fn round_trip() {
    let nasty: Vec<String> = [
        "plain", "", "two words", "it's", "say \"hi\"", "back\\slash", "trail\\", "q\\\"x", "\\\\\"", "$HOME `id` $(id)",
        "line1\nline2", "tab\there", "%PATH% ^ & | < > ( ) !", "caf\u{e9} \u{65e5}\u{672c}", "=tmls-probe a'b\"c:", "*", "~",
        "#{pane_current_path}", "a\\\\ b\\", "\"", "'", " lead", "trail ", "x\r\ny",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let mut argv = local::here().script("for a; do printf '%s\\0' \"$a\"; done".to_string());
    argv.push("sh".to_string());
    argv.extend(nasty.iter().cloned());
    let out = Command::new(&argv[0]).args(&argv[1..]).output().expect("wsl.exe");
    let got: Vec<String> = out.stdout.split(|b| *b == 0).map(|b| String::from_utf8_lossy(b).into_owned()).collect();
    let got = &got[..got.len().saturating_sub(1)];
    let same = got == nasty.as_slice();
    println!("round trip: {} words sent, {} came back, identical {same}", nasty.len(), got.len());
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("usage: wslprobe CONFIG_DIR [status|cleanup]"));
    let mode = args.next().unwrap_or_default();
    let cfg = Config::load(&dir, &[]);
    println!("config {}: local_name {:?}, local {:?}, hosts {:?}", dir.display(), cfg.local_name, cfg.local, cfg.hosts);
    if !cfg.local.is_wsl() {
        println!("this machine is not WSL (no wsl file in the config dir, or not Windows): nothing to probe");
        return;
    }
    local::use_local(cfg.local.clone());
    let me = cfg.local_name.clone();
    let key = |name: &str| SessionKey { host: me.clone(), name: name.to_string() };
    let holders = || sh("pgrep -fc 'tmls-keepaliv[e]'; pgrep -af 'tmls-keepaliv[e]'");

    if mode == "cleanup" {
        for name in names(&cfg).into_iter().filter(|name| name.starts_with("tmls-probe")) {
            show(&format!("kill {name:?}"), tmux::run(&tmux::kill_argv(&key(&name), &me), T));
        }
        names(&cfg);
        println!("holders: {}", holders());
        return;
    }
    if mode == "status" {
        names(&cfg);
        println!("holders: {}", holders());
        println!("tmux ls: {}", sh("tmux ls 2>&1"));
        println!("uptime: {}", sh("cat /proc/uptime"));
        return;
    }

    round_trip();
    names(&cfg);

    println!("holders before: {}", holders());
    println!("keep-alive 1: {:?}", local::ensure_keepalive());
    std::thread::sleep(Duration::from_secs(2));
    println!("holders after 1: {}", holders());
    println!("keep-alive 2: {:?}", local::ensure_keepalive());
    std::thread::sleep(Duration::from_secs(2));
    println!("holders after 2: {}", holders());

    let odd = "tmls-probe it's \"a b\"";
    println!("valid name {odd:?}: {:?}", tmux::valid_session_name(odd));
    let new = |name: &str, folder: &str, start: Start| NewSession {
        host: me.clone(),
        folder: folder.to_string(),
        name: name.to_string(),
        start,
        size: Some((120, 40)),
    };
    show("new session (odd name, ~)", tmux::run(&tmux::new_session_argv(&new(odd, "~", Start::Shell), &me), T));
    show("new session again (must refuse)", tmux::run(&tmux::new_session_argv(&new(odd, "~", Start::Shell), &me), T));
    show("new session in a missing folder (must refuse)", tmux::run(&tmux::new_session_argv(&new("tmls-probe-nofolder", "~/no such 'dir'", Start::Shell), &me), T));
    let claude = "tmls-probe-claude";
    let preset = Start::Preset(vec!["claude".to_string(), "--version".to_string()]);
    show("new session (claude --version)", tmux::run(&tmux::new_session_argv(&new(claude, "/tmp", preset), &me), T));
    show("derived name of /tmp", tmux::run(&tmux::derived_name_argv(&me, "/tmp", &me), T));

    let after = names(&cfg);
    println!("the odd name is listed: {}", after.iter().any(|name| name == odd));

    show("pane info (odd name)", tmux::run(&tmux::pane_info_argv(&key(odd), &me), T));
    show("running commands (odd name)", tmux::run(&tmux::running_commands_argv(&key(odd), &me), T));
    show("send keys (odd name)", tmux::run(&tmux::send_keys_argv(&key(odd), None, "Enter", &me), T));
    let buffer = tmux::buffer_name();
    let text = "echo \"sent $((6*7)) it's\"";
    show("send text (odd name)", tmux::run_with_stdin(&tmux::send_text_argv(&key(odd), None, &buffer, &me), text.as_bytes(), T));
    std::thread::sleep(Duration::from_secs(6));
    show("capture (odd name)", tmux::run(&tmux::capture_argv(&key(odd), None, &me), T));
    show("capture (claude)", tmux::run(&tmux::capture_argv(&key(claude), None, &me), T));
    println!("attach text: {}", tmux::attach_text(&key(odd), &me));

    // a file inside WSL: located as the viewer does, read, and its address for Explorer
    for path in ["/etc/os-release", "~/.bashrc", "os-release"] {
        let located = tmux::run(&files::locate_argv(&key(claude), path, &me), T);
        match located.as_deref().ok().and_then(files::parse_located) {
            Some(file) => {
                let bytes = tmux::run_bytes(&files::read_argv(&me, &file.path, 120, &me), T);
                let read = bytes.map(|b| String::from_utf8_lossy(&b).into_owned());
                println!("locate {path:?}: {file:?}");
                println!("  read (120 bytes at most): {read:?}");
                let address = local::desktop_path(&file.path);
                let windows = address.clone().map(|a| a.replace('/', "\\"));
                let there = windows.as_ref().map(|w| std::fs::metadata(w).map(|m| m.len()).map_err(|e| e.to_string()));
                println!("  desktop path {address:?}; Windows sees it: {there:?}");
            }
            None => println!("locate {path:?}: {located:?}"),
        }
    }

    // attach on a PTY, as a tab does
    let argv = tmux::attach_argv(&key(odd), &me);
    println!("attach argv: {argv:?}");
    match s2_core::pty::Pty::spawn(&argv, 100, 30) {
        Err(e) => println!("attach on a PTY: ERROR {e:#}"),
        Ok(pty) => {
            let mut reader = pty.reader().expect("reader");
            // what a terminal answers ConPTY's first question (where the cursor is) with
            let _ = pty.writer().write_all(b"\x1b[1;1R");
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                while let Ok(n) = reader.read(&mut buf) {
                    if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            });
            std::thread::sleep(Duration::from_secs(4));
            let bytes: Vec<u8> = rx.try_iter().flatten().collect();
            let text = String::from_utf8_lossy(&bytes);
            println!("attach on a PTY: {} bytes from tmux, the screen has the echo: {}", bytes.len(), text.contains("sent 42"));
            println!("clients while attached: {}", sh("tmux list-clients -F '#{client_tty} #{session_name} #{client_width}x#{client_height}' 2>&1"));
            drop(pty);
            std::thread::sleep(Duration::from_secs(2));
            println!("clients after the tab closed: {:?}", sh("tmux list-clients 2>&1"));
        }
    }

    // the events reader
    let (tx, rx) = std::sync::mpsc::channel();
    let tail = push::Tail::start(push::tail_argv(&me, &me), Box::new(move |line| drop(tx.send(format!("{line:?}")))));
    std::thread::sleep(Duration::from_secs(4));
    println!("push reader said: {:?}", rx.try_iter().collect::<Vec<_>>());
    println!("tails inside WSL while it runs: {}", sh("pgrep -fc 'tail -n0 -[F]'"));
    drop(tail);
    std::thread::sleep(Duration::from_secs(3));
    println!("tails inside WSL after it was dropped: {}", sh("pgrep -fc 'tail -n0 -[F]'"));

    println!("holders at the end: {}", holders());
    println!("left running: the tmls-probe sessions and the holder");
}
