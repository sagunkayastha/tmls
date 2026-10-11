//! Sessions on a host. The listing script and its parser come with the next commit.

/// A session's identity. `host` is never empty: this machine is its `Config::local_name`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionKey {
    pub host: String,
    pub name: String,
}
