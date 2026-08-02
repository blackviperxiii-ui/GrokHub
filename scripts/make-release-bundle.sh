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
OUT="dist-release/grokhub-desktop-v${VER}.tar.gz"
tar -C "$STAGE" -czf "$OUT" grokhub
echo "Wrote $OUT ($(du -h "$OUT" | awk '{print $1}'))"
echo "Attach this asset to the GitHub release as grokhub-desktop-v${VER}.tar.gz"
