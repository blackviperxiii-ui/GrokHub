#!/usr/bin/env bash
# Install grokhub + grokhub-hub + Grok Build CLI from this clone.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
SYSTEM=0

for arg in "$@"; do
  case "$arg" in
    --user) SYSTEM=0; PREFIX="${PREFIX:-$HOME/.local}" ;;
    --system) SYSTEM=1; PREFIX=/usr ;;
    --prefix=*) PREFIX="${arg#--prefix=}" ;;
    -h|--help)
      echo "usage: $0 [--user|--system] [--prefix=DIR]"
      exit 0
      ;;
    *)
      echo "unknown arg: $arg" >&2
      exit 2
      ;;
  esac
done

if [[ "$SYSTEM" -eq 1 && "$(id -u)" -ne 0 ]]; then
  echo "error: --system needs root" >&2
  exit 1
fi

cd "$ROOT"
cargo build --release --locked -p grokhub-app -p grokhub-hub

install -Dm755 "$ROOT/target/release/grokhub" "$PREFIX/bin/grokhub"
install -Dm755 "$ROOT/target/release/grokhub-hub" "$PREFIX/bin/grokhub-hub"
install -Dm644 "$ROOT/packaging/grokhub.desktop" \
  "$PREFIX/share/applications/grokhub.desktop"
install -Dm644 "$ROOT/packaging/grokhub.svg" \
  "$PREFIX/share/icons/hicolor/scalable/apps/grokhub.svg"
if [[ -d "$ROOT/packaging/icons/hicolor" ]]; then
  mkdir -p "$PREFIX/share/icons/hicolor"
  cp -a "$ROOT/packaging/icons/hicolor/." "$PREFIX/share/icons/hicolor/"
fi

if [[ "$SYSTEM" -eq 0 ]]; then
  install -Dm644 "$ROOT/packaging/systemd/grokhub-hub.service" \
    "$HOME/.config/systemd/user/grokhub-hub.service"
  install -Dm644 "$ROOT/packaging/systemd/grokhub.service" \
    "$HOME/.config/systemd/user/grokhub.service"
fi

# Overlay-safe package install. Never fail the cabin overlay if sudo/pkg is missing.
try_pkgs() {
  local kind="$1"
  shift
  if [[ "$#" -eq 0 ]]; then
    return 0
  fi
  if [[ "$(id -u)" -eq 0 ]]; then
    "$@" || echo "pkgs: $kind $*"
  else
    sudo "$@" || echo "pkgs: sudo $kind $*"
  fi
}

# Voice / Imagine still need ffmpeg and alsa. Grok Build owns computer-use — no grim/ydotool sidecars.
if command -v pacman >/dev/null; then
  try_pkgs pacman pacman -S --needed ffmpeg alsa-utils
elif command -v apt-get >/dev/null; then
  try_pkgs apt-get apt-get install -y ffmpeg alsa-utils
elif command -v dnf >/dev/null; then
  try_pkgs dnf dnf install -y ffmpeg alsa-utils
fi

if command -v systemctl >/dev/null && [[ "$SYSTEM" -eq 0 ]]; then
  systemctl --user daemon-reload >/dev/null 2>&1 || true
  systemctl --user enable grokhub.service >/dev/null 2>&1 || true
  systemctl --user enable --now grokhub-hub.service >/dev/null 2>&1 || true
fi

PREFIX="$PREFIX" bash "$ROOT/scripts/install-grok-cli.sh" \
  || echo "grok: install-grok-cli.sh continued"

CONFIG_DIR="${GROKHUB_CONFIG:-$HOME/.config/GrokHub}"
mkdir -p "$CONFIG_DIR"
printf '%s\n' "$ROOT" > "$CONFIG_DIR/source"

echo "installed $PREFIX/bin/grokhub"
echo "installed $PREFIX/bin/grokhub-hub"
if [[ -x "$PREFIX/bin/grok" || -x "$HOME/.grok/bin/grok" ]] || command -v grok >/dev/null 2>&1; then
  echo "installed Grok Build CLI (grok)"
else
  echo "grok: Grok Build CLI not on PATH — curl -fsSL https://x.ai/cli/install.sh | bash"
fi
if [[ "$SYSTEM" -eq 0 ]]; then
  echo "ensure $PREFIX/bin is on PATH"
fi
