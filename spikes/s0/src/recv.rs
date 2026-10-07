//! Far end of a path. Raw-mode stdin → tokenizer → JSONL log + ack on stdout.
//! Parsing never waits on stdout: acks and flood lines go through one channel drained by a
//! writer thread; if the channel is full the ack is dropped (counted) but the arrival is logged.
use crate::{log::{self, Jsonl, RecvRecord}, tokenizer::Tokenizer};
use nix::sys::termios::{self, InputFlags, LocalFlags, OutputFlags, SetArg, SpecialCharacterIndices};
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc, Arc};

pub struct Opts { pub log: std::path::PathBuf,
                 /// Flood stdout at this many bytes/s, starting at the first arrival (not before, so the
                 /// screen stays quiet and `S0READY` stays visible until a TUI has attached).
                 pub flood: Option<u64>, pub ack: bool,
                 /// N = events: exit once every seq in `0..N` has arrived at least once (duplicates and
                 /// out-of-range seqs are logged but do not count).
                 pub quit_after: Option<u64>,
                 /// Capacity of the stdout channel (acks + flood lines). Default 4096.
                 pub ack_queue: usize,
                 /// Print `S0READY:<token>` instead of `S0READY`, so the sender can tell this
                 /// receiver from any other screen that happens to show the plain marker.
                 pub ready_token: Option<String> }

/// A ready token must be short plain text (it is matched in escape-stripped screen output).
pub fn check_token(t: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!t.is_empty() && t.len() <= 64 && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
                    "--ready-token must be 1-64 chars of [A-Za-z0-9_-]");
    Ok(())
}

pub fn main(o: Opts) -> anyhow::Result<()> {
    if let Some(t) = &o.ready_token { check_token(t)?; }
    let ready_line = format!("\x1b[?2004h{}\r\n", crate::ack::ready_text(o.ready_token.as_deref())).into_bytes();
    // Open the log first: a bad --log path must fail before the tty goes raw or we signal ready.
    let mut log = Jsonl::create(&o.log)?;
    let stdin = std::io::stdin();
    // Not a TTY (pipe/file) → no termios; proceed without raw mode.
    let saved = termios::tcgetattr(stdin.as_fd()).ok();
    if let Some(s) = &saved {
        let mut raw = s.clone();
        raw.local_flags.remove(LocalFlags::ECHO | LocalFlags::ICANON | LocalFlags::ISIG | LocalFlags::IEXTEN);
        raw.input_flags.remove(InputFlags::IXON | InputFlags::ICRNL | InputFlags::INLCR | InputFlags::BRKINT | InputFlags::ISTRIP);
        raw.output_flags.remove(OutputFlags::OPOST);
        raw.control_chars[SpecialCharacterIndices::VMIN as usize] = 1;
        raw.control_chars[SpecialCharacterIndices::VTIME as usize] = 0;
        termios::tcsetattr(stdin.as_fd(), SetArg::TCSANOW, &raw)?;
    }
    // Writer thread owns stdout. Its first bytes ask the outer terminal for bracketed paste
    // (so a paste comes through as 200~/201~), then print the plain-text readiness marker
    // `S0READY` / `S0READY:<token>` (survives pyte/tmls rendering, unlike the escape). Spawned
    // only after raw mode.
    let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(o.ack_queue.max(1));
    let stop = Arc::new(AtomicBool::new(false));
    let writer = std::thread::spawn(move || {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(&ready_line);
        let _ = out.flush();
        for chunk in rx { if out.write_all(&chunk).is_err() { break; } let _ = out.flush(); }
    });
    // Started lazily by the parse loop on the first arrival.
    let start_flood = |bps: u64| {
        let tx = tx.clone(); let stop = stop.clone();
        std::thread::spawn(move || {
            /// Flood lines go through the same channel; blocking send is fine on this thread.
            struct Chan(mpsc::SyncSender<Vec<u8>>);
            impl Write for Chan {
                fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                    self.0.send(b.to_vec()).map(|_| b.len()).map_err(|_| std::io::ErrorKind::BrokenPipe.into())
                }
                fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
            }
            crate::flood::run(Chan(tx), bps, &stop);
        });
    };
    let mut flood_pending = o.flood;
    // Distinct seqs seen in 0..quit_after.
    // (Markers have six digits, so no seq is >= 1_000_000.)
    let mut seen = vec![false; o.quit_after.unwrap_or(0).min(1_000_000) as usize];
    let mut distinct = 0u64;
    let mut tok = Tokenizer::new();
    let mut buf = [0u8; 65536];
    let mut idx = 0u64; let mut dropped_acks = 0u64;
    let mut input = stdin.lock();
    let mut exit = "eof";
    'outer: loop {
        let n = match input.read(&mut buf) {
            Ok(0) => break,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => { exit = "error"; break }
            Ok(n) => n,
        };
        let before = idx;
        let wall = log::now_ns();
        for a in tok.feed(&buf[..n]) {
            log.write(&RecvRecord { seq: a.seq, intact: a.intact, got_hex: log::hex(&a.got), wall_ns: wall, idx })?;
            idx += 1;
            if o.ack && tx.try_send(format!("A{:06}\r\n", a.seq).into_bytes()).is_err() { dropped_acks += 1; }
            if let Some(bps) = flood_pending.take() { start_flood(bps); }
            if let Some(slot) = seen.get_mut(a.seq as usize) {
                if !*slot { *slot = true; distinct += 1; }
            }
            if o.quit_after.is_some_and(|q| distinct >= q) { exit = "quit_after"; break 'outer; }
        }
        // Flush every chunk that produced arrivals, so a killed/HUP'd receiver loses nothing
        // that was already parsed (missing log lines would read as key loss).
        if idx > before { log.flush()?; }
    }
    drop(input);
    stop.store(true, Ordering::Relaxed);
    log.write(&serde_json::json!({"done": true, "arrivals": idx, "distinct_in_range": distinct, "dropped_acks": dropped_acks, "exit": exit}))?;
    log.flush()?;
    let _ = tx.send(b"\x1b[?2004l\r\nS0DONE\r\n".to_vec());
    drop(tx);
    let _ = writer.join();
    if let Some(s) = saved { let _ = termios::tcsetattr(std::io::stdin().as_fd(), SetArg::TCSANOW, &s); }
    Ok(())
}
