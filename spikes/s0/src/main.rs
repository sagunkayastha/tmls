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
    /// Output flood at a bounded rate
    Flood { #[arg(long)] bytes_per_sec: u64 },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Recv { log, flood, ack, quit_after, ack_queue } =>
            s0::recv::main(s0::recv::Opts { log, flood, ack, quit_after, ack_queue }),
        Cmd::Flood { bytes_per_sec } => { s0::flood::main(bytes_per_sec); Ok(()) }
    }
}
