//! tmls-rs spike S1: can libghostty-vt 0.2.2 be the authoritative emulator and
//! snapshot source? Handles from libghostty-vt are !Send/!Sync; everything here
//! is single-threaded.
pub mod grid;
