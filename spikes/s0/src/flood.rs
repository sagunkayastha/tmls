//! Bounded-rate output flood: 100-byte coloured lines, paced every 10 ms.
use std::io::Write;
use std::time::{Duration, Instant};

/// One flood line: SGR colour (5 bytes) + 88-byte body starting `F` + reset (4) + CRLF = 99..100 bytes.
pub fn line(i: u64) -> Vec<u8> {
    let body = format!("F{i:08} {}", "x".repeat(70));
    format!("\x1b[3{}m{body:<88}\x1b[0m\r\n", (i % 7) + 1).into_bytes()
}

/// Writes `bytes_per_sec` to `out` until `stop` is set. Blocks on write; the caller decides
/// which thread/fd this runs on.
pub fn run(mut out: impl Write, bytes_per_sec: u64, stop: &std::sync::atomic::AtomicBool) {
    let tick = Duration::from_millis(10);
    let per_tick = (bytes_per_sec / 100).max(1);
    let mut i = 0u64;
    let mut next = Instant::now();
    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
        let mut sent = 0u64;
        while sent < per_tick {
            let l = line(i); i += 1; sent += l.len() as u64;
            if out.write_all(&l).is_err() { return; }
        }
        let _ = out.flush();
        next += tick;
        if let Some(d) = next.checked_duration_since(Instant::now()) { std::thread::sleep(d); }
    }
}

pub fn main(bytes_per_sec: u64) {
    let stop = std::sync::atomic::AtomicBool::new(false);
    run(std::io::stdout().lock(), bytes_per_sec, &stop);
}
