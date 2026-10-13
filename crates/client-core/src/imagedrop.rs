//! An image file dropped on a tab whose session runs on another host (tmls `image_paste.py`):
//! the file is copied to that host, under `~/.cache/tmls/images/` — the place tmls uses, so
//! both apps share it — and the path there is what gets pasted. A session on this machine
//! reads the file where it is, so its path is pasted as it came.
//!
//! What was dropped arrives as text: a path, a `file://` URI, or a `text/uri-list` of them
//! (`uris`). Only one image at a time is copied (`dropped_image`); anything else is text.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::tmux;

/// The file name endings that count as an image (tmls `FILE_SUFFIXES`), lower case.
pub const SUFFIXES: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];

/// A larger file is not copied: the copy runs through ssh's stdin, and a Claude session has
/// no use for one.
pub const MAX_BYTES: u64 = 32 * 1024 * 1024;

/// How long the copy to the host may take.
pub const TIMEOUT: Duration = Duration::from_secs(20);

/// The directory on the host, under its home.
const DIR: &str = ".cache/tmls/images";

/// The lines of a `text/uri-list` that name something: comments (`#`) and empty lines are not
/// among them, and the line ends (`\r\n`) are gone.
pub fn uris(list: &str) -> Vec<&str> {
    list.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

/// The path one line names: a `file://` URI of this machine, decoded, or the line itself.
/// `None` for an empty line, more than one line, or a `file://` URI of another machine.
pub fn local_path(text: &str) -> Option<PathBuf> {
    let value = text.trim();
    if value.is_empty() || value.contains(['\n', '\r']) {
        return None;
    }
    match value.strip_prefix("file://") {
        Some(rest) => {
            let (authority, path) = rest.split_at(rest.find('/')?);
            if !(authority.is_empty() || authority == "localhost") {
                return None;
            }
            Some(PathBuf::from(percent_decoded(path)?))
        }
        None => Some(PathBuf::from(value)),
    }
}

/// The text to paste for a dropped `text/uri-list`: one file's path as it is (so that
/// `dropped_image` can know it, and a prompt reads it); several, each quoted for a shell where
/// it needs it, with a space between. `None` when the list names nothing.
pub fn dropped_text(list: &str) -> Option<String> {
    let entry = |uri: &str| match local_path(uri) {
        Some(path) => path.to_string_lossy().into_owned(),
        None => uri.to_string(),
    };
    match uris(list).as_slice() {
        [] => None,
        [one] => Some(entry(one)),
        many => Some(many.iter().map(|uri| quoted(&entry(uri))).collect::<Vec<_>>().join(" ")),
    }
}

/// `word` as one word of a shell command line.
fn quoted(word: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "_./-+@%=:,".contains(c);
    if !word.is_empty() && word.chars().all(plain) {
        return word.to_string();
    }
    format!("'{}'", word.replace('\'', "'\\''"))
}

/// The path `text` names when it is one path or one `file://` URI of this machine whose name
/// ends like an image. Nothing is looked up on disk.
pub fn named_image(text: &str) -> Option<PathBuf> {
    let path = local_path(text)?;
    let suffix = path.extension()?.to_str()?.to_ascii_lowercase();
    SUFFIXES.contains(&suffix.as_str()).then_some(path)
}

/// `named_image`, when that file exists here.
pub fn dropped_image(text: &str) -> Option<PathBuf> {
    named_image(text).filter(|path| path.is_file())
}

/// `%41` → `A`; `None` when a `%` is not followed by two hex digits or the result is not UTF-8.
fn percent_decoded(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' {
            let hex = std::str::from_utf8(bytes.get(at + 1..at + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            out.push(bytes[at]);
            at += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The name the copy gets on the host: when it was copied (seconds and microseconds since
/// 1970, so two drops never share one) and the file's own ending, lower case.
pub fn copy_name(now: SystemTime, path: &Path) -> Option<String> {
    let suffix = path.extension()?.to_str()?.to_ascii_lowercase();
    if !SUFFIXES.contains(&suffix.as_str()) {
        return None;
    }
    let since = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    Some(format!("{}-{:06}.{suffix}", since.as_secs(), since.subsec_micros()))
}

/// What `host` runs to take the file from stdin as `name` (one of `copy_name`: digits, a
/// dash, a dot and letters, so it needs no quoting).
pub fn store_argv(host: &str, name: &str) -> Vec<String> {
    tmux::ssh(host, format!("mkdir -p \"$HOME/{DIR}\" && cat > \"$HOME/{DIR}/{name}\""))
}

/// The same on this machine, for when it is WSL (`local::here`): the file is one of Windows,
/// and a session inside WSL reads the copy.
pub fn store_here_argv(name: &str) -> Vec<String> {
    crate::local::here().script(format!("mkdir -p \"$HOME/{DIR}\" && cat > \"$HOME/{DIR}/{name}\""))
}

/// The path to paste for a copy named `name`: the host's shell and Claude both read `~/`.
pub fn pasted_path(name: &str) -> String {
    format!("~/{DIR}/{name}")
}

/// Runs an argv with bytes on its stdin and returns what it printed (`tmux::run_with_stdin`
/// with a timeout; a recorder in tests).
pub type Feed<'a> = &'a dyn Fn(&[String], &[u8]) -> anyhow::Result<String>;

/// Copies the image at `path` to `host` through `run` (argv, stdin → what it printed) and
/// returns the path to paste there. Blocks for as long as `run` does: not for the UI thread.
pub fn store(
    host: &str,
    path: &Path,
    now: SystemTime,
    run: Feed<'_>,
) -> Result<String, String> {
    store_with(path, now, run, &|name| store_argv(host, name))
}

/// [`store`] into this machine's WSL ([`store_here_argv`]).
pub fn store_here(path: &Path, now: SystemTime, run: Feed<'_>) -> Result<String, String> {
    store_with(path, now, run, &store_here_argv)
}

fn store_with(
    path: &Path,
    now: SystemTime,
    run: Feed<'_>,
    argv: &dyn Fn(&str) -> Vec<String>,
) -> Result<String, String> {
    let shown = path.display();
    let name = copy_name(now, path).ok_or_else(|| format!("{shown} is not an image file"))?;
    let size = std::fs::metadata(path).map_err(|e| format!("{shown}: {e}"))?.len();
    if size > MAX_BYTES {
        return Err(format!("{shown} is larger than {} MiB", MAX_BYTES / (1024 * 1024)));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{shown}: {e}"))?;
    run(&argv(&name), &bytes).map_err(|e| format!("{e:#}"))?;
    Ok(pasted_path(&name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_or_a_file_uri_of_this_machine_that_ends_like_an_image() {
        let path = |text: &str| named_image(text).map(|p| p.to_string_lossy().into_owned());
        assert_eq!(path("/tmp/a.png").as_deref(), Some("/tmp/a.png"));
        assert_eq!(path("  /tmp/shot.JPG\n").as_deref(), Some("/tmp/shot.JPG"));
        assert_eq!(path("file:///home/u/my%20pic.webp").as_deref(), Some("/home/u/my pic.webp"));
        assert_eq!(path("file://localhost/a/b.gif").as_deref(), Some("/a/b.gif"));
        assert_eq!(path("file:///a/caf%C3%A9.jpeg").as_deref(), Some("/a/café.jpeg"));
    }

    #[test]
    fn anything_else_is_text() {
        for text in [
            "",
            "   ",
            "/tmp/a.txt",
            "/tmp/noext",
            "/tmp/a.png\n/tmp/b.png",
            "file://otherhost/a.png",
            "file:///a/bad%zz.png",
            "file:///a/cut%4",
            "https://example.com/a.png extra\rline",
        ] {
            assert_eq!(named_image(text), None, "{text:?}");
        }
        // It ends like an image, but there is no such file.
        assert_eq!(dropped_image("/nonexistent/dir/a.png"), None);
    }

    #[test]
    fn a_uri_list_without_its_comments() {
        let list = "# from the file manager\r\nfile:///a/one.png\r\n\r\nfile:///a/two.png\r\n";
        assert_eq!(uris(list), ["file:///a/one.png", "file:///a/two.png"]);
        assert!(uris("\r\n# only a comment\r\n").is_empty());
    }

    #[test]
    fn a_drop_is_pasted_as_paths() {
        assert_eq!(dropped_text("file:///a/my%20pic.png\r\n").as_deref(), Some("/a/my pic.png"));
        assert_eq!(
            dropped_text("file:///a/one.png\r\nfile:///b/it's%20two.txt\r\n").as_deref(),
            Some("/a/one.png '/b/it'\\''s two.txt'")
        );
        // Not a file of this machine: pasted as it came.
        assert_eq!(dropped_text("https://example.com/x\r\n").as_deref(), Some("https://example.com/x"));
        assert_eq!(dropped_text("# nothing\r\n"), None);
    }

    #[test]
    fn the_copy_is_named_after_the_time_and_keeps_the_ending() {
        let now = UNIX_EPOCH + Duration::new(1_760_000_000, 42_000);
        assert_eq!(copy_name(now, Path::new("/x/Shot One.PNG")).as_deref(), Some("1760000000-000042.png"));
        assert_eq!(copy_name(now, Path::new("/x/notes.txt")), None);
        assert_eq!(pasted_path("1-000002.png"), "~/.cache/tmls/images/1-000002.png");
    }

    #[test]
    fn the_host_takes_the_file_from_stdin() {
        let argv = store_argv("nas", "1-000002.png");
        assert_eq!(argv.first().map(String::as_str), Some("ssh"));
        assert!(argv.contains(&"nas".to_string()));
        assert_eq!(
            argv.last().map(String::as_str),
            Some("mkdir -p \"$HOME/.cache/tmls/images\" && cat > \"$HOME/.cache/tmls/images/1-000002.png\"")
        );
    }
}
