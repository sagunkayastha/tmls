# Docker build host

The PC has a hard rule: nothing is installed on the host. Rust 1.96 and Zig 0.15.2
live in a Docker image; cargo's registry cache lives in a named volume.

Build the image (from the repo root):

    docker build -t tmls-rs-build:latest -f tooling/docker/Dockerfile.build tooling/docker

Run cargo (artifacts go to `target-docker/`, git-ignored):

    tooling/docker/build.sh test -p s0
    tooling/docker/build.sh build --release -p s0
    tooling/docker/build.sh --shell        # interactive shell

`build.sh test -p s0` works in the container: the s0 tests no longer hardcode
`/data/scratch/s0`. Integration tests write under cargo's `CARGO_TARGET_TMPDIR`
(`target-docker/tmp/`), the report unit test under `$S0_TEST_DIR` or the system temp dir.
On archbox, tests keep running with `TMPDIR=/data/scratch/s0` so nothing lands in `/tmp`.

`cargo test -p s0` builds without Zig. A workspace-root build (`cargo build`/`cargo test` with no `-p`) or
`-p s1` also builds `libghostty-vt-sys`, which needs **Zig 0.15.2** exactly (0.16 fails); the image has it.
Without a `GHOSTTY_SOURCE_DIR` checkout that build git-clones ghostty, so it needs network.

The image also carries the build-time libraries for the S2 GUI crates (`s2-wgpu`, `s2-gpui`): xkbcommon, wayland,
vulkan, X11, fontconfig, freetype, cmake, clang. Build them with `-p` (they are not default members):

    tooling/docker/build.sh build --release -p s2-wgpu
    ldd target-docker/release/s2-wgpu | grep "not found"     # must print nothing on the host

Remove everything:

    docker rmi tmls-rs-build:latest; docker volume rm tmls-rs-cargo
