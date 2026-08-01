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

# Ensure window/taskbar icons ship next to Electron main
if [[ -d "$ROOT/desktop/icons" ]]; then
  install -dm755 "$APP_LIB/desktop/icons"
  cp -a "$ROOT/desktop/icons/." "$APP_LIB/desktop/icons/"
fi

install -Dm755 "$ROOT/packaging/aur/grokhub.sh" "${PREFIX}/bin/grokhub"
install -Dm644 "$ROOT/packaging/grokhub.desktop" \
  "${PREFIX}/share/applications/grokhub.desktop"

# SVG + PNG hicolor theme (taskbar / launcher / dock)
install -Dm644 "$ROOT/packaging/grokhub.svg" \
  "${PREFIX}/share/icons/hicolor/scalable/apps/grokhub.svg"
if [[ -d "$ROOT/packaging/icons/hicolor" ]]; then
  while IFS= read -r -d '' png; do
    rel="${png#"$ROOT/packaging/icons/hicolor/"}"
    install -Dm644 "$png" "${PREFIX}/share/icons/hicolor/${rel}"
  done < <(find "$ROOT/packaging/icons/hicolor" -type f -name '*.png' -print0)
fi
# Legacy pixmaps fallback (some panels only look here)
if [[ -f "$ROOT/packaging/icons/grokhub-128.png" ]]; then
  install -Dm644 "$ROOT/packaging/icons/grokhub-128.png" \
    "${PREFIX}/share/pixmaps/grokhub.png"
fi

install -Dm644 "$ROOT/LICENSE" \
  "${PREFIX}/share/licenses/grokhub/LICENSE"

find "$APP_LIB" -type f -exec chmod 644 {} +
find "$APP_LIB" -type d -exec chmod 755 {} +
chmod 755 "${PREFIX}/bin/grokhub"
chmod 755 "$APP_LIB/desktop/main.mjs" || true

# Refresh icon/desktop caches so taskbar picks up Icon=grokhub immediately
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "${PREFIX}/share/applications" 2>/dev/null || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "${PREFIX}/share/icons/hicolor" 2>/dev/null || true
fi
if command -v xdg-icon-resource >/dev/null 2>&1; then
  for s in 16 32 48 64 128 256; do
    f="$ROOT/packaging/icons/grokhub-${s}.png"
    if [[ -f "$f" ]]; then
      xdg-icon-resource install --novendor --size "$s" "$f" grokhub 2>/dev/null || true
    fi
  done
fi

cat <<EOF

GrokHub v0.2.0 installed (with taskbar icons).

  Run:   grokhub
  Menu:  Applications → GrokHub
  Icon:  ${PREFIX}/share/icons/hicolor/*/apps/grokhub.png

If the panel still shows a generic Electron icon, log out/in once or run:
  gtk-update-icon-cache -f /usr/share/icons/hicolor

EOF
