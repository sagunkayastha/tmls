//! tmls-rs desktop v0: one native window with the session list on the left and the attached
//! tmux sessions in tabs on the right.
//!
//! - `tabs`  — the tab model: one terminal session per tab; open, switch, close, reattach.
//! - `keys`  — the window's own key bindings and which keys go to the terminal, the list, a form.
//! - `theme` — colours and sizes, taken from tmls.
//! - `draw`  — the layout of the window and everything but the terminal's cells as quads.
//! - `app`   — the winit event loop and the wgpu surface: wiring only.
//!
//! The terminal itself (emulator, glyph atlas, renderer, latency marks) is `s2-core` and
//! `s2-wgpu`, used as libraries.

pub mod app;
pub mod draw;
pub mod keys;
pub mod tabs;
pub mod theme;
