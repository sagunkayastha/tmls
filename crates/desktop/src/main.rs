//! `tmls-rs`: the tmux sessions of this machine and of the user's ssh hosts in one native
//! window — the list on the left, each attached session in a tab.

use std::path::PathBuf;
use std::process::ExitCode;

use desktop::app::{self, Args};

const USAGE: &str = "usage: tmls-rs [--bench <dir>] [HOST…]\n\
  HOST…          ssh hosts to list instead of those in ~/.config/tmls/hosts\n\
  --bench <dir>  write a latency log for the first tab opened to <dir>/latency.jsonl";

/// Parses the arguments after the program name.
fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Args, String> {
    let mut parsed = Args::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bench" => {
                let dir = args.next().filter(|dir| !dir.starts_with('-'));
                parsed.bench = Some(PathBuf::from(dir.ok_or("--bench needs a directory")?));
            }
            flag if flag.starts_with('-') => return Err(format!("unknown argument {flag:?}")),
            host => parsed.hosts.push(host.to_string()),
        }
    }
    Ok(parsed)
}

/// `~/.config/tmls`, the directory tmls keeps its files in (`$XDG_CONFIG_HOME` is not used by
/// tmls either).
fn config_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config/tmls")
}

fn main() -> ExitCode {
    // tmux refuses to attach from inside tmux, and every tab runs `tmux attach` with this
    // process's environment.
    // SAFETY: nothing else runs yet — no other thread exists that could read or write the
    // environment at the same time.
    unsafe { std::env::remove_var("TMUX") };

    let args = match parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("tmls-rs: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match app::run(args, config_dir()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tmls-rs: {e:#}");
            ExitCode::FAILURE
        }
    }
}
