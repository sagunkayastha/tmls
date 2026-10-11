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
/// line with the size in bytes, then the absolute path. Fails with `not found` on stderr when
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
             p=\"$d\"/{}",
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
