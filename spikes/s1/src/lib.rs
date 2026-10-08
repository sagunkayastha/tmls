//! tmls-rs spike S1: can libghostty-vt 0.2.2 be the authoritative emulator and
//! snapshot source? Handles from libghostty-vt are !Send/!Sync; everything here
//! is single-threaded.
// `gen` is a reserved keyword in edition 2024, hence the raw identifier.
pub mod r#gen;
pub mod grid;
pub mod snap;
pub mod splitter;
