#!/usr/bin/env bash
# Install GrokClaw system-wide on Arch / CachyOS from this repo (or a clone).
# Usage: sudo ./scripts/install-arch.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PREFIX="${PREFIX:-/usr}"
APP_LIB="${PREFIX}/lib/grokclaw"

if [[ "$(id -u)" -ne 0 ]]; then
  echo "Run as root: sudo $0" >&2
  exit 1
fi

if ! command -v electron >/dev/null 2>&1; then
  echo "Installing electron nodejs curl …"
  pacman -S --needed --noconfirm electron nodejs curl
fi

if [[ ! -f "$ROOT/.output/server/index.mjs" ]]; then
  echo "Building desktop runtime (GROKCLAW_DESKTOP=1) …"
  if [[ ! -d "$ROOT/node_modules" ]]; then
    (cd "$ROOT" && npm ci --ignore-scripts || npm install --ignore-scripts)
  fi
  (cd "$ROOT" && GROKCLAW_DESKTOP=1 npm run build)
fi

echo "Installing into ${APP_LIB} …"
rm -rf "$APP_LIB"
install -dm755 "$APP_LIB"
cp -a "$ROOT/.output" "$ROOT/desktop" "$APP_LIB/"

install -Dm755 "$ROOT/packaging/aur/grokclaw.sh" "${PREFIX}/bin/grokclaw"
install -Dm644 "$ROOT/packaging/grokclaw.desktop" \
  "${PREFIX}/share/applications/grokclaw.desktop"
install -Dm644 "$ROOT/packaging/grokclaw.svg" \
  "${PREFIX}/share/icons/hicolor/scalable/apps/grokclaw.svg"
install -Dm644 "$ROOT/LICENSE" \
  "${PREFIX}/share/licenses/grokclaw/LICENSE"

find "$APP_LIB" -type f -exec chmod 644 {} +
find "$APP_LIB" -type d -exec chmod 755 {} +
chmod 755 "${PREFIX}/bin/grokclaw"
chmod 755 "$APP_LIB/desktop/main.mjs" || true

update-desktop-database "${PREFIX}/share/applications" 2>/dev/null || true
gtk-update-icon-cache -q "${PREFIX}/share/icons/hicolor" 2>/dev/null || true

cat <<EOF

GrokClaw installed.

  Run:   grokclaw
  Menu:  Applications → GrokClaw

Unsandboxed host agent — shell/files/apps as your user.
UI listens on 127.0.0.1:18765

EOF
