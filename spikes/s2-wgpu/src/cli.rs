//! Command line, identical for `s2-wgpu` and `s2-gpui` so the bench driver is shared
//! (contract: "App CLI" in `docs/superpowers/plans/2026-10-08-s2-split-overview.md`).
//!
//! `--bench <dir>` · `--photon` · `--cmd <program> [args…]` (default `$SHELL`) ·
//! `--present fifo|mailbox|immediate` · `--load-label quiet|flood`.

use std::path::PathBuf;

pub const USAGE: &str = "usage: s2-wgpu [--bench <dir>] [--photon] [--present fifo|mailbox|immediate] \
[--load-label quiet|flood] [--cmd <program> [args…]]";

/// `--present`; the default is `fifo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Present {
    Fifo,
    Mailbox,
    Immediate,
}

/// `--load-label`: what the run is under; only written to the `meta` line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Load {
    Quiet,
    Flood,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    /// `--bench <dir>`: write `<dir>/latency.jsonl` and run the sampler.
    pub bench: Option<PathBuf>,
    /// `--photon`: the 64x64 square in the top-left corner; it flips once per presented frame
    /// that carries at least one key press (`session::Photon`).
    pub photon: bool,
    /// `--cmd <program> [args…]`: what runs in the PTY; never empty.
    pub cmd: Vec<String>,
    pub present: Present,
    pub load_label: Load,
}

impl Present {
    /// The word on the command line and in the `meta` line.
    pub fn as_str(self) -> &'static str {
        match self {
            Present::Fifo => "fifo",
            Present::Mailbox => "mailbox",
            Present::Immediate => "immediate",
        }
    }
}

impl Load {
    pub fn as_str(self) -> &'static str {
        match self {
            Load::Quiet => "quiet",
            Load::Flood => "flood",
        }
    }
}

/// Parses the arguments after the program name. `shell` is `$SHELL`, used when there is no
/// `--cmd`. `--cmd` takes everything after it, so it has to come last.
pub fn parse<I: IntoIterator<Item = String>>(
    args: I,
    shell: Option<String>,
) -> Result<Args, String> {
    let mut parsed = Args {
        bench: None,
        photon: false,
        cmd: Vec::new(),
        present: Present::Fifo,
        load_label: Load::Quiet,
    };
    let mut args = args.into_iter();
    // The value of a flag: the next argument, which must not itself be a flag.
    let value = |flag: &str, next: Option<String>| {
        next.filter(|v| !v.starts_with("--"))
            .ok_or_else(|| format!("{flag} needs a value"))
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bench" => parsed.bench = Some(PathBuf::from(value("--bench", args.next())?)),
            "--photon" => parsed.photon = true,
            "--present" => {
                parsed.present = match value("--present", args.next())?.as_str() {
                    "fifo" => Present::Fifo,
                    "mailbox" => Present::Mailbox,
                    "immediate" => Present::Immediate,
                    other => {
                        return Err(format!(
                            "--present {other}: expected fifo, mailbox or immediate"
                        ));
                    }
                }
            }
            "--load-label" => {
                parsed.load_label = match value("--load-label", args.next())?.as_str() {
                    "quiet" => Load::Quiet,
                    "flood" => Load::Flood,
                    other => return Err(format!("--load-label {other}: expected quiet or flood")),
                }
            }
            "--cmd" => {
                parsed.cmd = args.by_ref().collect();
                if parsed.cmd.is_empty() {
                    return Err("--cmd needs a program".to_string());
                }
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    if parsed.cmd.is_empty() {
        let shell = shell
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/sh".to_string());
        parsed.cmd = vec![shell];
    }
    Ok(parsed)
}
