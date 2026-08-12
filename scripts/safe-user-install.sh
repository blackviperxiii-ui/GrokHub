#!/usr/bin/env bash
# Safely install a grokhub-desktop-vX.Y.Z.tar.gz into ~/.local/lib/grokhub.
# ALWAYS stops a running app first — swapping the live tree under Electron hard-crashes.
set -euo pipefail

TAR="${1:-}"
if [[ -z "$TAR" || ! -f "$TAR" ]]; then
  echo "Usage: $0 /path/to/grokhub-desktop-vX.Y.Z.tar.gz" >&2
  exit 2
fi

DEST="${HOME}/.local/lib/grokhub"
PREV="${DEST}.prev"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

echo "[safe-install] stopping running GrokHub (if any)…"
# Best-effort: stop by name / install path without killing this shell
pkill -x grokhub 2>/dev/null || true
# electron children still holding the tree
if command -v pgrep >/dev/null; then
  while read -r pid; do
    [[ -n "$pid" ]] || continue
    cmd="$(tr '\0' ' ' <"/proc/$pid/cmdline" 2>/dev/null || true)"
    if [[ "$cmd" == *"${DEST}"* ]]; then
      kill "$pid" 2>/dev/null || true
    fi
  done < <(pgrep -f 'electron|index.mjs' 2>/dev/null || true)
fi
sleep 1

echo "[safe-install] extracting $TAR …"
tar -xzf "$TAR" -C "$STAGE"
if [[ ! -d "$STAGE/grokhub/desktop" || ! -f "$STAGE/grokhub/.output/server/index.mjs" ]]; then
  echo "[safe-install] FATAL: tarball missing desktop/ or .output/server/index.mjs" >&2
  exit 1
fi

mkdir -p "$(dirname "$DEST")"
if [[ -d "$DEST" ]]; then
  rm -rf "$PREV"
  mv "$DEST" "$PREV"
  echo "[safe-install] previous install → $PREV"
fi
mv "$STAGE/grokhub" "$DEST"
echo "[safe-install] installed → $DEST"
if [[ -f "$DEST/APP_VERSION" ]]; then
  echo "[safe-install] version $(tr -d '[:space:]' <"$DEST/APP_VERSION")"
fi

# Clear electron singleton locks after hard kills
rm -f "${XDG_CONFIG_HOME:-$HOME/.config}/GrokHub/SingletonLock" \
  "${XDG_CONFIG_HOME:-$HOME/.config}/GrokHub/SingletonCookie" 2>/dev/null || true

echo "[safe-install] starting GrokHub…"
if command -v grokhub >/dev/null 2>&1; then
  nohup grokhub >/dev/null 2>&1 &
else
  nohup env GROKHUB_HOME="$DEST" electron --class=grokhub --name=grokhub "$DEST/desktop/main.mjs" >/dev/null 2>&1 &
fi
echo "[safe-install] done"
