//! Local PTY (portable-pty). `PtyWriter` is the only path that writes to it (PLAN §5.1).
//!
//! The master is non-blocking and nothing here blocks in `read()` or `write()`: readers and the
//! writer thread wait in `poll()` together with an eventfd, so shutdown can always wake them.
//! A write the kernel will not take at once (a large paste, a child that does not read its
//! input) is queued and finished by the `s2-pty-write` thread, never by the caller.

use std::collections::VecDeque;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use anyhow::Context;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use crate::now_ns;

/// An eventfd: `signal` makes it readable (and so wakes a `poll`) until `clear`.
pub(crate) struct Wake(OwnedFd);

impl Wake {
    pub(crate) fn new() -> io::Result<Wake> {
        // SAFETY: eventfd has no preconditions; a non-negative result is a new fd we own.
        let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `fd` was just returned by eventfd and nothing else owns it.
        Ok(Wake(unsafe { OwnedFd::from_raw_fd(fd) }))
    }

    pub(crate) fn signal(&self) {
        let one = 1u64.to_ne_bytes();
        // SAFETY: writes 8 bytes from a live buffer to an fd we own. A failure can only be
        // EAGAIN (the counter is saturated), and then the fd is already readable.
        unsafe { libc::write(self.0.as_raw_fd(), one.as_ptr().cast(), one.len()) };
    }

    fn clear(&self) {
        let mut count = [0u8; 8];
        // SAFETY: reads 8 bytes into a live buffer from a non-blocking fd we own.
        unsafe { libc::read(self.0.as_raw_fd(), count.as_mut_ptr().cast(), count.len()) };
    }
}

/// Waits until `wake` is signalled or, when `io` is given, that fd reports one of the events
/// (or an error/hang-up). Returns `(io_ready, woken)`.
fn wait(io: Option<(RawFd, libc::c_short)>, wake: &Wake) -> io::Result<(bool, bool)> {
    let (fd, events) = io.unwrap_or((-1, 0)); // poll ignores a negative fd
    let mut fds = [
        libc::pollfd { fd: wake.0.as_raw_fd(), events: libc::POLLIN, revents: 0 },
        libc::pollfd { fd, events, revents: 0 },
    ];
    loop {
        // SAFETY: `fds` is a live array of two pollfd.
        if unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) } >= 0 {
            return Ok((fds[1].revents != 0, fds[0].revents != 0));
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            return Err(err);
        }
    }
}

fn dup(fd: RawFd) -> io::Result<OwnedFd> {
    // SAFETY: F_DUPFD_CLOEXEC on a valid fd returns a new fd we own, or -1.
    let new = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if new < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `new` was just returned by fcntl and nothing else owns it.
    Ok(unsafe { OwnedFd::from_raw_fd(new) })
}

fn set_nonblocking(fd: RawFd) -> io::Result<()> {
    // SAFETY: F_GETFL / F_SETFL on a valid fd have no memory preconditions.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Reads PTY output. `read` waits for data; it returns `Ok(0)` when the child side is closed
/// and, for the emulator's reader, when it was told to stop.
pub(crate) struct PtyReader {
    fd: OwnedFd,
    stop: Arc<Wake>,
}

impl PtyReader {
    /// Signalling this makes the current and every later `read` return `Ok(0)`.
    pub(crate) fn stopper(&self) -> Arc<Wake> {
        self.stop.clone()
    }
}

impl Read for PtyReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let (_, stopped) = wait(Some((self.fd.as_raw_fd(), libc::POLLIN)), &self.stop)?;
            if stopped {
                return Ok(0);
            }
            // SAFETY: reads at most `buf.len()` bytes into a live buffer.
            let n = unsafe { libc::read(self.fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n >= 0 {
                return Ok(n as usize);
            }
            let err = io::Error::last_os_error();
            match err.raw_os_error() {
                Some(libc::EAGAIN) | Some(libc::EINTR) => {}
                // What a PTY master reports once every slave fd is closed.
                Some(libc::EIO) => return Ok(0),
                _ => return Err(err),
            }
        }
    }
}

/// Called once the last byte of a write was accepted by the kernel, with the time the write
/// began and the time it finished.
pub(crate) type Written = Box<dyn FnOnce(u128, u128) + Send>;

struct Job {
    bytes: Vec<u8>,
    done: usize,
    t_began_ns: Option<u128>,
    written: Option<Written>,
}

struct WriteState {
    /// Dup of the master; `None` once the `Pty` is dropped.
    fd: Option<OwnedFd>,
    /// Writes in order. The front job may be partly written.
    queue: VecDeque<Job>,
    /// Copy of everything the PTY accepted, kept only after `start_recording` (tests, diagnostics).
    recorded: Option<Vec<u8>>,
}

struct WriterShared {
    state: Mutex<WriteState>,
    /// Wakes the writer thread: a job was queued behind a full PTY, or the PTY is closing.
    wake: Wake,
}

/// Clone + Send; keys, pastes and emulator replies all go through this one writer.
#[derive(Clone)]
pub struct PtyWriter {
    shared: Arc<WriterShared>,
}

/// Jobs that finished in one `pump`, with their begin and end times.
type Finished = Vec<(Written, u128, u128)>;

impl WriteState {
    /// Writes queued bytes until the queue is empty or the PTY takes no more. On a write error
    /// (the child side is gone) the PTY is closed and the queue dropped.
    fn pump(&mut self) -> Finished {
        let mut finished = Vec::new();
        let Some(fd) = self.fd.as_ref().map(AsRawFd::as_raw_fd) else { return finished };
        while let Some(job) = self.queue.front_mut() {
            let t_began_ns = *job.t_began_ns.get_or_insert_with(now_ns);
            let rest = &job.bytes[job.done..];
            // SAFETY: writes at most `rest.len()` bytes from a live buffer.
            let n = unsafe { libc::write(fd, rest.as_ptr().cast(), rest.len()) };
            if n < 0 {
                match io::Error::last_os_error().raw_os_error() {
                    Some(libc::EINTR) => continue,
                    Some(libc::EAGAIN) => break,
                    _ => {
                        self.fd = None;
                        self.queue.clear();
                        break;
                    }
                }
            }
            if n == 0 {
                break; // nothing taken and no error: treat as full
            }
            let n = n as usize;
            if let Some(rec) = self.recorded.as_mut() {
                rec.extend_from_slice(&rest[..n]);
            }
            job.done += n;
            if job.done == job.bytes.len() {
                let job = self.queue.pop_front().expect("the front job");
                if let Some(written) = job.written {
                    finished.push((written, t_began_ns, now_ns()));
                }
            }
        }
        finished
    }
}

fn notify(finished: Finished) {
    for (written, t_began_ns, t_done_ns) in finished {
        written(t_began_ns, t_done_ns);
    }
}

impl PtyWriter {
    /// Sends all of `bytes`, in order after every earlier write, and never blocks: what the PTY
    /// takes now is written before this returns, the rest is queued and written by the
    /// `s2-pty-write` thread as the child reads. Fails only when the PTY is closed.
    pub fn write_all(&self, bytes: &[u8]) -> io::Result<()> {
        self.write_then(bytes.to_vec(), None)
    }

    /// `write_all`, and `written` is called (on this thread or the writer thread) once the last
    /// byte was accepted. It is dropped uncalled if the PTY closes first.
    pub(crate) fn write_then(&self, bytes: Vec<u8>, written: Option<Written>) -> io::Result<()> {
        let (finished, queued) = {
            let mut state = self.shared.state.lock().unwrap();
            if state.fd.is_none() {
                return Err(io::ErrorKind::BrokenPipe.into());
            }
            let idle = state.queue.is_empty();
            state.queue.push_back(Job { bytes, done: 0, t_began_ns: None, written });
            // Behind a queued job the order must hold, and the writer thread is already waiting
            // for the PTY; otherwise write here, so a key costs no thread hop.
            let finished = if idle { state.pump() } else { Vec::new() };
            (finished, idle && !state.queue.is_empty())
        };
        notify(finished);
        if queued {
            self.shared.wake.signal();
        }
        Ok(())
    }

    pub fn start_recording(&self) {
        self.shared.state.lock().unwrap().recorded.get_or_insert_with(Vec::new);
    }

    /// The bytes the PTY has accepted since `start_recording`.
    pub fn recorded(&self) -> Vec<u8> {
        self.shared.state.lock().unwrap().recorded.clone().unwrap_or_default()
    }
}

/// The writer thread: finishes queued writes as the PTY becomes writable; ends when the `Pty`
/// is dropped or the child side is gone.
fn write_queued(shared: &WriterShared) {
    loop {
        let waiting = {
            let state = shared.state.lock().unwrap();
            let Some(fd) = state.fd.as_ref() else { return };
            // With nothing queued only the wake is watched: a hung-up master always polls ready.
            (!state.queue.is_empty()).then(|| (fd.as_raw_fd(), libc::POLLOUT))
        };
        match wait(waiting, &shared.wake) {
            Ok((_, woken)) => {
                if woken {
                    shared.wake.clear();
                }
            }
            Err(_) => return,
        }
        let finished = shared.state.lock().unwrap().pump();
        notify(finished);
    }
}

pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: PtyWriter,
    write_thread: Option<JoinHandle<()>>,
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize { rows, cols, pixel_width: 0, pixel_height: 0 }
}

impl Pty {
    pub fn spawn(cmd: &[String], cols: u16, rows: u16) -> anyhow::Result<Pty> {
        let (program, args) = cmd.split_first().context("empty command")?;
        let pair = native_pty_system().openpty(size(cols, rows))?;
        let mut builder = CommandBuilder::new(program);
        builder.args(args);
        builder.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(builder)?;
        drop(pair.slave);
        let master_fd = pair.master.as_raw_fd().context("the PTY master has no file descriptor")?;
        set_nonblocking(master_fd)?;
        let state = WriteState { fd: Some(dup(master_fd)?), queue: VecDeque::new(), recorded: None };
        let shared = Arc::new(WriterShared { state: Mutex::new(state), wake: Wake::new()? });
        let for_thread = shared.clone();
        let write_thread =
            std::thread::Builder::new().name("s2-pty-write".into()).spawn(move || write_queued(&for_thread))?;
        Ok(Pty { master: pair.master, child, writer: PtyWriter { shared }, write_thread: Some(write_thread) })
    }

    pub fn writer(&self) -> PtyWriter {
        self.writer.clone()
    }

    /// PTY output. `read` waits for data and returns `Ok(0)` once the child side is closed.
    pub fn reader(&self) -> anyhow::Result<Box<dyn Read + Send>> {
        Ok(Box::new(self.stoppable_reader()?))
    }

    pub(crate) fn stoppable_reader(&self) -> anyhow::Result<PtyReader> {
        let master_fd = self.master.as_raw_fd().context("the PTY master has no file descriptor")?;
        Ok(PtyReader { fd: dup(master_fd)?, stop: Arc::new(Wake::new()?) })
    }

    pub fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.master.resize(size(cols, rows))
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // Close the writer's fd (queued input is dropped) and end its thread, which waits in
        // `poll` and so wakes even when the child never reads.
        let fd = {
            let mut state = self.writer.shared.state.lock().unwrap();
            state.queue.clear();
            state.fd.take()
        };
        self.writer.shared.wake.signal();
        if let Some(thread) = self.write_thread.take() {
            let _ = thread.join();
        }
        drop(fd);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
