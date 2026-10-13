//! The sketchpad hub, as `~/.config/tmls/sketchpad` names it: its address, or `ssh HOST
//! ADDR:PORT` for a hub this machine reaches only through HOST. For the second, [`Forward`] keeps
//! one `ssh -N -L 127.0.0.1:LPORT:ADDR:PORT HOST` running and says where the hub is here.
//!
//! The forward has a connection of its own, never the shared one (`tmux::ssh_base`): a forward
//! asked of a ControlMaster stays in the master and outlives the app.

use std::io::{ErrorKind, Read};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::tmux::KEEPALIVE;

/// How long [`Forward::open`] waits for a new ssh to take connections on the local port.
const READY_TIMEOUT: Duration = Duration::from_secs(8);

/// How often the wait looks at the port and at ssh.
const TICK: Duration = Duration::from_millis(50);

/// How long one look at the local port may take.
const PROBE: Duration = Duration::from_millis(300);

/// How much of what ssh wrote to stderr is kept: its end.
const STDERR_KEPT: usize = 4096;

/// What the `sketchpad` file says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sketchpad {
    /// The hub's address, opened as it is.
    Url(String),
    /// `ssh HOST ADDR:PORT`: the hub is `ADDR:PORT` as `host` sees it.
    Forward { host: String, addr: String, port: u16 },
}

impl Sketchpad {
    /// One line of the file, trimmed and without its comment. A line whose first word is `ssh`
    /// is a forward or nothing (`None`), never an address: `ssh HOST ADDR:PORT` and no more,
    /// HOST and ADDR not starting with `-` (ssh would read an option), PORT 1 to 65535, an
    /// IPv6 ADDR in `[…]`.
    pub fn parse(line: &str) -> Option<Sketchpad> {
        let mut words = line.split_whitespace();
        if words.next() != Some("ssh") {
            return Some(Sketchpad::Url(line.to_string()));
        }
        let (host, target, None) = (words.next()?, words.next()?, words.next()) else { return None };
        let (addr, port) = target.rsplit_once(':')?;
        // digits only: `u16::from_str` takes `+8790` too
        let port = port.parse::<u16>().ok().filter(|n| *n != 0 && port.bytes().all(|b| b.is_ascii_digit()))?;
        let bracketed = addr.starts_with('[') && addr.ends_with(']');
        let usable = !host.starts_with('-') && !addr.is_empty() && !addr.starts_with('-') && (bracketed || !addr.contains(':'));
        usable.then(|| Sketchpad::Forward { host: host.to_string(), addr: addr.to_string(), port })
    }
}

/// The forward's command: this machine's `127.0.0.1:local_port` is `addr:port` as `host` sees
/// it. No remote command (`-N`), never a prompt, and ssh exits when it cannot listen. Its own
/// connection: `ControlMaster=no` and `ControlPath=none` come before anything `~/.ssh/config`
/// says (ssh keeps the first value of an option), and the shared connection's options are not
/// here at all; not on Windows, whose OpenSSH has no `ControlMaster`. `host` is last, after
/// every option; [`Sketchpad::parse`] gives none that starts with `-`.
pub fn forward_argv(host: &str, addr: &str, port: u16, local_port: u16) -> Vec<String> {
    let mut argv = vec!["ssh".to_string(), "-N".to_string(), "-L".to_string(), format!("127.0.0.1:{local_port}:{addr}:{port}")];
    let mut option = |value: &str| argv.extend(["-o".to_string(), value.to_string()]);
    if !cfg!(windows) {
        option("ControlMaster=no");
        option("ControlPath=none");
    }
    option("BatchMode=yes");
    option("ConnectTimeout=5");
    option("ExitOnForwardFailure=yes");
    argv.extend(KEEPALIVE.iter().map(|word| word.to_string()));
    argv.push(host.to_string());
    argv
}

/// The local port for the forward: `preferred` (the hub's own number, so its address here is
/// the same every time and the browser's login is kept) when 127.0.0.1 has it free, else one
/// the system gives. The port is free again when this returns; ssh binds it next.
pub fn local_port(preferred: u16) -> std::io::Result<u16> {
    let bind = |port: u16| TcpListener::bind((Ipv4Addr::LOCALHOST, port)).and_then(|listener| listener.local_addr());
    bind(preferred).or_else(|_| bind(0)).map(|addr| addr.port())
}

/// Whether something takes connections on 127.0.0.1's `port`.
fn accepts(port: u16) -> bool {
    TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)), PROBE).is_ok()
}

/// A running ssh, its local port and the end of what it wrote to stderr.
struct Tunnel {
    child: Child,
    port: u16,
    stderr: Arc<Mutex<Vec<u8>>>,
    /// The thread that reads its stderr has read it to the end.
    drained: Arc<AtomicBool>,
}

impl Tunnel {
    /// Starts `argv`, reading nothing; its stderr is read on a thread of its own for as long
    /// as it runs, so that ssh never waits on a full pipe.
    fn start(argv: &[String], port: u16) -> std::io::Result<Tunnel> {
        let (program, args) = argv.split_first().ok_or(ErrorKind::InvalidInput)?;
        // In this process's group (a Ctrl+C in the app's terminal ends it too) and, on Windows,
        // in its console, as the listing's ssh is: no window of its own.
        let mut child = Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).spawn()?;
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let drained = Arc::new(AtomicBool::new(false));
        if let Some(mut pipe) = child.stderr.take() {
            let kept = stderr.clone();
            let drained = drained.clone();
            std::thread::spawn(move || {
                let mut chunk = [0u8; 1024];
                loop {
                    match pipe.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => {
                            let mut kept = kept.lock().unwrap_or_else(PoisonError::into_inner);
                            kept.extend_from_slice(&chunk[..n]);
                            let over = kept.len().saturating_sub(STDERR_KEPT);
                            kept.drain(..over);
                        }
                        Err(error) if error.kind() == ErrorKind::Interrupted => {}
                        Err(_) => break,
                    }
                }
                drained.store(true, Ordering::SeqCst);
            });
        } else {
            drained.store(true, Ordering::SeqCst);
        }
        Ok(Tunnel { child, port, stderr, drained })
    }

    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// The last line ssh wrote that is not blank.
    fn last_line(&self) -> Option<String> {
        let kept = self.stderr.lock().unwrap_or_else(PoisonError::into_inner);
        String::from_utf8_lossy(&kept).lines().map(str::trim).rfind(|line| !line.is_empty()).map(str::to_string)
    }
}

impl Drop for Tunnel {
    /// Killed and reaped (nothing when it has ended already).
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Shared {
    host: String,
    addr: String,
    port: u16,
    /// The ssh, from its start until it is found dead, replaced, or the `Forward` is dropped.
    tunnel: Mutex<Option<Tunnel>>,
    /// An `open` is at work: the next one starts nothing.
    busy: AtomicBool,
    /// The `Forward` was dropped: nothing is started any more. Set and read with `tunnel` held.
    closed: AtomicBool,
}

/// The app's forward to the hub: at most one ssh, started by the first [`Forward::open`],
/// started again by a later one when it has died, killed and reaped when this is dropped.
pub struct Forward {
    shared: Arc<Shared>,
}

impl Forward {
    /// Nothing is started yet.
    pub fn new(host: &str, addr: &str, port: u16) -> Forward {
        let (tunnel, busy, closed) = (Mutex::new(None), AtomicBool::new(false), AtomicBool::new(false));
        Forward { shared: Arc::new(Shared { host: host.to_string(), addr: addr.to_string(), port, tunnel, busy, closed }) }
    }

    /// The host the forward goes through.
    pub fn host(&self) -> &str {
        &self.shared.host
    }

    /// Makes the hub reachable here and hands `done` the local port, or why not, as the status
    /// line says it. Returns at once: the work, and `done`, are on a thread of their own. A
    /// running ssh whose port takes connections is used as it is; else one is started and
    /// waited for until the port answers, ssh exits or 8 s have passed.
    ///
    /// `false`, and `done` is dropped unused, while an earlier `open` is still at work: no
    /// second ssh is started.
    pub fn open(&self, done: impl FnOnce(Result<u16, String>) + Send + 'static) -> bool {
        if self.shared.busy.swap(true, Ordering::SeqCst) {
            return false;
        }
        let shared = self.shared.clone();
        std::thread::spawn(move || {
            let result = shared.connect(&forward_argv, READY_TIMEOUT);
            shared.busy.store(false, Ordering::SeqCst);
            done(result);
        });
        true
    }
}

impl Drop for Forward {
    fn drop(&mut self) {
        let mut tunnel = self.shared.lock();
        self.shared.closed.store(true, Ordering::SeqCst);
        *tunnel = None;
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Option<Tunnel>> {
        self.tunnel.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// [`Forward::open`]'s work, with the command as `argv(host, addr, port, local_port)`.
    /// The lock is never held while waiting, so a drop of the `Forward` kills ssh at any time.
    fn connect(&self, argv: &dyn Fn(&str, &str, u16, u16) -> Vec<String>, timeout: Duration) -> Result<u16, String> {
        let host = &self.host;
        let running = self.lock().as_mut().and_then(|running| running.alive().then_some(running.port));
        if let Some(port) = running.filter(|port| accepts(*port)) {
            return Ok(port);
        }
        let local = {
            let mut tunnel = self.lock();
            // The old one first (dead, or alive and deaf): its port may be the one wanted.
            *tunnel = None;
            if self.closed.load(Ordering::SeqCst) {
                return Err(format!("sketchpad: no answer through {host}"));
            }
            let local = local_port(self.port).map_err(|e| format!("sketchpad: no local port: {e}"))?;
            let started = Tunnel::start(&argv(host, &self.addr, self.port, local), local);
            *tunnel = Some(started.map_err(|e| format!("sketchpad: ssh to {host} failed: {e}"))?);
            local
        };
        let deadline = Instant::now() + timeout;
        loop {
            {
                let mut tunnel = self.lock();
                let Some(running) = tunnel.as_mut() else {
                    return Err(format!("sketchpad: no answer through {host}"));
                };
                if !running.alive() {
                    // What it wrote last may still be on its way through the pipe: until its
                    // reader is at the end, for a second at most.
                    let drained = running.drained.clone();
                    drop(tunnel);
                    let until = Instant::now() + Duration::from_secs(1);
                    while !drained.load(Ordering::SeqCst) && Instant::now() < until {
                        std::thread::sleep(TICK);
                    }
                    let said = self.lock().take().and_then(|ended| ended.last_line());
                    return Err(match said {
                        Some(line) => format!("sketchpad: ssh to {host} failed: {line}"),
                        None => format!("sketchpad: ssh to {host} failed"),
                    });
                }
            }
            if accepts(local) {
                return Ok(local);
            }
            if Instant::now() >= deadline {
                *self.lock() = None;
                return Err(format!("sketchpad: no answer through {host}"));
            }
            std::thread::sleep(TICK);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forward(host: &str, addr: &str, port: u16) -> Option<Sketchpad> {
        Some(Sketchpad::Forward { host: host.into(), addr: addr.into(), port })
    }

    #[test]
    fn a_line_is_an_address_or_a_forward() {
        assert_eq!(Sketchpad::parse("http://10.0.0.5:8790"), Some(Sketchpad::Url("http://10.0.0.5:8790".into())));
        // Only the word `ssh` makes a forward.
        assert_eq!(Sketchpad::parse("sshhub.example"), Some(Sketchpad::Url("sshhub.example".into())));
        assert_eq!(Sketchpad::parse("ssh archbox 192.168.0.162:8790"), forward("archbox", "192.168.0.162", 8790));
        assert_eq!(Sketchpad::parse("ssh  me@archbox\tlocalhost:80"), forward("me@archbox", "localhost", 80));
        assert_eq!(Sketchpad::parse("ssh archbox [::1]:8790"), forward("archbox", "[::1]", 8790));
    }

    #[test]
    fn an_ssh_line_that_is_not_a_forward_is_nothing() {
        for line in [
            "ssh",
            "ssh archbox",
            "ssh archbox 192.168.0.162",
            "ssh archbox 192.168.0.162:",
            "ssh archbox :8790",
            "ssh archbox 192.168.0.162:0",
            "ssh archbox 192.168.0.162:65536",
            "ssh archbox 192.168.0.162:+8790",
            "ssh archbox 192.168.0.162:http",
            "ssh archbox ::1:8790",
            "ssh archbox 192.168.0.162:8790 extra",
            "ssh -oProxyCommand=x 192.168.0.162:8790",
            "ssh archbox -oProxyCommand=x:8790",
            "ssh -N archbox 192.168.0.162:8790",
        ] {
            assert_eq!(Sketchpad::parse(line), None, "{line}");
        }
    }

    #[test]
    fn the_forward_has_its_own_connection_and_the_host_last() {
        let argv = forward_argv("archbox", "192.168.0.162", 8790, 8790);
        let mut wanted = vec!["ssh", "-N", "-L", "127.0.0.1:8790:192.168.0.162:8790"];
        if !cfg!(windows) {
            wanted.extend(["-o", "ControlMaster=no", "-o", "ControlPath=none"]);
        }
        wanted.extend(["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", "-o", "ExitOnForwardFailure=yes"]);
        wanted.extend(["-o", "ServerAliveInterval=15", "-o", "ServerAliveCountMax=3", "archbox"]);
        assert_eq!(argv, wanted);
        // Another local port changes the listening side only.
        assert_eq!(forward_argv("archbox", "192.168.0.162", 8790, 40123)[3], "127.0.0.1:40123:192.168.0.162:8790");
        // Nothing of the shared connection, whose forward would outlive the app.
        assert!(!argv.iter().any(|word| word.contains("ControlPersist") || word.contains("ControlMaster=auto")));
    }

    #[test]
    fn the_local_port_is_the_hubs_or_a_free_one() {
        let taken = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = taken.local_addr().unwrap().port();
        let other = local_port(port).unwrap();
        assert!(other != port && other != 0);
        drop(taken);
        assert_eq!(local_port(port).unwrap(), port);
    }

    /// A stand-in for ssh: `sh -c SCRIPT`, whatever the forward is.
    #[cfg(unix)]
    fn sh(script: &'static str) -> impl Fn(&str, &str, u16, u16) -> Vec<String> {
        move |_, _, _, _| crate::tmux::strings(&["sh", "-c", script])
    }

    #[cfg(unix)]
    #[test]
    fn an_ssh_that_exits_is_a_failure_with_its_last_line() {
        let forward = Forward::new("archbox", "192.168.0.162", 1);
        let failed = forward.shared.connect(&sh("echo first >&2; echo 'Permission denied (publickey).' >&2; echo >&2; exit 255"), READY_TIMEOUT);
        assert_eq!(failed, Err("sketchpad: ssh to archbox failed: Permission denied (publickey).".to_string()));
        assert!(forward.shared.lock().is_none(), "the dead ssh is reaped and gone");
        let silent = forward.shared.connect(&sh("exit 1"), READY_TIMEOUT);
        assert_eq!(silent, Err("sketchpad: ssh to archbox failed".to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn an_ssh_that_never_listens_is_no_answer_and_is_killed() {
        let forward = Forward::new("archbox", "192.168.0.162", 1);
        let deaf = forward.shared.connect(&sh("exec sleep 30"), Duration::from_millis(200));
        assert_eq!(deaf, Err("sketchpad: no answer through archbox".to_string()));
        assert!(forward.shared.lock().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn a_listening_forward_is_kept_until_the_drop_and_started_again_when_dead() {
        // The test listens where the stand-in would: the hub's port is free, so it is the local one.
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let forward = Forward::new("archbox", "192.168.0.162", port);
        let starts = std::sync::atomic::AtomicUsize::new(0);
        let listening = std::cell::RefCell::new(None);
        let argv = |_: &str, _: &str, _: u16, local: u16| {
            starts.fetch_add(1, Ordering::SeqCst);
            *listening.borrow_mut() = Some(TcpListener::bind((Ipv4Addr::LOCALHOST, local)).unwrap());
            crate::tmux::strings(&["sh", "-c", "exec sleep 30"])
        };
        assert_eq!(forward.shared.connect(&argv, READY_TIMEOUT), Ok(port));
        // Alive and answering: used as it is.
        assert_eq!(forward.shared.connect(&argv, READY_TIMEOUT), Ok(port));
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        // Dead: the next one starts another (on another port: the test still holds the first).
        let pid = forward.shared.lock().as_ref().unwrap().child.id();
        // SAFETY: kill takes no pointers; the child is not reaped, so the pid is its own.
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        while forward.shared.lock().as_mut().unwrap().alive() {
            std::thread::sleep(TICK);
        }
        let again = forward.shared.connect(&argv, READY_TIMEOUT).unwrap();
        assert_eq!(starts.load(Ordering::SeqCst), 2);
        assert_ne!(again, port);
        let shared = forward.shared.clone();
        drop(forward);
        assert!(shared.lock().is_none(), "dropped: ssh is killed and reaped");
        assert_eq!(shared.connect(&argv, READY_TIMEOUT), Err("sketchpad: no answer through archbox".to_string()));
        assert_eq!(starts.load(Ordering::SeqCst), 2, "and nothing is started after it");
    }

    #[cfg(unix)]
    #[test]
    fn a_second_open_while_one_is_at_work_starts_nothing() {
        let forward = Forward::new("archbox", "192.168.0.162", 1);
        forward.shared.busy.store(true, Ordering::SeqCst);
        assert!(!forward.open(|_| panic!("not called")));
        assert!(forward.shared.lock().is_none());
    }
}
