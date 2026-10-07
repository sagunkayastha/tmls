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

Remove everything:

    docker rmi tmls-rs-build:latest; docker volume rm tmls-rs-cargo
