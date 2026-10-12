#!/usr/bin/env bash
# Installs the tmls-rs icon and desktop entry for this user (under ~/.local/share only), so the
# launcher lists it and the task bar shows Rem's face. Idempotent. Usage:
#   tooling/desktop/install.sh [/absolute/path/to/tmls-rs]
# Default binary: `command -v tmls-rs`, else <repo>/target-docker/release/tmls-rs.
set -euo pipefail
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
DATA=${XDG_DATA_HOME:-$HOME/.local/share}
SIZES=(16 32 48 64 128 256 512)

BIN=${1:-$(command -v tmls-rs || echo "$REPO/target-docker/release/tmls-rs")}
case "$BIN" in
  /*) ;;
  *) BIN=$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN") ;;
esac
[ -x "$BIN" ] || echo "warning: $BIN is not an executable (yet); the entry points at it anyway" >&2

for n in "${SIZES[@]}"; do
  dir="$DATA/icons/hicolor/${n}x${n}/apps"
  mkdir -p "$dir"
  install -m 644 "$REPO/crates/desktop/assets/icons/tmls-rs-$n.png" "$dir/tmls-rs.png"
  echo "wrote $dir/tmls-rs.png"
done

apps="$DATA/applications"
mkdir -p "$apps"
# Quote the path per the Desktop Entry spec in case it has spaces.
esc=${BIN//\\/\\\\}
esc=${esc//&/\\&}
esc=${esc//|/\\|}
sed "s|^Exec=.*|Exec=\"$esc\"|" "$HERE/tmls-rs.desktop" > "$apps/tmls-rs.desktop.new"
chmod 644 "$apps/tmls-rs.desktop.new"
mv -f "$apps/tmls-rs.desktop.new" "$apps/tmls-rs.desktop"
echo "wrote $apps/tmls-rs.desktop (Exec=\"$BIN\")"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$apps" >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "$DATA/icons/hicolor" >/dev/null 2>&1 || true
fi
