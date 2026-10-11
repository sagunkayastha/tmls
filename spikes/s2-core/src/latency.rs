//! Latency log (`<dir>/latency.jsonl`) and its percentile report.
//! One JSON object per line with an `"ev"` discriminator; all times are ns from `now_ns()`.
//! Schema: `docs/superpowers/plans/2026-10-08-s2-split-overview.md`.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "ev", rename_all = "snake_case")]
pub enum Event {
    Meta { app: String, git: String, profile: String, hz: f64, scale: f64, present_mode: String, load: String },
    Key { id: u64, t_key_ns: u64 },
    Write { id: u64, t_write_ns: u64 },
    Frame {
        seq: u64,
        t_output_ns: u64,
        t_submit_ns: u64,
        t_present_ns: u64,
        /// `present_return` | `work_done` | `frame_callback`. None of these is photon time.
        present_kind: String,
        keys: Vec<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        t_work_done_ns: Option<u64>,
    },
    Sample { t_ns: u64, cpu_pct: f64, rss_kb: u64, fps: f64, gpu_util: Option<f64>, gpu_w: Option<f64> },
}

struct Inner {
    out: BufWriter<File>,
    /// Frames the front-end took but has not presented yet: seq → (t_output_ns, keys).
    taken: HashMap<u64, (u64, Vec<u64>)>,
    frames_logged: u64,
}

/// Clone + Send; the front-end and the emulator thread share one log.
#[derive(Clone)]
pub struct Log {
    inner: Arc<Mutex<Inner>>,
}

impl Log {
    /// Creates `<dir>/latency.jsonl` (and `dir`).
    pub fn create(dir: &Path) -> anyhow::Result<Log> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join("latency.jsonl");
        let file = File::create(&path).with_context(|| format!("create {}", path.display()))?;
        let inner = Inner { out: BufWriter::new(file), taken: HashMap::new(), frames_logged: 0 };
        Ok(Log { inner: Arc::new(Mutex::new(inner)) })
    }

    pub fn event(&self, ev: &Event) {
        let mut inner = self.inner.lock().unwrap();
        if serde_json::to_writer(&mut inner.out, ev).is_ok() {
            let _ = inner.out.write_all(b"\n");
        }
    }

    pub fn key(&self, id: u64, t_key_ns: u128) {
        self.event(&Event::Key { id, t_key_ns: t_key_ns as u64 });
    }

    pub fn write(&self, id: u64, t_write_ns: u128) {
        self.event(&Event::Write { id, t_write_ns: t_write_ns as u64 });
    }

    pub(crate) fn frame_taken(&self, seq: u64, t_output_ns: u128, keys: &[u64]) {
        self.inner.lock().unwrap().taken.insert(seq, (t_output_ns as u64, keys.to_vec()));
    }

    /// One line per rendered frame. `seq` is the taken `Frame::seq`; its output time and keys
    /// were recorded when the front-end took it from the slot.
    pub fn frame(&self, seq: u64, t_submit_ns: u128, t_present_ns: u128, t_present_kind: &str) {
        self.frame_with_work_done(seq, t_submit_ns, t_present_ns, t_present_kind, None);
    }

    pub fn frame_with_work_done(
        &self,
        seq: u64,
        t_submit_ns: u128,
        t_present_ns: u128,
        t_present_kind: &str,
        t_work_done_ns: Option<u128>,
    ) {
        let (t_output_ns, keys) = self.inner.lock().unwrap().taken.remove(&seq).unwrap_or_default();
        self.event(&Event::Frame {
            seq,
            t_output_ns,
            t_submit_ns: t_submit_ns as u64,
            t_present_ns: t_present_ns as u64,
            present_kind: t_present_kind.to_string(),
            keys,
            t_work_done_ns: t_work_done_ns.map(|t| t as u64),
        });
        self.inner.lock().unwrap().frames_logged += 1;
    }

    pub fn frames_logged(&self) -> u64 {
        self.inner.lock().unwrap().frames_logged
    }

    pub fn flush(&self) {
        let _ = self.inner.lock().unwrap().out.flush();
    }
}

/// Nearest-rank percentiles in ns: `sorted[ceil(p/100 * n) - 1]`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pcts {
    pub n: usize,
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
}

impl Pcts {
    pub fn of(mut values: Vec<u64>) -> Pcts {
        if values.is_empty() {
            return Pcts::default();
        }
        values.sort_unstable();
        let rank = |p: usize| values[(p * values.len()).div_ceil(100).max(1) - 1];
        Pcts { n: values.len(), p50: rank(50), p95: rank(95), p99: rank(99) }
    }
}

/// Per-key stage times. Only keys with a `key`, a `write` and a presented frame are counted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub key_to_write: Pcts,
    pub write_to_output: Pcts,
    pub output_to_submit: Pcts,
    pub submit_to_present: Pcts,
    pub key_to_present: Pcts,
}

pub fn report(path: &Path) -> anyhow::Result<Report> {
    let mut t_key = HashMap::new();
    let mut t_write = HashMap::new();
    let mut frames = Vec::new();
    for (i, line) in BufReader::new(File::open(path)?).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let ev: Event = serde_json::from_str(&line).with_context(|| format!("line {}", i + 1))?;
        match ev {
            Event::Key { id, t_key_ns } => drop(t_key.insert(id, t_key_ns)),
            Event::Write { id, t_write_ns } => drop(t_write.insert(id, t_write_ns)),
            Event::Frame { t_output_ns, t_submit_ns, t_present_ns, keys, .. } => {
                frames.push((t_output_ns, t_submit_ns, t_present_ns, keys));
            }
            Event::Meta { .. } | Event::Sample { .. } => {}
        }
    }
    let mut stages: [Vec<u64>; 5] = Default::default();
    for (t_output, t_submit, t_present, keys) in frames {
        for id in keys {
            let (Some(&key), Some(&write)) = (t_key.get(&id), t_write.get(&id)) else { continue };
            let spans = [
                write.saturating_sub(key),
                t_output.saturating_sub(write),
                t_submit.saturating_sub(t_output),
                t_present.saturating_sub(t_submit),
                t_present.saturating_sub(key),
            ];
            for (stage, span) in stages.iter_mut().zip(spans) {
                stage.push(span);
            }
        }
    }
    let [a, b, c, d, e] = stages.map(Pcts::of);
    Ok(Report { key_to_write: a, write_to_output: b, output_to_submit: c, submit_to_present: d, key_to_present: e })
}
