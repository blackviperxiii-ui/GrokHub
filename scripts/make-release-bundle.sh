#!/usr/bin/env bash
# Build a desktop tarball that INCLUDES .output for in-app updates / AUR.
# Output: dist-release/grokhub-desktop-vX.Y.Z.tar.gz
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
VER="$(tr -d '[:space:]' < APP_VERSION 2>/dev/null || node -p "require('./package.json').version")"
export GROKHUB_DESKTOP=1 NODE_ENV=production
npm run desktop:build
mkdir -p dist-release
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "$STAGE/grokhub"
cp -a .output desktop packaging package.json APP_VERSION LICENSE README.md "$STAGE/grokhub/" 2>/dev/null || true
# optional
[[ -f VERSION ]] && cp VERSION "$STAGE/grokhub/" || echo "$VER" > "$STAGE/grokhub/VERSION"
# Ship smoke/version scripts so installed trees can self-check (audit P0)
mkdir -p "$STAGE/grokhub/scripts"
for s in assert-versions.mjs smoke-unit.mjs smoke-agent-finish.mjs start-ui.sh repair-install.sh browser-smoke.mjs release-smoke.sh; do
  if [[ -f "scripts/$s" ]]; then
    cp -a "scripts/$s" "$STAGE/grokhub/scripts/"
  fi
done
# pin install source for dual-install safety
printf '%s
' "{"source":"user-tarball","version":"$VER","preferredRoot":"\$HOME/.local/lib/grokhub"}"   > "$STAGE/grokhub/INSTALL_META.json" 2>/dev/null || true
OUT="dist-release/grokhub-desktop-v${VER}.tar.gz"
tar -C "$STAGE" -czf "$OUT" grokhub
echo "Wrote $OUT ($(du -h "$OUT" | awk '{print $1}'))"
echo "Attach this asset to the GitHub release as grokhub-desktop-v${VER}.tar.gz"
