#!/usr/bin/env bash
# Lightweight release gate for GrokHub desktop (Linux).
# Usage: from install root or repo root — bash scripts/release-smoke.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
PORT="${GROKHUB_PORT:-18765}"
URL="http://127.0.0.1:${PORT}/"
fail=0

echo "==> release-smoke root=$ROOT"

if [[ -f scripts/assert-versions.mjs ]]; then
  node scripts/assert-versions.mjs || fail=1
else
  echo "warn: assert-versions.mjs missing"
  fail=1
fi

if [[ -f scripts/smoke-unit.mjs ]]; then
  node scripts/smoke-unit.mjs || fail=1
else
  echo "warn: smoke-unit.mjs missing"
  fail=1
fi

if [[ ! -f .output/server/index.mjs ]]; then
  echo "error: missing .output/server/index.mjs"
  fail=1
fi

# Manifest hygiene: warn if >3 tanstack manifests
n=$(find .output/server -maxdepth 1 -name '_tanstack-start-manifest_*.mjs' 2>/dev/null | wc -l | tr -d ' ')
if [[ "${n:-0}" -gt 3 ]]; then
  echo "warn: $n tanstack manifests (expected ≤3 after hygiene)"
  fail=1
fi

# OAuth UA must track APP_VERSION
ver=$(tr -d '[:space:]' < APP_VERSION 2>/dev/null || true)
if [[ -n "$ver" ]] && grep -q 'XAI_UA' desktop/grok-bridge.cjs 2>/dev/null; then
  if grep -E 'XAI_UA\s*=' desktop/grok-bridge.cjs | grep -q '0\.8\.10'; then
    echo "error: OAuth UA still hardcodes 0.8.10"
    fail=1
  fi
  if ! grep -E 'XAI_UA\s*=' desktop/grok-bridge.cjs | grep -q 'APP_VERSION'; then
    echo "warn: XAI_UA may not derive from APP_VERSION"
  fi
fi

# fuser -k guard covered by smoke-unit.mjs above

# Optional: if UI already up, curl it
if curl -sf -o /dev/null --max-time 2 "$URL"; then
  echo "ok: UI healthy at $URL"
else
  echo "note: UI not running at $URL (skip live probe)"
fi

if [[ "$fail" -ne 0 ]]; then
  echo "release-smoke FAILED"
  exit 1
fi
echo "release-smoke OK"
