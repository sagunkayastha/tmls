//! Status pushed by Claude's hooks (D1, D2). tmls (`tmls hooks install`, branch
//! `feat/native-attach`) makes every Claude on a host append one JSON line per hook event to
//! `~/.cache/tmls/events.jsonl` (`claude_hook.py`: `t, ev, sid, pane, cwd` and `msg`, `bg`,
//! `err`, `tool`, `nt`). tmls-rs only reads that file: one `tail -n0 -F` per online host (here
//! `sh -c`, elsewhere over `tmux::ssh`), restarted 10 s after it ends. Its first line says
//! whether the file is there (`· poll` in the list when it is not).
//!
//! An event moves the session it names (by Claude's session id, else by its pane) as tmls's
//! `push.apply` does; one older than what the listing already saw is dropped. The listing stays
//! the truth (D3): the app applies events to its copy of the last listing, and the next listing
//! replaces it.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::listing::{HostSnapshot, SessionKey, Status};
use crate::tmux::{script_argv, valid_pane};

/// How long after its reader ended a host's file is read again.
pub const RESTART_AFTER: Duration = Duration::from_secs(10);
/// The longest reply kept, in characters.
pub const REPLY_CHARS: usize = 120;

/// Says whether the file is there, then follows it from its end; rotation (tmls renames it at
/// 5 MB) is followed by `-F`. Reads only.
const TAIL_SCRIPT: &str = r#"f="$HOME/.cache/tmls/events.jsonl"; if [ -f "$f" ]; then echo tmls-push on; else echo tmls-push off; fi; exec tail -n0 -F "$f" 2>/dev/null"#;

/// What a hook event says about a session.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// `UserPromptSubmit`, `PreToolUse`: working.
    Busy,
    /// `PermissionRequest`, or `Notification` of type `permission_prompt`: waits, for this.
    Waiting(String),
    /// The turn ended; `bg` background tasks still run; `msg` is the reply.
    Stop { msg: Option<String>, bg: u32 },
    /// The turn ended in an API error.
    Failed { err: String, msg: Option<String> },
    SessionStart,
    SessionEnd,
    /// Anything else (`StatusLine`, other notifications): nothing to do.
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PushEvent {
    /// When, by the host's clock (s).
    pub t: f64,
    pub sid: Option<String>,
    /// A valid pane id, else `None`.
    pub pane: Option<String>,
    pub kind: Kind,
}

/// One line of a reader.
#[derive(Clone, Debug, PartialEq)]
pub enum PushLine {
    /// The host has `events.jsonl` (or not: the list says `· poll`).
    Available(bool),
    Event(PushEvent),
}

/// What an event did to a listing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applied {
    Unchanged,
    Changed,
    /// A Claude session started or ended: list the host again to learn its pane.
    Relist,
}

/// One line of the reader; `None` for anything that is not a line this module knows (not
/// JSON, no `t` or `ev`).
pub fn parse_line(line: &str) -> Option<PushLine> {
    match line.trim() {
        "tmls-push on" => return Some(PushLine::Available(true)),
        "tmls-push off" => return Some(PushLine::Available(false)),
        _ => {}
    }
    let value: Value = serde_json::from_str(line).ok()?;
    let e = value.as_object()?;
    let text = |k: &str| e.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
    let t = e.get("t").and_then(Value::as_f64)?;
    let kind = match e.get("ev").and_then(Value::as_str)? {
        "UserPromptSubmit" | "PreToolUse" => Kind::Busy,
        "PermissionRequest" => Kind::Waiting("permission prompt".to_string()),
        "Notification" if text("nt").as_deref() == Some("permission_prompt") => {
            Kind::Waiting("permission prompt".to_string())
        }
        "Stop" => Kind::Stop {
            msg: text("msg"),
            bg: e.get("bg").and_then(Value::as_u64).map_or(0, |n| n.min(u64::from(u32::MAX)) as u32),
        },
        "StopFailure" => Kind::Failed { err: text("err").unwrap_or_else(|| "error".to_string()), msg: text("msg") },
        "SessionStart" => Kind::SessionStart,
        "SessionEnd" => Kind::SessionEnd,
        _ => Kind::Other,
    };
    let pane = text("pane").filter(|pane| valid_pane(pane));
    Some(PushLine::Event(PushEvent { t, sid: text("sid"), pane, kind }))
}

/// The first non-blank line of `text`, trimmed, cut to `REPLY_CHARS`.
pub fn one_line(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    Some(line.chars().take(REPLY_CHARS).collect())
}

/// Moves the session `event` names in `snap` (tmls `push.apply`).
pub fn apply(snap: &mut HostSnapshot, event: &PushEvent) -> Applied {
    if matches!(event.kind, Kind::SessionStart | Kind::SessionEnd) {
        return Applied::Relist;
    }
    if !snap.online {
        return Applied::Unchanged;
    }
    let by_sid = event
        .sid
        .as_ref()
        .and_then(|sid| snap.sessions.iter().position(|s| s.session_id.as_ref() == Some(sid)));
    let by_pane = || event.pane.as_ref().and_then(|pane| snap.sessions.iter().position(|s| s.pane.as_ref() == Some(pane)));
    let Some(index) = by_sid.or_else(by_pane) else {
        return Applied::Unchanged;
    };
    let session = &mut snap.sessions[index];
    let t = event.t.max(0.0) as u64;
    if session.activity.is_some_and(|since| t < since) {
        return Applied::Unchanged; // the listing already saw something newer
    }
    let (status, reason, reply) = match &event.kind {
        Kind::Busy => (Status::Running, None, None),
        Kind::Waiting(why) => (Status::Waiting, Some(why.clone()), None),
        Kind::Stop { msg, bg } => {
            let status = if *bg > 0 { Status::Running } else { Status::Idle };
            (status, None, msg.as_deref().and_then(one_line))
        }
        Kind::Failed { err, msg } => (Status::Error, Some(err.clone()), one_line(msg.as_deref().unwrap_or(err))),
        Kind::SessionStart | Kind::SessionEnd | Kind::Other => return Applied::Unchanged,
    };
    // the app's copy holds the listing's statuses, which are never `Unseen`; a copy that went
    // through `Seen` would say it for an idle session
    let listed = if session.status == Status::Unseen { Status::Idle } else { session.status };
    let new_reply = reply.is_some() && reply != session.last_reply;
    if listed == status && session.reason == reason && !new_reply {
        return Applied::Unchanged;
    }
    if listed != status {
        session.activity = Some(t);
    }
    session.status = status;
    session.reason = reason;
    if reply.is_some() {
        session.last_reply = reply;
    }
    Applied::Changed
}

/// Keeps the last replies across listings, which never have them: a reply in `snap` is
/// remembered, a remembered one is put back into `snap`; the host's sessions an online listing
/// no longer has are forgotten.
pub fn carry_replies(snap: &mut HostSnapshot, replies: &mut HashMap<SessionKey, String>) {
    if !snap.online {
        return;
    }
    replies.retain(|key, _| key.host != snap.host || snap.sessions.iter().any(|s| s.key == *key));
    for session in &mut snap.sessions {
        match &session.last_reply {
            Some(reply) => {
                replies.insert(session.key.clone(), reply.clone());
            }
            None => session.last_reply = replies.get(&session.key).cloned(),
        }
    }
}

/// The reader of `host`'s events.
pub fn tail_argv(host: &str, local_name: &str) -> Vec<String> {
    script_argv(host, TAIL_SCRIPT.to_string(), local_name)
}

/// One host's reader: a thread that runs `argv`, hands each line it knows to `on_line`, and
/// starts it again `restart` after it ended, until dropped.
pub struct Tail {
    stop: Arc<AtomicBool>,
    /// The running reader's pid (its process group's id); the lock is held while it is killed
    /// or reaped, so a pid is never used after its process was reaped.
    child: Arc<Mutex<Option<u32>>>,
    thread: Option<JoinHandle<()>>,
}

impl Tail {
    pub fn start(argv: Vec<String>, on_line: Box<dyn Fn(PushLine) + Send>) -> Tail {
        Tail::start_with(argv, RESTART_AFTER, on_line)
    }

    pub fn start_with(argv: Vec<String>, restart: Duration, on_line: Box<dyn Fn(PushLine) + Send>) -> Tail {
        let stop = Arc::new(AtomicBool::new(false));
        let child = Arc::new(Mutex::new(None));
        let (thread_stop, thread_child) = (stop.clone(), child.clone());
        let thread = std::thread::Builder::new()
            .name("tmls-push".to_string())
            .spawn(move || follow(&argv, restart, &thread_stop, &thread_child, &*on_line))
            .expect("spawning a push reader");
        Tail { stop, child, thread: Some(thread) }
    }
}

fn kill_group(pid: u32) {
    // SAFETY: plain syscall on a process group this module started and has not reaped.
    unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) };
}

fn follow(argv: &[String], restart: Duration, stop: &AtomicBool, child: &Mutex<Option<u32>>, on_line: &dyn Fn(PushLine)) {
    let Some((program, args)) = argv.split_first() else { return };
    while !stop.load(Ordering::SeqCst) {
        let spawned = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn();
        if let Ok(mut process) = spawned {
            *child.lock().unwrap_or_else(PoisonError::into_inner) = Some(process.id());
            if stop.load(Ordering::SeqCst) {
                kill_group(process.id()); // dropped while it started
            }
            if let Some(out) = process.stdout.take() {
                for line in BufReader::new(out).lines() {
                    let Ok(line) = line else { break };
                    if let Some(parsed) = parse_line(&line) {
                        on_line(parsed);
                    }
                }
            }
            let mut held = child.lock().unwrap_or_else(PoisonError::into_inner);
            *held = None;
            kill_group(process.id()); // ssh may have left something behind
            let _ = process.wait();
            drop(held);
        }
        let until = Instant::now() + restart;
        while !stop.load(Ordering::SeqCst) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Tail {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(pid) = *self.child.lock().unwrap_or_else(PoisonError::into_inner) {
            kill_group(pid);
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
