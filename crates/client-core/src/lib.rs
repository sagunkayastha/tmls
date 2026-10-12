//! tmls-rs desktop v0, the part with no UI: which hosts to show, what tmux sessions each has
//! and what they are doing, the tmux commands that act on them, and the user's tmls configuration.

pub mod config;
pub mod listing;
pub mod poll;
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
