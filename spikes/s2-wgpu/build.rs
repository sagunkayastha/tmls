//! Bakes the git commit into the binary for the `meta` line of the latency log:
//! `<short hash>`, or `<short hash>-dirty` when a tracked file differs from that commit.

use std::path::Path;
use std::process::Command;

#[path = "src/provenance.rs"]
mod provenance;

/// Standard output of `git <args>`, run in the crate directory; `None` when git is missing or
/// fails (no repository, for example).
fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

fn main() {
    let hash = git(&["rev-parse", "--short", "HEAD"]);
    // Tracked files only: scratch files beside the sources do not make a build dirty.
    // `--no-optional-locks`: do not rewrite the index of a repository that may be mounted
    // read-only or used by another git at the same time.
    let dirty = git(&[
        "--no-optional-locks",
        "status",
        "--porcelain",
        "--untracked-files=no",
    ])
    .map(|out| provenance::is_dirty(&out));
    println!(
        "cargo:rustc-env=S2_GIT={}",
        provenance::git_label(hash.as_deref(), dirty)
    );

    // When to run again. A commit moves the branch ref, a checkout moves HEAD. In a linked
    // worktree `.git` is a file and HEAD is elsewhere, so git is asked where it is; without
    // git, the usual places.
    let one_line = |args: &[&str]| git(args).map(|out| out.trim().to_string());
    let git_dir = one_line(&["rev-parse", "--git-dir"]).unwrap_or_else(|| "../../.git".into());
    let common_dir =
        one_line(&["rev-parse", "--git-common-dir"]).unwrap_or_else(|| git_dir.clone());
    for path in [
        Path::new(&git_dir).join("HEAD"),
        Path::new(&common_dir).join("refs/heads"),
        Path::new(&common_dir).join("packed-refs"),
    ] {
        // A path that does not exist would make cargo run this script on every build.
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    // An edit to a tracked file without a commit: the sources that end up in the binary. (An
    // edit elsewhere in the repository is seen at the next run of this script, not before.)
    for path in [
        "build.rs",
        "src",
        "Cargo.toml",
        "../s2-core/src",
        "../s2-core/Cargo.toml",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
}
