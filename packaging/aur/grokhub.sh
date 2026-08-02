#!/bin/bash
# GrokHub system launcher — starts local nitro UI then Electron shell.
set -euo pipefail

APP_ROOT="${GROKHUB_HOME:-/usr/lib/grokhub}"
export GROKHUB_HOME="$APP_ROOT"
PORT="${GROKHUB_PORT:-18765}"
URL="${GROKHUB_URL:-http://127.0.0.1:${PORT}}"
RUNTIME="${XDG_RUNTIME_DIR:-/tmp}/grokhub"
LOG="${RUNTIME}/ui.log"
PIDFILE="${RUNTIME}/ui.pid"

export GROKHUB_WAYLAND="${GROKHUB_WAYLAND:-1}"
export GROKHUB_TRAY="${GROKHUB_TRAY:-1}"

# Help desktop environments map this process to grokhub.desktop (pin / taskbar)
export ELECTRON_FORCE_WINDOW_MENU_BAR=0
# Chromium app name hints (used by some shells alongside --class)
export CHROME_DESKTOP="${CHROME_DESKTOP:-grokhub.desktop}"

mkdir -p "$RUNTIME"

if ! command -v electron >/dev/null 2>&1; then
  echo "error: electron not found (pacman -S electron)" >&2
  exit 1
fi

if ! command -v node >/dev/null 2>&1; then
  echo "error: node not found (pacman -S nodejs)" >&2
  exit 1
fi

if [[ ! -f "$APP_ROOT/desktop/main.mjs" ]]; then
  echo "error: GrokHub not installed at $APP_ROOT" >&2
  exit 1
fi

ui_up() {
  curl -sf -o /dev/null --max-time 1 "${URL}/" 2>/dev/null
}

start_ui() {
  if ui_up; then
    return 0
  fi

  if [[ -f "$PIDFILE" ]]; then
    old="$(cat "$PIDFILE" 2>/dev/null || true)"
    if [[ -n "${old:-}" ]] && kill -0 "$old" 2>/dev/null; then
      :
    else
      rm -f "$PIDFILE"
    fi
  fi

  if [[ ! -f "$APP_ROOT/.output/server/index.mjs" ]]; then
    echo "error: missing UI build at $APP_ROOT/.output (reinstall package)" >&2
    exit 1
  fi

  echo "Starting GrokHub UI on ${URL} …"
  (
    cd "$APP_ROOT"
    export PORT="$PORT"
    export NITRO_PORT="$PORT"
    export HOST="127.0.0.1"
    export NITRO_HOST="127.0.0.1"
    exec node .output/server/index.mjs
  ) >>"$LOG" 2>&1 &
  echo $! >"$PIDFILE"

  for _ in $(seq 1 80); do
    if ui_up; then
      return 0
    fi
    sleep 0.15
  done

  echo "error: UI failed to start — see $LOG" >&2
  tail -n 40 "$LOG" >&2 || true
  exit 1
}

start_ui

export GROKHUB_URL="$URL"

# Resolve electron binary (absolute) so pinned launchers don't lose the path
ELECTRON_BIN="$(command -v electron)"

# Taskbar / panel identity:
#  - --class / --name match StartupWMClass=GrokHub in grokhub.desktop
#  - exec -a GrokHub sets argv0 so some panels don't label the pin "Electron"
#  - CHROME_DESKTOP + app.setDesktopName map the window to grokhub.desktop
#
# Flags MUST come before the main script path.
exec -a GrokHub "$ELECTRON_BIN" \
  --class=GrokHub \
  --name=GrokHub \
  --enable-features=UseOzonePlatform \
  "$APP_ROOT/desktop/main.mjs" \
  "$@"
