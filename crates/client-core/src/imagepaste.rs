//! Image paste (Alt+Shift+V, tmls `image_paste.py`): the clipboard's image, read with
//! `wl-paste`, becomes a file in `~/.cache/tmls-rs/paste/` on this machine (`stage`). From
//! there it is an image file like a dropped one: for a session on another host `imagedrop`
//! copies it to that host's `~/.cache/tmls/images/` and the path there is pasted; a session on
//! this machine gets the staged file's own path. Nothing here removes a staged file: one that
//! did not reach its host is still there to paste again.
//!
//! The clipboard's reader and the copy's runner are parameters, so no test reads a clipboard.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::imagedrop;

/// Where the clipboard's image is written first, under this machine's home.
pub const STAGING: &str = ".cache/tmls-rs/paste";

/// The status line when the clipboard holds no PNG and no JPEG: text, nothing, another type.
pub const NO_IMAGE: &str = "clipboard has no image";

/// The status line when `wl-paste` could not be started because there is none.
pub const NO_WL_PASTE: &str = "wl-paste is not installed: no image paste";

/// How long one `wl-paste` may take.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// Runs an argv and returns the bytes it printed (`tmux::run_bytes` with `TIMEOUT`; a
/// stand-in for the clipboard in tests).
pub type Reader<'a> = &'a dyn Fn(&[String]) -> anyhow::Result<Vec<u8>>;

/// The image type to ask for, of those `wl-paste --list-types` printed: PNG, else JPEG.
pub fn pick_type(list: &str) -> Option<&'static str> {
    ["image/png", "image/jpeg"].into_iter().find(|mime| list.lines().any(|line| line.trim() == *mime))
}

/// The file name ending for one of `pick_type`'s answers.
pub fn extension(mime: &str) -> &'static str {
    if mime == "image/jpeg" { "jpg" } else { "png" }
}

/// The staged file's name: `imagedrop::copy_name`'s, so the copy on the host, made at the
/// same `now`, has the same one.
pub fn stamp_name(now: SystemTime, ext: &str) -> Option<String> {
    imagedrop::copy_name(now, &Path::new("image").with_extension(ext))
}

/// Why a `wl-paste` failed, for the status line.
fn why_not(e: &anyhow::Error) -> String {
    let not_found = |cause: &(dyn std::error::Error + 'static)| {
        cause.downcast_ref::<std::io::Error>().is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound)
    };
    if e.chain().any(not_found) {
        return NO_WL_PASTE.to_string();
    }
    let said = first_line(&format!("{e:#}"));
    // What wl-paste says of an empty clipboard, and of one without the type asked for.
    if said.contains("Nothing is copied") || said.contains("No suitable type") {
        return NO_IMAGE.to_string();
    }
    format!("couldn't read the clipboard: {said}")
}

/// The first line of `text` that says something: a status line has one row.
fn first_line(text: &str) -> String {
    text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("it failed").to_string()
}

/// Writes the clipboard's image into `staging` (made if it is not there, for the user alone)
/// and returns the file; or what the status line says instead. Never writes over a file.
/// Blocks for as long as `read` does: not for the UI thread.
pub fn stage(staging: &Path, now: SystemTime, read: Reader<'_>) -> Result<PathBuf, String> {
    let wl_paste = |args: &[&str]| {
        let argv: Vec<String> = std::iter::once("wl-paste").chain(args.iter().copied()).map(String::from).collect();
        read(&argv).map_err(|e| why_not(&e))
    };
    let types = wl_paste(&["--list-types"])?;
    let mime = pick_type(&String::from_utf8_lossy(&types)).ok_or_else(|| NO_IMAGE.to_string())?;
    let bytes = wl_paste(&["--type", mime])?;
    if bytes.is_empty() {
        return Err(NO_IMAGE.to_string());
    }
    let name = stamp_name(now, extension(mime)).ok_or_else(|| NO_IMAGE.to_string())?;
    let file = staging.join(name);
    create_staging(staging)
        .and_then(|()| std::fs::OpenOptions::new().write(true).create_new(true).open(&file))
        .and_then(|mut out| out.write_all(&bytes))
        .map_err(|e| format!("couldn't write {}: {e}", file.display()))?;
    Ok(file)
}

/// Makes `staging`, and on Unix for the user alone (mode 0700).
#[cfg(unix)]
fn create_staging(staging: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(staging)
}

/// The same on Windows, which has no file modes.
#[cfg(windows)]
fn create_staging(staging: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(staging)
}

/// A pasted image: the staged file's name, and the path to paste into the session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pasted {
    pub name: String,
    pub path: String,
}

/// The clipboard's image for a session: staged, then copied to `host` (`imagedrop::store`
/// through `feed`; `note` gets the status line while that runs), or with `None` — a session on
/// this machine — left where it was staged (copied into the distro when this machine is
/// WSL). `Err` is the status line, and then there is no
/// path: nothing is to be typed. A staged file whose copy failed stays.
pub fn paste(
    host: Option<&str>,
    staging: &Path,
    now: SystemTime,
    read: Reader<'_>,
    feed: imagedrop::Feed<'_>,
    note: &dyn Fn(String),
) -> Result<Pasted, String> {
    let file = stage(staging, now, read)?;
    let name = file.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let path = match host {
        // WSL: the staged file is one of Windows, the session reads a copy inside the distro
        None if crate::local::here().is_wsl() => {
            note(format!("copying {name} into WSL …"));
            imagedrop::store_here(&file, now, feed).map_err(|why| format!("copy into WSL failed: {}", first_line(&why)))?
        }
        None => file.to_string_lossy().into_owned(),
        Some(host) => {
            note(format!("copying {name} to {host} …"));
            imagedrop::store(host, &file, now, feed)
                .map_err(|why| format!("copy to {host} failed: {}", first_line(&why)))?
        }
    };
    Ok(Pasted { name, path })
}
