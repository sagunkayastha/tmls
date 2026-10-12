//! Keeps the listing of every host fresh: one thread per host, so a host that hangs delays
//! nobody else. Each thread runs the host's listing command with a timeout, hands the snapshot
//! to the callback when it differs from the host's previous one in anything but the sessions'
//! output ages (an online host going offline always does), then sleeps for the period or until [`Poller::refresh_now`].
//!
//! A listing runs in a process group of its own and is read without blocking, so a timeout or
//! a stopping poller kills the command and everything it started, and never waits on a pipe.

use std::io::{ErrorKind, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::listing::{HostSnapshot, LISTING_SCRIPT, listing_command, parse_listing};

/// `HostSnapshot::error` of a host whose ssh wants a password, a key or a host-key answer:
/// the user has to sign in by hand.
pub const SIGN_IN_REQUIRED: &str = "sign-in required";

/// What ssh prints when it cannot sign in without asking (tmls: `hosts.list_host`).
const SIGN_IN_PHRASES: [&str; 4] =
    ["permission denied", "host key verification failed", "keyboard-interactive", "password + otp"];

/// How often a running listing is checked for exit, timeout and stop.
const TICK: Duration = Duration::from_millis(20);

/// No listing is this long; a host that prints more is not read further.
const MAX_OUTPUT: usize = 16 << 20;

/// Makes the listing command of a host; [`LISTING_SCRIPT`] goes to its stdin.
pub type CommandFactory = Box<dyn Fn(&str) -> Command + Send + Sync>;

pub struct Poller {
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
}

struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

struct State {
    stop: bool,
    /// Counts `refresh_now` calls: a thread that sees it move lists again.
    refresh: u64,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Poller {
    /// Lists every host of `cfg` now and then every `period`; a listing that takes longer than
    /// `timeout` is killed and its host reported offline. `on_snapshot` runs on the polling
    /// threads, one call at a time.
    pub fn start(
        cfg: &Config,
        period: Duration,
        timeout: Duration,
        on_snapshot: Box<dyn Fn(HostSnapshot) + Send>,
    ) -> Poller {
        let local_name = cfg.local_name.clone();
        let command = Box::new(move |host: &str| listing_command(host, &local_name));
        Poller::start_with(&cfg.hosts, command, period, timeout, on_snapshot)
    }

    /// [`Poller::start`] with the hosts and their listing commands given (tests use fake hosts).
    pub fn start_with(
        hosts: &[String],
        command: CommandFactory,
        period: Duration,
        timeout: Duration,
        on_snapshot: Box<dyn Fn(HostSnapshot) + Send>,
    ) -> Poller {
        let shared = Arc::new(Shared { state: Mutex::new(State { stop: false, refresh: 0 }), wake: Condvar::new() });
        let command = Arc::new(command);
        let deliver = Arc::new(Mutex::new(on_snapshot));
        let threads = hosts
            .iter()
            .map(|host| {
                let (host, shared, command, deliver) = (host.clone(), shared.clone(), command.clone(), deliver.clone());
                std::thread::Builder::new()
                    .name(format!("tmls-poll-{host}"))
                    .spawn(move || poll_host(&host, &shared, &**command, period, timeout, &deliver))
                    .expect("spawning a polling thread")
            })
            .collect();
        Poller { shared, threads }
    }

    /// Lists every host again without waiting for the period to end.
    pub fn refresh_now(&self) {
        self.shared.state().refresh += 1;
        self.shared.wake.notify_all();
    }
}

impl Drop for Poller {
    /// Stops and joins every thread; a listing still running is killed.
    fn drop(&mut self) {
        self.shared.state().stop = true;
        self.shared.wake.notify_all();
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

fn poll_host(
    host: &str,
    shared: &Shared,
    command: &(dyn Fn(&str) -> Command + Send + Sync),
    period: Duration,
    timeout: Duration,
    deliver: &Mutex<Box<dyn Fn(HostSnapshot) + Send>>,
) {
    let mut last: Option<HostSnapshot> = None;
    loop {
        let refresh = shared.state().refresh;
        let stopped = || shared.state().stop;
        let Some(snapshot) = run(host, command(host), timeout, &stopped) else { return };
        if stopped() {
            return;
        }
        // compared without the output ages: they grow at every listing of a host with sessions
        let comparable = without_ages(&snapshot);
        if last.as_ref() != Some(&comparable) {
            (deliver.lock().unwrap_or_else(PoisonError::into_inner))(snapshot);
            last = Some(comparable);
        }

        let wake_at = Instant::now() + period;
        let mut state = shared.state();
        loop {
            if state.stop {
                return;
            }
            let left = wake_at.saturating_duration_since(Instant::now());
            if state.refresh != refresh || left.is_zero() {
                break;
            }
            state = shared.wake.wait_timeout(state, left).unwrap_or_else(PoisonError::into_inner).0;
        }
    }
}

/// The snapshot with every `last_output_age_s` and `activity` blanked: what "the same as
/// before" compares. Ages grow at every listing; activity moves with output, which reaches the
/// app with the session's next status change (a shell that prints turns running).
fn without_ages(snapshot: &HostSnapshot) -> HostSnapshot {
    let mut snapshot = snapshot.clone();
    for session in &mut snapshot.sessions {
        session.last_output_age_s = None;
        session.activity = None;
    }
    snapshot
}

/// Lists `host` once with `command` (see [`listing_command`]), on the calling thread. Any
/// failure is an offline snapshot that says why: no answer within `timeout` (the command is
/// killed), ssh could not connect or sign in, or what came back is not a listing.
pub fn list_once(host: &str, command: Command, timeout: Duration) -> HostSnapshot {
    run(host, command, timeout, &|| false).expect("never stopped")
}

fn offline(host: &str, error: String) -> HostSnapshot {
    HostSnapshot { host: host.to_string(), online: false, error: Some(error), sessions: Vec::new() }
}

/// Runs one listing; `None` when `stopped` said so first.
fn run(host: &str, mut command: Command, timeout: Duration, stopped: &dyn Fn() -> bool) -> Option<HostSnapshot> {
    // a group of its own: killing it takes what the command started too (a local `sh` waiting on tmux)
    command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).process_group(0);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => return Some(offline(host, format!("cannot run {}: {e}", command.get_program().to_string_lossy()))),
    };
    if let Some(mut stdin) = child.stdin.take() {
        // far smaller than a pipe's buffer, so this cannot block; a command that exits without
        // reading it is that command's failure, reported from its exit status below
        let _ = stdin.write_all(LISTING_SCRIPT.as_bytes());
    }
    let mut stdout = Pipe::new(child.stdout.take());
    let mut stderr = Pipe::new(child.stderr.take());
    let give_up_at = Instant::now() + timeout;
    let status = loop {
        stdout.read_available();
        stderr.read_available();
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => {
                kill_group(&mut child);
                return Some(offline(host, format!("cannot wait for the listing: {e}")));
            }
        }
        if stopped() {
            kill_group(&mut child);
            return None;
        }
        if Instant::now() >= give_up_at {
            kill_group(&mut child);
            return Some(offline(host, format!("no answer in {:.0} s", timeout.as_secs_f64())));
        }
        wait_readable(&stdout, &stderr);
    };
    // what it wrote before exiting; anything it left running may keep the pipes open, so no wait for EOF
    stdout.read_available();
    stderr.read_available();
    Some(snapshot(host, status, &stdout.data, &stderr.data))
}

fn snapshot(host: &str, status: ExitStatus, stdout: &[u8], stderr: &[u8]) -> HostSnapshot {
    if status.success() {
        return parse_listing(host, &String::from_utf8_lossy(stdout));
    }
    // the script itself always succeeds: this is ssh failing, or a connection lost mid-listing
    let stderr = String::from_utf8_lossy(stderr);
    let lower = stderr.to_lowercase();
    if SIGN_IN_PHRASES.iter().any(|phrase| lower.contains(phrase)) {
        return offline(host, SIGN_IN_REQUIRED.to_string());
    }
    let last_line = stderr.lines().map(str::trim).rfind(|line| !line.is_empty());
    offline(host, last_line.map_or_else(|| status.to_string(), str::to_string))
}

/// SIGKILL to the listing's whole process group, then reaps the command.
fn kill_group(child: &mut Child) {
    // SAFETY: plain syscall. The child is not reaped yet, so its pid (the group's id, from
    // `process_group(0)`) still names this group and nothing else.
    unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL) };
    let _ = child.kill();
    let _ = child.wait();
}

/// One of the child's output pipes, read without blocking.
struct Pipe<R> {
    source: Option<R>,
    data: Vec<u8>,
}

impl<R: Read + AsRawFd> Pipe<R> {
    fn new(source: Option<R>) -> Pipe<R> {
        if let Some(source) = &source {
            let fd = source.as_raw_fd();
            // SAFETY: fcntl on a descriptor this process owns. The flag is on the read end only.
            unsafe { libc::fcntl(fd, libc::F_SETFL, libc::fcntl(fd, libc::F_GETFL) | libc::O_NONBLOCK) };
        }
        Pipe { source, data: Vec::new() }
    }

    /// Takes what is in the pipe now; closes it at end of file.
    fn read_available(&mut self) {
        let mut buf = [0u8; 8192];
        while let Some(source) = &mut self.source {
            match source.read(&mut buf) {
                Ok(0) => self.source = None,
                Ok(n) if self.data.len() < MAX_OUTPUT => self.data.extend_from_slice(&buf[..n]),
                Ok(_) => {}
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(_) => self.source = None,
            }
        }
    }
}

/// Sleeps one [`TICK`], less when a pipe has something to read.
fn wait_readable<A: AsRawFd, B: AsRawFd>(a: &Pipe<A>, b: &Pipe<B>) {
    let open = [a.source.as_ref().map(AsRawFd::as_raw_fd), b.source.as_ref().map(AsRawFd::as_raw_fd)];
    let mut fds: Vec<libc::pollfd> =
        open.into_iter().flatten().map(|fd| libc::pollfd { fd, events: libc::POLLIN, revents: 0 }).collect();
    // SAFETY: `fds` is a valid array of `fds.len()` pollfd for the length of the call.
    unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, TICK.as_millis() as libc::c_int) };
}
