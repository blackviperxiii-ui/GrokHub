#!/usr/bin/env bash
# Repair GrokHub without wiping user data.
#   ./scripts/repair-install.sh --user   (default when not root)
#   sudo ./scripts/repair-install.sh --system
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
USER_MODE=0
for arg in "$@"; do
  case "$arg" in
    --user|-u) USER_MODE=1 ;;
    --system) USER_MODE=0 ;;
    --help|-h) echo "Usage: $0 [--user|--system]"; exit 0 ;;
  esac
done
if [[ "$(id -u)" -ne 0 && "$#" -eq 0 ]]; then USER_MODE=1; fi
echo "==> GrokHub repair ($([[ $USER_MODE -eq 1 ]] && echo user || echo system))"
[[ -d node_modules ]] || npm install --ignore-scripts || npm install
export GROKHUB_DESKTOP=1 NODE_ENV=production
npm run desktop:build
[[ -f .output/server/index.mjs ]] || { echo "build failed" >&2; exit 1; }
if [[ "$USER_MODE" -eq 1 ]]; then
  bash "$ROOT/scripts/install-arch.sh" --user
  bash "$ROOT/scripts/sync-user-integration.sh" || true
else
  if [[ "$(id -u)" -eq 0 ]]; then bash "$ROOT/scripts/install-arch.sh"
  else sudo bash "$ROOT/scripts/install-arch.sh"; fi
fi
RUNTIME="${XDG_RUNTIME_DIR:-/tmp}/grokhub"
if [[ -f "$RUNTIME/ui.pid" ]]; then
  old="$(tr -d ' \n\0' <"$RUNTIME/ui.pid" 2>/dev/null || true)"
  if [[ -n "${old:-}" ]] && kill -0 "$old" 2>/dev/null; then
    cmd="$(tr '\0' ' ' <"/proc/$old/cmdline" 2>/dev/null || true)"
    if [[ "$cmd" == *node* && ( "$cmd" == *".output/server"* || "$cmd" == *"index.mjs"* ) ]]; then
      kill "$old" 2>/dev/null || true; sleep 0.2; kill -9 "$old" 2>/dev/null || true
    fi
  fi
fi
rm -f "$RUNTIME/ui.pid" "$RUNTIME/ui.lock" 2>/dev/null || true
echo "Repair complete. Launch: grokhub"
