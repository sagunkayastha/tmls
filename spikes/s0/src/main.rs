use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "s0", about = "tmls-rs spike S0: key-loss harness")]
struct Cli { #[command(subcommand)] cmd: Cmd }

#[derive(Subcommand)]
enum Cmd {
    /// Far end: raw stdin → log + acks
    Recv {
        #[arg(long)] log: PathBuf,
        /// Also flood stdout at this many bytes/sec
        #[arg(long)] flood: Option<u64>,
        /// Echo an ack `A<seq:06>` per arrival (`--ack false` disables)
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)] ack: bool,
        /// Exit once all of seqs 0..N have arrived (N = events; dups/out-of-range don't count)
        #[arg(long)] quit_after: Option<u64>,
        /// Stdout channel capacity in chunks (test hook)
        #[arg(long, default_value_t = 4096, hide = true)] ack_queue: usize,
        /// Print `S0READY:<tok>` instead of plain `S0READY` (pair with `send --ready-token`)
        #[arg(long)] ready_token: Option<String>,
    },
    /// Near end: spawn a path command in a PTY, drive nav, send numbered events, record acks
    Send {
        #[arg(long)] log: PathBuf,
        #[arg(long, default_value_t = 10_000)] events: u32,
        #[arg(long, default_value_t = 50.0)] rate: f64,
        #[arg(long, default_value_t = 120)] cols: u16,
        #[arg(long, default_value_t = 40)] rows: u16,
        /// Nav script, e.g. `wait:3000,expect:s0-recv:10000,key:j,key:enter,wait:1500`
        #[arg(long)] nav: Option<String>,
        /// Print the escape-stripped screen to stderr after nav
        #[arg(long)] dump_screen: bool,
        #[arg(long, default_value_t = 5000)] settle_ms: u64,
        /// Max wait for the receiver's ready marker (after the last nav key)
        #[arg(long, default_value_t = 30_000)] ready_timeout_ms: u64,
        /// Sleep this long between each event's payload and its marker (0 = off)
        #[arg(long, default_value_t = 0)] marker_gap_ms: u64,
        /// Open the ready gate only on the exact text `S0READY:<tok>` (plain `S0READY` is ignored)
        #[arg(long)] ready_token: Option<String>,
        #[arg(last = true, required = true)] cmd: Vec<String>,
    },
    /// Join send/recv logs into counts and latency percentiles
    Report {
        #[arg(long)] send: PathBuf,
        #[arg(long)] recv: PathBuf,
        #[arg(long)] path: String,
        #[arg(long)] load: String,
        #[arg(long)] json: Option<PathBuf>,
        /// Print the Markdown header row first
        #[arg(long)] header: bool,
        #[arg(long)] skew_note: Option<String>,
        /// Events sent (default: the send done line's `events`, else the number of send records)
        #[arg(long)] events: Option<u32>,
    },
    /// Output flood at a bounded rate
    Flood { #[arg(long)] bytes_per_sec: u64 },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Recv { log, flood, ack, quit_after, ack_queue, ready_token } =>
            s0::recv::main(s0::recv::Opts { log, flood, ack, quit_after, ack_queue, ready_token }),
        Cmd::Send { log, events, rate, cols, rows, nav, dump_screen, settle_ms, ready_timeout_ms, marker_gap_ms, ready_token, cmd } =>
            s0::send::main(s0::send::Opts { log, events, rate, cols, rows, nav, dump_screen, settle_ms,
                                            ready_timeout_ms, marker_gap_ms, ready_token, cmd }),
        Cmd::Report { send, recv, path, load, json, header, skew_note, events } =>
            s0::report::main(s0::report::Opts { send, recv, path, load, json, header, skew_note, events }),
        Cmd::Flood { bytes_per_sec } => { s0::flood::main(bytes_per_sec); Ok(()) }
    }
}
