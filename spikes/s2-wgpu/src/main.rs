//! `s2-wgpu`: a terminal window on winit + wgpu, instrumented for the S2 latency runs.

use std::process::ExitCode;

use s2_wgpu::{app, cli};

fn main() -> ExitCode {
    let args = match cli::parse(std::env::args().skip(1), std::env::var("SHELL").ok()) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("s2-wgpu: {e}\n{}", cli::USAGE);
            return ExitCode::from(2);
        }
    };
    match app::run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("s2-wgpu: {e:#}");
            ExitCode::FAILURE
        }
    }
}
