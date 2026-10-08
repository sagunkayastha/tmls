//! s1 debug CLI. `s1 capture --file <bytes> [--cols 80 --rows 24]` feeds the raw
//! byte file into a fresh terminal and prints the captured `(Grid, TermState)` as JSON.
//! `s1 gen [--out DIR]` writes the adversarial streams from `s1::gen` as `<name>.bin`.
//! `s1 run --corpus-dir DIR [--gen] --out DIR` runs the snapshot/restore experiment (both
//! strategies) and writes `cases.jsonl`, `streams.json`, `summary.md`, `taxonomy.md`.

use std::io::Write as _;
use std::path::PathBuf;
use std::time::Instant;

use clap::{Parser, Subcommand};
use libghostty_vt::terminal::{Options, Terminal};

#[derive(Parser)]
#[command(about = "tmls-rs spike S1: libghostty-vt snapshot/restore fidelity")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Feed a byte file into a fresh terminal and print the capture as JSON.
    Capture {
        #[arg(long)]
        file: PathBuf,
        #[arg(long, default_value_t = 80)]
        cols: u16,
        #[arg(long, default_value_t = 24)]
        rows: u16,
    },
    /// Write every adversarial stream to `<out>/<name>.bin`.
    Gen {
        #[arg(long, default_value = "/data/scratch/s1/corpus/gen")]
        out: PathBuf,
    },
    /// Snapshot/restore experiment over recorded corpus streams (+ generated ones with --gen).
    Run {
        #[arg(long, default_value = "/data/scratch/s1/corpus")]
        corpus_dir: PathBuf,
        /// Also run the in-memory adversarial streams from `s1::gen`.
        #[arg(long)]
        r#gen: bool,
        #[arg(long)]
        out: PathBuf,
        /// Size for recorded streams without a `.meta.json` (meta cols/rows win when present).
        #[arg(long, default_value_t = 120)]
        cols: u16,
        #[arg(long, default_value_t = 40)]
        rows: u16,
        /// Size for generated streams (except `wrap_pending`, which assumes 120 columns).
        #[arg(long, default_value_t = 80)]
        gen_cols: u16,
        #[arg(long, default_value_t = 24)]
        gen_rows: u16,
        #[arg(long, default_value_t = 2000)]
        scrollback: u32,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// Recorded stream names to skip (comma-separated).
        #[arg(long, default_value = "claude", value_delimiter = ',')]
        skip: Vec<String>,
        /// Per-stream split caps, `name=N` (comma-separated); default cap is 400.
        #[arg(long, value_delimiter = ',')]
        stream_cap: Vec<String>,
        /// Only run these streams (comma-separated; empty = all).
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
    },
}

fn meta_size(path: &std::path::Path) -> Option<(u16, u16)> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    Some((v.get("cols")?.as_u64()? as u16, v.get("rows")?.as_u64()? as u16))
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Capture { file, cols, rows } => {
            let bytes = std::fs::read(&file)?;
            let mut t = Terminal::new(Options { cols, rows, max_scrollback: 10_000 })?;
            t.vt_write(&bytes);
            let cap = s1::grid::capture(&mut t)?;
            println!("{}", serde_json::to_string_pretty(&cap)?);
        }
        Cmd::Gen { out } => {
            std::fs::create_dir_all(&out)?;
            for (name, bytes) in s1::r#gen::adversarial() {
                let p = out.join(format!("{name}.bin"));
                std::fs::write(&p, &bytes)?;
                println!("{:>7} {}", bytes.len(), p.display());
            }
        }
        Cmd::Run { corpus_dir, r#gen, out, cols, rows, gen_cols, gen_rows, scrollback, seed, skip, stream_cap, only } => {
            use s1::experiment::{self, Case, Strategy};
            use s1::report::{Record, StreamInfo};
            // (name, source, cols, rows, bytes)
            let mut streams: Vec<(String, String, u16, u16, Vec<u8>)> = Vec::new();
            let mut files: Vec<PathBuf> = std::fs::read_dir(&corpus_dir)?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "bin"))
                .collect();
            files.sort();
            for f in files {
                let name = f.file_stem().unwrap().to_string_lossy().to_string();
                if skip.contains(&name) { eprintln!("skip {name}"); continue; }
                let (c, r) = meta_size(&f.with_extension("meta.json")).unwrap_or((cols, rows));
                streams.push((name, "recorded".into(), c, r, std::fs::read(&f)?));
            }
            if r#gen {
                for (name, b) in s1::r#gen::adversarial() {
                    let (c, r) = if name == "wrap_pending" { (120, gen_rows) } else { (gen_cols, gen_rows) };
                    streams.push((name, "gen".into(), c, r, b));
                }
            }
            if !only.is_empty() { streams.retain(|s| only.contains(&s.0)); }
            let caps: std::collections::HashMap<String, usize> = stream_cap.iter()
                .filter_map(|kv| kv.split_once('=').and_then(|(k, v)| Some((k.to_string(), v.parse().ok()?))))
                .collect();

            std::fs::create_dir_all(&out)?;
            let mut jsonl = std::io::BufWriter::new(std::fs::File::create(out.join("cases.jsonl"))?);
            let mut recs: Vec<Record> = Vec::new();
            let mut infos: Vec<StreamInfo> = Vec::new();
            let t_all = Instant::now();
            for (name, source, c, r, bytes) in &streams {
                let t0 = Instant::now();
                let bounds = s1::splitter::boundaries(bytes);
                let cap = caps.get(name).copied().unwrap_or(experiment::MAX_SPLITS);
                let splits = experiment::split_points_capped(bytes, &bounds, seed, cap);
                let d = experiment::reference(bytes, *c, *r, scrollback);
                for &p in &splits {
                    for st in [Strategy::Naive, Strategy::Aligned] {
                        let case = Case { stream: name.clone(), split: p, strategy: st };
                        let res = experiment::run_case_with(bytes, &bounds, &d, &case, *c, *r, scrollback);
                        let rec = Record { stream: name.clone(), source: source.clone(), cols: *c, rows: *r,
                            stream_bytes: bytes.len(), split: p, strategy: st, result: res };
                        serde_json::to_writer(&mut jsonl, &rec)?;
                        jsonl.write_all(b"\n")?;
                        recs.push(rec);
                    }
                }
                let secs = t0.elapsed().as_secs_f64();
                eprintln!("{name:>26} {c}x{r} {:>6} B {:>3} splits {secs:6.1}s", bytes.len(), splits.len());
                infos.push(StreamInfo { name: name.clone(), source: source.clone(), cols: *c, rows: *r,
                    bytes: bytes.len(), splits: splits.len(), split_cap: cap, seconds: secs,
                    d_scrollback_rows: d.cap.1.scrollback_rows, d_alt_screen: d.cap.1.alt_screen });
            }
            jsonl.flush()?;
            let header = format!(
                "Run: `{}`  \ncorpus `{}`, gen={}, max_scrollback {scrollback} (ghostty: BYTES, rounded up to the PageList minimum), seed {seed}, skipped {:?}, \
                 build {}, total wall {:.1}s. Diff direction: D (reference, whole stream) vs C (restored + tail). \
                 Grids normalised (`grid::normalize`); history compared as Formatter Plain text (unwrap off, \
                 first scrollback_rows lines, right-trimmed).",
                std::env::args().collect::<Vec<_>>().join(" "), corpus_dir.display(), r#gen, skip,
                if cfg!(debug_assertions) { "debug" } else { "release" }, t_all.elapsed().as_secs_f64());
            std::fs::write(out.join("streams.json"), serde_json::to_string_pretty(&infos)?)?;
            std::fs::write(out.join("summary.md"), s1::report::summary_md(&header, &infos, &recs))?;
            std::fs::write(out.join("taxonomy.md"), s1::report::taxonomy_md(&header, &recs))?;
            eprintln!("wrote {}", out.display());
        }
    }
    Ok(())
}
