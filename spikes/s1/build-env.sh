# Source before any cargo command touching the s1 crate:
#   source spikes/s1/build-env.sh && cargo test -p s1
# libghostty-vt 0.2.2 pins ghostty a887df42 which needs Zig 0.15.2 exactly (0.16 fails).
# GHOSTTY_SOURCE_DIR points at an existing checkout of that commit so build.rs skips the git clone.
# It is exported only if that checkout exists (it lives in a probe build dir under /data/scratch);
# otherwise build.rs clones ghostty over the network into OUT_DIR.
export PATH=/data/scratch/zig/y/zig-x86_64-linux-0.15.2:$PATH
_s1_gsrc=/data/scratch/s1-probe/target/debug/build/libghostty-vt-sys-90a98b82a8a190aa/out/ghostty-src
if [ -f "$_s1_gsrc/build.zig" ]; then
  export GHOSTTY_SOURCE_DIR=$_s1_gsrc
else
  echo "build-env: no ghostty checkout at $_s1_gsrc; the libghostty-vt-sys build will git-clone ghostty" >&2
fi
unset _s1_gsrc
export TMPDIR=/data/scratch/s1
mkdir -p "$TMPDIR"
