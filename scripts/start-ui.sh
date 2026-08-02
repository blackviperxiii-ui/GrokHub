#!/usr/bin/env bash
# Start GrokHub Nitro UI with absolute paths (safe from any cwd).
set -euo pipefail
ROOT="${GROKHUB_HOME:-}"
if [[ -z "$ROOT" || ! -f "$ROOT/.output/server/index.mjs" ]]; then
  for cand in     "${HOME}/.local/lib/grokhub"     "${HOME}/.local/share/grokhub"     "/usr/lib/grokhub"     "$(cd "$(dirname "$0")/.." && pwd)"; do
    if [[ -f "$cand/.output/server/index.mjs" ]]; then
      ROOT="$cand"
      break
    fi
  done
fi
if [[ -z "$ROOT" || ! -f "$ROOT/.output/server/index.mjs" ]]; then
  echo "error: GrokHub UI build not found (set GROKHUB_HOME)" >&2
  exit 1
fi
export GROKHUB_HOME="$ROOT"
PORT="${GROKHUB_PORT:-18765}"
ENTRY="$ROOT/.output/server/index.mjs"
RUNTIME="${XDG_RUNTIME_DIR:-/tmp}/grokhub"
mkdir -p "$RUNTIME"
LOG="${XDG_CONFIG_HOME:-$HOME/.config}/GrokHub/logs/ui.log"
mkdir -p "$(dirname "$LOG")"
# Drop stale unhealthy pid
for f in "$RUNTIME/ui.pid" "$RUNTIME/ui.lock"; do
  if [[ -f "$f" ]]; then
    old=$(cat "$f" 2>/dev/null || true)
    if [[ -n "$old" ]] && kill -0 "$old" 2>/dev/null; then
      if curl -sf -o /dev/null --max-time 1 "http://127.0.0.1:${PORT}/"; then
        echo "UI already healthy pid=$old"
        exit 0
      fi
      kill "$old" 2>/dev/null || true
      sleep 0.3
      kill -9 "$old" 2>/dev/null || true
    fi
    rm -f "$f"
  fi
done
cd "$ROOT"
export PORT NITRO_PORT="$PORT" HOST=127.0.0.1 NITRO_HOST=127.0.0.1
echo "[start-ui] $(date -Iseconds) root=$ROOT entry=$ENTRY" | tee -a /tmp/grokhub-ui-restart.log >>"$LOG"
nohup node "$ENTRY" >>"$LOG" 2>&1 &
echo $! | tee "$RUNTIME/ui.pid" >"$RUNTIME/ui.lock"
echo "started pid=$(cat "$RUNTIME/ui.pid") log=$LOG"
