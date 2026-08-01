#!/usr/bin/env bash
# GrokHub Arch desktop launcher — starts UI if needed, then Electron shell.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export GROKHUB_WAYLAND="${GROKHUB_WAYLAND:-1}"
export GROKHUB_TRAY="${GROKHUB_TRAY:-1}"
PORT="${GROKHUB_PORT:-8080}"
URL="${GROKHUB_URL:-http://127.0.0.1:${PORT}}"

if ! command -v electron >/dev/null 2>&1; then
  echo "electron not found. Install: sudo pacman -S electron" >&2
  exit 1
fi

ui_up() {
  curl -sf -o /dev/null --max-time 1 "${URL}/" 2>/dev/null
}

if ! ui_up; then
  echo "Starting GrokHub UI on ${URL} …"
  if [[ -d .vercel/output/static ]] || [[ -d dist ]]; then
    npm run preview >>/tmp/grokhub-ui.log 2>&1 &
  else
    npm run dev >>/tmp/grokhub-ui.log 2>&1 &
  fi
  for _ in $(seq 1 60); do
    ui_up && break
    sleep 0.25
  done
  if ! ui_up; then
    echo "UI failed to start. See /tmp/grokhub-ui.log" >&2
    exit 1
  fi
fi

export GROKHUB_URL="$URL"
exec electron "$ROOT/desktop/main.mjs" "$@"
