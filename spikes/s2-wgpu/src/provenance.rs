//! The string `meta.git` carries. Compiled twice: into `build.rs` (which asks git and bakes
//! the answer into the binary as `S2_GIT`) and into the library, so the tests reach it.

/// `hash`: `git rev-parse --short HEAD`, `None` when git or the repository is missing.
/// `dirty`: whether a tracked file differs from `HEAD` (untracked files do not count);
/// `None` when git could not say — then the hash stands alone.
///
/// A run's `meta.git` is its only provenance: `<hash>-dirty` says the binary was built from
/// a tree that is not exactly that commit.
pub fn git_label(hash: Option<&str>, dirty: Option<bool>) -> String {
    match hash.map(str::trim).filter(|hash| !hash.is_empty()) {
        None => "unknown".to_string(),
        Some(hash) if dirty == Some(true) => format!("{hash}-dirty"),
        Some(hash) => hash.to_string(),
    }
}

/// `git status --porcelain --untracked-files=no` output → is the tree dirty?
pub fn is_dirty(porcelain: &str) -> bool {
    porcelain.lines().any(|line| !line.trim().is_empty())
}
