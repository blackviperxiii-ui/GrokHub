#!/bin/bash
# GrokClaw system launcher — starts local nitro UI then Electron shell.
set -euo pipefail

APP_ROOT="${GROKCLAW_HOME:-/usr/lib/grokclaw}"
PORT="${GROKCLAW_PORT:-18765}"
URL="${GROKCLAW_URL:-http://127.0.0.1:${PORT}}"
RUNTIME="${XDG_RUNTIME_DIR:-/tmp}/grokclaw"
LOG="${RUNTIME}/ui.log"
PIDFILE="${RUNTIME}/ui.pid"

export GROKCLAW_WAYLAND="${GROKCLAW_WAYLAND:-1}"
export GROKCLAW_TRAY="${GROKCLAW_TRAY:-1}"

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
  echo "error: GrokClaw not installed at $APP_ROOT" >&2
  exit 1
fi

ui_up() {
  curl -sf -o /dev/null --max-time 1 "${URL}/" 2>/dev/null
}

start_ui() {
  if ui_up; then
    return 0
  fi

  # Reap stale pid
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

  echo "Starting GrokClaw UI on ${URL} …"
  (
    cd "$APP_ROOT"
    export PORT="$PORT"
    export NITRO_PORT="$PORT"
    export HOST="127.0.0.1"
    export NITRO_HOST="127.0.0.1"
    # Prefer loopback only for the agent UI
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

export GROKCLAW_URL="$URL"
exec electron "$APP_ROOT/desktop/main.mjs" "$@"
