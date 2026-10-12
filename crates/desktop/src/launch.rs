//! Leaving the shell at once, like a desktop app: `tmls-rs` typed in a terminal starts itself
//! again as its own process in a new session (no controlling terminal, stdin from `/dev/null`,
//! stdout and stderr to a log file) and returns. Closing the terminal then leaves the window up.

use std::fs::File;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The flags that keep `tmls-rs` in the foreground; the detached child gets `--foreground`.
pub fn is_foreground_flag(arg: &str) -> bool {
    matches!(arg, "--foreground" | "--fg")
}

/// The subcommands, which print and exit: they stay in the terminal. Any other first word is a
/// host (`tmls-rs HOST…`).
const SUBCOMMANDS: &[&str] = &["keys"];

/// The subcommand the arguments ask for: `None` when the first word is none; its name when it
/// stands alone (foreground flags aside); an error, for the usage, when other words follow it
/// — `tmls-rs keys extra` is not the app with the hosts `keys` and `extra`.
pub fn subcommand(args: &[String]) -> Option<Result<&str, String>> {
    let first = args
        .first()
        .filter(|first| SUBCOMMANDS.contains(&first.as_str()))?;
    let extra: Vec<&str> = args[1..]
        .iter()
        .map(String::as_str)
        .filter(|arg| !is_foreground_flag(arg))
        .collect();
    Some(match extra.is_empty() {
        true => Ok(first.as_str()),
        false => Err(format!("{first} takes no arguments, got {extra:?}")),
    })
}

/// Whether to run in this process rather than detach. `env_fg` is `$TMLS_RS_FOREGROUND`.
///
/// Stays: with `--bench` (the e2e and bench scripts read its stderr), `--foreground`/`--fg`,
/// `TMLS_RS_FOREGROUND` set to anything but empty or `0`, or a subcommand as the first
/// argument (`tmls-rs keys`).
pub fn wants_foreground(args: &[String], env_fg: Option<&str>) -> bool {
    if env_fg.is_some_and(|v| !v.is_empty() && v != "0") {
        return true;
    }
    if args
        .first()
        .is_some_and(|first| SUBCOMMANDS.contains(&first.as_str()))
    {
        return true;
    }
    args.iter()
        .any(|arg| arg == "--bench" || is_foreground_flag(arg))
}

/// `$XDG_CACHE_HOME/tmls-rs/tmls-rs.log`, else `~/.cache/tmls-rs/tmls-rs.log` (an empty or
/// relative `$XDG_CACHE_HOME` is ignored, as the XDG spec says).
pub fn log_path(xdg_cache_home: Option<&str>, home: &str) -> PathBuf {
    let cache = match xdg_cache_home {
        Some(dir) if Path::new(dir).is_absolute() => PathBuf::from(dir),
        _ => Path::new(home).join(".cache"),
    };
    cache.join("tmls-rs").join("tmls-rs.log")
}

/// The command that runs `exe` again with `args` and `--foreground` (added only if no
/// foreground flag is there already), in a new session, stdin from `/dev/null`, stdout and
/// stderr to `log`. The working directory and environment are inherited.
pub fn detach_command(exe: &Path, args: &[String], log: File) -> io::Result<Command> {
    let mut cmd = Command::new(exe);
    cmd.args(args);
    if !args.iter().any(|arg| is_foreground_flag(arg)) {
        cmd.arg("--foreground");
    }
    cmd.stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    // SAFETY: runs in the child between fork and exec; setsid is async-signal-safe and
    // touches no memory of ours.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok(cmd)
}

/// Starts this program again detached (see [`detach_command`]), its log truncated, and returns
/// once it is spawned. An error means nothing was started: the caller runs in the foreground.
pub fn detach(args: &[String]) -> Result<(), String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let xdg = std::env::var("XDG_CACHE_HOME").ok();
    let path = log_path(xdg.as_deref(), &home);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let log = File::create(&path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let exe = std::env::current_exe().map_err(|e| format!("cannot find own executable: {e}"))?;
    detach_command(&exe, args, log)
        .and_then(|mut cmd| cmd.spawn())
        .map(drop)
        .map_err(|e| format!("cannot start {}: {e}", exe.display()))
}
