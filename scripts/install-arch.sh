#!/usr/bin/env bash
# Install GrokHub on Arch / CachyOS from this repo.
#
# System (default):  sudo ./scripts/install-arch.sh
# User (no root):    ./scripts/install-arch.sh --user
#
# User layout:
#   ~/.local/lib/grokhub   app files
#   ~/.local/bin/grokhub   launcher
#   ~/.local/share/applications/grokhub.desktop
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
USER_MODE=0
for arg in "$@"; do
  case "$arg" in
    --user|-u) USER_MODE=1 ;;
    --help|-h)
      echo "Usage: $0 [--user]"
      echo "  default: system install to /usr (requires root)"
      echo "  --user:  install to ~/.local (no root)"
      exit 0
      ;;
  esac
done

if [[ "$USER_MODE" -eq 1 ]]; then
  PREFIX="${HOME}/.local"
  APP_LIB="${PREFIX}/lib/grokhub"
  BIN="${PREFIX}/bin/grokhub"
  DESKTOP_DIR="${PREFIX}/share/applications"
  ICON_ROOT="${PREFIX}/share/icons/hicolor"
  NEED_ROOT=0
else
  PREFIX="${PREFIX:-/usr}"
  APP_LIB="${PREFIX}/lib/grokhub"
  BIN="${PREFIX}/bin/grokhub"
  DESKTOP_DIR="${PREFIX}/share/applications"
  ICON_ROOT="${PREFIX}/share/icons/hicolor"
  NEED_ROOT=1
fi

if [[ "$NEED_ROOT" -eq 1 && "$(id -u)" -ne 0 ]]; then
  echo "Run as root: sudo $0" >&2
  echo "Or user install: $0 --user" >&2
  exit 1
fi

if ! command -v electron >/dev/null 2>&1; then
  if [[ "$NEED_ROOT" -eq 1 ]]; then
    echo "Installing electron nodejs curl …"
    pacman -S --needed --noconfirm electron nodejs curl
  else
    echo "error: electron not found (pacman -S electron)" >&2
    exit 1
  fi
fi

if ! command -v node >/dev/null 2>&1; then
  echo "error: node not found (pacman -S nodejs)" >&2
  exit 1
fi

build_ui() {
  echo "Building desktop runtime (GROKHUB_DESKTOP=1) …"
  if [[ ! -d "$ROOT/node_modules" ]]; then
    (cd "$ROOT" && npm ci --ignore-scripts || npm install --ignore-scripts)
  fi
  (cd "$ROOT" && GROKHUB_DESKTOP=1 npm run desktop:build)
}

if [[ ! -f "$ROOT/.output/server/index.mjs" ]]; then
  build_ui
fi

# Always ensure PGLite assets exist (may be missing on older builds)
if [[ ! -f "$ROOT/.output/server/_libs/pglite.data" ]]; then
  echo "PGLite assets missing — rebuilding desktop UI …"
  build_ui
fi

if [[ ! -f "$ROOT/.output/server/index.mjs" ]]; then
  echo "error: build failed — missing .output/server/index.mjs" >&2
  exit 1
fi
if [[ ! -f "$ROOT/.output/server/_libs/pglite.data" ]]; then
  echo "error: build incomplete — missing .output/server/_libs/pglite.data" >&2
  exit 1
fi

echo "Installing into ${APP_LIB} …"
rm -rf "$APP_LIB"
install -dm755 "$APP_LIB"
cp -a "$ROOT/.output" "$ROOT/desktop" "$APP_LIB/"

# Optional packaging bits for user launcher
if [[ -d "$ROOT/packaging" ]]; then
  cp -a "$ROOT/packaging" "$APP_LIB/" 2>/dev/null || true
fi
if [[ -f "$ROOT/package.json" ]]; then
  cp -a "$ROOT/package.json" "$APP_LIB/" 2>/dev/null || true
fi
if [[ -f "$ROOT/APP_VERSION" ]]; then
  cp -a "$ROOT/APP_VERSION" "$APP_LIB/" 2>/dev/null || true
fi
if [[ -f "$ROOT/LICENSE" ]]; then
  cp -a "$ROOT/LICENSE" "$APP_LIB/" 2>/dev/null || true
fi

if [[ -d "$ROOT/desktop/icons" ]]; then
  install -dm755 "$APP_LIB/desktop/icons"
  cp -a "$ROOT/desktop/icons/." "$APP_LIB/desktop/icons/"
fi

install -dm755 "$(dirname "$BIN")"
install -Dm755 "$ROOT/packaging/aur/grokhub.sh" "$BIN"

# Desktop entry with absolute Exec pointing at this install's launcher
install -dm755 "$DESKTOP_DIR"
if [[ -f "$ROOT/packaging/grokhub.desktop" ]]; then
  # Rewrite every /usr/bin/grokhub (main + Desktop Actions) to this install's launcher
  sed -e "s|/usr/bin/grokhub|${BIN}|g" \
      -e "s|^TryExec=.*|TryExec=${BIN}|" \
      "$ROOT/packaging/grokhub.desktop" >"${DESKTOP_DIR}/grokhub.desktop"
  # Ensure main Exec has %U once
  if ! grep -q '^Exec=.*%U' "${DESKTOP_DIR}/grokhub.desktop"; then
    sed -i "0,/^Exec=/s|^Exec=.*|Exec=${BIN} %U|" "${DESKTOP_DIR}/grokhub.desktop"
  fi
  chmod 644 "${DESKTOP_DIR}/grokhub.desktop"
else
  cat >"${DESKTOP_DIR}/grokhub.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=GrokHub
Exec=${BIN} %U
TryExec=${BIN}
Icon=grokhub
Terminal=false
Categories=Utility;Development;
StartupWMClass=grokhub
EOF
fi

# Icons
if [[ -f "$ROOT/packaging/grokhub.svg" ]]; then
  install -Dm644 "$ROOT/packaging/grokhub.svg" \
    "${ICON_ROOT}/scalable/apps/grokhub.svg"
fi
if [[ -d "$ROOT/packaging/icons/hicolor" ]]; then
  while IFS= read -r -d '' png; do
    rel="${png#"$ROOT/packaging/icons/hicolor/"}"
    install -Dm644 "$png" "${ICON_ROOT}/${rel}"
  done < <(find "$ROOT/packaging/icons/hicolor" -type f -name '*.png' -print0)
fi

find "$APP_LIB" -type f -exec chmod 644 {} + 2>/dev/null || true
find "$APP_LIB" -type d -exec chmod 755 {} + 2>/dev/null || true
chmod 755 "$BIN"
chmod 755 "$APP_LIB/desktop/main.mjs" || true
chmod 755 "$APP_LIB/packaging/aur/grokhub.sh" 2>/dev/null || true

# Stamp versions for updater
if command -v git >/dev/null 2>&1 && [[ -d "$ROOT/.git" ]]; then
  git -C "$ROOT" rev-parse HEAD >"$APP_LIB/VERSION" 2>/dev/null || true
fi
if [[ -f "$ROOT/package.json" ]]; then
  node -e "const p=require(process.argv[1]); require('fs').writeFileSync(process.argv[2], p.version+'\\n')" \
    "$ROOT/package.json" "$APP_LIB/APP_VERSION" 2>/dev/null \
    || cp -f "$ROOT/APP_VERSION" "$APP_LIB/APP_VERSION" 2>/dev/null \
    || echo "0.8.0" >"$APP_LIB/APP_VERSION"
fi

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "$ICON_ROOT" 2>/dev/null || true
fi

# Ensure user bin is on PATH hint
if [[ "$USER_MODE" -eq 1 ]]; then
  if ! echo ":$PATH:" | grep -q ":${HOME}/.local/bin:"; then
    echo "note: add ~/.local/bin to PATH if 'grokhub' is not found"
  fi
fi

# Sync menu/actions again (covers Desktop Action Exec lines)
if [[ "$USER_MODE" -eq 1 && -x "$ROOT/scripts/sync-user-integration.sh" ]]; then
  bash "$ROOT/scripts/sync-user-integration.sh" || true
fi

cat <<EOF

GrokHub installed.

  Mode:    $([[ "$USER_MODE" -eq 1 ]] && echo user || echo system)
  App:     ${APP_LIB}
  Launch:  ${BIN}
  Menu:    ${DESKTOP_DIR}/grokhub.desktop

Run:  grokhub
      # or: GROKHUB_HOME=${APP_LIB} ${BIN}

Optional always-on agent (systemd --user):
  bash $ROOT/scripts/sync-user-integration.sh --agent --now

Note: the standalone xAI CLI under ~/.grok is NOT GrokHub (command: grok ≠ grokhub).

EOF
