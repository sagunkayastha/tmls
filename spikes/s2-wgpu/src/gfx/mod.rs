//! Window-independent rendering for the terminal grid. A front-end wires the three parts up:
//!
//! ```text
//! Frame (s2-core) ──► grid::Scene ──► [Instance] layers ──► render::Renderer ──► a texture view
//!                          │                                       ▲
//!                          └──────── atlas::Atlas (glyphs) ────────┘
//! ```
//!
//! - `atlas`  — fonts, shaping, glyph cache and the atlas image (CPU only).
//! - `grid`   — `Frame` → instanced quads; cursor, preedit, selection, photon square.
//! - `render` — the wgpu pipeline, atlas upload, and an offscreen (no window) target.
//!
//! Each module's header lists the calls a front-end needs.

pub mod atlas;
pub mod grid;
pub mod render;

/// The one WGSL module: instanced solid quads and glyph quads.
pub const SHADER_WGSL: &str = include_str!("shader.wgsl");
