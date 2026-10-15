//! tmls-rs desktop v0, the part with no UI: which hosts to show, what tmux sessions each has
//! and what they are doing, the tmux commands that act on them, and the user's tmls configuration.
//! `approve` reads Claude's permission dialog off a pane's capture; `push` follows the status
//! Claude's hooks write to tmls's events.jsonl; `gamemode` is game mode's status; `imagepaste`
//! makes a file of the clipboard's image, which `imagedrop` copies to a session's host;
//! `sketchpad` is the sketchpad hub's address and the ssh forward to a hub behind a host;
//! `local` is how this machine runs a command (on Windows: inside WSL); `detect` reads what an agent
//! CLI other than Claude is doing off its pane, by herdr's rules.

pub mod approve;
pub mod config;
pub mod detect;
pub mod files;
pub mod gamemode;
pub mod imagedrop;
pub mod imagepaste;
pub mod listing;
pub mod local;
pub mod poll;
pub mod push;
pub mod seen;
pub mod sketchpad;
pub mod tmux;

pub use config::{Config, HostColor, default_color};
pub use local::Local;
pub use listing::{
    HostSnapshot, KITTY, LISTING_SCRIPT, SessionInfo, SessionKey, Status, Tool, listing_command,
    parse_listing,
};
pub use poll::Poller;
pub use seen::Seen;
pub use sketchpad::Sketchpad;
pub use tmux::{NewSession, Start};
