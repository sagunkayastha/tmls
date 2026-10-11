//! `--bench` sampler: process CPU %, RSS, frames/s and (if `nvidia-smi` is on PATH) GPU
//! utilisation and power, one `sample` line every 500 ms.

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::latency::{Event, Log};
use crate::now_ns;

const PERIOD: Duration = Duration::from_millis(500);

pub struct Sampler {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

/// utime + stime of this process, in clock ticks.
fn cpu_ticks() -> Option<u64> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    // Fields after the ")" that closes comm; utime and stime are the 14th and 15th overall.
    let rest = stat.rsplit_once(')')?.1;
    let mut fields = rest.split_whitespace().skip(11);
    Some(fields.next()?.parse::<u64>().ok()? + fields.next()?.parse::<u64>().ok()?)
}

fn rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// (utilisation %, power W) of the first GPU, or `None` without a working `nvidia-smi`.
fn gpu() -> Option<(f64, f64)> {
    let out = Command::new("nvidia-smi")
        .args(["--query-gpu=utilization.gpu,power.draw", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let (util, watts) = text.lines().next()?.split_once(',')?;
    Some((util.trim().parse().ok()?, watts.trim().parse().ok()?))
}

impl Sampler {
    pub fn start(log: Log) -> Sampler {
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::spawn(move || {
            // SAFETY: sysconf with a valid name has no preconditions.
            let ticks_per_sec = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as f64;
            let (mut last_t, mut last_cpu, mut last_frames) = (now_ns(), cpu_ticks().unwrap_or(0), log.frames_logged());
            while !stopped.load(Ordering::Relaxed) {
                std::thread::sleep(PERIOD);
                let (t, cpu, frames) = (now_ns(), cpu_ticks().unwrap_or(last_cpu), log.frames_logged());
                let secs = (t - last_t) as f64 / 1e9;
                let gpu = gpu();
                log.event(&Event::Sample {
                    t_ns: t as u64,
                    cpu_pct: (cpu - last_cpu) as f64 / ticks_per_sec / secs * 100.0,
                    rss_kb: rss_kb().unwrap_or(0),
                    fps: (frames - last_frames) as f64 / secs,
                    gpu_util: gpu.map(|g| g.0),
                    gpu_w: gpu.map(|g| g.1),
                });
                log.flush();
                (last_t, last_cpu, last_frames) = (t, cpu, frames);
            }
        });
        Sampler { stop, thread: Some(thread) }
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
