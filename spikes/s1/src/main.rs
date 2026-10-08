//! s1 debug CLI. `s1 capture --file <bytes> [--cols 80 --rows 24]` feeds the raw
//! byte file into a fresh terminal and prints the captured `(Grid, TermState)` as JSON.

use std::path::PathBuf;

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
    }
    Ok(())
}
