//! Near end of a path: spawn the path command in a PTY, run nav, type numbered events, record acks.
//!
//! Order: spawn → nav (if any) → wait for the ready marker → event loop → settle (acks) → wait for
//! `S0DONE` → log → kill child.
//! Ready gate: with `--ready-token T` only the exact text `S0READY:T` counts (plain `S0READY` does
//! not), and only if it arrives in a read made after the last nav key was written — so a screen
//! that showed the marker before/while navigating (e.g. a preview) cannot open the gate.
//! A reader thread drains the PTY master into an [`AckScanner`] (acks timestamped at read time) and
//! keeps an escape-stripped copy of the output for `expect:` nav steps and `--dump-screen`.
use crate::{ack::{AckScanner, Stripper}, event, log::{self, Jsonl, SendRecord}, nav::{self, Step}};
use anyhow::{bail, Context};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{atomic::{AtomicBool, AtomicU64, Ordering}, mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Opts {
    pub log: PathBuf, pub events: u32, pub rate: f64, pub cols: u16, pub rows: u16,
    pub nav: Option<String>, pub dump_screen: bool, pub settle_ms: u64, pub ready_timeout_ms: u64,
    /// Pause between an event's payload and its marker (0 = none). Control for parsers that
    /// accumulate an escape sequence across bytes that arrive in one read (e.g. `ESC a#000002`).
    pub marker_gap_ms: u64,
    /// Accept only `S0READY:<token>` as the ready marker.
    pub ready_token: Option<String>,
    pub cmd: Vec<String>,
}

/// The writes for `seq`, each with the sleep (ms) before it: payload write(s) then the marker.
/// Repeat keeps 2 ms between its five bytes. The marker waits `marker_gap_ms` if > 0, else the
/// kind's normal spacing (2 ms after a Repeat byte, 0 otherwise).
pub fn write_plan(seq: u32, marker_gap_ms: u64) -> Vec<(Vec<u8>, u64)> {
    let chunks = event::encode(seq);
    let last = chunks.len() - 1;
    let spacing = if event::kind_of(seq) == event::Kind::Repeat { 2 } else { 0 };
    chunks.into_iter().enumerate().map(|(j, c)| {
        let ms = if j == 0 { 0 } else if j == last && marker_gap_ms > 0 { marker_gap_ms } else { spacing };
        (c, ms)
    }).collect()
}

/// Escape-stripped output kept for `expect:` and `--dump-screen` (bounded).
const SCREEN_CAP: usize = 256 * 1024;
const DUMP_CHARS: usize = 4000;

/// Write all of `buf`, retrying on short writes, `WouldBlock` and `Interrupted` (1 ms backoff).
pub fn write_full(w: &mut dyn Write, buf: &[u8]) -> std::io::Result<()> {
    let mut off = 0;
    while off < buf.len() {
        match w.write(&buf[off..]) {
            Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
            Ok(k) => off += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(1)),
            Err(e) => return Err(e),
        }
    }
    w.flush()
}

/// State shared between the reader thread and the main loop.
struct Shared { screen: Mutex<String>, done: AtomicBool, eof: AtomicBool,
                /// Monotonic clock origin for `ready_at` / the nav cut.
                base: Instant,
                /// 1 + ns since `base` of the most recent read that completed the ready marker;
                /// 0 = never seen.
                ready_at: AtomicU64,
                /// Wall clock (ns) of the read in which `S0DONE` was first seen.
                done_ns: Mutex<Option<u128>> }

impl Shared {
    fn mono_ns(&self) -> u64 { self.base.elapsed().as_nanos() as u64 + 1 }
    fn contains(&self, s: &str) -> bool { self.screen.lock().unwrap().contains(s) }
    fn tail(&self) -> String {
        let s = self.screen.lock().unwrap();
        let n = s.chars().count();
        s.chars().skip(n.saturating_sub(DUMP_CHARS)).collect()
    }
    fn dump(&self, why: &str) {
        eprintln!("---- s0 send: screen ({why}), last {DUMP_CHARS} chars ----\n{}\n---- end screen ----", self.tail());
    }
}

fn spawn_reader(mut r: Box<dyn Read + Send>, sh: Arc<Shared>, acks: mpsc::Sender<(u32, u128)>,
                ready_token: Option<String>) -> mpsc::Receiver<()> {
    let (fin_tx, fin_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut scan = AckScanner::with_ready_token(ready_token.as_deref());
        let mut strip = Stripper::new();
        let mut buf = vec![0u8; 65536];
        let mut plain = Vec::new();
        loop {
            let n = match r.read(&mut buf) {
                Ok(0) => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break, // EIO once the child side is gone
                Ok(n) => n,
            };
            let t = log::now_ns();
            let t_mono = sh.mono_ns(); // taken before scanning: bytes read before a nav key stay before it
            for seq in scan.feed(&buf[..n]) { let _ = acks.send((seq, t)); }
            plain.clear();
            strip.feed_into(&buf[..n], &mut plain);
            {
                let mut s = sh.screen.lock().unwrap();
                s.push_str(&String::from_utf8_lossy(&plain));
                if s.len() > SCREEN_CAP {
                    let mut cut = s.len() - SCREEN_CAP / 2;
                    while !s.is_char_boundary(cut) { cut += 1; }
                    s.drain(..cut);
                }
            }
            if scan.ready_in_last_feed { sh.ready_at.store(t_mono, Ordering::Release); }
            if scan.saw_done && !sh.done.load(Ordering::Acquire) {
                *sh.done_ns.lock().unwrap() = Some(t);
                sh.done.store(true, Ordering::Release);
            }
        }
        sh.eof.store(true, Ordering::Release);
        let _ = fin_tx.send(());
    });
    fin_rx
}

/// Poll `cond` every 10 ms until true or `timeout`.
fn wait_for(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + timeout;
    loop {
        if cond() { return true; }
        if Instant::now() >= end { return false; }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Runs the nav script. Returns the monotonic time (see [`Shared::mono_ns`]) just after the last
/// key/text write, or 0 if nav wrote nothing: only ready sightings later than that count.
fn run_nav(steps: &[Step], w: &mut dyn Write, sh: &Shared) -> anyhow::Result<u64> {
    let mut cut = 0;
    for st in steps {
        match st {
            Step::Wait(ms) => std::thread::sleep(Duration::from_millis(*ms)),
            Step::Text(t) => { write_full(w, t.as_bytes())?; cut = sh.mono_ns(); }
            Step::Key(k) => { write_full(w, nav::key_bytes(k))?; cut = sh.mono_ns(); }
            Step::ExpectText(t, ms) => {
                let found = {
                    let end = Instant::now() + Duration::from_millis(*ms);
                    loop {
                        if sh.contains(t) { break true; }
                        if Instant::now() >= end { break false; }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                };
                if !found { sh.dump("expect timeout"); bail!("nav: {t:?} not on screen within {ms} ms"); }
            }
        }
    }
    Ok(cut)
}

/// What the run produced so far; logged even when the run fails part-way.
#[derive(Default)]
struct Progress { recs: Vec<SendRecord>, ack_ns: Vec<Option<u128>>, acked: usize }

impl Progress {
    fn drain(&mut self, acks: &mpsc::Receiver<(u32, u128)>) {
        while let Ok((seq, t)) = acks.try_recv() {
            if let Some(slot) = self.ack_ns.get_mut(seq as usize) {
                if slot.is_none() { *slot = Some(t); self.acked += 1; }
            }
        }
    }
    /// events / (last write_end - first write_start), in Hz; None with fewer than 2 events.
    fn effective_rate(&self) -> Option<f64> {
        let (f, l) = (self.recs.first()?, self.recs.last()?);
        let secs = l.write_end_ns.checked_sub(f.write_start_ns)? as f64 / 1e9;
        (self.recs.len() >= 2 && secs > 0.0).then(|| self.recs.len() as f64 / secs)
    }
}

pub fn main(o: Opts) -> anyhow::Result<()> {
    anyhow::ensure!(o.rate > 0.0, "--rate must be > 0");
    anyhow::ensure!(o.events <= 1_000_000, "--events must be <= 1000000 (6-digit marker)");
    anyhow::ensure!(!o.cmd.is_empty(), "no command given after --");
    if let Some(t) = &o.ready_token { crate::recv::check_token(t)?; }
    let steps = match o.nav.as_deref() { Some(s) => nav::parse(s)?, None => Vec::new() };
    let mut log_out = Jsonl::create(&o.log)?;

    let pair = native_pty_system()
        .openpty(PtySize { rows: o.rows, cols: o.cols, pixel_width: 0, pixel_height: 0 })
        .context("openpty")?;
    let mut cmd = CommandBuilder::new(&o.cmd[0]);
    cmd.args(&o.cmd[1..]);
    // portable-pty defaults the child cwd to $HOME (and resolves the program against it).
    cmd.cwd(std::env::current_dir()?);
    cmd.env("TERM", "xterm-256color");
    let mut child = pair.slave.spawn_command(cmd).with_context(|| format!("spawn {:?}", o.cmd))?;
    drop(pair.slave);
    let mut writer = pair.master.take_writer()?;
    let reader = pair.master.try_clone_reader()?;
    let sh = Arc::new(Shared { screen: Mutex::new(String::new()), base: Instant::now(), ready_at: AtomicU64::new(0),
                               done: AtomicBool::new(false), eof: AtomicBool::new(false),
                               done_ns: Mutex::new(None) });
    let (ack_tx, ack_rx) = mpsc::channel();
    let fin_rx = spawn_reader(reader, sh.clone(), ack_tx, o.ready_token.clone());

    let mut p = Progress { ack_ns: vec![None; o.events as usize], ..Default::default() };
    let result = drive(&o, &steps, &mut *writer, &sh, &ack_rx, &mut p);
    let _ = child.kill();
    let _ = child.wait();
    drop(writer);
    let _ = fin_rx.recv_timeout(Duration::from_secs(2));
    p.drain(&ack_rx);
    for r in &mut p.recs { r.ack_ns = p.ack_ns[r.seq as usize]; }
    for r in &p.recs { log_out.write(r)?; }
    // Time from the last event's write_end to the read that carried S0DONE.
    let done_seen_ms = match (*sh.done_ns.lock().unwrap(), p.recs.last()) {
        (Some(d), Some(l)) => Some((d as f64 - l.write_end_ns as f64) / 1e6),
        _ => None,
    };
    let mut done = serde_json::json!({"done": true, "acked": p.acked, "saw_done": sh.done.load(Ordering::Acquire),
                                      "done_seen_ms": done_seen_ms,
                                      "events": o.events, "sent": p.recs.len(), "rate": o.rate,
                                      "effective_rate": p.effective_rate(), "marker_gap_ms": o.marker_gap_ms,
                                      "ready_token": o.ready_token.is_some(),
                                      "cmd": o.cmd});
    if let Err(e) = &result { done["error"] = format!("{e:#}").into(); }
    log_out.write(&done)?;
    log_out.flush()?;
    result
}

/// Nav, readiness wait, paced event loop and settle. Records go into `p` as they are made, so a
/// failure part-way (e.g. EIO when ssh drops) still leaves them for the log.
fn drive(o: &Opts, steps: &[Step], w: &mut dyn Write, sh: &Shared, acks: &mpsc::Receiver<(u32, u128)>,
         p: &mut Progress) -> anyhow::Result<()> {
    let cut = run_nav(steps, w, sh)?;
    if o.dump_screen { sh.dump("after nav"); }
    let marker = crate::ack::ready_text(o.ready_token.as_deref());
    // Only a sighting from a read made after the last nav key counts (earlier ones are ignored).
    if !wait_for(Duration::from_millis(o.ready_timeout_ms), || sh.ready_at.load(Ordering::Acquire) > cut) {
        sh.dump("ready timeout");
        bail!("{marker} not seen (after the last nav key) within {} ms", o.ready_timeout_ms);
    }

    let n = o.events as usize;
    let period = Duration::from_secs_f64(1.0 / o.rate);
    let mut base = Instant::now();
    for seq in 0..o.events {
        // Absolute schedule base + seq/rate; if behind, rebase to now rather than bursting.
        let target = base + period.mul_f64(seq as f64);
        let now = Instant::now();
        if target > now { std::thread::sleep(target - now); } else { base = now - period.mul_f64(seq as f64); }
        // The path closed its side (exited / connection dropped): writes would vanish or block.
        if sh.eof.load(Ordering::Acquire) { bail!("PTY closed by path command before seq {seq}"); }
        let kind = event::kind_of(seq);
        let write_start_ns = log::now_ns();
        for (c, ms) in write_plan(seq, o.marker_gap_ms) {
            if ms > 0 { std::thread::sleep(Duration::from_millis(ms)); }
            write_full(w, &c).with_context(|| format!("write seq {seq}"))?;
        }
        let write_end_ns = log::now_ns();
        p.recs.push(SendRecord { seq, kind, write_start_ns, write_end_ns, ack_ns: None });
        p.drain(acks);
    }
    // Settle: keep draining until the receiver says done, everything is acked, or timeout.
    wait_for(Duration::from_millis(o.settle_ms), || {
        p.drain(acks);
        sh.done.load(Ordering::Acquire) || p.acked >= n
    });
    // No S0DONE yet. If everything is acked the receiver has (almost) finished: it flushes its done
    // line before emitting S0DONE, so give it at most 1 s. If settle ran out with acks missing, the
    // receiver may still be logging late arrivals: wait up to another settle window before the kill.
    let done_wait = if p.acked >= n { o.settle_ms.min(1000) } else { o.settle_ms };
    wait_for(Duration::from_millis(done_wait), || {
        p.drain(acks);
        sh.done.load(Ordering::Acquire) || sh.eof.load(Ordering::Acquire)
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Writes at most one byte per call and returns WouldBlock on every third call.
    struct Stingy { got: Vec<u8>, calls: u32 }
    impl std::io::Write for Stingy {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.calls += 1;
            if self.calls % 3 == 0 { return Err(std::io::ErrorKind::WouldBlock.into()); }
            if b.is_empty() { return Ok(0); }
            self.got.push(b[0]); Ok(1)
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    #[test]
    fn write_plan_no_gap_matches_encode() {
        for seq in 0..16u32 {
            let plan = write_plan(seq, 0);
            let chunks: Vec<Vec<u8>> = plan.iter().map(|(c, _)| c.clone()).collect();
            assert_eq!(chunks, event::encode(seq), "seq {seq}");
            assert_eq!(plan[0].1, 0, "first write never waits");
            let want_rest = if event::kind_of(seq) == event::Kind::Repeat { 2 } else { 0 };
            assert!(plan[1..].iter().all(|(_, ms)| *ms == want_rest), "seq {seq}: {plan:?}");
        }
    }
    #[test]
    fn write_plan_marker_gap() {
        // AltLetter: payload ESC a, then the marker 30 ms later.
        let plan = write_plan(2, 30);
        assert_eq!(plan, vec![(event::expected_payload(2), 0), (event::marker(2), 30)]);
        // Repeat keeps 2 ms between its five bytes; only the marker gets the gap.
        let plan = write_plan(6, 30);
        assert_eq!(plan.len(), 6);
        assert_eq!(plan.iter().map(|(_, ms)| *ms).collect::<Vec<_>>(), vec![0, 2, 2, 2, 2, 30]);
        assert_eq!(plan.last().unwrap().0, event::marker(6));
    }
    #[test]
    fn short_write_loop() {
        let mut w = Stingy { got: Vec::new(), calls: 0 };
        let buf = b"\x1b[200~p0001\x1b[201~#000007";
        write_full(&mut w, buf).unwrap();
        assert_eq!(w.got, buf);
        assert!(w.calls > buf.len() as u32);
    }
}
