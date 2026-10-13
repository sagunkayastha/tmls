//! Game mode (the PC's `game-mode` script, spec G2): while `game-mode status` says `on`,
//! tmls-rs lists the remote hosts every 300 s instead of every 5 s (R1: their ssh is what
//! wakes the network) and runs no notification command. Asked every 30 s, on this machine
//! only; a machine without the script has no game mode.

use std::time::Duration;

/// The script's name on PATH.
pub const PROGRAM: &str = "game-mode";
/// The listing period of a remote host while game mode is on.
pub const REMOTE_PERIOD: Duration = Duration::from_secs(300);
/// How long `game-mode status` may take before it is given up on: no answer (see [`answer`]).
pub const TIMEOUT: Duration = Duration::from_secs(2);

/// `game-mode status`.
pub fn status_argv() -> Vec<String> {
    vec![PROGRAM.to_string(), "status".to_string()]
}

/// Whether what `game-mode status` printed (exit status 0) means on: `on`, nothing else.
pub fn parse(out: &str) -> bool {
    out.trim().eq_ignore_ascii_case("on")
}

/// What a run of `game-mode status` says: on or off when it answered, `None` when it failed or
/// took longer than [`TIMEOUT`]. No answer is not "off": under a game's load the script is
/// slow, and game mode stays what it was.
pub fn answer(ran: anyhow::Result<String>) -> Option<bool> {
    ran.ok().map(|out| parse(&out))
}

/// The listing period of a remote host: [`REMOTE_PERIOD`] in game mode (`on`), `None` (the
/// poller's own) otherwise — and for a host that waits for a sign-in (`sign_in`), which is not
/// slowed: once the user has signed in, the list is to say so within seconds, not minutes.
pub fn remote_period(on: bool, sign_in: bool) -> Option<Duration> {
    (on && !sign_in).then_some(REMOTE_PERIOD)
}

/// Whether the remote hosts may be asked about their other tmux servers: not in game mode
/// (`on`), and not before the first `game-mode status` is over (`known`), which may say on.
pub fn remote_scan(known: bool, on: bool) -> bool {
    known && !on
}
