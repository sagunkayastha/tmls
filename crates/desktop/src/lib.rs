//! tmls-rs desktop v0: one native window with the session list on the left and the attached
//! tmux sessions in tabs on the right.
//!
//! - `tabs`  — the tab model: one terminal session per tab; open, switch, close, reattach.
//! - `sidebar` — the session list's model: host sections, rows, selection by identity, pins.
//! - `keys`  — the window's own key bindings and which keys go to the terminal, the list, a form.
//! - `forms` — the new-session form, rename, the kill confirm and a session's menu.
//! - `mouse` — the mouse over the terminal: to the program in the tab, or a selection of our own.
//! - `links` — the URL or the file path under a cell, and opening one.
//! - `viewer` — a clicked file: found and read on the session's host, shown in the panel
//!   beside the terminal or given to the desktop's app.
//! - `alerts` — which sessions started to need the user: the bell, its list, notifications.
//! - `theme` — colours and sizes, taken from tmls.
//! - `draw`  — the layout of the window and everything but the terminal's cells as quads.
//! - `app`   — the winit event loop and the wgpu surface: wiring only.
//!
//! The terminal itself (emulator, glyph atlas, renderer, latency marks) is `s2-core` and
//! `s2-wgpu`, used as libraries.

pub mod alerts;
pub mod app;
pub mod draw;
pub mod forms;
pub mod keys;
pub mod links;
pub mod mouse;
pub mod sidebar;
pub mod tabs;
pub mod theme;
pub mod viewer;

/// Writes one line to stderr. Unlike `eprintln!` it cannot panic: the terminal the app was
/// started from (`tmls-rs &`) may be closed, and a line nobody reads must not end the app.
pub fn errln(line: std::fmt::Arguments) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "{line}");
}
