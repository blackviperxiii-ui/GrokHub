#!/usr/bin/env bash
# Install grokhub + grokhub-hub + Grok Build CLI from this clone.
#   --channel beta    fetch origin, check out the beta branch, build, install
#   --channel stable  same for main (stable = main)
# The channel is remembered in $GROKHUB_CONFIG/channel (the install receipt),
# so a plain ./scripts/install.sh --user and the in-app Update stay on it.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
SYSTEM=0
CHANNEL=""
DRY_RUN=0

usage() {
  echo "usage: $0 [--user|--system] [--prefix=DIR] [--channel beta|stable] [--dry-run]"
}

set_channel() {
  case "$1" in
    beta|stable) CHANNEL="$1" ;;
    *)
      echo "error: --channel takes beta or stable, not '$1'" >&2
      exit 2
      ;;
  esac
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --user) SYSTEM=0; PREFIX="${PREFIX:-$HOME/.local}" ;;
    --system) SYSTEM=1; PREFIX=/usr ;;
    --prefix=*) PREFIX="${1#--prefix=}" ;;
    --channel=*) set_channel "${1#--channel=}" ;;
    --channel)
      if [[ $# -lt 2 ]]; then
        echo "error: --channel takes beta or stable" >&2
        exit 2
      fi
      set_channel "$2"
      shift
      ;;
    --dry-run) DRY_RUN=1 ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown arg: $1" >&2
      exit 2
      ;;
  esac
  shift
done

CONFIG_DIR="${GROKHUB_CONFIG:-$HOME/.config/GrokHub}"
RECEIPT="$CONFIG_DIR/channel"
SWITCH=0
if [[ -n "$CHANNEL" ]]; then
  SWITCH=1
else
  # No --channel: stay on the remembered channel (stable when there is none).
  CHANNEL="$(sed -n '/[^[:space:]]/{s/[[:space:]]//g;p;q;}' "$RECEIPT" 2>/dev/null || true)"
  case "$CHANNEL" in
    beta|stable) ;;
    *) CHANNEL=stable ;;
  esac
fi
if [[ "$CHANNEL" == beta ]]; then BRANCH=beta; else BRANCH=main; fi

if [[ "$DRY_RUN" -eq 1 ]]; then
  echo "channel=$CHANNEL branch=$BRANCH switch=$SWITCH prefix=$PREFIX receipt=$RECEIPT"
  exit 0
fi

if [[ "$SYSTEM" -eq 1 && "$(id -u)" -ne 0 ]]; then
  echo "error: --system needs root" >&2
  exit 1
fi

cd "$ROOT"
if [[ "$SWITCH" -eq 1 ]]; then
  if ! git diff --quiet || ! git diff --cached --quiet; then
    echo "error: $ROOT has uncommitted changes; commit or stash them before --channel $CHANNEL" >&2
    exit 1
  fi
  git fetch origin "+refs/heads/$BRANCH:refs/remotes/origin/$BRANCH"
  if git show-ref --verify --quiet "refs/heads/$BRANCH"; then
    git checkout "$BRANCH"
    if ! git merge --ff-only "origin/$BRANCH"; then
      echo "error: local $BRANCH has commits that are not on origin/$BRANCH; sort that out by hand" >&2
      exit 1
    fi
  else
    git checkout -b "$BRANCH" --track "origin/$BRANCH"
  fi
  echo "channel $CHANNEL: $(git rev-parse --short=7 HEAD) on $BRANCH"
fi

GROKHUB_CHANNEL="$CHANNEL" cargo build --release --locked -p grokhub-app -p grokhub-hub
TARGET="${CARGO_TARGET_DIR:-$ROOT/target}"

install -Dm755 "$TARGET/release/grokhub" "$PREFIX/bin/grokhub"
install -Dm755 "$TARGET/release/grokhub-hub" "$PREFIX/bin/grokhub-hub"
install_desktop_entry() {
  local src="$1"
  local dst="$2"
  local bin="$3"
  local tmp
  tmp="$(mktemp)"
  sed -e "s|^Exec=grokhub\$|Exec=${bin}|" \
      -e "s|^TryExec=grokhub\$|TryExec=${bin}|" \
      "$src" >"$tmp"
  install -Dm644 "$tmp" "$dst"
  rm -f "$tmp"
}
install_desktop_entry \
  "$ROOT/packaging/grokhub.desktop" \
  "$PREFIX/share/applications/grokhub.desktop" \
  "$PREFIX/bin/grokhub"
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$PREFIX/share/applications" >/dev/null 2>&1 || true
fi
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

mkdir -p "$CONFIG_DIR"
printf '%s\n' "$ROOT" > "$CONFIG_DIR/source"
printf '%s\n' "$CHANNEL" > "$RECEIPT"

echo "installed $PREFIX/bin/grokhub ($("$PREFIX/bin/grokhub" --version 2>/dev/null || echo "channel $CHANNEL"))"
echo "installed $PREFIX/bin/grokhub-hub"
if [[ -x "$PREFIX/bin/grok" || -x "$HOME/.grok/bin/grok" ]] || command -v grok >/dev/null 2>&1; then
  echo "installed Grok Build CLI (grok)"
else
  echo "grok: Grok Build CLI not on PATH — curl -fsSL https://x.ai/cli/install.sh | GROK_CHANNEL=alpha bash"
fi
if [[ "$SYSTEM" -eq 0 ]]; then
  echo "ensure $PREFIX/bin is on PATH"
fi
