#!/usr/bin/env bash
# Install GrokHub system-wide on Arch / CachyOS from this repo (or a clone).
# Usage: sudo ./scripts/install-arch.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PREFIX="${PREFIX:-/usr}"
APP_LIB="${PREFIX}/lib/grokhub"

if [[ "$(id -u)" -ne 0 ]]; then
  echo "Run as root: sudo $0" >&2
  exit 1
fi

if ! command -v electron >/dev/null 2>&1; then
  echo "Installing electron nodejs curl …"
  pacman -S --needed --noconfirm electron nodejs curl
fi

if [[ ! -f "$ROOT/.output/server/index.mjs" ]]; then
  echo "Building desktop runtime (GROKHUB_DESKTOP=1) …"
  if [[ ! -d "$ROOT/node_modules" ]]; then
    (cd "$ROOT" && npm ci --ignore-scripts || npm install --ignore-scripts)
  fi
  (cd "$ROOT" && GROKHUB_DESKTOP=1 npm run build)
fi

echo "Installing into ${APP_LIB} …"
rm -rf "$APP_LIB"
install -dm755 "$APP_LIB"
cp -a "$ROOT/.output" "$ROOT/desktop" "$APP_LIB/"

install -Dm755 "$ROOT/packaging/aur/grokhub.sh" "${PREFIX}/bin/grokhub"
install -Dm644 "$ROOT/packaging/grokhub.desktop" \
  "${PREFIX}/share/applications/grokhub.desktop"
install -Dm644 "$ROOT/packaging/grokhub.svg" \
  "${PREFIX}/share/icons/hicolor/scalable/apps/grokhub.svg"
install -Dm644 "$ROOT/LICENSE" \
  "${PREFIX}/share/licenses/grokhub/LICENSE"

find "$APP_LIB" -type f -exec chmod 644 {} +
find "$APP_LIB" -type d -exec chmod 755 {} +
chmod 755 "${PREFIX}/bin/grokhub"
chmod 755 "$APP_LIB/desktop/main.mjs" || true

update-desktop-database "${PREFIX}/share/applications" 2>/dev/null || true
gtk-update-icon-cache -q "${PREFIX}/share/icons/hicolor" 2>/dev/null || true

cat <<EOF

GrokHub v0.1 installed.

  Run:   grokhub
  Menu:  Applications → GrokHub

Unsandboxed host agent — shell/files/apps as your user.
UI listens on 127.0.0.1:18765

EOF
