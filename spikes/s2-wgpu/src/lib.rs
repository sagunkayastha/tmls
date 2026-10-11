//! `s2-wgpu`: the winit + wgpu front-end of spike S2.
//!
//! - `gfx`     — rendering that needs no window: glyph atlas, quad building, wgpu pipeline.
//! - `cli`     — the command line shared with `s2-gpui`.
//! - `input`   — winit keys, IME commits and the wheel → `s2-core` input.
//! - `select`  — mouse selection in cell coordinates and the text it copies.
//! - `marks`   — the submit / present / work-done timestamps of the latency log.
//! - `session` — one terminal: emulator, key ids, overlays; everything but the window.
//! - `surface` — what to do when the surface gives no texture; when the app may exit.
//! - `app`     — the winit event loop and the wgpu surface. Thin: the logic is in the others.

pub mod app;
pub mod cli;
pub mod gfx;
pub mod input;
pub mod marks;
pub mod select;
pub mod session;
pub mod surface;
