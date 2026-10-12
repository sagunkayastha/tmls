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

use std::ffi::OsStr;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
pub(crate) fn quote(word: &str) -> String {
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

pub(crate) fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

/// What every ssh tmls-rs starts says first: a probe every 15 s, given up after three
/// unanswered, so a link that died (a laptop asleep, Wi-Fi gone) ends the command — and an
/// attached tab shows "ended" — within 45 s instead of hanging.
const KEEPALIVE: [&str; 4] = ["-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3"];

/// The ControlMaster options, once `use_control_master` ran; never set in a process that does
/// not call it (the tests), whose ssh commands then have the keepalive only.
static CONTROL: OnceLock<Vec<String>> = OnceLock::new();

/// Where the shared connections' sockets go: `$XDG_RUNTIME_DIR/tmls-rs`, else
/// `~/.cache/tmls-rs`. `None` when neither is known; when the socket's path would not fit: ssh
/// binds the ControlPath (the directory, `/cm-`, the 40 characters of `%C`) plus `.` and 16
/// random characters into a Unix socket's 107 bytes, so the directory has 46 bytes at most; and
/// when the directory has whitespace or a `%` (`-o ControlPath=` is not quoted, and `%` starts
/// one of ssh's tokens).
pub fn control_dir(runtime: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    let dir = match (runtime.filter(|r| !r.is_empty()), home.filter(|h| !h.is_empty())) {
        (Some(runtime), _) => Path::new(runtime).join("tmls-rs"),
        (None, Some(home)) => Path::new(home).join(".cache/tmls-rs"),
        (None, None) => return None,
    };
    let bytes = dir.as_os_str().as_encoded_bytes();
    let plain = !bytes.iter().any(|b| b.is_ascii_whitespace() || *b == b'%');
    (plain && bytes.len() + 44 <= 90).then_some(dir)
}

/// One master connection per host, kept 10 minutes after its last user (tmls does the same).
pub fn control_options(dir: &Path) -> Vec<String> {
    vec![
        "-o".to_string(),
        "ControlMaster=auto".to_string(),
        "-o".to_string(),
        "ControlPersist=10m".to_string(),
        "-o".to_string(),
        format!("ControlPath={}/cm-%C", dir.display()),
    ]
}

/// Whether to share connections, from `TMLS_RS_NO_CONTROLMASTER`: off only for `1`.
pub fn control_wanted(value: Option<&OsStr>) -> bool {
    value != Some(OsStr::new("1"))
}

/// Makes `dir` (mode 0700, also when it existed with another mode) and from then on every ssh
/// this process starts shares one connection per host through it. Call once, at start; a
/// second call changes nothing.
pub fn use_control_master(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    let _ = CONTROL.set(control_options(dir));
    Ok(())
}

/// The start of every ssh argv: `ssh`, the keepalive, the shared connection once switched on,
/// `flags`, then `host`. The caller adds the remote command.
pub fn ssh_base(host: &str, flags: &[&str]) -> Vec<String> {
    let mut argv = vec!["ssh".to_string()];
    argv.extend(KEEPALIVE.iter().map(|word| word.to_string()));
    if let Some(control) = CONTROL.get() {
        argv.extend(control.iter().cloned());
    }
    argv.extend(flags.iter().map(|word| word.to_string()));
    argv.push(host.to_string());
    argv
}

/// `command` run by `host`'s shell without ever prompting (as the listing does).
pub(crate) fn ssh(host: &str, command: String) -> Vec<String> {
    let mut argv = ssh_base(host, &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5"]);
    argv.push(command);
    argv
}

/// `script` run by sh, here or on `host`.
pub(crate) fn script_argv(host: &str, script: String, local_name: &str) -> Vec<String> {
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
        let mut argv = ssh_base(&key.host, &["-t"]);
        argv.push(format!("tmux -u attach -t {}", quote(&target)));
        argv
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

/// What the sign-in row runs in a terminal (tmls `SignInRow`): ssh with a terminal and without
/// `BatchMode`, so it may ask for a password, a code or a host key, then `true`. Once in, the
/// shared connection stays up and the next listing finds the host.
pub fn sign_in_argv(host: &str) -> Vec<String> {
    let mut argv = ssh_base(host, &["-t"]);
    argv.push("true".to_string());
    argv
}

/// `folder` as a shell word. `~` must stay outside the quotes to mean the host's home.
pub(crate) fn folder_word(folder: &str) -> String {
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

/// Prints the command each pane of the session runs, one per line (shells included): what a
/// kill would end.
pub fn running_commands_argv(key: &SessionKey, local_name: &str) -> Vec<String> {
    let target = format!("={}", key.name);
    let format = "#{pane_current_command}";
    if key.host == local_name {
        strings(&["tmux", "list-panes", "-s", "-t", &target, "-F", format])
    } else {
        ssh(&key.host, format!("tmux list-panes -s -t {} -F {}", quote(&target), quote(format)))
    }
}

/// Which way `split_argv` splits the session's current pane (tmls `tmux_ops.split`: tmux's
/// `-h` puts the new pane beside it, `-v` under it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Split {
    SideBySide,
    TopBottom,
}

/// `tmux ARGS` here, or over ssh with every word single-quoted for the host's shell.
fn session_tmux(key: &SessionKey, args: &[&str], local_name: &str) -> Vec<String> {
    if key.host == local_name {
        let mut argv = strings(&["tmux"]);
        argv.extend(strings(args));
        argv
    } else {
        let words: Vec<String> = args.iter().map(|word| quote(word)).collect();
        ssh(&key.host, format!("tmux {}", words.join(" ")))
    }
}

/// A new window in the session, in the current pane's folder (tmls `new_window`).
pub fn new_window_argv(key: &SessionKey, local_name: &str) -> Vec<String> {
    let target = format!("={}:", key.name);
    session_tmux(key, &["new-window", "-t", &target, "-c", "#{pane_current_path}"], local_name)
}

/// Renames the session's current window. Check `new_name` with [`valid_window_name`] first.
pub fn rename_window_argv(key: &SessionKey, new_name: &str, local_name: &str) -> Vec<String> {
    let target = format!("={}:", key.name);
    session_tmux(key, &["rename-window", "-t", &target, "--", new_name], local_name)
}

/// Splits the session's current pane, the new one in the same folder (tmls `split`).
pub fn split_argv(key: &SessionKey, split: Split, local_name: &str) -> Vec<String> {
    let target = format!("={}:", key.name);
    let way = match split {
        Split::SideBySide => "-h",
        Split::TopBottom => "-v",
    };
    session_tmux(key, &["split-window", way, "-t", &target, "-c", "#{pane_current_path}"], local_name)
}

/// Ends the session's active pane; the last one ends the session (tmls `kill_pane`).
pub fn kill_pane_argv(key: &SessionKey, local_name: &str) -> Vec<String> {
    let target = format!("={}:", key.name);
    session_tmux(key, &["kill-pane", "-t", &target], local_name)
}

/// Prints one line per pane of the whole session: one line means the active pane is its last.
pub fn pane_count_argv(key: &SessionKey, local_name: &str) -> Vec<String> {
    let target = format!("={}:", key.name);
    session_tmux(key, &["list-panes", "-s", "-t", &target, "-F", "#{pane_id}"], local_name)
}

/// Prints what the session's active pane runs (tmls `pane_info`).
pub fn pane_command_argv(key: &SessionKey, local_name: &str) -> Vec<String> {
    let target = format!("={}:", key.name);
    session_tmux(key, &["display-message", "-p", "-t", &target, "#{pane_current_command}"], local_name)
}

/// Whether tmux can name a window `name`; the error is the reason, for the user. A window may
/// have `:` `.` `#` in its name; it needs one, without control characters.
pub fn valid_window_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("the window needs a name".to_string());
    }
    if name.chars().any(char::is_control) {
        return Err("names can't contain control characters".to_string());
    }
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!("names can't be longer than {MAX_NAME_CHARS} characters"));
    }
    Ok(())
}

/// What a command for Claude's pane targets: the pane (`%7`) when the listing has a valid one,
/// else the session's current pane (`=NAME:`).
pub fn pane_target(key: &SessionKey, pane: Option<&str>) -> String {
    match pane.filter(|pane| valid_pane(pane)) {
        Some(pane) => pane.to_string(),
        None => format!("={}:", key.name),
    }
}

/// Prints what the pane shows now (tmls `approve._capture_script`).
pub fn capture_argv(key: &SessionKey, pane: Option<&str>, local_name: &str) -> Vec<String> {
    let target = pane_target(key, pane);
    if key.host == local_name {
        strings(&["tmux", "capture-pane", "-p", "-t", &target])
    } else {
        ssh(&key.host, format!("tmux capture-pane -p -t {}", quote(&target)))
    }
}

/// Presses `keys` (a tmux key name: `1`, `Escape`, `Enter`) in the pane.
pub fn send_keys_argv(key: &SessionKey, pane: Option<&str>, keys: &str, local_name: &str) -> Vec<String> {
    let target = pane_target(key, pane);
    if key.host == local_name {
        strings(&["tmux", "send-keys", "-t", &target, keys])
    } else {
        ssh(&key.host, format!("tmux send-keys -t {} {}", quote(&target), quote(keys)))
    }
}

/// Types the text on the script's stdin into `target` and presses Enter (tmls `prompts.send`):
/// through a paste buffer of its own (`buffer`), as a bracketed paste when the program asks for
/// one, so a message of several lines arrives whole; the buffer is deleted after the paste, or
/// when the paste fails. The text never appears in the script.
pub fn send_text_script(target: &str, buffer: &str) -> String {
    let (t, b) = (quote(target), quote(buffer));
    format!(
        "tmux load-buffer -b {b} - && {{ tmux paste-buffer -p -d -b {b} -t {t} || {{ tmux delete-buffer -b {b}; false; }}; }} && sleep 0.2 && tmux send-keys -t {t} Enter"
    )
}

/// [`send_text_script`] for Claude's pane of the session (`pane_target`), here or on its host.
pub fn send_text_argv(key: &SessionKey, pane: Option<&str>, buffer: &str, local_name: &str) -> Vec<String> {
    script_argv(&key.host, send_text_script(&pane_target(key, pane), buffer), local_name)
}

/// A paste buffer's name of its own (`tmls-` and 8 hex digits): two sends in the same moment
/// cannot swap their texts.
pub fn buffer_name() -> String {
    static COUNT: AtomicU32 = AtomicU32::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
    let n = COUNT.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    format!("tmls-{:08x}", nanos ^ std::process::id().rotate_left(16) ^ n.wrapping_mul(0x9e37_79b9))
}

/// tmls's name for a session in `folder` when git knows nothing: the folder's own name.
pub fn folder_name(folder: &str) -> &str {
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

/// Whether `pane` is a tmux pane id (`%` and digits): the only form of a pane that goes into a
/// command.
pub fn valid_pane(pane: &str) -> bool {
    pane.strip_prefix('%').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
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
    fn bytes(&self, deadline: Instant) -> Vec<u8> {
        let _ = self.ended.recv_timeout(deadline.saturating_duration_since(Instant::now()));
        std::mem::take(&mut *self.bytes.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// The same as text; bytes that are not UTF-8 show as U+FFFD.
    fn text(&self, deadline: Instant) -> String {
        String::from_utf8_lossy(&self.bytes(deadline)).into_owned()
    }
}

/// Runs a command one of the builders made and returns what it printed. Blocks for up to
/// `timeout`, so not for the UI thread. A command that fails gives an error whose text is what
/// it wrote to stderr (else to stdout); one that is not done after `timeout` is killed, with
/// everything it started, and is an error too. The command reads nothing: its stdin is
/// /dev/null (see [`run_with_stdin`]).
pub fn run(argv: &[String], timeout: Duration) -> anyhow::Result<String> {
    run_bytes(argv, timeout).map(|out| String::from_utf8_lossy(&out).into_owned())
}

/// [`run`], with what the command printed as the bytes they are: for the contents of a file,
/// which need not be text.
pub fn run_bytes(argv: &[String], timeout: Duration) -> anyhow::Result<Vec<u8>> {
    run_input(argv, None, timeout)
}

/// [`run`] with `input` on the command's stdin (a message for tmux's paste buffer, a text for
/// `wl-copy`). The input is written on a thread of its own and its end closes stdin.
pub fn run_with_stdin(argv: &[String], input: &[u8], timeout: Duration) -> anyhow::Result<String> {
    run_input(argv, Some(input), timeout).map(|out| String::from_utf8_lossy(&out).into_owned())
}

fn run_input(argv: &[String], input: Option<&[u8]>, timeout: Duration) -> anyhow::Result<Vec<u8>> {
    let (program, args) = argv.split_first().context("empty command")?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0) // its own, so a timeout kills what it started as well
        .spawn()
        .with_context(|| format!("couldn't start {program}"))?;
    if let (Some(input), Some(mut pipe)) = (input, child.stdin.take()) {
        let input = input.to_vec();
        // Not on this thread: a command that writes before it has read everything would wait
        // for us while we wait for it.
        std::thread::spawn(move || {
            let _ = pipe.write_all(&input);
        });
    }
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
    let stdout = stdout.bytes(grace);
    // A success is its stdout alone: no waiting for a stderr that something it left running
    // still holds (wl-copy's daemon, which keeps the clipboard).
    if status.success() {
        return Ok(stdout);
    }
    let stderr = stderr.text(grace);
    let stdout = String::from_utf8_lossy(&stdout);
    let said = if stderr.trim().is_empty() { stdout.trim() } else { stderr.trim() };
    if said.is_empty() {
        bail!("{program} failed ({status})");
    }
    bail!("{said}")
}
