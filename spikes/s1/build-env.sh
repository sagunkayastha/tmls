# Source before any cargo command touching the s1 crate:
#   source spikes/s1/build-env.sh && cargo test -p s1
# libghostty-vt 0.2.2 pins ghostty a887df42 which needs Zig 0.15.2 exactly (0.16 fails).
# GHOSTTY_SOURCE_DIR points at an existing checkout of that commit so build.rs skips the git clone.
export PATH=/data/scratch/zig/y/zig-x86_64-linux-0.15.2:$PATH
export GHOSTTY_SOURCE_DIR=/data/scratch/s1-probe/target/debug/build/libghostty-vt-sys-90a98b82a8a190aa/out/ghostty-src
export TMPDIR=/data/scratch/s1
mkdir -p "$TMPDIR"
