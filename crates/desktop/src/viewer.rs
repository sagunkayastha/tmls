//! Opening a file a terminal names: where it goes, how it is fetched, and the panel that shows
//! text. No window and no drawing here; `draw` paints a `SideView`, `app` runs `open_link` on
//! a worker thread and keeps the `SidePanel`.
//!
//! As tmls (`viewer.py`, and `open_file_link` in `app.py`). The file is on the host of the
//! tab's session; `client_core::files` builds the commands, which run here or through ssh,
//! each with a time limit.
//!
//! 1. **Locate** (`locate`): the absolute path, whether it is a file or a folder, its size. A
//!    relative path is under the directory of the session's pane, `~/` under the host's home.
//! 2. **Text** goes to the panel: an extension that is not in `OUTSIDE`, at most `MAX_BYTES`
//!    bytes, valid UTF-8 and no NUL byte (`Opened::Text`).
//! 3. **Everything else** — a folder, an image, a PDF, an archive, a file that is too large or
//!    not text — is for the desktop's own app (`Opened::Outside`, which `links::open_path`
//!    gives to `xdg-open`). A file of this machine is opened where it is. One of another host
//!    is copied first, to `$XDG_CACHE_HOME/tmls-rs/view/HOST/NAME` (`~/.cache` without the
//!    variable), if it has at most `MAX_OUTSIDE_BYTES` bytes; a folder of another host is
//!    refused. (tmls gives the desktop an `sftp://` address instead; a copy needs no KDE.)
//!    A copy of the same name from the same host replaces the one before.
//!
//! **The panel** (`SidePanel`) has the right half of the terminal's area, one at a time for
//! the window. Its body is one of `Body`'s kinds; text is the only one yet (tmls also has an
//! image pane). `TextBody` keeps the lines, the line that was asked for and how far it is
//! scrolled; `rows` gives what is in view. A line longer than the panel is cut, not wrapped
//! (tmls: `soft_wrap=False`); a tab goes to the next multiple of `TAB_COLS` columns. The panel
//! never has the keyboard.

use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use client_core::SessionKey;
use client_core::files::{self, Kind, Located};
use client_core::tmux;
use s2_wgpu::gfx::grid::clusters;

use crate::links::FileLink;

/// The largest file the panel shows (tmls `MAX_BYTES`).
pub const MAX_BYTES: usize = 2_000_000;
/// The largest file copied from another host to be opened here (tmls `MAX_IMAGE_BYTES`).
pub const MAX_OUTSIDE_BYTES: u64 = 25_000_000;
/// Extensions the desktop's own apps show instead of the panel: images, PDFs, media,
/// archives, office files (tmls `OUTSIDE`).
pub const OUTSIDE: [&str; 35] = [
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "tif", "tiff", "ico", "avif", "heic", "pdf",
    "mp4", "mkv", "webm", "mov", "avi", "mp3", "wav", "ogg", "flac", "m4a", "zip", "tar", "gz",
    "tgz", "xz", "7z", "docx", "xlsx", "pptx", "odt", "ods", "odp",
];
/// Columns between tab stops (Textual's `TextArea`, which tmls shows files in).
pub const TAB_COLS: usize = 4;
/// What the status line says of a click on a text file when the window cannot give the panel
/// and the terminal `PANEL_MIN_COLS` columns each (`draw::layout_with` lays out no panel then):
/// the file was fetched, nothing opened.
pub const TOO_NARROW: &str = "window too narrow for the file panel";

/// How long locating and reading text may take (ssh's own connect limit is 5 s), and how long
/// a copy of up to `MAX_OUTSIDE_BYTES` (tmls: 60 s for an image).
pub const LOCATE_TIMEOUT: Duration = Duration::from_secs(10);
pub const READ_TIMEOUT: Duration = Duration::from_secs(10);
pub const COPY_TIMEOUT: Duration = Duration::from_secs(60);

/// Runs a command within a time limit and returns the bytes it printed; `run` in the app.
pub type Runner<'a> = &'a dyn Fn(&[String], Duration) -> anyhow::Result<Vec<u8>>;

/// The runner the app uses: `tmux::run_bytes`, which kills a command that takes too long.
pub fn run(argv: &[String], timeout: Duration) -> anyhow::Result<Vec<u8>> {
    tmux::run_bytes(argv, timeout)
}

/// A path's extension in lower case, as Python's `posixpath.splitext` splits: none for
/// `.bashrc`, `gz` for `x.tar.gz`.
pub fn extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = name.trim_start_matches('.');
    match stem.rsplit_once('.') {
        Some((_, ext)) => ext.to_lowercase(),
        None => String::new(),
    }
}

/// Whether the desktop's apps get it and not the panel, by what it is called: a folder, or
/// an extension in `OUTSIDE` (tmls `opens_outside`).
pub fn opens_outside(path: &str, is_dir: bool) -> bool {
    is_dir || OUTSIDE.contains(&extension(path).as_str())
}

/// A text file, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub host: String,
    /// Absolute, on `host`.
    pub path: String,
    /// The line asked for, from 1.
    pub line: Option<usize>,
    pub text: String,
}

/// What became of a path that was clicked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Opened {
    /// For the panel.
    Text(Document),
    /// For the desktop's app: the file or folder on this machine to give to `xdg-open`, and
    /// the line for the status line once that is started.
    Outside { target: String, said: String },
}

/// `path` with its host, unless that is this machine: how messages name a file.
fn named(host: &str, path: &str, local_name: &str) -> String {
    if host == local_name {
        path.to_string()
    } else {
        format!("{path} on {host}")
    }
}

/// The first line of what went wrong.
fn why(error: &anyhow::Error) -> String {
    let all = format!("{error:#}");
    let line = all.lines().map(str::trim).find(|line| !line.is_empty());
    line.unwrap_or("it failed").to_string()
}

/// Where `path` is on the session's host and what it is; the error is the line for the
/// status line.
pub fn locate(
    key: &SessionKey,
    path: &str,
    local_name: &str,
    run: Runner,
) -> Result<Located, String> {
    let name = named(&key.host, path, local_name);
    let out = run(&files::locate_argv(key, path, local_name), LOCATE_TIMEOUT)
        .map_err(|e| format!("{name}: {}", why(&e)))?;
    files::parse_located(&String::from_utf8_lossy(&out))
        .ok_or_else(|| format!("{name}: the host did not say where that is"))
}

/// The bytes as text for the panel, or back as they are when they are none: not UTF-8, or
/// with a NUL byte (tmls: "binary file").
pub fn text_of(bytes: Vec<u8>) -> Result<String, Vec<u8>> {
    if bytes.contains(&0) {
        return Err(bytes);
    }
    String::from_utf8(bytes).map_err(|e| e.into_bytes())
}

/// Finds the clicked path on the session's host and reads it for the panel, or gets it ready
/// for the desktop's app. `cache`: where copies from other hosts go (`cache_dir`). Blocks
/// for as long as the host takes: not for the UI thread.
pub fn open_link(
    key: &SessionKey,
    link: &FileLink,
    local_name: &str,
    cache: Option<&Path>,
    run: Runner,
) -> Result<Opened, String> {
    let located = locate(key, &link.path, local_name, run)?;
    let host = key.host.as_str();
    let name = named(host, &located.path, local_name);
    let text_sized = located.size <= MAX_BYTES as u64;
    if located.kind != Kind::File || opens_outside(&located.path, false) || !text_sized {
        return outside(host, &located, local_name, cache, None, run);
    }
    // One byte more than fits: a file that grew since is not shown cut off.
    let argv = files::read_argv(host, &located.path, MAX_BYTES as u64 + 1, local_name);
    let bytes = run(&argv, READ_TIMEOUT).map_err(|e| format!("{name}: {}", why(&e)))?;
    if bytes.len() > MAX_BYTES {
        return outside(host, &located, local_name, cache, None, run);
    }
    match text_of(bytes) {
        Ok(text) => Ok(Opened::Text(Document {
            host: host.to_string(),
            path: located.path,
            line: link.line,
            text,
        })),
        // Not text after all: the bytes are the whole file, no need to fetch it again.
        Err(bytes) => outside(host, &located, local_name, cache, Some(bytes), run),
    }
}

/// Gets the file at the absolute `path` on `host` ready for the desktop's app: the panel's
/// `Open in app`.
pub fn open_in_app(
    host: &str,
    path: &str,
    local_name: &str,
    cache: Option<&Path>,
    run: Runner,
) -> Result<Opened, String> {
    // An absolute path asks no session for its directory.
    let key = SessionKey {
        host: host.to_string(),
        name: String::new(),
    };
    let located = locate(&key, path, local_name, run)?;
    outside(host, &located, local_name, cache, None, run)
}

/// What `xdg-open` is to be given for `located`: the path itself on this machine, else a
/// copy in `cache`. `have`: the file's bytes, when they were read already.
fn outside(
    host: &str,
    located: &Located,
    local_name: &str,
    cache: Option<&Path>,
    have: Option<Vec<u8>>,
    run: Runner,
) -> Result<Opened, String> {
    let path = located.path.as_str();
    let name = named(host, path, local_name);
    if located.kind == Kind::Other {
        return Err(format!("{name}: not a file or a folder"));
    }
    if host == local_name {
        return Ok(Opened::Outside {
            target: path.to_string(),
            said: format!("opened {path} in its app"),
        });
    }
    if located.kind == Kind::Dir {
        return Err(format!("{name}: a folder of another host is not opened"));
    }
    let too_large = |bytes: u64| {
        let limit = MAX_OUTSIDE_BYTES / 1_000_000;
        let size = bytes as f64 / 1e6;
        format!("{name}: too large to copy ({size:.1} MB, the limit is {limit} MB)")
    };
    if located.size > MAX_OUTSIDE_BYTES {
        return Err(too_large(located.size));
    }
    let cache = cache.ok_or_else(|| format!("{name}: no cache directory to copy it to"))?;
    let bytes = match have {
        Some(bytes) => bytes,
        None => {
            let argv = files::read_argv(host, path, MAX_OUTSIDE_BYTES + 1, local_name);
            run(&argv, COPY_TIMEOUT).map_err(|e| format!("{name}: {}", why(&e)))?
        }
    };
    if bytes.len() as u64 > MAX_OUTSIDE_BYTES {
        return Err(too_large(bytes.len() as u64));
    }
    let copy = save(cache, host, path, &bytes).map_err(|e| format!("{name}: {e}"))?;
    Ok(Opened::Outside {
        said: format!("copied {name} to {} and opened it", copy.display()),
        target: copy.to_string_lossy().into_owned(),
    })
}

/// Where copies of other hosts' files go: `$XDG_CACHE_HOME/tmls-rs/view`, or
/// `~/.cache/tmls-rs/view`. A variable that is empty or not an absolute path does not count
/// (the XDG base directory rules); `None` when neither does.
pub fn cache_dir(xdg_cache_home: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    let absolute = |dir: Option<&OsStr>| dir.map(PathBuf::from).filter(|dir| dir.is_absolute());
    let base = absolute(xdg_cache_home).or_else(|| Some(absolute(home)?.join(".cache")))?;
    Some(base.join("tmls-rs").join("view"))
}

/// Where the copy of `path` from `host` goes under `cache`: a directory for the host, the
/// file's own name in it. `None` for a path without a name (`/`, `/a/..`).
pub fn cache_path(cache: &Path, host: &str, path: &str) -> Option<PathBuf> {
    let name = path.trim_end_matches('/').rsplit('/').next()?;
    if name.is_empty() || name == "." || name == ".." || name.contains('\0') {
        return None;
    }
    // A host is a word of the user's own config; whatever could leave the directory is not
    // kept.
    let safe = |c: char| c.is_alphanumeric() || "._@-".contains(c);
    let mut dir: String = host
        .chars()
        .map(|c| if safe(c) { c } else { '_' })
        .collect();
    if dir.is_empty() || dir.starts_with('.') {
        dir.insert(0, '_');
    }
    Some(cache.join(dir).join(name))
}

/// Writes the copy; only the user can read it, and it cannot be run.
fn save(cache: &Path, host: &str, path: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let copy = cache_path(cache, host, path).ok_or("it has no name to keep")?;
    let dir = copy.parent().ok_or("it has no name to keep")?;
    let said =
        |e: std::io::Error| format!("the copy could not be written to {}: {e}", dir.display());
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(said)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&copy)
        .map_err(said)?;
    file.write_all(bytes).map_err(said)?;
    Ok(copy)
}

/// One row of text in view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextRow {
    /// The line's number, from 1.
    pub number: usize,
    /// The line with its tabs as spaces. It may be longer than the panel: drawing cuts it.
    pub text: String,
    /// The line that was asked for.
    pub marked: bool,
}

/// What the panel shows now: what `draw` paints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SideView {
    /// The header's text: the file.
    pub title: String,
    pub body: SideBody,
}

/// What is under the header, by the kind of panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SideBody {
    /// `gutter`: the columns of the line numbers and the space after them.
    Text { gutter: usize, rows: Vec<TextRow> },
}

/// A file's text in the panel: its lines, the one that is marked, and the first one in view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextBody {
    lines: Vec<String>,
    /// Index of the marked line.
    mark: Option<usize>,
    /// Index of the first line in view, as last asked for; `top` is what it comes to.
    top: usize,
}

impl TextBody {
    /// `line`: the line to mark, from 1; one past the end marks the last (tmls clamps too).
    pub fn new(text: &str, line: Option<usize>) -> TextBody {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut lines: Vec<String> = text
            .split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
            .collect();
        // The newline that ends the last line starts no line of its own.
        if lines.len() > 1 && lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        let mark = line.map(|line| line.clamp(1, lines.len()) - 1);
        TextBody {
            lines,
            mark,
            top: 0,
        }
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Columns of the line numbers: the digits of the last one, and a space after them.
    pub fn gutter_cols(&self) -> usize {
        self.lines.len().to_string().len() + 1
    }

    /// The last line that can be first in view when `visible` lines fit: the file's end
    /// stays at the panel's foot.
    pub fn max_top(&self, visible: usize) -> usize {
        self.lines.len().saturating_sub(visible.max(1))
    }

    /// Index of the first line in view when `visible` lines fit.
    pub fn top(&self, visible: usize) -> usize {
        self.top.min(self.max_top(visible))
    }

    /// Scrolls so the marked line is in view, a third down the panel (tmls `on_mount`); to
    /// the top without one.
    pub fn reveal(&mut self, visible: usize) {
        self.top = self.mark.map_or(0, |mark| mark.saturating_sub(visible / 3));
        self.top = self.top(visible);
    }

    /// Scrolls by `rows` lines (negative: up). Returns whether anything moved.
    pub fn scroll(&mut self, rows: i32, visible: usize) -> bool {
        let from = self.top(visible);
        let to = (from as i64 + rows as i64).clamp(0, self.max_top(visible) as i64) as usize;
        self.top = to;
        to != from
    }

    /// The lines in view when `visible` lines of `cols` columns fit, the line numbers'
    /// columns included.
    pub fn rows(&self, visible: usize, cols: usize) -> Vec<TextRow> {
        let top = self.top(visible);
        let text_cols = cols.saturating_sub(self.gutter_cols());
        (top..self.lines.len().min(top + visible))
            .map(|index| TextRow {
                number: index + 1,
                text: expand(&self.lines[index], text_cols),
                marked: self.mark == Some(index),
            })
            .collect()
    }
}

/// `line` with each tab as the spaces to the next tab stop, and no more of it than a panel of
/// `cols` columns can show part of: a line of a megabyte costs what is seen of it. (Four
/// characters a column: room for marks that take no column of their own.)
pub fn expand(line: &str, cols: usize) -> String {
    let end = line
        .char_indices()
        .nth(4 * (cols + 2))
        .map_or(line.len(), |(at, _)| at);
    let mut out = String::new();
    let mut col = 0;
    for (n, piece) in line[..end].split('\t').enumerate() {
        if n > 0 {
            let stop = (col / TAB_COLS + 1) * TAB_COLS;
            out.extend(std::iter::repeat_n(' ', stop - col));
            col = stop;
        }
        col += clusters(piece)
            .iter()
            .map(|(_, width)| width)
            .sum::<usize>();
        out.push_str(piece);
    }
    out
}

/// What a panel shows under its header. One kind yet; an image is to be the second.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    Text(TextBody),
}

/// The panel beside the terminal: one file of one host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidePanel {
    pub host: String,
    /// Absolute, on `host`: what `Open in app` opens.
    pub path: String,
    title: String,
    pub body: Body,
}

impl SidePanel {
    /// A panel for a text file. Its header names the host too unless that is this machine
    /// (tmls: `host:path:line`; the line is marked here instead).
    pub fn text(document: &Document, local_name: &str) -> SidePanel {
        let title = match document.host == local_name {
            true => document.path.clone(),
            false => format!("{}:{}", document.host, document.path),
        };
        SidePanel {
            host: document.host.clone(),
            path: document.path.clone(),
            title,
            body: Body::Text(TextBody::new(&document.text, document.line)),
        }
    }

    /// Just opened, with room for `lines` lines: scrolls to what was asked for.
    pub fn reveal(&mut self, lines: usize) {
        match &mut self.body {
            Body::Text(text) => text.reveal(lines),
        }
    }

    /// The wheel over the panel. Returns whether the picture changed.
    pub fn scroll(&mut self, rows: i32, lines: usize) -> bool {
        match &mut self.body {
            Body::Text(text) => text.scroll(rows, lines),
        }
    }

    /// What to draw in a panel of `lines` lines of `cols` columns under its header.
    pub fn view(&self, lines: usize, cols: usize) -> SideView {
        let body = match &self.body {
            Body::Text(text) => SideBody::Text {
                gutter: text.gutter_cols(),
                rows: text.rows(lines, cols),
            },
        };
        SideView {
            title: self.title.clone(),
            body,
        }
    }
}
