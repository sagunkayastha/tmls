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
        /// Exit after this many arrivals
        #[arg(long)] quit_after: Option<u64>,
        /// Stdout channel capacity in chunks (test hook)
        #[arg(long, default_value_t = 4096, hide = true)] ack_queue: usize,
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
        /// Max wait for the receiver's `S0READY` marker (after nav)
        #[arg(long, default_value_t = 30_000)] ready_timeout_ms: u64,
        #[arg(last = true, required = true)] cmd: Vec<String>,
    },
    /// Output flood at a bounded rate
    Flood { #[arg(long)] bytes_per_sec: u64 },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Recv { log, flood, ack, quit_after, ack_queue } =>
            s0::recv::main(s0::recv::Opts { log, flood, ack, quit_after, ack_queue }),
        Cmd::Send { log, events, rate, cols, rows, nav, dump_screen, settle_ms, ready_timeout_ms, cmd } =>
            s0::send::main(s0::send::Opts { log, events, rate, cols, rows, nav, dump_screen, settle_ms, ready_timeout_ms, cmd }),
        Cmd::Flood { bytes_per_sec } => { s0::flood::main(bytes_per_sec); Ok(()) }
    }
}
