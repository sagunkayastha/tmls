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
//! 2. **A picture** goes to the panel: an extension in `IMAGES`, at most `MAX_IMAGE_BYTES`
//!    bytes, read and decoded here (`decode`) to RGBA of at most `MAX_SIDE` pixels a side
//!    (`Opened::Image`). One that is too large or no picture after all goes to the desktop's
//!    app like the rest, and the status line says why; one that is broken opens nowhere.
//! 3. **Text** goes to the panel: an extension that is in neither list, at most `MAX_BYTES`
//!    bytes, valid UTF-8 and no NUL byte (`Opened::Text`).
//! 4. **Everything else** — a folder, a PDF, an archive, a picture of another kind, a file
//!    that is too large or not text — is for the desktop's own app (`Opened::Outside`, which `links::open_path`
//!    gives to `xdg-open`). A file of this machine is opened where it is. One of another host
//!    is copied first, to `$XDG_CACHE_HOME/tmls-rs/view/HOST/NAME` (`~/.cache` without the
//!    variable), if it has at most `MAX_OUTSIDE_BYTES` bytes; a folder of another host is
//!    refused. (tmls gives the desktop an `sftp://` address instead; a copy needs no KDE.)
//!    A copy of the same name from the same host replaces the one before.
//!
//! **The panel** (`SidePanel`) has the right half of the terminal's area, one at a time for
//! the window. Its body is one of `Body`'s kinds. `TextBody` keeps the lines, the line that was
//! asked for and how far it is scrolled; `rows` gives what is in view. A line longer than the
//! panel is cut, not wrapped (tmls: `soft_wrap=False`); a tab goes to the next multiple of
//! `TAB_COLS` columns.
//!
//! **A picture** in the panel (`ImageBody`) is fitted into the body, never larger than it is;
//! a click shows it at its own size (one texel a pixel) and the wheel then moves it, a click
//! again fits it. `place` says where it is drawn and which part of it, from the body's
//! rectangle as it is now: it is asked again for every frame, so a window, a font size or a
//! divider that changed never stretches it. The pixels are not kept here: the app uploads
//! them once (`picture::PictureRenderer`) and the panel remembers the size. The header says
//! the size and how large it is drawn (`1280×800 · 43 %`).
//!
//! **The images a session made** (`MadeBody`): the panel can open without a file, on the list
//! of the pictures the Claude session of the tab in front names in its transcript and that
//! are still on its host, newest first (`made` asks the host: `client_core::files::made_argv`;
//! a path with a space is not found). A row is the time or the day the file was written, its
//! name and its folder. One of them is under the cursor; opening it shows the picture as any
//! other, and that panel keeps the list (`SidePanel::back`) to return to. A tab whose session
//! is not Claude's has no list and says so. Only such a panel takes the keys, and only once
//! it was clicked (`SidePanel::takes_keys`): a file's panel never has them. The list's answer
//! comes when the host gives it, whatever the panel shows by then (`SidePanel::list_of`).

use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use client_core::SessionKey;
use client_core::files::{self, Kind, LocalTime, Located, Made};
use client_core::tmux;
use s2_wgpu::gfx::grid::clusters;

use crate::draw::{Rect, keep_end};
use crate::links::FileLink;

/// The largest file the panel shows (tmls `MAX_BYTES`).
pub const MAX_BYTES: usize = 2_000_000;
/// The largest file copied from another host to be opened here (tmls `MAX_IMAGE_BYTES`).
pub const MAX_OUTSIDE_BYTES: u64 = 25_000_000;
/// Extensions of the pictures the panel shows itself (tmls `IMAGES`, less `svg`). A GIF shows
/// its first frame.
pub const IMAGES: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];
/// The largest picture file the panel reads (tmls `MAX_IMAGE_BYTES`).
pub const MAX_IMAGE_BYTES: u64 = 25_000_000;
/// The most pixels a picture may have to be decoded: 200 MB as RGBA.
pub const MAX_IMAGE_PIXELS: u64 = 50_000_000;
/// The longest side a picture keeps; a larger one is made smaller when it is decoded. Every
/// GPU has textures this large.
pub const MAX_SIDE: u32 = 4096;
/// Extensions the desktop's own apps show instead of the panel: the pictures that are not in
/// `IMAGES`, PDFs, media, archives, office files (tmls `OUTSIDE`).
pub const OUTSIDE: [&str; 29] = [
    "svg", "tif", "tiff", "ico", "avif", "heic", "pdf", "mp4", "mkv", "webm", "mov", "avi", "mp3",
    "wav", "ogg", "flac", "m4a", "zip", "tar", "gz", "tgz", "xz", "7z", "docx", "xlsx", "pptx",
    "odt", "ods", "odp",
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
/// How long reading a picture of up to `MAX_IMAGE_BYTES` may take (tmls: 60 s).
pub const IMAGE_TIMEOUT: Duration = Duration::from_secs(60);

/// How long listing a session's images may take: as long as reading a file's text.
pub const MADE_TIMEOUT: Duration = READ_TIMEOUT;

/// What the list says when the session's transcript names no picture that is still there.
pub const NO_IMAGES: &str = "no images from this session yet";
/// What it says in a tab whose session is not Claude's.
pub const NO_CLAUDE: &str = "no Claude session in this tab";
/// While the host is asked.
pub const MADE_LOADING: &str = "looking for images …";
/// The host did not answer; the status line says why.
pub const MADE_FAILED: &str = "the images could not be listed";

/// Columns of a row's time or day, and where its name starts.
const WHEN_COLS: usize = 6;
pub const MADE_NAME_COL: usize = WHEN_COLS + 1;
/// A folder is shown in no fewer columns than this.
const FOLDER_MIN_COLS: usize = 4;

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

/// Whether the panel shows it as a picture, by what it is called: an extension in `IMAGES`.
pub fn is_image(path: &str) -> bool {
    IMAGES.contains(&extension(path).as_str())
}

/// A picture, decoded: `width * height` pixels of four bytes, top row first — red, green,
/// blue as sRGB, and alpha that is not multiplied in.
#[derive(Clone, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Picture {
    /// Without the pixels: megabytes of them say nothing.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (width, height, bytes) = (self.width, self.height, self.rgba.len());
        write!(f, "Picture({width}x{height}, {bytes} bytes)")
    }
}

/// Why a file's bytes are no picture for the panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// More pixels than `MAX_IMAGE_PIXELS`, or more bytes of them than a decode may ask for.
    TooLarge,
    /// No picture by its first bytes, or one of a kind that is not decoded here.
    NotAnImage,
    /// A picture that is cut off or damaged: what the decoder says of it.
    Broken(String),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::TooLarge => f.write_str("image too large"),
            Refused::NotAnImage => f.write_str("not an image"),
            Refused::Broken(why) => write!(f, "broken image: {why}"),
        }
    }
}

impl From<image::ImageError> for Refused {
    fn from(error: image::ImageError) -> Refused {
        use image::error::{ImageError, LimitErrorKind};
        match &error {
            ImageError::Limits(limit)
                if !matches!(limit.kind(), LimitErrorKind::Unsupported { .. }) =>
            {
                return Refused::TooLarge;
            }
            ImageError::Unsupported(_) => return Refused::NotAnImage,
            _ => {}
        }
        // The first line of what the decoder says is wrong.
        let all = error.to_string();
        let line = all.lines().map(str::trim).find(|line| !line.is_empty());
        Refused::Broken(line.unwrap_or("it cannot be decoded").to_string())
    }
}

/// A picture file, decoded: the pixels kept, and the file's own size in pixels (as it is
/// seen, turned by its orientation), which is larger when the picture was made smaller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub picture: Picture,
    pub file: (u32, u32),
}

/// `decode_file` for callers that want the pixels and a line to show.
pub fn decode(bytes: &[u8], max_side: u32) -> Result<Picture, String> {
    match decode_file(bytes, max_side) {
        Ok(decoded) => Ok(decoded.picture),
        Err(refused) => Err(refused.to_string()),
    }
}

/// Decodes a picture file's bytes (PNG, JPEG, GIF, WebP or BMP, told by its first bytes, not
/// its name) to RGBA, turned as its EXIF orientation says and made smaller, its shape kept,
/// if a side is longer than `max_side`. A GIF gives its first frame.
///
/// Refused by the header alone, before a buffer for the pixels is asked for: more than
/// `MAX_IMAGE_PIXELS` pixels, or pixels that take more than four bytes each of that many as
/// the file has them (16 bits a channel count double). So the decoded picture is at most
/// 200 MB; on top of it come what the decoder keeps of its own and, at the end, the RGBA
/// copy of at most `max_side` squared pixels — a quarter of a gigabyte at the peak for the
/// largest picture taken, not a multiple of it. Never panics on bad bytes by intent; the
/// caller guards against a decoder that does (`guarded`). Takes as long as the picture is
/// large: not for the UI thread.
pub fn decode_file(bytes: &[u8], max_side: u32) -> Result<Decoded, Refused> {
    use image::ImageDecoder;
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| Refused::NotAnImage)?;
    if reader.format().is_none() {
        return Err(Refused::NotAnImage);
    }
    // Reads the header only.
    let mut decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 {
        return Err(Refused::Broken("it has no pixels".to_string()));
    }
    if width as u64 * height as u64 > MAX_IMAGE_PIXELS {
        return Err(Refused::TooLarge);
    }
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(4 * MAX_IMAGE_PIXELS);
    // The buffer `from_decoder` is about to ask for; nothing else checks it on this path.
    limits.reserve(decoder.total_bytes())?;
    decoder.set_limits(limits)?;
    // Before the pixels: a decoder that cannot say gives none.
    let orientation = decoder.orientation().ok();
    let mut image = image::DynamicImage::from_decoder(decoder)?;
    if let Some(orientation) = orientation {
        image.apply_orientation(orientation);
    }
    let file = (image.width(), image.height());
    let max_side = max_side.max(1);
    if image.width() > max_side || image.height() > max_side {
        // Fits it into the square with its shape kept; a side never comes to nothing. In
        // the pixels' own kind: no copy of the whole picture as floats.
        image = image.thumbnail(max_side, max_side);
    }
    let image = image.into_rgba8();
    let picture = Picture {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    };
    Ok(Decoded { picture, file })
}

/// Runs `work` and gives its answer; work that panics (a decoder's bug on bytes it did not
/// expect) answers too, so the status line does not stay on "opening …".
pub fn guarded(work: impl FnOnce() -> Result<Opened, String>) -> Result<Opened, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
        .unwrap_or_else(|_| Err("broken image".to_string()))
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
    /// A picture for the panel, decoded. `path` is absolute, on `host`; `file` is the
    /// file's size in pixels, which `picture` may be smaller than.
    Image {
        host: String,
        path: String,
        file: (u32, u32),
        picture: Picture,
    },
    /// For the desktop's app: the file or folder on this machine to give to `xdg-open`, and
    /// the line for the status line once that is started.
    Outside { target: String, said: String },
    /// The images the Claude session in the tmux session `session` on `host` made, newest
    /// first: the panel's list.
    Made {
        host: String,
        session: String,
        files: Vec<Made>,
    },
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
    open_link_while(key, link, local_name, cache, run, &|| true)
}

/// `open_link` for a click that may be overtaken: `wanted` says whether its answer is still
/// waited for. Asked once the path is located, before anything is read; once a picture is
/// read, and again at its turn to be decoded (`DECODING`) — the decode of a large one takes a
/// quarter of a gigabyte for a moment, which a click nobody waits for is not given; and before
/// a copy for the desktop's app is written, which would else take the place of the copy a
/// newer click made of a file of the same name. A click that is overtaken gives `overtaken`'s
/// line, which nobody reads: its answer is dropped.
pub fn open_link_while(
    key: &SessionKey,
    link: &FileLink,
    local_name: &str,
    cache: Option<&Path>,
    run: Runner,
    wanted: &dyn Fn() -> bool,
) -> Result<Opened, String> {
    let located = locate(key, &link.path, local_name, run)?;
    let host = key.host.as_str();
    let name = named(host, &located.path, local_name);
    if !wanted() {
        return Err(overtaken(&name));
    }
    if located.kind == Kind::File && is_image(&located.path) {
        return image(host, &located, local_name, cache, run, wanted);
    }
    let text_sized = located.size <= MAX_BYTES as u64;
    if located.kind != Kind::File || opens_outside(&located.path, false) || !text_sized {
        return outside(host, &located, local_name, cache, None, run, wanted);
    }
    // One byte more than fits: a file that grew since is not shown cut off.
    let argv = files::read_argv(host, &located.path, MAX_BYTES as u64 + 1, local_name);
    let bytes = run(&argv, READ_TIMEOUT).map_err(|e| format!("{name}: {}", why(&e)))?;
    if bytes.len() > MAX_BYTES {
        return outside(host, &located, local_name, cache, None, run, wanted);
    }
    match text_of(bytes) {
        Ok(text) => Ok(Opened::Text(Document {
            host: host.to_string(),
            path: located.path,
            line: link.line,
            text,
        })),
        // Not text after all: the bytes are the whole file, no need to fetch it again.
        Err(bytes) => outside(host, &located, local_name, cache, Some(bytes), run, wanted),
    }
}

/// What a click that a newer one overtook answers with, for the file called `name`.
fn overtaken(name: &str) -> String {
    format!("{name}: a newer click took its place")
}

/// Held for as long as a picture is decoded: one at a time, whatever the number of clicks.
/// Impatient clicks on a large picture would else each take their quarter of a gigabyte at
/// once. The clicks that wait hold the file's bytes they read and nothing more.
static DECODING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Reads the picture `located` on `host` and decodes it for the panel.
///
/// One the panel does not take goes to the desktop's app as any other file does (`outside`),
/// and the line says why: larger than `MAX_IMAGE_BYTES` (not read, or not decoded if it grew
/// since it was located), too many pixels, or no picture of a kind decoded here. One that is
/// broken opens nowhere. `wanted`: see `open_link_while`.
fn image(
    host: &str,
    located: &Located,
    local_name: &str,
    cache: Option<&Path>,
    run: Runner,
    wanted: &dyn Fn() -> bool,
) -> Result<Opened, String> {
    let path = located.path.as_str();
    let name = named(host, path, local_name);
    // `have`: the file's bytes, when they were read already.
    let instead = |why: &str, have: Option<Vec<u8>>| match outside(
        host, located, local_name, cache, have, run, wanted,
    )? {
        Opened::Outside { target, said } => Ok(Opened::Outside {
            target,
            said: format!("{why}; {said}"),
        }),
        other => Ok(other),
    };
    const TOO_LARGE: &str = "too large for the panel";
    if located.size > MAX_IMAGE_BYTES {
        return instead(TOO_LARGE, None);
    }
    let argv = files::read_argv(host, path, MAX_IMAGE_BYTES + 1, local_name);
    let bytes = run(&argv, IMAGE_TIMEOUT).map_err(|e| format!("{name}: {}", why(&e)))?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return instead(TOO_LARGE, Some(bytes));
    }
    if !wanted() {
        return Err(overtaken(&name));
    }
    let decoded = {
        // Nothing is kept under the lock: one that a decoder's panic left poisoned is as good.
        let _turn = DECODING
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Its turn may have come after a newer click: then it does not start.
        if !wanted() {
            return Err(overtaken(&name));
        }
        decode_file(&bytes, MAX_SIDE)
    };
    match decoded {
        Ok(Decoded { picture, file }) => Ok(Opened::Image {
            host: host.to_string(),
            path: located.path.clone(),
            file,
            picture,
        }),
        Err(Refused::TooLarge) => instead(TOO_LARGE, Some(bytes)),
        Err(Refused::NotAnImage) => instead("not a picture the panel shows", Some(bytes)),
        Err(broken @ Refused::Broken(_)) => Err(format!("{name}: {broken}")),
    }
}

/// Asks the host of the session `key` for the images its Claude session `session_id` made
/// (`files::made_argv`, within `MADE_TIMEOUT`). A session without a transcript has none. The
/// error is the line for the status line. Blocks for as long as the host takes: not for the UI
/// thread.
pub fn made(
    key: &SessionKey,
    session_id: &str,
    local_name: &str,
    run: Runner,
) -> Result<Opened, String> {
    made_while(key, session_id, local_name, run, &|| true)
}

/// `made` for a request that may be overtaken: `wanted` says whether its answer is still
/// waited for. Asked before the host is: a request nobody waits for starts no ssh.
pub fn made_while(
    key: &SessionKey,
    session_id: &str,
    local_name: &str,
    run: Runner,
    wanted: &dyn Fn() -> bool,
) -> Result<Opened, String> {
    let argv = files::made_argv(&key.host, session_id, None, local_name);
    let name = named(&key.host, &key.name, local_name);
    if !wanted() {
        return Err(format!("images of {name}: a newer request took its place"));
    }
    let out = run(&argv, MADE_TIMEOUT).map_err(|e| format!("images of {name}: {}", why(&e)))?;
    let files = files::parse_made(&String::from_utf8_lossy(&out)).unwrap_or_default();
    Ok(Opened::Made {
        host: key.host.clone(),
        session: key.name.clone(),
        files,
    })
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
    outside(host, &located, local_name, cache, None, run, &|| true)
}

/// What `links::open_folder` is to be given for the folder the absolute `path` on `host` is
/// in, and the line for the status line: the folder itself on this machine, else its
/// `sftp://` address, which the desktop's file manager reads the folder through (as tmls
/// does; nothing is copied).
pub fn folder_target(host: &str, path: &str, local_name: &str) -> Result<(String, String), String> {
    let name = named(host, path, local_name);
    if !path.starts_with('/') {
        return Err(format!("{name}: no folder to open"));
    }
    let folder = match path.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) | None => "/",
        Some((folder, _)) => folder,
    };
    if host == local_name {
        return Ok((folder.to_string(), format!("opened {folder}")));
    }
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_');
    if host.is_empty() || host.starts_with('-') || !host.chars().all(plain) {
        return Err(format!("{name}: no sftp address for the host"));
    }
    let mut target = format!("sftp://{host}");
    for byte in folder.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'.' | b'-' | b'_' | b'~' => {
                target.push(byte as char)
            }
            other => target.push_str(&format!("%{other:02X}")),
        }
    }
    Ok((target, format!("opened {folder} on {host}")))
}

/// What `xdg-open` is to be given for `located`: the path itself on this machine, else a
/// copy in `cache`. `have`: the file's bytes, when they were read already. `wanted`: see
/// `open_link_while`; asked before the copy is written.
fn outside(
    host: &str,
    located: &Located,
    local_name: &str,
    cache: Option<&Path>,
    have: Option<Vec<u8>>,
    run: Runner,
    wanted: &dyn Fn() -> bool,
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
    if !wanted() {
        return Err(overtaken(&name));
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

/// What a panel's header has besides its title and the `×`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SideLook {
    /// `Refresh` where a file has `Open in app`: the list of a session's images.
    pub refresh: bool,
    /// `‹` at its start: a picture opened from that list.
    pub back: bool,
}

/// What the panel shows now: what `draw` paints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SideView {
    /// The header's text: the file, or whose images.
    pub title: String,
    pub look: SideLook,
    pub body: SideBody,
}

/// What is under the header, by the kind of panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SideBody {
    /// `gutter`: the columns of the line numbers and the space after them.
    Text { gutter: usize, rows: Vec<TextRow> },
    /// A picture: the chrome draws the body's background, `picture::PictureRenderer` the
    /// picture over it.
    Image,
    /// The images a session made: the rows in view, or the one line said in their place.
    Made {
        rows: Vec<MadeRow>,
        message: Option<&'static str>,
    },
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

/// Where a picture of `image` pixels is drawn in the panel's body `area` and which part of
/// it: the rectangle on the screen in pixels, and the picture's rectangle there in texels.
///
/// Fitted (`actual` false): the whole picture, as large as fits with its shape kept and never
/// larger than it is, in the middle. At its own size (`actual`): one texel a pixel; what does
/// not fit is cut, and `scroll` — texels from the top-left corner — says where the part shown
/// starts, held between the picture's ends. Both rectangles are on whole pixels. A body or a
/// picture of no size gives a rectangle of no size.
pub fn place(image: (u32, u32), area: Rect, actual: bool, scroll: (f32, f32)) -> (Rect, Rect) {
    let (iw, ih) = (image.0 as f32, image.1 as f32);
    let (aw, ah) = (area.w.floor(), area.h.floor());
    // Not `aw < 1.0`: a size that is no number draws nothing either.
    if image.0 == 0 || image.1 == 0 || !(aw >= 1.0 && ah >= 1.0) {
        let nothing = Rect {
            x: area.x,
            y: area.y,
            w: 0.0,
            h: 0.0,
        };
        return (
            nothing,
            Rect {
                x: 0.0,
                y: 0.0,
                ..nothing
            },
        );
    }
    // In the middle of the body, on a whole pixel.
    let centred = |w: f32, h: f32| Rect {
        x: (area.x + ((area.w - w) / 2.0).floor()).round(),
        y: (area.y + ((area.h - h) / 2.0).floor()).round(),
        w,
        h,
    };
    if actual {
        let (w, h) = (iw.min(aw), ih.min(ah));
        let start = |scroll: f32, most: f32| match scroll.is_finite() {
            true => scroll.round().clamp(0.0, most),
            false => 0.0,
        };
        let source = Rect {
            x: start(scroll.0, iw - w),
            y: start(scroll.1, ih - h),
            w,
            h,
        };
        return (centred(w, h), source);
    }
    let whole = Rect {
        x: 0.0,
        y: 0.0,
        w: iw,
        h: ih,
    };
    let (w, h) = if iw <= aw && ih <= ah {
        (iw, ih)
    } else if aw * ih <= ah * iw {
        // The width is what stops it.
        (aw, (aw * ih / iw).round().clamp(1.0, ah))
    } else {
        ((ah * iw / ih).round().clamp(1.0, aw), ah)
    };
    (centred(w, h), whole)
}

/// A picture in the panel: its size in pixels as it is kept (the texture's) and as the file
/// has it (larger, if it was made smaller to be kept), whether it is shown at the texture's
/// own size and not fitted, and how far it is scrolled then, in texels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageBody {
    pub size: (u32, u32),
    pub file: (u32, u32),
    pub actual: bool,
    pub scroll: (f32, f32),
}

impl ImageBody {
    /// Just opened: fitted.
    pub fn new(size: (u32, u32), file: (u32, u32)) -> ImageBody {
        ImageBody {
            size,
            file,
            actual: false,
            scroll: (0.0, 0.0),
        }
    }

    /// A click: between fitted and its own size, which starts at the top-left corner.
    pub fn toggle(&mut self) {
        self.actual = !self.actual;
        self.scroll = (0.0, 0.0);
    }

    /// Moves the part shown by `by` texels in a body of `area`, as far as the picture goes.
    /// Returns whether the picture changed: never while it is fitted.
    pub fn pan(&mut self, by: (f32, f32), area: Rect) -> bool {
        if !self.actual {
            return false;
        }
        let at = |scroll| {
            let (_, source) = place(self.size, area, true, scroll);
            (source.x, source.y)
        };
        let from = at(self.scroll);
        let to = at((from.0 + by.0, from.1 + by.1));
        self.scroll = to;
        to != from
    }

    /// How large it is drawn in a body of `area`, in hundredths of the file's size: a
    /// picture that was made smaller to be kept never comes to 100.
    pub fn percent(&self, area: Rect) -> u32 {
        let (dest, source) = place(self.size, area, self.actual, self.scroll);
        if source.w <= 0.0 || self.file.0 == 0 {
            return 0;
        }
        let kept = self.size.0 as f32 / self.file.0 as f32;
        (100.0 * kept * dest.w / source.w).round() as u32
    }
}

/// What a row of the list says of when its file was written: the time for one of today
/// (`14:05`), the day for any other (`Oct 11`).
pub fn when(modified: LocalTime, now: LocalTime) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let day = |t: LocalTime| (t.year, t.month, t.day);
    if day(modified) == day(now) {
        return format!("{:02}:{:02}", modified.hour, modified.minute);
    }
    let month = (modified.month as usize)
        .checked_sub(1)
        .and_then(|index| MONTHS.get(index));
    format!("{} {}", month.unwrap_or(&"?"), modified.day)
}

/// One image of the list, as its row says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MadeRow {
    /// `when`'s words, on this machine's clock.
    pub when: String,
    /// The file's name. It may be longer than the panel: drawing cuts it.
    pub name: String,
    /// The folder it is in, cut at its start to the columns left after the name; empty when
    /// those are too few.
    pub folder: String,
    /// The cursor is on it.
    pub selected: bool,
}

/// How far the list of a session's images is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MadeState {
    /// The host is asked and has not answered.
    Loading,
    /// The host answered: the list is what it said, which may be nothing.
    Listed,
    /// The tab's session is not Claude's: nothing was asked.
    NoClaude,
    /// The host did not answer in time, or not with a list.
    Failed,
}

/// The images the Claude session of one tab made: the files, newest first, the one the
/// cursor is on, and the first one in view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MadeBody {
    /// The tab's session.
    pub key: SessionKey,
    files: Vec<Made>,
    /// Index of the file the cursor is on.
    cursor: usize,
    /// Index of the first file in view, as last asked for; `top` is what it comes to.
    top: usize,
    state: MadeState,
}

impl MadeBody {
    fn new(key: SessionKey, files: Vec<Made>, state: MadeState) -> MadeBody {
        MadeBody {
            key,
            files,
            cursor: 0,
            top: 0,
            state,
        }
    }

    /// The host is being asked.
    pub fn loading(key: SessionKey) -> MadeBody {
        MadeBody::new(key, Vec::new(), MadeState::Loading)
    }

    /// A tab whose session is not Claude's.
    pub fn no_claude(key: SessionKey) -> MadeBody {
        MadeBody::new(key, Vec::new(), MadeState::NoClaude)
    }

    /// The list as the host gave it, the cursor on the newest.
    pub fn listed(key: SessionKey, files: Vec<Made>) -> MadeBody {
        MadeBody::new(key, files, MadeState::Listed)
    }

    /// The host answered (again): `files` in place of the list. The cursor stays on the file
    /// it was on, on the same line of the panel; if that file is gone it stays where it was
    /// in the list, or goes to the last one.
    pub fn set_files(&mut self, files: Vec<Made>) {
        let on = self.selected().map(|file| file.path.as_str());
        let found = on.and_then(|path| files.iter().position(|file| file.path == path));
        let line = self.cursor.saturating_sub(self.top);
        self.cursor = found
            .unwrap_or(self.cursor)
            .min(files.len().saturating_sub(1));
        self.top = self.cursor.saturating_sub(line);
        self.files = files;
        self.state = MadeState::Listed;
    }

    /// The host did not answer. A list that is there stays: it is as old as it was.
    pub fn failed(&mut self) {
        if self.state == MadeState::Loading {
            self.state = MadeState::Failed;
        }
    }

    pub fn state(&self) -> MadeState {
        self.state
    }

    /// How many images are listed.
    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// The file the cursor is on.
    pub fn selected(&self) -> Option<&Made> {
        self.files.get(self.cursor)
    }

    /// The one line said in place of the rows, when there are none.
    pub fn message(&self) -> Option<&'static str> {
        match self.state {
            MadeState::Loading => Some(MADE_LOADING),
            MadeState::NoClaude => Some(NO_CLAUDE),
            MadeState::Failed => Some(MADE_FAILED),
            MadeState::Listed if self.files.is_empty() => Some(NO_IMAGES),
            MadeState::Listed => None,
        }
    }

    /// The last file that can be first in view when `visible` rows fit.
    fn max_top(&self, visible: usize) -> usize {
        self.files.len().saturating_sub(visible.max(1))
    }

    /// Index of the first file in view when `visible` rows fit.
    pub fn top(&self, visible: usize) -> usize {
        self.top.min(self.max_top(visible))
    }

    /// Moves the cursor by `delta` rows (negative: up), as far as the list goes, and scrolls
    /// so it is in view when `visible` rows fit. Returns whether it moved.
    pub fn move_by(&mut self, delta: i32, visible: usize) -> bool {
        let Some(last) = self.files.len().checked_sub(1) else {
            return false;
        };
        let from = self.cursor.min(last);
        let to = (from as i64 + delta as i64).clamp(0, last as i64) as usize;
        self.cursor = to;
        let top = self.top(visible);
        self.top = if to < top {
            to
        } else {
            top.max((to + 1).saturating_sub(visible.max(1)))
        };
        to != from
    }

    /// The wheel: scrolls by `rows` rows (negative: up); the cursor stays on its file.
    /// Returns whether anything moved.
    pub fn scroll(&mut self, rows: i32, visible: usize) -> bool {
        let from = self.top(visible);
        let to = (from as i64 + rows as i64).clamp(0, self.max_top(visible) as i64) as usize;
        self.top = to;
        to != from
    }

    /// A click on line `line` of the `visible` in view: the cursor goes to the file there,
    /// which is returned. `None` under the last row.
    pub fn click(&mut self, line: usize, visible: usize) -> Option<&Made> {
        let index = self.top(visible) + line;
        if line >= visible || index >= self.files.len() {
            return None;
        }
        self.cursor = index;
        self.files.get(index)
    }

    /// The rows in view when `visible` rows of `cols` columns fit; `now_s`: the time now, in
    /// seconds since 1970, which decides what is of today.
    pub fn rows(&self, visible: usize, cols: usize, now_s: u64) -> Vec<MadeRow> {
        let top = self.top(visible);
        let now = files::local_time(now_s);
        let end = self.files.len().min(top + visible);
        (top..end)
            .map(|index| {
                let file = &self.files[index];
                let (folder, name) = file.path.rsplit_once('/').unwrap_or(("", &file.path));
                let folder = if folder.is_empty() { "/" } else { folder };
                let name_cols = columns(name).min(cols.saturating_sub(MADE_NAME_COL));
                // Two columns between the name and its folder.
                let left = cols.saturating_sub(MADE_NAME_COL + name_cols + 2);
                let modified = files::local_time(file.modified_s);
                MadeRow {
                    when: match (modified, now) {
                        (Some(modified), Some(now)) => when(modified, now),
                        _ => String::new(),
                    },
                    name: name.to_string(),
                    folder: match left >= FOLDER_MIN_COLS {
                        true => keep_end(folder, left),
                        false => String::new(),
                    },
                    selected: index == self.cursor,
                }
            })
            .collect()
    }
}

/// Columns a text takes on the cell grid.
fn columns(text: &str) -> usize {
    clusters(text).iter().map(|(_, width)| width).sum()
}

/// What a panel shows under its header.
#[derive(Clone, Debug, PartialEq)]
pub enum Body {
    Text(TextBody),
    Image(ImageBody),
    Made(MadeBody),
}

/// The panel beside the terminal: one file of one host, or the list of a session's images.
#[derive(Clone, Debug, PartialEq)]
pub struct SidePanel {
    pub host: String,
    /// Absolute, on `host`: what `Open in app` opens. Empty for a list.
    pub path: String,
    title: String,
    pub body: Body,
    /// The list a picture was opened from, as it was then: where `‹` leads back to.
    pub back: Option<MadeBody>,
}

/// What a panel's header calls the file: with its host unless that is this machine.
fn titled(host: &str, path: &str, local_name: &str) -> String {
    match host == local_name {
        true => path.to_string(),
        false => format!("{host}:{path}"),
    }
}

impl SidePanel {
    /// A panel for a text file. Its header names the host too unless that is this machine
    /// (tmls: `host:path:line`; the line is marked here instead).
    pub fn text(document: &Document, local_name: &str) -> SidePanel {
        SidePanel {
            host: document.host.clone(),
            path: document.path.clone(),
            title: titled(&document.host, &document.path, local_name),
            body: Body::Text(TextBody::new(&document.text, document.line)),
            back: None,
        }
    }

    /// A panel for the picture at the absolute `path` on `host`, fitted: `size` pixels as it
    /// is kept, `file` as the file has it.
    pub fn image(
        host: &str,
        path: &str,
        size: (u32, u32),
        file: (u32, u32),
        local_name: &str,
    ) -> SidePanel {
        SidePanel {
            host: host.to_string(),
            path: path.to_string(),
            title: titled(host, path, local_name),
            body: Body::Image(ImageBody::new(size, file)),
            back: None,
        }
    }

    /// A panel for the list of the images a session made: `Images · SESSION`, and how many
    /// once the host has said.
    pub fn made(body: MadeBody) -> SidePanel {
        SidePanel {
            host: body.key.host.clone(),
            path: String::new(),
            title: format!("Images · {}", body.key.name),
            body: Body::Made(body),
            back: None,
        }
    }

    /// The picture, if that is what the panel shows.
    pub fn image_body(&self) -> Option<&ImageBody> {
        match &self.body {
            Body::Image(image) => Some(image),
            Body::Text(_) | Body::Made(_) => None,
        }
    }

    /// The list of a session's images, if that is what the panel shows.
    pub fn made_body(&self) -> Option<&MadeBody> {
        match &self.body {
            Body::Made(made) => Some(made),
            Body::Text(_) | Body::Image(_) => None,
        }
    }

    pub fn made_body_mut(&mut self) -> Option<&mut MadeBody> {
        match &mut self.body {
            Body::Made(made) => Some(made),
            Body::Text(_) | Body::Image(_) => None,
        }
    }

    /// The list of the images of the session `key`, where an answer of its host goes: the
    /// one the panel shows, or the one behind a picture that was opened from it. `None` when
    /// the panel has neither (a file from the terminal, another session's list).
    pub fn list_of(&mut self, key: &SessionKey) -> Option<&mut MadeBody> {
        let list = match &mut self.body {
            Body::Made(made) => Some(made),
            Body::Text(_) | Body::Image(_) => self.back.as_mut(),
        };
        list.filter(|list| list.key == *key)
    }

    /// Whether a click on the panel gives it the keys: the list (the cursor, Enter) and a
    /// picture opened from it (Backspace or Esc lead back). A file opened from the terminal
    /// has nothing for them.
    pub fn takes_keys(&self) -> bool {
        self.back.is_some() || matches!(self.body, Body::Made(_))
    }

    /// What the header has besides the title and the `×`.
    pub fn look(&self) -> SideLook {
        SideLook {
            refresh: matches!(self.body, Body::Made(_)),
            back: self.back.is_some(),
        }
    }

    /// Just opened, with room for `lines` lines: scrolls to what was asked for.
    pub fn reveal(&mut self, lines: usize) {
        match &mut self.body {
            Body::Text(text) => text.reveal(lines),
            Body::Image(_) | Body::Made(_) => {}
        }
    }

    /// The wheel over a panel of text or a list. Returns whether the picture changed.
    pub fn scroll(&mut self, rows: i32, lines: usize) -> bool {
        match &mut self.body {
            Body::Text(text) => text.scroll(rows, lines),
            Body::Made(made) => made.scroll(rows, lines),
            Body::Image(_) => false,
        }
    }

    /// The wheel over a picture in a body of `area`: `by` pixels across and down
    /// (`ImageBody::pan`). Returns whether the picture changed.
    pub fn pan(&mut self, by: (f32, f32), area: Rect) -> bool {
        match &mut self.body {
            Body::Image(image) => image.pan(by, area),
            Body::Text(_) | Body::Made(_) => false,
        }
    }

    /// A click on the body. Returns whether the picture changed: a picture goes between
    /// fitted and its own size, text has nothing to click. A list's rows are `MadeBody::click`'s,
    /// which needs the line.
    pub fn click(&mut self) -> bool {
        match &mut self.body {
            Body::Image(image) => {
                image.toggle();
                true
            }
            Body::Text(_) | Body::Made(_) => false,
        }
    }

    /// What to draw in a panel of `lines` lines of `cols` columns under its header; `area`
    /// is that body in pixels (`Layout::side_body`), which a picture's header needs. A list's
    /// rows say the time by this machine's clock as it is now.
    pub fn view(&self, lines: usize, cols: usize, area: Rect) -> SideView {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
        self.view_at(lines, cols, area, now.map_or(0, |now| now.as_secs()))
    }

    /// `view` at the time `now_s`, in seconds since 1970.
    pub fn view_at(&self, lines: usize, cols: usize, area: Rect, now_s: u64) -> SideView {
        let (title, body) = match &self.body {
            Body::Text(text) => {
                let rows = text.rows(lines, cols);
                let gutter = text.gutter_cols();
                (self.title.clone(), SideBody::Text { gutter, rows })
            }
            Body::Image(image) => {
                let (width, height) = image.file;
                let percent = image.percent(area);
                let title = format!("{} {width}×{height} · {percent} %", self.title);
                (title, SideBody::Image)
            }
            Body::Made(made) => {
                let title = match made.state() {
                    MadeState::Listed => format!("{} · {}", self.title, made.len()),
                    _ => self.title.clone(),
                };
                let rows = made.rows(lines, cols, now_s);
                let message = made.message();
                (title, SideBody::Made { rows, message })
            }
        };
        SideView {
            title,
            look: self.look(),
            body,
        }
    }
}
