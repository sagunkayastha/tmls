//! The builders against a real tmux, on this machine.
//!
//! Never the user's tmux server: before anything else each test points this process at a
//! private one (`TMUX_TMPDIR` = a fresh directory under cargo's target tmp dir, `TMUX` removed),
//! and every tmux process the tests start inherits that: the ones behind `tmux::run` and the
//! attach child on the PTY alike. Every tmux command first asserts the private directory is in
//! effect. Every session is named `tmlsrs-test-…` and a drop guard kills the ones its test made,
//! and nothing else; the private server exits by itself with its last session.
//!
//! Every wait has a timeout that fails the test.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use client_core::listing::SessionKey;
use client_core::tmux::{
    NewSession, Start, attach_argv, kill_argv, new_session_argv, rename_argv, run, valid_session_name,
};
use s2_core::pty::Pty;

const PREFIX: &str = "tmlsrs-test-";

/// This machine's name in every test.
const LOCAL: &str = "here";

const TIMEOUT: Duration = Duration::from_secs(10);

/// One private server for the three tests, so they run one at a time: the server exits with
/// its last session, and a test must not start one while another's is going down.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

static PRIVATE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Whether tmux, started from this process now, can only reach the private server.
fn private_in_effect() -> bool {
    let Some(dir) = PRIVATE_DIR.get() else { return false };
    dir.starts_with(env!("CARGO_TARGET_TMPDIR"))
        && dir.is_dir()
        && std::env::var_os("TMUX").is_none()
        && std::env::var_os("TMUX_TMPDIR").as_deref() == Some(dir.as_os_str())
}

/// Call first in every test. Points the process at the private server and returns its
/// directory; holds the other tests off until the guard is dropped.
fn private_server() -> (MutexGuard<'static, ()>, &'static Path) {
    let alone = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let dir = PRIVATE_DIR.get_or_init(|| {
        // short: a socket path holds about a hundred bytes
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("tmx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the private tmux directory");
        // SAFETY: the environment is changed once, here, under ONE_AT_A_TIME, before this
        // binary starts any process or thread of its own; the other test threads wait on that
        // lock at their first statement and do not read the environment.
        unsafe {
            std::env::set_var("TMUX_TMPDIR", &dir);
            std::env::remove_var("TMUX");
        }
        dir
    });
    assert!(private_in_effect(), "the private tmux directory {dir:?} is not in effect");
    (alone, dir)
}

/// Runs a command that reaches tmux; only against the private server.
fn go(argv: &[String]) -> anyhow::Result<String> {
    assert!(private_in_effect(), "no tmux command without the private server");
    run(argv, TIMEOUT)
}

fn tmux(args: &[&str]) -> anyhow::Result<String> {
    let mut argv = vec!["tmux".to_string()];
    argv.extend(args.iter().map(|a| a.to_string()));
    go(&argv)
}

fn key(name: &str) -> SessionKey {
    SessionKey { host: LOCAL.to_string(), name: name.to_string() }
}

/// The sessions a test made (or tried to); killed when it ends, however it ends.
struct Created(Vec<String>);

impl Created {
    fn name(&mut self, name: &str) -> String {
        assert!(name.starts_with(PREFIX), "{name:?} is not a test session name");
        self.0.push(name.to_string());
        name.to_string()
    }
}

impl Drop for Created {
    fn drop(&mut self) {
        for name in &self.0 {
            if private_in_effect() && name.starts_with(PREFIX) {
                let _ = run(&kill_argv(&key(name), LOCAL), TIMEOUT); // already gone is fine
            }
        }
    }
}

fn shell_in(folder: &Path, name: &str) -> NewSession {
    NewSession {
        host: LOCAL.to_string(),
        folder: folder.to_str().expect("a utf-8 path").to_string(),
        name: name.to_string(),
        start: Start::Shell,
        size: None,
    }
}

/// The private server's session names; none when it is not running.
fn sessions() -> Vec<String> {
    tmux(&["list-sessions", "-F", "#{session_name}"]).map(|out| out.lines().map(str::to_string).collect()).unwrap_or_default()
}

fn only(name: &str) -> Vec<String> {
    vec![name.to_string()]
}

/// A tmux format of the session's active pane.
fn show(name: &str, format: &str) -> String {
    tmux(&["display-message", "-p", "-t", &format!("={name}:"), format]).expect("display-message").trim_end().to_string()
}

/// Fails the test unless `done` comes true within [`TIMEOUT`].
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + TIMEOUT;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn assert_on_private_socket(name: &str, dir: &Path) {
    let socket = show(name, "#{socket_path}");
    assert!(Path::new(&socket).starts_with(dir), "{name} lives on {socket}, not under {dir:?}");
}

#[test]
fn create_list_rename_kill_round_trip() {
    let (_alone, dir) = private_server();
    let mut created = Created(Vec::new());
    let name = created.name("tmlsrs-test-a b'c");
    let renamed = created.name("tmlsrs-test-re \"named\" $HOME;x");
    let longer = created.name("tmlsrs-test-a b'c and more");
    for each in [&name, &renamed, &longer] {
        assert_eq!(valid_session_name(each), Ok(()));
    }

    let mut request = shell_in(dir, &name);
    request.size = Some((100, 30));
    go(&new_session_argv(&request, LOCAL)).expect("creating the session");
    assert_on_private_socket(&name, dir);
    assert_eq!(sessions(), only(&name));
    assert_eq!(show(&name, "#{window_width}"), "100", "the session starts at the size asked for");

    let again = go(&new_session_argv(&request, LOCAL)).expect_err("the name is taken");
    assert_eq!(again.to_string(), format!("a session named {name} already exists"));

    // "=NAME" is that session and no other: the shorter name must not reach the longer one
    go(&new_session_argv(&shell_in(dir, &longer), LOCAL)).expect("creating the second session");
    go(&kill_argv(&key(&name), LOCAL)).expect("killing by the exact name");
    assert_eq!(sessions(), only(&longer));
    go(&kill_argv(&key(&name), LOCAL)).expect_err("a prefix of a name is not that session");
    go(&rename_argv(&key(&name), &renamed, LOCAL)).expect_err("nor for a rename");
    assert_eq!(sessions(), only(&longer));

    go(&rename_argv(&key(&longer), &renamed, LOCAL)).expect("renaming");
    assert_eq!(sessions(), only(&renamed));
    go(&rename_argv(&key(&renamed), &name, LOCAL)).expect("renaming back to the quoted name");
    assert_eq!(sessions(), only(&name));

    go(&kill_argv(&key(&name), LOCAL)).expect("killing");
    assert_eq!(sessions(), Vec::<String>::new());
}

#[test]
fn new_session_starts_in_the_folder() {
    let (_alone, dir) = private_server();
    let mut created = Created(Vec::new());
    let folder = dir.join("start here, it's $HOME");
    std::fs::create_dir_all(&folder).unwrap();
    let folder = folder.canonicalize().unwrap();

    let name = created.name("tmlsrs-test-folder");
    go(&new_session_argv(&shell_in(&folder, &name), LOCAL)).expect("creating the session");
    assert_on_private_socket(&name, dir);
    wait_until("the session's shell to be in the folder", || Path::new(&show(&name, "#{pane_current_path}")) == folder);

    let missing = created.name("tmlsrs-test-missing");
    let nowhere = folder.join("no such folder");
    let error = go(&new_session_argv(&shell_in(&nowhere, &missing), LOCAL)).expect_err("the folder is not there");
    assert_eq!(error.to_string(), format!("no such folder: {}", nowhere.display()));
    assert_eq!(sessions(), only(&name), "and no session was made");

    // a preset is typed into the session's own shell, in the folder
    let preset = created.name("tmlsrs-test-preset");
    let mut request = shell_in(&folder, &preset);
    request.start = Start::Preset(["printf", "tmlsrs-%s-ok\\n", "42"].map(str::to_string).to_vec());
    go(&new_session_argv(&request, LOCAL)).expect("creating the preset session");
    wait_until("the preset's output", || {
        tmux(&["capture-pane", "-p", "-t", &format!("={preset}:")]).expect("capture-pane").contains("tmlsrs-42-ok")
    });
    assert_eq!(Path::new(&show(&preset, "#{pane_current_path}")), folder);
}

/// What the PTY has printed so far, read on a thread of its own.
struct Screen {
    chunks: Receiver<Vec<u8>>,
    text: String,
    ended: bool,
}

impl Screen {
    fn of(pty: &Pty) -> Screen {
        let mut reader = pty.reader().expect("the PTY's reader");
        let (tx, chunks) = channel();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        Screen { chunks, text: String::new(), ended: false }
    }

    /// Reads on until `done` says so; fails the test after [`TIMEOUT`] or when the output ends first.
    fn wait_for(&mut self, what: &str, done: impl Fn(&Screen) -> bool) {
        let deadline = Instant::now() + TIMEOUT;
        while !done(self) {
            assert!(!self.ended, "the attach command ended before {what}; it printed: {:?}", self.text);
            match self.chunks.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(chunk) => self.text.push_str(&String::from_utf8_lossy(&chunk)),
                Err(RecvTimeoutError::Disconnected) => self.ended = true,
                Err(RecvTimeoutError::Timeout) => panic!("timed out waiting for {what}; it printed: {:?}", self.text),
            }
        }
    }
}

/// The byte a terminal sends for tmux's prefix key (`C-b` unless the user's tmux.conf says otherwise).
fn prefix_byte() -> u8 {
    let prefix = tmux(&["show-options", "-gv", "prefix"]).expect("the prefix option");
    match prefix.trim().strip_prefix("C-").map(str::as_bytes) {
        Some([letter]) if letter.is_ascii_alphabetic() => letter.to_ascii_lowercase() & 0x1f,
        _ => panic!("this test can only type a Ctrl-letter tmux prefix, not {:?}", prefix.trim()),
    }
}

#[test]
fn attach_argv_attaches() {
    let (_alone, dir) = private_server();
    let mut created = Created(Vec::new());
    let name = created.name("tmlsrs-test-attach it's");
    go(&new_session_argv(&shell_in(dir, &name), LOCAL)).expect("creating the session");
    assert_on_private_socket(&name, dir);
    assert_eq!(show(&name, "#{session_attached}"), "0");

    assert!(private_in_effect(), "no attach without the private server");
    let pty = Pty::spawn(&attach_argv(&key(&name), LOCAL), 80, 24).expect("starting the attach command");
    let mut screen = Screen::of(&pty);
    wait_until("the client to attach", || show(&name, "#{session_attached}") == "1");

    // the shell behind the prompt answers: the quotes make the answer differ from the typed line
    pty.writer().write_all(b"echo tmlsrs-''42-ok\r").expect("typing");
    screen.wait_for("the shell's answer on the PTY", |s| s.text.contains("tmlsrs-42-ok"));

    pty.writer().write_all(&[prefix_byte(), b'd']).expect("typing the detach keys");
    screen.wait_for("the attach command to end", |s| s.ended);
    assert!(screen.text.contains("[detached"), "tmux did not say it detached: {:?}", screen.text);
    assert_eq!(show(&name, "#{session_attached}"), "0");
    assert_eq!(sessions(), only(&name), "detaching leaves the session running");
}
