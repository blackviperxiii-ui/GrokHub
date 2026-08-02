#!/usr/bin/env bash
# Repair a broken GrokHub system install without wiping user data.
# Rebuilds .output + reinstalls /usr/lib/grokhub desktop shell.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "==> GrokHub repair install"
echo "    Source: $ROOT"
echo "    User data (~/.config/GrokHub) is NOT modified"

if [[ ! -d node_modules ]]; then
  echo "==> npm install"
  npm install --ignore-scripts || npm install
fi

echo "==> Building desktop UI"
export GROKHUB_DESKTOP=1
export NODE_ENV=production
npm run desktop:build

if [[ ! -f .output/server/index.mjs ]]; then
  echo "error: build failed — missing .output/server/index.mjs" >&2
  exit 1
fi

if [[ "$(id -u)" -eq 0 ]]; then
  echo "==> Installing to /usr (root)"
  bash "$ROOT/scripts/install-arch.sh"
else
  echo "==> Installing to /usr (needs sudo)"
  sudo bash "$ROOT/scripts/install-arch.sh"
fi

# Clear stale UI pid
RUNTIME="${XDG_RUNTIME_DIR:-/tmp}/grokhub"
rm -f "$RUNTIME/ui.pid" "$RUNTIME/ui.lock" 2>/dev/null || true
fuser -k 18765/tcp 2>/dev/null || true

echo ""
echo "Repair complete. Launch: grokhub"
echo "If the window is still blank: tail -n 50 ${RUNTIME}/ui.log"
