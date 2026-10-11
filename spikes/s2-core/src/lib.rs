//! S2 shared core: a local PTY, a libghostty-vt terminal on its own thread, the `Frame` it
//! publishes, toolkit-neutral key encoding, and the latency log both front-ends write.
//! Contract: `docs/superpowers/plans/2026-10-08-s2-split-overview.md`.

pub mod bench;
pub mod emu;
pub mod frame;
pub mod keys;
pub mod latency;
pub mod pty;

pub use emu::{Emu, InputMsg};
pub use frame::{Cell, CellFlags, Cursor, CursorShape, Frame, FrameSlot, Wide};
pub use keys::{Key, KeyEvent, Mods, MouseButton, MouseKind, Named};
pub use pty::{Pty, PtyWriter};

/// CLOCK_MONOTONIC in nanoseconds; every timestamp in every log uses this.
pub fn now_ns() -> u128 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `ts` is a valid timespec and CLOCK_MONOTONIC always exists on Linux.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u128 * 1_000_000_000 + ts.tv_nsec as u128
}
