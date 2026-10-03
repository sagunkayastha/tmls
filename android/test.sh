#!/usr/bin/env bash
# Unit tests + lint for the app, in the build image (nothing installed on the host).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
exec "$here/in-docker.sh" bash /w/build.sh test
