//! What the user set and expects to find again at the next start: the width of the session
//! list and of the file panel (dragged), and the hosts whose sections are folded.
//!
//! Kept in `~/.config/tmls-rs/ui` — a directory of this app's own; `~/.config/tmls` is
//! tmls's, and tmls-rs writes nothing there but what tmls itself would. The file is lines of
//! `key=value`:
//!
//! ```text
//! sidebar_cols=34
//! panel_cols=61
//! folded=nas,pc
//! ```
//!
//! A width that was never dragged has no line: the app's own default goes on applying; with
//! no host folded there is no `folded` line. Lines
//! that say nothing this version knows are skipped, so a file from a newer or an older
//! version, or a damaged one, loads as far as it can be read — never an error.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

/// The widths in columns; `None` where the user has not chosen one. Whether a width still
/// fits the window is the layout's to say (`draw::layout_with`), not this file's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UiState {
    /// The session list (`Theme::sidebar_cols`).
    pub sidebar_cols: Option<usize>,
    /// The file panel's text.
    pub panel_cols: Option<usize>,
    /// The hosts whose sections are folded (`Sidebar::folded`).
    pub folded: BTreeSet<String>,
}

/// Where the file is: `tmls-rs/ui` under `$XDG_CONFIG_HOME`, or under `$HOME/.config` when
/// that is not set. A value that is empty or not an absolute path counts as not set (the
/// XDG base directory specification); `None` when neither is usable.
pub fn path(xdg_config_home: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    let absolute = |dir: Option<&str>| dir.map(PathBuf::from).filter(|dir| dir.is_absolute());
    let base = absolute(xdg_config_home).or_else(|| Some(absolute(home)?.join(".config")))?;
    Some(base.join("tmls-rs").join("ui"))
}

/// Reads the file's text. Of a key that is there twice the last readable line counts; a
/// width of zero columns is none. Host names are separated by commas.
pub fn parse(text: &str) -> UiState {
    let mut state = UiState::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() == "folded" {
            state.folded = value
                .split(',')
                .map(str::trim)
                .filter(|host| !host.is_empty())
                .map(str::to_string)
                .collect();
            continue;
        }
        let Some(cols) = value.trim().parse::<usize>().ok().filter(|cols| *cols > 0) else {
            continue;
        };
        match key.trim() {
            "sidebar_cols" => state.sidebar_cols = Some(cols),
            "panel_cols" => state.panel_cols = Some(cols),
            _ => {}
        }
    }
    state
}

/// The file's text: a line for every width that is set, and one for the folded hosts if any.
pub fn render(state: &UiState) -> String {
    let lines = [
        ("sidebar_cols", state.sidebar_cols),
        ("panel_cols", state.panel_cols),
    ];
    let mut text: String = lines
        .into_iter()
        .filter_map(|(key, cols)| Some(format!("{key}={}\n", cols?)))
        .collect();
    if !state.folded.is_empty() {
        let hosts: Vec<&str> = state.folded.iter().map(String::as_str).collect();
        text.push_str(&format!("folded={}\n", hosts.join(",")));
    }
    text
}

/// What the file at `path` says; nothing when it is not there or cannot be read.
pub fn load(path: &Path) -> UiState {
    std::fs::read_to_string(path)
        .map(|text| parse(&text))
        .unwrap_or_default()
}

/// Writes the file, and the directory it is in if that is missing.
pub fn save(path: &Path, state: &UiState) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, render(state))
}
