//! The commands that find and read a file named in a session's terminal, on that session's host.
//! Ported from tmls (`viewer.py`: `locate`, `load_file`, `load_image`).
//!
//! As in `tmux`, every builder returns an argv, never a shell string: `sh -c SCRIPT` or the
//! program itself on this machine, `ssh … HOST COMMAND` on any other. The path comes from
//! terminal output and is not trusted: it is always one single-quoted word of the script, and
//! what the commands are given is the absolute path `locate_argv` resolved, after `--`, so a
//! name that starts with `-` is never an option.
//!
//! A relative path is resolved against the current directory of the session's active pane
//! (`tmux display-message -p -t '=NAME:' '#{pane_current_path}'`; tmux 3.3a needs the colon),
//! `~` and `~/…` against the host's home. Nothing is written on the host.
//!
//! The images a Claude session made (`made_argv`, `parse_made`): every image path its transcript
//! names that is a file on the host now, newest first. The only thing put into that command is
//! Claude's id of the session, and only when it is one (`valid_session_id`).

use crate::listing::SessionKey;
use crate::tmux::{folder_word, quote, script_argv, ssh, strings};

/// What a located path is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A regular file (or a link to one).
    File,
    Dir,
    /// A device, a pipe, a socket: nothing to read to its end.
    Other,
}

/// A path as the host knows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Located {
    /// Absolute.
    pub path: String,
    pub kind: Kind,
    /// Bytes of a regular file; 0 for everything else.
    pub size: u64,
}

/// Prints what `path` is on the session's host, for [`parse_located`]: a line `f`, `d` or `o`, a
/// line with the size in bytes, then the absolute path. A relative path that is not in the
/// pane's directory is looked for in the directories right below it, and taken when exactly one
/// has it (or all that have it mean the same file, as with a path that starts with `..`): Claude
/// names files from the directory it works in, which may be one below the one it
/// was started in. Fails with `not found` on stderr when
/// nothing is there, and with the shell's own words when the file cannot be read.
pub fn locate_argv(key: &SessionKey, path: &str, local_name: &str) -> Vec<String> {
    let fixed = path.starts_with('/') || path == "~" || path.starts_with("~/");
    let set = if fixed {
        format!("p={}", folder_word(path))
    } else {
        // Relative: against the directory the session's active pane is in.
        let target = quote(&format!("={}:", key.name));
        format!(
            "d=$(tmux display-message -p -t {target} '#{{pane_current_path}}') || exit 1; \
             [ -n \"$d\" ] || {{ echo 'the session has no current directory' >&2; exit 1; }}; \
             r={}; q=; p=\"$d/$r\"; \
             [ -e \"$p\" ] || {{ n=0; for c in \"$d\"/*/; do \
             [ -e \"$c$r\" ] && ! [ \"$c$r\" -ef \"$q\" ] && {{ n=$((n+1)); q=\"$c$r\"; }}; done; \
             [ \"$n\" = 1 ] && {{ p=\"$q\"; e=$(cd \"${{q%/*}}\" 2>/dev/null && pwd -P) && \
             [ -e \"$e/${{q##*/}}\" ] && p=\"$e/${{q##*/}}\"; }}; }}",
            quote(path)
        )
    };
    let script = format!(
        "{set}; [ -e \"$p\" ] || {{ echo 'not found' >&2; exit 1; }}; \
         if [ -d \"$p\" ]; then echo d; echo 0; \
         elif [ -f \"$p\" ]; then echo f; wc -c <\"$p\" || exit 1; \
         else echo o; echo 0; fi; printf %s \"$p\""
    );
    script_argv(&key.host, script, local_name)
}

/// Reads what [`locate_argv`]'s command printed. `None` for anything else, and for a path that
/// is not absolute (a home or a pane directory the host could not name).
pub fn parse_located(out: &str) -> Option<Located> {
    let (kind, rest) = out.split_once('\n')?;
    let (size, path) = rest.split_once('\n')?;
    let kind = match kind.trim() {
        "f" => Kind::File,
        "d" => Kind::Dir,
        "o" => Kind::Other,
        _ => return None,
    };
    let size = size.trim().parse().ok()?;
    path.starts_with('/').then(|| Located { path: path.to_string(), kind, size })
}

/// Prints the first `max_bytes` bytes of the file at `path` on `host`. `path` is absolute, as
/// [`parse_located`] gave it.
pub fn read_argv(host: &str, path: &str, max_bytes: u64, local_name: &str) -> Vec<String> {
    let max = max_bytes.to_string();
    if host == local_name {
        strings(&["head", "-c", &max, "--", path])
    } else {
        ssh(host, format!("head -c {max} -- {}", quote(path)))
    }
}

/// The most images a session's list has.
pub const MADE_MAX: usize = 500;

/// An image a Claude session's transcript names, as it is on the host now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Made {
    /// Absolute.
    pub path: String,
    /// When it was last written: seconds since 1970.
    pub modified_s: u64,
    pub bytes: u64,
}

/// The host has no transcript of the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoTranscript;

/// What [`made_argv`]'s command prints for a session without a transcript.
const NO_TRANSCRIPT: &str = "no-transcript";

/// Whether `id` is a session id as Claude Code writes them: 36 characters, each a lower-case hex
/// digit or `-`. Nothing else is ever put into [`made_argv`]'s command, so no character of an id
/// means anything to a shell.
pub fn valid_session_id(id: &str) -> bool {
    id.len() == 36 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'-'))
}

/// Prints the images the Claude session `session_id` made on `host`, for [`parse_made`]: of
/// every absolute path in its transcript that ends like an image (`png`, `jpg`, `jpeg`, `gif`,
/// `webp`, `bmp`) and is a file now, a line `MODIFIED<TAB>BYTES<TAB>PATH`, the newest first, at
/// most [`MADE_MAX`]. A path with a space or any character outside `A-Za-z0-9_.~+@%=/-` is not
/// found (a comma ends one, so `a.jpg,/b.png` is two), and a `file:///x.png` is `/x.png`: slashes
/// at a path's start count as one, so a file has one line. Prints `no-transcript` when the host has no transcript of that id. Reads only.
///
/// `session_id` comes from a file on the host and is the one thing put into the script: an id
/// that is not one ([`valid_session_id`]) is refused, and the command built then only says
/// `no-transcript`. `cwd`, the session's directory, is not used and never part of the command:
/// the transcript is found by its id in whichever project folder has it (the newest, if several
/// do), so the folder's name, which Claude derives from the directory, need not be guessed.
pub fn made_argv(host: &str, session_id: &str, _cwd: Option<&str>, local_name: &str) -> Vec<String> {
    if !valid_session_id(session_id) {
        return script_argv(host, format!("echo {NO_TRANSCRIPT}"), local_name);
    }
    let script = format!(
        "f=$(ls -t \"$HOME\"/.claude/projects/*/{session_id}.jsonl 2>/dev/null | head -n 1)\n\
         [ -n \"$f\" ] || {{ echo \"{NO_TRANSCRIPT}\"; exit 0; }}\n\
         grep -aoE '/[A-Za-z0-9_.~+@%=/-]+\\.(png|jpe?g|gif|webp|bmp)' \"$f\" | sed 's|^//*|/|' | sort -u |\n\
         while IFS= read -r p; do\n  [ -f \"$p\" ] && stat -c '%Y\t%s\t%n' -- \"$p\"\n\
         done | sort -rn | head -n {MADE_MAX}\n"
    );
    script_argv(host, script, local_name)
}

/// Reads what [`made_argv`]'s command printed, in its order, at most [`MADE_MAX`] files. A line
/// that is not `NUMBER<TAB>NUMBER<TAB>/PATH` is skipped (a login script's text).
pub fn parse_made(out: &str) -> Result<Vec<Made>, NoTranscript> {
    let mut files = Vec::new();
    for line in out.lines() {
        if line.trim() == NO_TRANSCRIPT {
            return Err(NoTranscript);
        }
        let mut fields = line.splitn(3, '\t');
        let (Some(modified), Some(bytes), Some(path)) = (fields.next(), fields.next(), fields.next()) else { continue };
        let (Ok(modified_s), Ok(bytes)) = (modified.parse(), bytes.parse()) else { continue };
        if path.starts_with('/') && files.len() < MADE_MAX {
            files.push(Made { path: path.to_string(), modified_s, bytes });
        }
    }
    Ok(files)
}

/// A moment as this machine's clock shows it, in its own time zone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    pub year: i32,
    /// From 1.
    pub month: u32,
    /// Of the month, from 1.
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

/// `secs` seconds since 1970 on this machine's clock; `None` for a time it cannot name.
#[cfg(unix)]
pub fn local_time(secs: u64) -> Option<LocalTime> {
    let secs = libc::time_t::try_from(secs).ok()?;
    // SAFETY: `tm` is plain numbers (and a pointer `localtime_r` sets), so all zeroes is one;
    // both pointers are to values that live through the call, and `localtime_r` keeps neither.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&secs, &mut tm).is_null() {
            return None;
        }
        tm
    };
    let field = |value: libc::c_int| u32::try_from(value).ok();
    Some(LocalTime {
        year: tm.tm_year.checked_add(1900)?,
        month: field(tm.tm_mon)? + 1,
        day: field(tm.tm_mday)?,
        hour: field(tm.tm_hour)?,
        minute: field(tm.tm_min)?,
    })
}

/// The same on Windows: the seconds become a `FILETIME` (100 ns ticks from 1601), which
/// `FileTimeToSystemTime` reads as UTC and `SystemTimeToTzSpecificLocalTime` moves to this
/// machine's zone.
#[cfg(windows)]
pub fn local_time(secs: u64) -> Option<LocalTime> {
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    // seconds between 1601-01-01 and 1970-01-01, in 100 ns ticks
    const EPOCH_TICKS: u64 = 116_444_736_000_000_000;
    let ticks = secs.checked_mul(10_000_000)?.checked_add(EPOCH_TICKS)?;
    let file = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
    // SAFETY: both `SYSTEMTIME`s are plain numbers written by the calls below; `null` asks for
    // this machine's own zone, and no pointer outlives the call.
    let mut utc: SYSTEMTIME = unsafe { std::mem::zeroed() };
    let mut local: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe {
        if FileTimeToSystemTime(&file, &mut utc) == 0 {
            return None;
        }
        if SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) == 0 {
            return None;
        }
    }
    Some(LocalTime {
        year: i32::from(local.wYear),
        month: u32::from(local.wMonth),
        day: u32::from(local.wDay),
        hour: u32::from(local.wHour),
        minute: u32::from(local.wMinute),
    })
}
