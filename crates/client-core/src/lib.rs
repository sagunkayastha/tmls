//! tmls-rs desktop v0, the part with no UI: which hosts to show, what tmux sessions each has
//! and what they are doing, and the user's tmls configuration.

pub mod config;
pub mod listing;

pub use config::{Config, HostColor, default_color};
pub use listing::{HostSnapshot, LISTING_SCRIPT, SessionInfo, SessionKey, Status, Tool, listing_command, parse_listing};
