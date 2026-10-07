//! Far end of a path. Raw-mode stdin → tokenizer → JSONL log + ack on stdout.
//! Parsing never waits on stdout: acks and flood lines go through one channel drained by a
//! writer thread; if the channel is full the ack is dropped (counted) but the arrival is logged.
use crate::{log::{self, Jsonl, RecvRecord}, tokenizer::Tokenizer};
use nix::sys::termios::{self, InputFlags, LocalFlags, OutputFlags, SetArg, SpecialCharacterIndices};
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc, Arc};

pub struct Opts { pub log: std::path::PathBuf, pub flood: Option<u64>, pub ack: bool, pub quit_after: Option<u64> }

pub fn main(o: Opts) -> anyhow::Result<()> {
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
    // (so a paste comes through as 200~/201~); they also tell a harness that raw mode is on.
    let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(4096);
    let stop = Arc::new(AtomicBool::new(false));
    let writer = std::thread::spawn(move || {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(b"\x1b[?2004h");
        let _ = out.flush();
        for chunk in rx { if out.write_all(&chunk).is_err() { break; } let _ = out.flush(); }
    });
    if let Some(bps) = o.flood {
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
    }
    let mut log = Jsonl::create(&o.log)?;
    let mut tok = Tokenizer::new();
    let mut buf = [0u8; 65536];
    let mut idx = 0u64; let mut dropped_acks = 0u64;
    let mut input = stdin.lock();
    'outer: loop {
        let n = match input.read(&mut buf) { Ok(0) | Err(_) => break, Ok(n) => n };
        let wall = log::now_ns();
        for a in tok.feed(&buf[..n]) {
            log.write(&RecvRecord { seq: a.seq, intact: a.intact, got_hex: log::hex(&a.got), wall_ns: wall, idx })?;
            idx += 1;
            if o.ack && tx.try_send(format!("A{:06}\r\n", a.seq).into_bytes()).is_err() { dropped_acks += 1; }
            if o.quit_after.is_some_and(|q| idx >= q) { break 'outer; }
        }
        if idx % 256 == 0 { log.flush()?; }
    }
    drop(input);
    stop.store(true, Ordering::Relaxed);
    log.write(&serde_json::json!({"done": true, "arrivals": idx, "dropped_acks": dropped_acks}))?;
    log.flush()?;
    let _ = tx.send(b"\x1b[?2004l\r\nS0DONE\r\n".to_vec());
    drop(tx);
    let _ = writer.join();
    if let Some(s) = saved { let _ = termios::tcsetattr(std::io::stdin().as_fd(), SetArg::TCSANOW, &s); }
    Ok(())
}
