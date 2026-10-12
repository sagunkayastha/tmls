//! tmls-rs desktop v0, the part with no UI: which hosts to show, what tmux sessions each has
//! and what they are doing, the tmux commands that act on them, and the user's tmls configuration.
//! `approve` reads Claude's permission dialog off a pane's capture; `push` follows the status
//! Claude's hooks write to tmls's events.jsonl.

pub mod approve;
pub mod config;
pub mod files;
pub mod imagedrop;
pub mod listing;
pub mod poll;
pub mod push;
pub mod seen;
pub mod tmux;

pub use config::{Config, HostColor, default_color};
pub use listing::{
    HostSnapshot, KITTY, LISTING_SCRIPT, SessionInfo, SessionKey, Status, Tool, listing_command,
    parse_listing,
};
pub use poll::Poller;
pub use seen::Seen;
pub use tmux::{NewSession, Start};
