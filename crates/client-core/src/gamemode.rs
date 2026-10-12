//! Game mode (the PC's `game-mode` script, spec G2): while `game-mode status` says `on`,
//! tmls-rs lists the remote hosts every 300 s instead of every 5 s (R1: their ssh is what
//! wakes the network) and runs no notification command. Asked every 30 s, on this machine
//! only; a machine without the script has no game mode.

use std::time::Duration;

/// The script's name on PATH.
pub const PROGRAM: &str = "game-mode";
/// The listing period of a remote host while game mode is on.
pub const REMOTE_PERIOD: Duration = Duration::from_secs(300);
/// How long `game-mode status` may take before it counts as off.
pub const TIMEOUT: Duration = Duration::from_secs(2);

/// `game-mode status`.
pub fn status_argv() -> Vec<String> {
    vec![PROGRAM.to_string(), "status".to_string()]
}

/// Whether what `game-mode status` printed (exit status 0) means on: `on`, nothing else.
pub fn parse(out: &str) -> bool {
    out.trim().eq_ignore_ascii_case("on")
}
