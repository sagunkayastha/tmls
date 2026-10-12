#!/usr/bin/env bash
# Removes exactly the files tooling/desktop/install.sh wrote, nothing else.
set -euo pipefail
DATA=${XDG_DATA_HOME:-$HOME/.local/share}
SIZES=(16 32 48 64 128 256 512)

for n in "${SIZES[@]}"; do
  f="$DATA/icons/hicolor/${n}x${n}/apps/tmls-rs.png"
  if [ -e "$f" ]; then rm -f "$f" && echo "removed $f"; fi
done
f="$DATA/applications/tmls-rs.desktop"
if [ -e "$f" ]; then rm -f "$f" && echo "removed $f"; fi

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$DATA/applications" >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "$DATA/icons/hicolor" >/dev/null 2>&1 || true
fi
