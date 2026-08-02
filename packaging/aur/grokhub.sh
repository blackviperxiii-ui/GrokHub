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

# Map this process to grokhub.desktop (basename = Wayland app_id = "grokhub")
export CHROME_DESKTOP="grokhub.desktop"
export ELECTRON_FORCE_WINDOW_MENU_BAR=0

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

# Require a real HTML document (not empty / connection refuse)
ui_healthy() {
  local body
  body="$(curl -sf --max-time 2 "${URL}/" 2>/dev/null || true)"
  if [[ -z "$body" ]]; then
    return 1
  fi
  echo "$body" | grep -qiE 'GrokHub|<!DOCTYPE html>|tanstack|/assets/' 
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

  for _ in $(seq 1 100); do
    if ui_healthy; then
      return 0
    fi
    sleep 0.15
  done

  echo "error: UI failed health check — see $LOG" >&2
  tail -n 40 "$LOG" >&2 || true
  exit 1
}

start_ui

# Refuse to open Electron if UI still unhealthy
if ! ui_healthy; then
  echo "error: GrokHub UI not healthy at ${URL}" >&2
  exit 1
fi

export GROKHUB_URL="$URL"

ELECTRON_BIN="$(command -v electron)"

# Use a stable argv0 + WM class that match StartupWMClass=grokhub / desktop id.
# Flags MUST come before the main script path.
# --class/--name lowercase "grokhub" so GNOME/KDE group the window with the pin.
exec -a grokhub "$ELECTRON_BIN" \
  --class=grokhub \
  --name=grokhub \
  "$APP_ROOT/desktop/main.mjs" \
  "$@"
