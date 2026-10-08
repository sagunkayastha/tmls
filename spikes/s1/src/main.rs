//! s1 debug CLI. `s1 capture --file <bytes> [--cols 80 --rows 24]` feeds the raw
//! byte file into a fresh terminal and prints the captured `(Grid, TermState)` as JSON.
//! `s1 gen [--out DIR]` writes the adversarial streams from `s1::gen` as `<name>.bin`.
//! `s1 run --corpus-dir DIR [--gen] --out DIR` runs the snapshot/restore experiment (both
//! strategies) and writes `cases.jsonl`, `streams.json`, `summary.md`, `taxonomy.md`.
//! `s1 portability --corpus-dir DIR [--gen] --out DIR` feeds the end-of-stream snapshot (and the
//! original stream) to pyte and compares with libghostty (`<out>/portability/`).
//! `s1 resize --corpus-dir DIR [--gen] --out DIR` runs the resize-at-split variants (`<out>/resize/`).

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
    /// pyte portability probe: end-of-stream snapshot (and original bytes) rendered by pyte vs
    /// libghostty Plain of the visible screen. Writes `<out>/portability/`.
    Portability {
        #[command(flatten)]
        src: StreamArgs,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = s1::portability::DEFAULT_PYTHON)]
        python: String,
    },
    /// Resize-at-split experiment at the boundary nearest the middle. Writes `<out>/resize/`.
    Resize {
        #[command(flatten)]
        src: StreamArgs,
        #[arg(long)]
        out: PathBuf,
        /// Intermediate size.
        #[arg(long, default_value_t = 100)]
        to_cols: u16,
        #[arg(long, default_value_t = 30)]
        to_rows: u16,
        /// Streams to run (comma-separated).
        #[arg(long, value_delimiter = ',', default_value = "vim,less,scrollback_200,wrap_pending")]
        streams: Vec<String>,
    },
}

/// Stream selection shared by `portability` and `resize` (same sizes as `run`).
#[derive(clap::Args)]
struct StreamArgs {
    #[arg(long, default_value = "/data/scratch/s1/corpus")]
    corpus_dir: PathBuf,
    #[arg(long)]
    r#gen: bool,
    #[arg(long, default_value_t = 120)]
    cols: u16,
    #[arg(long, default_value_t = 40)]
    rows: u16,
    #[arg(long, default_value_t = 80)]
    gen_cols: u16,
    #[arg(long, default_value_t = 24)]
    gen_rows: u16,
    #[arg(long, default_value_t = 2000)]
    scrollback: u32,
    #[arg(long, default_value = "claude", value_delimiter = ',')]
    skip: Vec<String>,
}

/// (name, source, cols, rows, bytes): recorded `*.bin` (size from `.meta.json`, else
/// `cols`x`rows`), then the in-memory generated streams (`gen_cols`x`gen_rows`, `wrap_pending` 120 cols).
type Stream = (String, String, u16, u16, Vec<u8>);

fn load_streams(corpus_dir: &std::path::Path, with_gen: bool, cols: u16, rows: u16, gen_cols: u16, gen_rows: u16, skip: &[String]) -> anyhow::Result<Vec<Stream>> {
    let mut streams: Vec<Stream> = Vec::new();
    let mut files: Vec<PathBuf> = std::fs::read_dir(corpus_dir)?
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
    if with_gen {
        for (name, b) in s1::r#gen::adversarial() {
            let (c, r) = if name == "wrap_pending" { (120, gen_rows) } else { (gen_cols, gen_rows) };
            streams.push((name, "gen".into(), c, r, b));
        }
    }
    Ok(streams)
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
            let mut streams = load_streams(&corpus_dir, r#gen, cols, rows, gen_cols, gen_rows, &skip)?;
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
                let plan = experiment::split_plan(bytes, &bounds, seed, cap);
                let splits = plan.points.clone();
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
                    d_scrollback_rows: d.cap.1.scrollback_rows, d_alt_screen: d.cap.1.alt_screen, plan });
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
        Cmd::Portability { src, out, python } => {
            use s1::portability::{self as port, PortRecord, PyteSide};
            let streams = load_streams(&src.corpus_dir, src.r#gen, src.cols, src.rows, src.gen_cols, src.gen_rows, &src.skip)?;
            let dir = out.join("portability");
            std::fs::create_dir_all(&dir)?;
            let mut recs = Vec::new();
            let t0 = Instant::now();
            for (name, source, c, r, bytes) in &streams {
                let mut d = Terminal::new(Options { cols: *c, rows: *r, max_scrollback: src.scrollback as usize })?;
                d.vt_write(bytes);
                // Aligned snapshot at len: len is always a boundary, so q = len.
                let s = s1::snap::snapshot(&d, s1::snap::SnapOpts { palette: false });
                let vt = dir.join(format!("{name}.vt"));
                std::fs::write(&vt, &s)?;
                let orig = dir.join(format!("{name}.orig.bin"));
                std::fs::write(&orig, bytes)?;
                let d_lines = port::visible_plain(&d, *r);
                let (g, st) = s1::grid::capture(&mut d)?;
                let plain_matches_grid = port::grid_text_lines(&g) == d_lines;
                let rest = s1::snap::restore(*c, *r, src.scrollback as usize, &s);
                let rest_lines = port::visible_plain(&rest, *r);
                let ghostty_restore = port::compare_lines(&d_lines, &rest_lines);
                let ghostty_restore_shift = port::shifted_by_one(&d_lines, &rest_lines);
                let pyte_s = port::run_pyte(&python, &vt, *c, *r);
                let pyte_vs_restore = match &pyte_s {
                    Ok(p) => port::compare_lines(&rest_lines, &p.lines),
                    Err(_) => port::LineCmp::default(),
                };
                let cur = (st.cursor_x, st.cursor_y);
                let rec = PortRecord {
                    stream: name.clone(), source: source.clone(), cols: *c, rows: *r,
                    stream_bytes: bytes.len(), snapshot_bytes: s.len(), d_alt_screen: st.alt_screen,
                    d_cursor: cur, d_pending_wrap: st.pending_wrap, plain_matches_grid, ghostty_restore, ghostty_restore_shift,
                    pyte_snapshot: PyteSide::from(pyte_s, &d_lines, cur), pyte_vs_restore,
                    pyte_original: PyteSide::from(port::run_pyte(&python, &orig, *c, *r), &d_lines, cur),
                };
                eprintln!("{name:>26} {c}x{r} p-vs-restore {}/{} snap {}/{} orig {}/{} ignored {}/{}", rec.pyte_vs_restore.lines_equal, rec.pyte_vs_restore.rows, rec.pyte_snapshot.cmp.lines_equal,
                    rec.pyte_snapshot.cmp.rows, rec.pyte_original.cmp.lines_equal, rec.pyte_original.cmp.rows,
                    rec.pyte_snapshot.ignored, rec.pyte_original.ignored);
                recs.push(rec);
            }
            let mut jsonl = std::io::BufWriter::new(std::fs::File::create(dir.join("portability.jsonl"))?);
            for r in &recs { serde_json::to_writer(&mut jsonl, r)?; jsonl.write_all(b"\n")?; }
            jsonl.flush()?;
            let header = format!(
                "Run: `{}`  \npython `{python}`, helper `{}`, max_scrollback {}, total wall {:.1}s. \
                 D = libghostty terminal fed the whole stream; snapshot = `snap::snapshot(D)` (Aligned at len) = `<stream>.vt`; \
                 compared: Formatter Plain of the visible screen vs pyte `screen.display`, both right-trimmed, line by line. \
                 'ghostty restore' = Plain of libghostty restore(snapshot), so 'pyte(snapshot) vs ghostty restore' isolates the emulator from the Formatter's own losses. Cursor: pyte `screen.cursor.x/y` vs D `cursor_x/y`.",
                std::env::args().collect::<Vec<_>>().join(" "), port::helper_path().display(), src.scrollback,
                t0.elapsed().as_secs_f64());
            std::fs::write(dir.join("summary.md"), port::summary_md(&header, &recs))?;
            eprintln!("wrote {}", dir.display());
        }
        Cmd::Resize { src, out, to_cols, to_rows, streams: names } => {
            let streams = load_streams(&src.corpus_dir, src.r#gen, src.cols, src.rows, src.gen_cols, src.gen_rows, &src.skip)?;
            let dir = out.join("resize");
            std::fs::create_dir_all(&dir)?;
            let mut recs = Vec::new();
            let t0 = Instant::now();
            for n in &names {
                let Some((name, _src, c, r, bytes)) = streams.iter().find(|s| &s.0 == n) else {
                    eprintln!("resize: stream {n} not found"); continue;
                };
                let q = s1::resize::middle_boundary(bytes);
                let rr = s1::resize::run_stream(name, bytes, *c, *r, to_cols, to_rows, src.scrollback, q);
                for x in &rr { eprintln!("{name:>16} q={q} {:<34} identical={} class={:?}", x.variant, x.identical, x.class); }
                recs.extend(rr);
            }
            let mut jsonl = std::io::BufWriter::new(std::fs::File::create(dir.join("resize.jsonl"))?);
            for r in &recs { serde_json::to_writer(&mut jsonl, r)?; jsonl.write_all(b"\n")?; }
            jsonl.flush()?;
            let header = format!(
                "Run: `{}`  \nmax_scrollback {}, resize pixel cell size {}x{}, total wall {:.2}s. Split = splitter boundary nearest len/2; \
                 grids `normalize`d; history = Plain text (see experiment.rs). Variants: see `src/resize.rs` module docs.",
                std::env::args().collect::<Vec<_>>().join(" "), src.scrollback, s1::resize::CELL_W, s1::resize::CELL_H,
                t0.elapsed().as_secs_f64());
            std::fs::write(dir.join("summary.md"), s1::resize::summary_md(&header, &recs))?;
            eprintln!("wrote {}", dir.display());
        }
    }
    Ok(())
}
