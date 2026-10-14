//! Copy and paste: `arboard` (Wayland data-control, X11) first; when it fails on Wayland,
//! `wl-copy` / `wl-paste` as child processes if they are installed. Every outcome is a line for
//! the status line (`copy_with`, `paste_with`): what was copied or pasted, that there was
//! nothing, or why it failed — nothing goes only to stderr any more.
//!
//! A program in a tab copies too: tmux with `mouse on` takes a plain drag for its own selection
//! and writes it to the clipboard with OSC 52 when the button is released (`Tabs::take_copied`,
//! `NO_OSC52` turns it off).

use std::time::Duration;

use client_core::tmux;

use crate::forms::{self, InputRunner, Runner};

/// Set (to anything), arboard is not tried and `wl-copy` / `wl-paste` do every copy and paste:
/// for the test of the fallback (`tests/clip_wayland.rs`, run by the e2e).
pub const NO_ARBOARD: &str = "TMLS_RS_NO_ARBOARD";

/// Set (to anything), a program in a tab cannot write to the clipboard (OSC 52): a selection
/// made in tmux with a plain drag is then not copied; Shift+drag still is.
pub const NO_OSC52: &str = "TMLS_RS_NO_OSC52";

/// How long `wl-copy` / `wl-paste` may take: they run on the UI thread, so a copy or a paste
/// fails fast rather than freezing the window.
pub const WL_TIMEOUT: Duration = Duration::from_secs(1);

/// The status line after a copy of `text` (`None` or empty: nothing was selected) through `set`.
pub fn copy_with(
    text: Option<String>,
    set: impl FnOnce(&str) -> Result<(), String>,
) -> Result<String, String> {
    let Some(text) = text.filter(|text| !text.is_empty()) else {
        return Ok("nothing selected".to_string());
    };
    match set(&text) {
        Ok(()) => Ok(format!("copied {} chars", text.chars().count())),
        Err(e) => Err(format!("copy failed: {e}")),
    }
}

/// The status line after a paste: `got` is what the clipboard gave, `put` takes it when there
/// is some.
pub fn paste_with(got: Result<String, String>, put: impl FnOnce(&str)) -> Result<String, String> {
    match got {
        Ok(text) if text.is_empty() => Ok("clipboard empty".to_string()),
        Ok(text) => {
            put(&text);
            Ok(format!("pasted {} chars", text.chars().count()))
        }
        Err(e) => Err(format!("paste failed: {e}")),
    }
}

/// `wl-copy`, the text on its stdin.
pub fn wl_copy(text: &str, feed: InputRunner) -> Result<(), String> {
    feed(&["wl-copy".to_string()], text.as_bytes())
        .map(|_| ())
        .map_err(|e| format!("{e:#}"))
}

/// `wl-paste --no-newline`; an empty clipboard is an empty text, not an error.
pub fn wl_paste(run: Runner) -> Result<String, String> {
    match run(&["wl-paste".to_string(), "--no-newline".to_string()]) {
        Ok(text) => Ok(text),
        Err(e) if format!("{e:#}").contains("Nothing is copied") => Ok(String::new()),
        Err(e) => Err(format!("{e:#}")),
    }
}

/// The text a browser's "Copy image" leaves beside the picture: an HTML snippet with an `<img`
/// in it. A text paste of it is never what was meant; the picture is (Windows, `App::paste`).
pub fn is_image_html(text: &str) -> bool {
    let text = text.trim_start();
    text.starts_with('<') && text.to_ascii_lowercase().contains("<img")
}

/// Whether the Windows clipboard holds a picture.
#[cfg(windows)]
pub fn windows_has_image() -> bool {
    arboard::Clipboard::new().is_ok_and(|mut clipboard| clipboard.get_image().is_ok())
}

/// `imagepaste::stage`'s reader on Windows, where there is no `wl-paste`: answers the two
/// questions stage asks it (`--list-types`, then `--type image/png`) from the clipboard's
/// picture through arboard, encoded as PNG. Runs on the paste's own thread.
#[cfg(windows)]
pub fn windows_image(argv: &[String]) -> anyhow::Result<Vec<u8>> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| anyhow::anyhow!("{e}"))?;
    let picture = match clipboard.get_image() {
        Ok(picture) => picture,
        // stage turns "no type" into "clipboard has no image"
        Err(arboard::Error::ContentNotAvailable) => return Ok(Vec::new()),
        Err(e) => anyhow::bail!("{e}"),
    };
    if argv.iter().any(|arg| arg == "--list-types") {
        return Ok(b"image/png\n".to_vec());
    }
    let rgba = image::RgbaImage::from_raw(picture.width as u32, picture.height as u32, picture.bytes.into_owned())
        .ok_or_else(|| anyhow::anyhow!("the clipboard's picture has a wrong size"))?;
    let mut png = Vec::new();
    rgba.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
    Ok(png)
}

/// The window's clipboard.
pub struct Clipboard {
    arboard: Option<arboard::Clipboard>,
    /// Why arboard could not be had, once it was tried.
    broken: Option<String>,
    /// On Wayland with `wl-copy` and `wl-paste` on `PATH`: they are the fallback.
    wl: bool,
}

impl Default for Clipboard {
    fn default() -> Clipboard {
        Clipboard::new()
    }
}

impl Clipboard {
    pub fn new() -> Clipboard {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        Clipboard {
            arboard: None,
            broken: std::env::var_os(NO_ARBOARD).map(|_| format!("arboard is off ({NO_ARBOARD})")),
            wl: wayland && forms::on_path("wl-copy") && forms::on_path("wl-paste"),
        }
    }

    fn arboard(&mut self) -> Result<&mut arboard::Clipboard, String> {
        if self.arboard.is_none() && self.broken.is_none() {
            match arboard::Clipboard::new() {
                Ok(clipboard) => self.arboard = Some(clipboard),
                Err(e) => self.broken = Some(e.to_string()),
            }
        }
        match (&mut self.arboard, &self.broken) {
            (Some(clipboard), _) => Ok(clipboard),
            (None, Some(why)) => Err(why.clone()),
            (None, None) => Err("no clipboard".to_string()),
        }
    }

    /// Puts `text` on the clipboard; the error says what arboard and `wl-copy` said.
    pub fn set(&mut self, text: &str) -> Result<(), String> {
        let first = match self.arboard() {
            Ok(clipboard) => match clipboard.set_text(text.to_string()) {
                Ok(()) => return Ok(()),
                Err(e) => e.to_string(),
            },
            Err(e) => e,
        };
        if !self.wl {
            return Err(first);
        }
        let feed = |argv: &[String], input: &[u8]| tmux::run_with_stdin(argv, input, WL_TIMEOUT);
        wl_copy(text, &feed).map_err(|e| format!("{first}; wl-copy: {e}"))
    }

    /// The clipboard's text; empty when it has none.
    pub fn get(&mut self) -> Result<String, String> {
        let first = match self.arboard() {
            Ok(clipboard) => match clipboard.get_text() {
                Ok(text) => return Ok(text),
                Err(arboard::Error::ContentNotAvailable) => return Ok(String::new()),
                Err(e) => e.to_string(),
            },
            Err(e) => e,
        };
        if !self.wl {
            return Err(first);
        }
        let run = |argv: &[String]| tmux::run(argv, WL_TIMEOUT);
        wl_paste(&run).map_err(|e| format!("{first}; wl-paste: {e}"))
    }
}
