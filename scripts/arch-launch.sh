#!/usr/bin/env bash
# GrokClaw Arch desktop launcher — starts UI if needed, then Electron shell.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export GROKCLAW_WAYLAND="${GROKCLAW_WAYLAND:-1}"
export GROKCLAW_TRAY="${GROKCLAW_TRAY:-1}"
PORT="${GROKCLAW_PORT:-8080}"
URL="${GROKCLAW_URL:-http://127.0.0.1:${PORT}}"

if ! command -v electron >/dev/null 2>&1; then
  echo "electron not found. Install: sudo pacman -S electron" >&2
  exit 1
fi

ui_up() {
  curl -sf -o /dev/null --max-time 1 "${URL}/" 2>/dev/null
}

if ! ui_up; then
  echo "Starting GrokClaw UI on ${URL} …"
  if [[ -d .vercel/output/static ]] || [[ -d dist ]]; then
    npm run preview >>/tmp/grokclaw-ui.log 2>&1 &
  else
    npm run dev >>/tmp/grokclaw-ui.log 2>&1 &
  fi
  for _ in $(seq 1 60); do
    ui_up && break
    sleep 0.25
  done
  if ! ui_up; then
    echo "UI failed to start. See /tmp/grokclaw-ui.log" >&2
    exit 1
  fi
fi

export GROKCLAW_URL="$URL"
exec electron "$ROOT/desktop/main.mjs" "$@"
