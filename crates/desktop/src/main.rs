//! `tmls-rs`: the tmux sessions of this machine and of the user's ssh hosts in one native
//! window — the list on the left, each attached session in a tab.

use std::path::PathBuf;
use std::process::ExitCode;

use desktop::app::{self, Args};

const USAGE: &str = "usage: tmls-rs [--bench <dir>] [HOST…]\n       tmls-rs keys\n\
  HOST…          ssh hosts to list instead of those in ~/.config/tmls/hosts\n\
  --bench <dir>  write a latency log for the first tab opened to <dir>/latency.jsonl\n\
  keys           print the keys and mouse actions, and exit";

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
            flag if desktop::launch::is_foreground_flag(flag) => {}
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

/// Starts the window as its own process and returns the exit code to leave with, unless this
/// run stays in the foreground (`launch::wants_foreground`, or arguments that are wrong: their
/// error goes to this terminal). If detaching fails, says why and stays in the foreground.
fn detach_unless_foreground() -> Option<ExitCode> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let env_fg = std::env::var("TMLS_RS_FOREGROUND").ok();
    if desktop::launch::wants_foreground(&args, env_fg.as_deref()) || parse(args.clone()).is_err() {
        return None;
    }
    match desktop::launch::detach(&args) {
        Ok(()) => Some(ExitCode::SUCCESS),
        Err(e) => {
            desktop::errln(format_args!("tmls-rs: {e}; running in the foreground"));
            None
        }
    }
}

fn main() -> ExitCode {
    if let Some(code) = detach_unless_foreground() {
        return code;
    }
    // tmux refuses to attach from inside tmux, and every tab runs `tmux attach` with this
    // process's environment.
    // SAFETY: nothing else runs yet — no other thread exists that could read or write the
    // environment at the same time.
    unsafe { std::env::remove_var("TMUX") };

    let args: Vec<String> = std::env::args().skip(1).collect();
    match desktop::launch::subcommand(&args) {
        Some(Ok(_keys)) => {
            use std::io::Write;
            // Not `print!`: a closed pipe (`tmls-rs keys | head`) must not panic.
            let _ = std::io::stdout().write_all(desktop::keys::help_text().as_bytes());
            return ExitCode::SUCCESS;
        }
        Some(Err(e)) => {
            desktop::errln(format_args!("tmls-rs: {e}\n{USAGE}"));
            return ExitCode::from(2);
        }
        None => {}
    }
    let args = match parse(args) {
        Ok(args) => args,
        Err(e) => {
            desktop::errln(format_args!("tmls-rs: {e}\n{USAGE}"));
            return ExitCode::from(2);
        }
    };
    match app::run(args, config_dir()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            desktop::errln(format_args!("tmls-rs: {e:#}"));
            ExitCode::FAILURE
        }
    }
}
