#!/usr/bin/env bash
# Refresh ~/.local desktop entry + optional hub unit after a user install.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
AGENT=0
NOW=0

for arg in "$@"; do
  case "$arg" in
    --agent|--hub) AGENT=1 ;;
    --now) NOW=1 ;;
    -h|--help)
      echo "usage: $0 [--hub] [--now]"
      exit 0
      ;;
  esac
done

if [[ ! -x "$PREFIX/bin/grokhub" ]]; then
  echo "error: $PREFIX/bin/grokhub missing — run ./scripts/install.sh --user" >&2
  exit 1
fi

sed -e "s|^Exec=grokhub\$|Exec=$PREFIX/bin/grokhub|" \
    -e "s|^TryExec=grokhub\$|TryExec=$PREFIX/bin/grokhub|" \
    "$ROOT/packaging/grokhub.desktop" \
  | install -Dm644 /dev/stdin "$PREFIX/share/applications/grokhub.desktop"
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$PREFIX/share/applications" >/dev/null 2>&1 || true
fi

if [[ "$AGENT" -eq 1 ]]; then
  install -Dm644 "$ROOT/packaging/systemd/grokhub-hub.service" \
    "$HOME/.config/systemd/user/grokhub-hub.service"
  systemctl --user daemon-reload
  if [[ "$NOW" -eq 1 ]]; then
    systemctl --user enable --now grokhub-hub.service
  else
    systemctl --user enable grokhub-hub.service
  fi
fi

echo "desktop entry: $PREFIX/share/applications/grokhub.desktop"
