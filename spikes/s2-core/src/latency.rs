//! Latency log (`<dir>/latency.jsonl`) and its percentile report.
//! One JSON object per line with an `"ev"` discriminator; all times are ns from `now_ns()`.
//! Schema: `docs/superpowers/plans/2026-10-08-s2-split-overview.md`.

use std::collections::{BTreeMap, HashMap};
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
        /// When the redraw that presented the frame began (the front-end stopped taking events).
        /// Left out of the line when the front-end does not report it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        t_redraw_ns: Option<u64>,
    },
    Sample { t_ns: u64, cpu_pct: f64, rss_kb: u64, fps: f64, gpu_util: Option<f64>, gpu_w: Option<f64> },
}

/// Optional fields of the `meta` line that only some front-ends write, after the fixed ones
/// (`Log::meta_with`). A field that is `None` is left out of the line, so a line without any is
/// byte for byte what `Log::event` writes. `Event::Meta` itself is unchanged and ignores them
/// when a line is read as an `Event`; read the same line as a `MetaExtra` to get them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaExtra {
    /// Whether the front-end's non-terminal UI was on (`s2-wgpu`: `S2_WGPU_NO_UI` not set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<bool>,
    /// Where `hz` came from: `current_monitor` | `only_monitor` | `unknown` (`s2-wgpu`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hz_source: Option<String>,
}

struct Inner {
    out: BufWriter<File>,
    /// Frames the front-end took and no `frame` line covers yet: seq → (t_output_ns, keys).
    /// At most `MAX_TAKEN` entries.
    taken: BTreeMap<u64, (u64, Vec<u64>)>,
    frames_logged: u64,
}

impl Inner {
    fn write(&mut self, ev: &Event) {
        if serde_json::to_writer(&mut self.out, ev).is_ok() {
            let _ = self.out.write_all(b"\n");
        }
    }
}

/// How many taken frames wait for a `frame` line before the oldest are merged.
const MAX_TAKEN: usize = 256;

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
        let inner = Inner { out: BufWriter::new(file), taken: BTreeMap::new(), frames_logged: 0 };
        Ok(Log { inner: Arc::new(Mutex::new(inner)) })
    }

    pub fn event(&self, ev: &Event) {
        self.inner.lock().unwrap().write(ev);
    }

    /// Writes a `meta` line with the optional fields of `extra` after the fixed ones; with no
    /// extras it is `event(meta)`. For any other event the extras are ignored.
    pub fn meta_with(&self, meta: &Event, extra: &MetaExtra) {
        let (Event::Meta { .. }, Ok(mut line), Ok(extra)) = (meta, serde_json::to_string(meta), serde_json::to_string(extra)) else {
            return self.event(meta);
        };
        // `extra` is `{}` or `{"ui":…,…}`: its fields go before the line's closing brace.
        if extra.len() > 2 && line.ends_with('}') {
            line.pop();
            line.push(',');
            line.push_str(&extra[1..]);
        }
        let mut inner = self.inner.lock().unwrap();
        let _ = inner.out.write_all(line.as_bytes());
        let _ = inner.out.write_all(b"\n");
    }

    pub fn key(&self, id: u64, t_key_ns: u128) {
        self.event(&Event::Key { id, t_key_ns: t_key_ns as u64 });
    }

    pub fn write(&self, id: u64, t_write_ns: u128) {
        self.event(&Event::Write { id, t_write_ns: t_write_ns as u64 });
    }

    pub(crate) fn frame_taken(&self, seq: u64, t_output_ns: u128, keys: &[u64]) {
        let mut inner = self.inner.lock().unwrap();
        inner.taken.insert(seq, (t_output_ns as u64, keys.to_vec()));
        // A front-end that takes frames and never logs one must not grow this map: past the cap
        // the oldest entry's keys move into the next one, so no key is dropped.
        while inner.taken.len() > MAX_TAKEN {
            let (_, (_, mut keys)) = inner.taken.pop_first().expect("non-empty");
            if let Some(mut next) = inner.taken.first_entry() {
                keys.append(&mut next.get_mut().1);
                next.get_mut().1 = keys;
            }
        }
    }

    /// One line per presented frame. `seq` is the `Frame::seq` that was drawn. The line carries
    /// that frame's output time and the keys of every frame taken up to and including `seq` that
    /// no earlier line covered: a front-end that takes two frames and presents only the second
    /// still gets the first one's keys, on the frame that showed them.
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
        self.frame_with_marks(seq, t_submit_ns, t_present_ns, t_present_kind, t_work_done_ns, None);
    }

    /// `frame_with_work_done` plus `t_redraw_ns`: when the redraw that presented the frame began.
    /// Each optional time is left out of the line when it is `None`.
    pub fn frame_with_marks(
        &self,
        seq: u64,
        t_submit_ns: u128,
        t_present_ns: u128,
        t_present_kind: &str,
        t_work_done_ns: Option<u128>,
        t_redraw_ns: Option<u128>,
    ) {
        let mut inner = self.inner.lock().unwrap();
        let newer = inner.taken.split_off(&seq.saturating_add(1));
        let covered = std::mem::replace(&mut inner.taken, newer);
        let (mut t_output_ns, mut keys) = (0, Vec::new());
        for (_, (t, mut frame_keys)) in covered {
            t_output_ns = t;
            keys.append(&mut frame_keys);
        }
        let line = Event::Frame {
            seq,
            t_output_ns,
            t_submit_ns: t_submit_ns as u64,
            t_present_ns: t_present_ns as u64,
            present_kind: t_present_kind.to_string(),
            keys,
            t_work_done_ns: t_work_done_ns.map(|t| t as u64),
            t_redraw_ns: t_redraw_ns.map(|t| t as u64),
        };
        inner.write(&line);
        inner.frames_logged += 1;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_lines(dir: &Path) -> Vec<(u64, u64, Vec<u64>)> {
        let text = std::fs::read_to_string(dir.join("latency.jsonl")).unwrap();
        let mut lines = Vec::new();
        for line in text.lines() {
            if let Event::Frame { seq, t_output_ns, keys, .. } = serde_json::from_str(line).unwrap() {
                lines.push((seq, t_output_ns, keys));
            }
        }
        lines
    }

    /// `<target>/tmp/<name>`, the directory integration tests get as `CARGO_TARGET_TMPDIR`
    /// (the test binary is `<target>/<profile>/deps/<name>`). Never the system temp directory.
    fn scratch(name: &str) -> std::path::PathBuf {
        let exe = std::env::current_exe().unwrap();
        exe.ancestors().nth(3).expect("a target directory").join("tmp").join(name)
    }

    #[test]
    fn frame_line_folds_in_keys_of_taken_frames_never_logged() {
        let dir = scratch("latency_fold");
        let log = Log::create(&dir).unwrap();
        log.frame_taken(1, 100, &[1]);
        log.frame_taken(2, 200, &[]);
        log.frame_taken(3, 300, &[2, 3]);
        log.frame_taken(4, 400, &[4]);
        // The front-end drew frame 3 only: it showed what frames 1 and 2 would have.
        log.frame(3, 310, 320, "present_return");
        assert_eq!(log.inner.lock().unwrap().taken.len(), 1, "frame 4 still waits");
        log.frame(4, 410, 420, "present_return");
        // A seq that was never taken (or is logged twice) has nothing to report.
        log.frame(4, 510, 520, "present_return");
        log.flush();
        assert_eq!(frame_lines(&dir), [(3, 300, vec![1, 2, 3]), (4, 400, vec![4]), (4, 0, vec![])]);
        assert_eq!(log.frames_logged(), 3);
        assert!(log.inner.lock().unwrap().taken.is_empty());
    }

    #[test]
    fn redraw_time_is_on_the_line_only_when_given() {
        let dir = scratch("latency_redraw");
        let log = Log::create(&dir).unwrap();
        log.frame_taken(1, 100, &[7]);
        log.frame_with_marks(1, 110, 120, "present_return", Some(125), Some(105));
        log.frame_with_marks(2, 210, 220, "present_return", None, Some(205));
        // The older calls, and a front-end without the mark: the line is what it always was.
        log.frame_with_marks(3, 310, 320, "frame_callback", None, None);
        log.frame_with_work_done(4, 410, 420, "present_return", Some(425));
        log.frame(5, 510, 520, "present_return");
        log.flush();
        let text = std::fs::read_to_string(dir.join("latency.jsonl")).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines,
            [
                r#"{"ev":"frame","seq":1,"t_output_ns":100,"t_submit_ns":110,"t_present_ns":120,"present_kind":"present_return","keys":[7],"t_work_done_ns":125,"t_redraw_ns":105}"#,
                r#"{"ev":"frame","seq":2,"t_output_ns":0,"t_submit_ns":210,"t_present_ns":220,"present_kind":"present_return","keys":[],"t_redraw_ns":205}"#,
                r#"{"ev":"frame","seq":3,"t_output_ns":0,"t_submit_ns":310,"t_present_ns":320,"present_kind":"frame_callback","keys":[]}"#,
                r#"{"ev":"frame","seq":4,"t_output_ns":0,"t_submit_ns":410,"t_present_ns":420,"present_kind":"present_return","keys":[],"t_work_done_ns":425}"#,
                r#"{"ev":"frame","seq":5,"t_output_ns":0,"t_submit_ns":510,"t_present_ns":520,"present_kind":"present_return","keys":[]}"#,
            ]
        );
        assert_eq!(log.frames_logged(), 5);
        // Lines with and without the field read back.
        let read: Vec<Option<u64>> = lines
            .iter()
            .map(|line| match serde_json::from_str(line).unwrap() {
                Event::Frame { t_redraw_ns, .. } => t_redraw_ns,
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(read, [Some(105), Some(205), None, None, None]);
    }

    #[test]
    fn taken_frames_stay_bounded_and_keep_their_keys() {
        let dir = scratch("latency_bound");
        let log = Log::create(&dir).unwrap();
        for seq in 1..=10_000u64 {
            log.frame_taken(seq, u128::from(seq), &[seq]);
        }
        assert_eq!(log.inner.lock().unwrap().taken.len(), MAX_TAKEN);
        log.frame(10_000, 1, 2, "present_return");
        log.flush();
        let lines = frame_lines(&dir);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].1, 10_000);
        assert_eq!(lines[0].2, (1..=10_000).collect::<Vec<u64>>());
    }

    #[test]
    fn meta_extras_are_on_the_line_only_when_given() {
        let dir = scratch("latency_meta_extra");
        let log = Log::create(&dir).unwrap();
        let meta = Event::Meta {
            app: "wgpu".into(),
            git: "abc1234".into(),
            profile: "release".into(),
            hz: 165.0,
            scale: 1.25,
            present_mode: "fifo".into(),
            load: "quiet".into(),
        };
        let both = MetaExtra { ui: Some(false), hz_source: Some("current_monitor".into()) };
        let ui_only = MetaExtra { ui: Some(true), hz_source: None };
        log.event(&meta);
        log.meta_with(&meta, &MetaExtra::default());
        log.meta_with(&meta, &both);
        log.meta_with(&meta, &ui_only);
        log.flush();
        let text = std::fs::read_to_string(dir.join("latency.jsonl")).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        // The line as it has always been: fixed fields only.
        let plain = r#"{"ev":"meta","app":"wgpu","git":"abc1234","profile":"release","hz":165.0,"scale":1.25,"present_mode":"fifo","load":"quiet"}"#;
        assert_eq!(lines[0], plain);
        assert_eq!(lines[1], plain, "no extras: byte for byte the old line");
        // Extras come after the fixed fields; an absent one is not written at all.
        assert_eq!(lines[2], format!(r#"{},"ui":false,"hz_source":"current_monitor"}}"#, &plain[..plain.len() - 1]));
        assert_eq!(lines[3], format!(r#"{},"ui":true}}"#, &plain[..plain.len() - 1]));
        assert_eq!(lines.len(), 4);
        // Every line still reads as the same `Event` (old readers lose nothing they had), and
        // the extras read back from the same line; an old line has none.
        let extras = [MetaExtra::default(), MetaExtra::default(), both, ui_only];
        for (line, extra) in lines.iter().zip(extras) {
            assert_eq!(serde_json::from_str::<Event>(line).unwrap(), meta);
            assert_eq!(serde_json::from_str::<MetaExtra>(line).unwrap(), extra);
        }
        // Only a `meta` line takes extras.
        let key = Event::Key { id: 1, t_key_ns: 5 };
        let log = Log::create(&scratch("latency_meta_extra_key")).unwrap();
        log.meta_with(&key, &MetaExtra { ui: Some(true), hz_source: None });
        log.flush();
        let text = std::fs::read_to_string(scratch("latency_meta_extra_key").join("latency.jsonl")).unwrap();
        assert_eq!(text, "{\"ev\":\"key\",\"id\":1,\"t_key_ns\":5}\n");
    }
}
