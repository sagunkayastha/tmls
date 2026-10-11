//! The tmux commands behind the session list: attach, new session, rename, kill, and the name a
//! new session gets from its folder. Ported from tmls (`hosts.py`, `create.py`, `tmux_ops.py`).
//!
//! Every builder returns an argv, never a shell string. On this machine (`key.host ==
//! local_name`) the argv is tmux itself, or `sh -c SCRIPT` where a script is needed. On any other
//! host it is `ssh … HOST COMMAND`: ssh hands COMMAND to the host's shell, so every word in it
//! that comes from the user (session name, folder, preset) is single-quoted. Sessions are always
//! targeted as `=NAME`, the exact name: a bare name is a prefix match in tmux.
//!
//! Nothing here strips `TMUX` from the environment. A caller that attaches from a process which
//! itself runs inside tmux has to remove it for the attach child (tmux refuses to nest).

use std::io::{ErrorKind, Read};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};

use crate::listing::SessionKey;

/// Longest session name [`valid_session_name`] accepts, in characters. tmux has no limit of its
/// own; a longer name fits no sidebar row or tab.
pub const MAX_NAME_CHARS: usize = 100;

/// How often [`run`] checks its command for exit and timeout.
const TICK: Duration = Duration::from_millis(2);

/// How long [`run`] waits for the pipes of a command that has exited to close (something it
/// started may still hold them); what was read by then is the output.
const OUTPUT_GRACE: Duration = Duration::from_secs(1);

/// A new tmux session to create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewSession {
    pub host: String,
    /// Where the session starts. `~` and `~/…` are the host's home; empty means `~`.
    pub folder: String,
    pub name: String,
    pub start: Start,
    /// Columns and lines the session starts at, so a TUI started in it lays out for the window
    /// it will be seen in and not for tmux's 80x24.
    pub size: Option<(u16, u16)>,
}

/// What a new session runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Start {
    Shell,
    /// `claude --name NAME`.
    Claude,
    /// An argv from `agent-presets.json`; a `claude` one gets `--name NAME` unless it names
    /// itself. An empty preset is a shell.
    Preset(Vec<String>),
}

/// `word` as one single-quoted shell word.
fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// Python's `shlex.quote`: quoted only when the shell would read `word` as anything else.
fn shlex_quote(word: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c);
    if !word.is_empty() && word.chars().all(plain) {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', "'\"'\"'"))
    }
}

/// Python's `shlex.join`.
fn shlex_join<S: AsRef<str>>(words: &[S]) -> String {
    words.iter().map(|w| shlex_quote(w.as_ref())).collect::<Vec<_>>().join(" ")
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

/// `command` run by `host`'s shell without ever prompting (as the listing does).
fn ssh(host: &str, command: String) -> Vec<String> {
    let mut argv = strings(&["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=5", host]);
    argv.push(command);
    argv
}

/// `script` run by sh, here or on `host`.
fn script_argv(host: &str, script: String, local_name: &str) -> Vec<String> {
    if host == local_name { vec!["sh".to_string(), "-c".to_string(), script] } else { ssh(host, script) }
}

/// Attaches to the session, for a PTY: `tmux -u attach -t =NAME` here, `ssh -t HOST "tmux -u
/// attach -t '=NAME'"` elsewhere. `-u`: ssh without a login often has no UTF-8 locale, and tmux
/// then draws `_` and `lqqk`. ssh may prompt here (it has the terminal), so no `BatchMode`.
pub fn attach_argv(key: &SessionKey, local_name: &str) -> Vec<String> {
    let target = format!("={}", key.name);
    if key.host == local_name {
        strings(&["tmux", "-u", "attach", "-t", &target])
    } else {
        strings(&["ssh", "-t", &key.host, &format!("tmux -u attach -t {}", quote(&target))])
    }
}

/// The attach command as the user would type it, safe to paste into a shell: what tmls's Copy
/// button copies, byte for byte (quoted only where a shell needs it).
pub fn attach_text(key: &SessionKey, local_name: &str) -> String {
    let tmux = ["tmux", "-u", "attach", "-t", &format!("={}", key.name)].map(str::to_string);
    if key.host == local_name {
        shlex_join(&tmux)
    } else {
        shlex_join(&["ssh", "-t", &key.host, &shlex_join(&tmux)])
    }
}

/// `folder` as a shell word. `~` must stay outside the quotes to mean the host's home.
fn folder_word(folder: &str) -> String {
    match folder {
        "" | "~" | "~/" => "\"$HOME\"".to_string(),
        _ => match folder.strip_prefix("~/") {
            Some(rest) => format!("\"$HOME\"/{}", quote(rest)),
            None => quote(folder),
        },
    }
}

/// What the new session's shell is asked to run: `command`, and for a `claude` that does not
/// name itself, `--name NAME`, so Claude's own session name matches tmux's.
fn named(command: &[String], name: &str) -> Vec<String> {
    let mut command = command.to_vec();
    let is_claude = command.first().is_some_and(|c| c.rsplit('/').next() == Some("claude"));
    let names_itself = command.iter().skip(1).any(|a| a == "-n" || a == "--name" || a.starts_with("--name="));
    if is_claude && !names_itself {
        command.extend(["--name".to_string(), name.to_string()]);
    }
    command
}

/// Creates the session, detached. The command prints why and exits 3 when the folder does not
/// exist or a session of that name already does. Check the name with [`valid_session_name`]
/// first: tmux rewrites `:` and `.` in it without saying so.
pub fn new_session_argv(req: &NewSession, local_name: &str) -> Vec<String> {
    let say = |message: String| format!("{{ echo {}; exit 3; }}", quote(&message));
    let name = &req.name;
    let mut script = format!(
        "cd -- {} 2>/dev/null || {}; tmux has-session -t {} 2>/dev/null && {}; tmux new-session -d -s {} -c \"$PWD\"",
        folder_word(&req.folder),
        say(format!("no such folder: {}", req.folder)),
        quote(&format!("={name}")),
        say(format!("a session named {name} already exists")),
        quote(name),
    );
    if let Some((columns, lines)) = req.size {
        script += &format!(" -x {columns} -y {lines}");
    }
    let command = match &req.start {
        Start::Shell => Vec::new(),
        Start::Claude => named(&["claude".to_string()], name),
        Start::Preset(argv) => named(argv, name),
    };
    if !command.is_empty() {
        // typed into the new session's own shell: a bare `tmux new … claude` runs without the
        // login PATH, where claude lives
        script +=
            &format!("; tmux send-keys -t {} {} Enter", quote(&format!("={name}:")), quote(&shlex_join(&command)));
    }
    script_argv(&req.host, script, local_name)
}

/// Renames the session. Check `new_name` with [`valid_session_name`] first.
pub fn rename_argv(key: &SessionKey, new_name: &str, local_name: &str) -> Vec<String> {
    let target = format!("={}", key.name);
    if key.host == local_name {
        strings(&["tmux", "rename-session", "-t", &target, "--", new_name])
    } else {
        ssh(&key.host, format!("tmux rename-session -t {} -- {}", quote(&target), quote(new_name)))
    }
}

/// Ends the session and everything running in it.
pub fn kill_argv(key: &SessionKey, local_name: &str) -> Vec<String> {
    let target = format!("={}", key.name);
    if key.host == local_name {
        strings(&["tmux", "kill-session", "-t", &target])
    } else {
        ssh(&key.host, format!("tmux kill-session -t {}", quote(&target)))
    }
}

/// tmls's name for a session in `folder` when git knows nothing: the folder's own name.
fn folder_name(folder: &str) -> &str {
    if folder.trim_matches(['~', '/']).is_empty() {
        return "home";
    }
    let folder = folder.trim_end_matches('/');
    folder.rsplit('/').next().unwrap_or(folder)
}

/// Prints, on one line, the name a new session in `folder` on `host` gets by default: the name
/// of the git repository the folder is in, else the folder's name (`home` for `~` and `/`).
/// The name is a suggestion: it can be one [`valid_session_name`] refuses (`my.repo`).
pub fn derived_name_argv(host: &str, folder: &str, local_name: &str) -> Vec<String> {
    let script = format!(
        "if t=$(git -C {} rev-parse --show-toplevel 2>/dev/null); then n=${{t##*/}}; [ -n \"$n\" ] || n=home; \
         else n={}; fi; printf '%s\\n' \"$n\"",
        folder_word(folder),
        quote(folder_name(folder)),
    );
    script_argv(host, script, local_name)
}

/// Whether tmux can use `name` for a session as it is; the error is the reason, for the user.
/// tmux rewrites `:` and `.` in names, expands `#{…}` and runs `#(…)` in them, and reads a
/// leading `-` as a flag; the listing is one session per line.
pub fn valid_session_name(name: &str) -> Result<(), String> {
    let refuse = |why: &str| Err(why.to_string());
    if name.is_empty() {
        return refuse("the session needs a name");
    }
    if name.contains([':', '.']) {
        return refuse("names can't contain : or .");
    }
    if name.contains('#') {
        return refuse("names can't contain #");
    }
    if name.chars().any(char::is_control) {
        return refuse("names can't contain control characters");
    }
    if name.starts_with('-') {
        return refuse("names can't start with -");
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!("names can't be longer than {MAX_NAME_CHARS} characters"));
    }
    Ok(())
}

/// What a pipe has given so far, and a channel that closes when the pipe does.
struct Output {
    bytes: Arc<Mutex<Vec<u8>>>,
    ended: Receiver<()>,
}

/// Reads `pipe` on a thread of its own, into a buffer the caller can take from at any time.
fn drain(pipe: Option<impl Read + Send + 'static>) -> Output {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let (end, ended) = channel::<()>();
    if let Some(mut pipe) = pipe {
        let bytes = bytes.clone();
        std::thread::spawn(move || {
            let _end = end; // dropped, and so `ended` closed, when the pipe is at its end
            let mut chunk = [0u8; 8192];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => bytes.lock().unwrap_or_else(PoisonError::into_inner).extend_from_slice(&chunk[..n]),
                    Err(error) if error.kind() == ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        });
    }
    Output { bytes, ended }
}

impl Output {
    /// Everything read by the time the pipe closes, or by `deadline` when something the command
    /// started still holds it open (an ssh connection master, a background job).
    fn text(&self, deadline: Instant) -> String {
        let _ = self.ended.recv_timeout(deadline.saturating_duration_since(Instant::now()));
        String::from_utf8_lossy(&self.bytes.lock().unwrap_or_else(PoisonError::into_inner)).into_owned()
    }
}

/// Runs a command one of the builders made and returns what it printed. Blocks for up to
/// `timeout`, so not for the UI thread. A command that fails gives an error whose text is what
/// it wrote to stderr (else to stdout); one that is not done after `timeout` is killed, with
/// everything it started, and is an error too. The command reads nothing: its stdin is
/// /dev/null.
pub fn run(argv: &[String], timeout: Duration) -> anyhow::Result<String> {
    let (program, args) = argv.split_first().context("empty command")?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0) // its own, so a timeout kills what it started as well
        .spawn()
        .with_context(|| format!("couldn't start {program}"))?;
    let (stdout, stderr) = (drain(child.stdout.take()), drain(child.stderr.take()));
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            // SAFETY: kill takes no pointers. The child has not been waited for, so its pid,
            // which is also its process group's id, is still its own.
            unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
            let _ = child.wait();
            bail!("{program} timed out after {:.1} s", timeout.as_secs_f64());
        }
        std::thread::sleep(TICK);
    };
    let grace = Instant::now() + OUTPUT_GRACE;
    let (stdout, stderr) = (stdout.text(grace), stderr.text(grace));
    if status.success() {
        return Ok(stdout);
    }
    let said = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };
    if said.is_empty() {
        bail!("{program} failed ({status})");
    }
    bail!("{said}")
}
