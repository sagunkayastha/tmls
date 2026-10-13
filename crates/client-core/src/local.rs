//! How a command is run on this machine: as it is on Unix, inside WSL on Windows.
//!
//! Windows has no tmux of its own. With the file `~/.config/tmls/wsl` (`Config::load`) the
//! Windows build takes a WSL distro for this machine: every command the builders in `tmux`,
//! `files`, `push` and `listing` make for `local_name` then starts with `wsl.exe [-d DISTRO] -e`,
//! and what follows is the Unix argv, word for word. `wsl.exe -e` runs the program without a
//! shell and reads its arguments as any Windows program does, which is what `Command` writes
//! them for: a word with spaces, quotes, `$`, backslashes or newlines arrives as it was sent
//! (checked on Windows 11 with `examples/wslprobe.rs`).
//!
//! [`Local`] and its builders are plain values and functions, the same on every system; `cfg`
//! only decides which one a build may pick ([`Local::from_file`]). The one in use is a
//! process-wide choice made once at start ([`use_local`]), as the shared ssh connection is: a
//! process that never calls it (the tests) runs its commands as Unix does.
//!
//! **Keep-alive.** Windows stops a distro a short while after its last `wsl.exe` ended, and tmux
//! with it. [`ensure_keepalive`] leaves one hidden `wsl.exe` behind that only sleeps, marked
//! [`KEEPALIVE_MARK`], unless the distro has one already. No setting of Windows or WSL is
//! changed, and nothing here ends the holder: `pkill -f tmls-keepalive` inside WSL does.

use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use crate::tmux::{run, strings};

/// How this machine runs a command.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Local {
    /// As it is: `sh -c SCRIPT`, `tmux …`.
    #[default]
    Sh,
    /// Inside WSL: `wsl.exe [-d DISTRO] -e …`. Without a distro it is WSL's default one.
    Wsl { distro: Option<String> },
}

impl Local {
    /// What the `wsl` file says: with `text` its contents (`None`: no such file) and `windows`
    /// whether this is a Windows build. The first line that is not blank or a `#` comment names
    /// the distro; a file without one means WSL's default distro. Anywhere but on Windows, and
    /// without the file, commands run as they are.
    pub fn from_file(text: Option<&str>, windows: bool) -> Local {
        match text {
            Some(text) if windows => {
                let named = text.lines().map(|line| line.split('#').next().unwrap_or("").trim());
                // "-…" would reach wsl.exe as an option
                let distro = named.filter(|line| !line.is_empty() && !line.starts_with('-')).map(str::to_string).next();
                Local::Wsl { distro }
            }
            _ => Local::Sh,
        }
    }

    pub fn is_wsl(&self) -> bool {
        matches!(self, Local::Wsl { .. })
    }

    /// What comes before a Unix argv to run it on this machine: nothing, or `wsl.exe [-d DISTRO] -e`.
    pub fn prefix(&self) -> Vec<String> {
        match self {
            Local::Sh => Vec::new(),
            Local::Wsl { distro: None } => strings(&["wsl.exe", "-e"]),
            Local::Wsl { distro: Some(distro) } => strings(&["wsl.exe", "-d", distro, "-e"]),
        }
    }

    /// `argv` (a program of this machine and its arguments) as this machine runs it.
    pub fn argv(&self, argv: Vec<String>) -> Vec<String> {
        let mut all = self.prefix();
        all.extend(argv);
        all
    }

    /// `script` run by this machine's sh.
    pub fn script(&self, script: String) -> Vec<String> {
        self.argv(vec!["sh".to_string(), "-c".to_string(), script])
    }

    /// This machine's sh reading its script from stdin (the listing).
    pub fn stdin_script(&self) -> Vec<String> {
        self.argv(strings(&["sh", "-s"]))
    }

    /// What the desktop's opener is given for the file or folder at the absolute `path` of this
    /// machine: the path itself, or for WSL `//wsl.localhost/DISTRO/PATH`, which Windows reads
    /// as the file inside the distro once the slashes are backslashes (`links::opener` turns
    /// them). `asked`: the distro's name as WSL gave it, for when the `wsl` file names none.
    /// `None` when no name is known.
    pub fn desktop_path(&self, path: &str, asked: Option<&str>) -> Option<String> {
        match self {
            Local::Sh => Some(path.to_string()),
            Local::Wsl { distro } => {
                let distro = distro.as_deref().or(asked).filter(|name| !name.is_empty())?;
                Some(format!("//wsl.localhost/{distro}/{}", path.trim_start_matches('/')))
            }
        }
    }
}

/// The choice [`use_local`] made; never set in a process that does not call it.
static HERE: OnceLock<Local> = OnceLock::new();

/// The default distro's name, once WSL said it ([`ensure_keepalive`]).
static DISTRO: OnceLock<String> = OnceLock::new();

/// From now on every command for this machine runs as `local` says. Call once, at start (with
/// `Config::local`); a second call changes nothing.
pub fn use_local(local: Local) {
    let _ = HERE.set(local);
}

/// How this machine runs a command: what [`use_local`] was given, else as it is.
pub fn here() -> &'static Local {
    static SH: Local = Local::Sh;
    HERE.get().unwrap_or(&SH)
}

/// [`Local::desktop_path`] for this process; the error is the line for the status line.
pub fn desktop_path(path: &str) -> Result<String, String> {
    here()
        .desktop_path(path, DISTRO.get().map(String::as_str))
        .ok_or_else(|| format!("{path}: WSL has not said its distro's name yet"))
}

/// What marks the holder inside WSL: its `$0`, so `pgrep -f` finds it.
pub const KEEPALIVE_MARK: &str = "tmls-keepalive";

/// How long the check may take: a distro that was stopped needs seconds to start.
pub const KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(60);

/// Prints the distro's name, then `held` when a holder runs in it, else `none`. The pattern is
/// `tmls-keepaliv[e]`: it matches the mark and not this script's own command line.
const CHECK_SCRIPT: &str =
    "echo \"distro $WSL_DISTRO_NAME\"; if pgrep -f 'tmls-keepaliv[e]' >/dev/null 2>&1; then echo held; else echo none; fi";

/// What the holder runs: sleeps for good. A loop and not `exec sleep`, so the process keeps the
/// mark in its command line.
const HOLD_SCRIPT: &str = "while :; do sleep 86400; done";

/// The command that says whether the distro has a holder ([`parse_keepalive_check`]).
pub fn keepalive_check_argv(local: &Local) -> Vec<String> {
    local.script(CHECK_SCRIPT.to_string())
}

/// What [`keepalive_check_argv`]'s command printed: the distro's name if it gave one, and
/// whether a holder runs. `None` when it said neither `held` nor `none`.
pub fn parse_keepalive_check(out: &str) -> Option<(Option<String>, bool)> {
    let mut lines = out.lines().map(str::trim);
    let distro = lines.clone().find_map(|line| line.strip_prefix("distro ")).map(str::trim);
    let held = lines.find_map(|line| match line {
        "held" => Some(true),
        "none" => Some(false),
        _ => None,
    })?;
    Some((distro.filter(|name| !name.is_empty()).map(str::to_string), held))
}

/// The holder: sh in the distro, sleeping, with [`KEEPALIVE_MARK`] as its `$0`.
pub fn keepalive_argv(local: &Local) -> Vec<String> {
    let mut argv = local.script(HOLD_SCRIPT.to_string());
    argv.push(KEEPALIVE_MARK.to_string());
    argv
}

/// What [`ensure_keepalive`] found or did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keepalive {
    /// This machine is not WSL: nothing to hold.
    NotNeeded,
    /// The distro had a holder already.
    Held,
    /// A holder was started now.
    Started,
}

/// Makes sure the distro this machine is ([`here`]) has one holder, and learns the distro's
/// name for [`desktop_path`]. Blocks while WSL starts and answers (up to
/// [`KEEPALIVE_TIMEOUT`]): not for the UI thread. The error is a line for the log.
pub fn ensure_keepalive() -> Result<Keepalive, String> {
    ensure_keepalive_of(here())
}

/// [`ensure_keepalive`] for `local`.
pub fn ensure_keepalive_of(local: &Local) -> Result<Keepalive, String> {
    if !local.is_wsl() {
        return Ok(Keepalive::NotNeeded);
    }
    let out = run(&keepalive_check_argv(local), KEEPALIVE_TIMEOUT).map_err(|e| format!("WSL did not answer: {e:#}"))?;
    let (distro, held) = parse_keepalive_check(&out).ok_or("WSL did not say whether it is held")?;
    if let Some(distro) = distro {
        let _ = DISTRO.set(distro);
    }
    if held {
        return Ok(Keepalive::Held);
    }
    spawn_holder(&keepalive_argv(local)).map_err(|e| format!("the WSL holder did not start: {e}"))?;
    Ok(Keepalive::Started)
}

/// Starts `argv` so that it outlives this process, with no window and no pipes.
#[cfg(windows)]
fn spawn_holder(argv: &[String]) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    // A console of its own that is never shown: closing ours does not end it.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    // Out of the job of whatever started us (a terminal, sshd), which may end with it; refused
    // by a job that does not let go, and then the holder is started inside it.
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    let spawn = |flags: u32| {
        Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(flags)
            .spawn()
    };
    // A child inherits every handle of ours that can be inherited, whatever its own stdio is:
    // the holder would keep our console's or our caller's pipes open for as long as it lives
    // (an ssh that ran us would never end). Ours stop being inheritable; a later child still
    // gets its stdio, which `Command` hands over as copies of its own.
    {
        use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation};
        use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
        for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            // SAFETY: both calls take plain values; a handle that is none or invalid is skipped.
            unsafe {
                let handle = GetStdHandle(which);
                if handle != 0 && handle != INVALID_HANDLE_VALUE {
                    SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
                }
            }
        }
    }
    let base = CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP;
    // Not waited for: it ends when something ends it, long after us.
    spawn(base | CREATE_BREAKAWAY_FROM_JOB).or_else(|_| spawn(base)).map(drop)
}

/// The same on Unix, where no build picks WSL: a process group of its own.
#[cfg(unix)]
fn spawn_holder(argv: &[String]) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wsl(distro: Option<&str>) -> Local {
        Local::Wsl { distro: distro.map(str::to_string) }
    }

    #[test]
    fn the_wsl_file_counts_on_windows_only() {
        assert_eq!(Local::from_file(None, true), Local::Sh);
        assert_eq!(Local::from_file(None, false), Local::Sh);
        assert_eq!(Local::from_file(Some("archlinux\n"), false), Local::Sh);
        assert_eq!(Local::from_file(Some(""), true), wsl(None));
        assert_eq!(Local::from_file(Some("\n  # the default\n\n"), true), wsl(None));
        assert_eq!(Local::from_file(Some("# mine\n\n  archlinux  # arch\nUbuntu\n"), true), wsl(Some("archlinux")));
        assert_eq!(Local::from_file(Some("Ubuntu 24.04\n"), true), wsl(Some("Ubuntu 24.04")));
        // never an option of wsl.exe
        assert_eq!(Local::from_file(Some("--shutdown\n"), true), wsl(None));
    }

    #[test]
    fn unix_runs_a_command_as_it_is() {
        assert_eq!(Local::Sh.prefix(), Vec::<String>::new());
        assert_eq!(Local::Sh.argv(strings(&["tmux", "kill-session", "-t", "=a b"])), ["tmux", "kill-session", "-t", "=a b"]);
        assert_eq!(Local::Sh.script("echo 'a b'".to_string()), ["sh", "-c", "echo 'a b'"]);
        assert_eq!(Local::Sh.stdin_script(), ["sh", "-s"]);
        assert_eq!(here(), &Local::Sh, "a process that never chose runs as Unix does");
    }

    #[test]
    fn wsl_runs_the_same_words_after_wsl_exe() {
        assert_eq!(
            wsl(None).argv(strings(&["tmux", "-u", "attach", "-t", "=it's \"mine\""])),
            ["wsl.exe", "-e", "tmux", "-u", "attach", "-t", "=it's \"mine\""]
        );
        assert_eq!(
            wsl(Some("archlinux")).argv(strings(&["tmux", "kill-session", "-t", "=a b"])),
            ["wsl.exe", "-d", "archlinux", "-e", "tmux", "kill-session", "-t", "=a b"]
        );
        let script = "cd -- \"$HOME\"/'a b' || { echo 'no'; exit 3; }\ntmux new-session -d -s 'x\\y'";
        assert_eq!(wsl(None).script(script.to_string()), ["wsl.exe", "-e", "sh", "-c", script]);
        assert_eq!(wsl(Some("Ubuntu 24.04")).script(script.to_string()), ["wsl.exe", "-d", "Ubuntu 24.04", "-e", "sh", "-c", script]);
        assert_eq!(wsl(None).stdin_script(), ["wsl.exe", "-e", "sh", "-s"]);
        assert_eq!(wsl(Some("archlinux")).stdin_script(), ["wsl.exe", "-d", "archlinux", "-e", "sh", "-s"]);
    }

    #[test]
    fn a_file_of_wsl_has_a_windows_address() {
        assert_eq!(Local::Sh.desktop_path("/home/u/a b.png", None).as_deref(), Some("/home/u/a b.png"));
        assert_eq!(
            wsl(Some("archlinux")).desktop_path("/home/u/a b.png", Some("other")).as_deref(),
            Some("//wsl.localhost/archlinux/home/u/a b.png")
        );
        assert_eq!(wsl(None).desktop_path("/", Some("Ubuntu")).as_deref(), Some("//wsl.localhost/Ubuntu/"));
        assert_eq!(wsl(None).desktop_path("/etc/hosts", None), None);
        assert_eq!(wsl(None).desktop_path("/etc/hosts", Some("")), None);
    }

    #[test]
    fn the_holder_is_marked_and_the_check_does_not_find_itself() {
        let hold = keepalive_argv(&wsl(Some("archlinux")));
        assert_eq!(hold, ["wsl.exe", "-d", "archlinux", "-e", "sh", "-c", HOLD_SCRIPT, KEEPALIVE_MARK]);
        let check = keepalive_check_argv(&wsl(None));
        assert_eq!(check[..4], ["wsl.exe", "-e", "sh", "-c"]);
        assert!(!check.iter().any(|word| word.contains(KEEPALIVE_MARK)), "pgrep -f would match the check");
        assert_eq!(parse_keepalive_check("distro archlinux\nheld\n"), Some((Some("archlinux".to_string()), true)));
        assert_eq!(parse_keepalive_check("distro archlinux\r\nnone\r\n"), Some((Some("archlinux".to_string()), false)));
        assert_eq!(parse_keepalive_check("distro \nnone\n"), Some((None, false)));
        assert_eq!(parse_keepalive_check("distro archlinux\n"), None);
        assert_eq!(parse_keepalive_check(""), None);
    }

    /// The check and the holder for real, with sh standing in for WSL: no holder, then one.
    #[cfg(unix)]
    #[test]
    fn the_check_sees_a_holder_only_while_one_runs() {
        assert_eq!(ensure_keepalive_of(&Local::Sh), Ok(Keepalive::NotNeeded));
        // a mark of this test's own, so a holder of anything else on this machine is not seen
        let mark = format!("tmls-keepalive-test-{}", std::process::id());
        let pattern = format!("{}[{}]", &mark[..mark.len() - 1], &mark[mark.len() - 1..]);
        let check = || {
            let script = CHECK_SCRIPT.replace("tmls-keepaliv[e]", &pattern);
            parse_keepalive_check(&run(&Local::Sh.script(script), Duration::from_secs(10)).unwrap()).unwrap().1
        };
        assert!(!check());
        use std::os::unix::process::CommandExt;
        let mut holder =
            Command::new("sh").args(["-c", HOLD_SCRIPT, &mark]).stdin(Stdio::null()).process_group(0).spawn().unwrap();
        let held = check();
        // SAFETY: kill takes no pointers; the child is not reaped yet, so its pid, which is
        // also its group's id, is its own. The group: sh and the sleep it started.
        unsafe { libc::kill(-(holder.id() as libc::pid_t), libc::SIGKILL) };
        let _ = holder.wait();
        assert!(held);
        assert!(!check());
    }
}
