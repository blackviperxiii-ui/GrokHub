#!/usr/bin/env bash
# Install grokhub + grokhub-hub from this clone.
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
if [[ "$SWITCH" -eq 0 ]]; then
  # The remembered channel must match the checkout, or the build is labeled
  # for one channel while it holds the other and Update then refuses the clone.
  CUR="$(git symbolic-ref -q --short HEAD 2>/dev/null || true)"
  if [[ ( "$CUR" == main || "$CUR" == beta ) && "$CUR" != "$BRANCH" ]]; then
    if [[ "$CUR" == beta ]]; then OTHER=beta; else OTHER=stable; fi
    echo "error: $ROOT is on $CUR but the $CHANNEL channel builds $BRANCH; run with --channel $OTHER to switch, or git checkout $BRANCH" >&2
    exit 1
  fi
fi
if [[ "$SWITCH" -eq 1 ]]; then
  CHANGED="$(git status --porcelain --untracked-files=no)"
  if [[ "$CHANGED" == " M Cargo.lock" ]]; then
    # A lone Cargo.lock rewrite comes from a cargo or rust-analyzer run without
    # --locked, not from someone's edit. Set it aside under a name so it can be
    # found again (git stash list). Never touch any other change.
    STASH_MSG="grokhub install.sh: Cargo.lock set aside before --channel $CHANNEL"
    git stash push -q -m "$STASH_MSG" -- Cargo.lock
    echo "set aside a tool-made Cargo.lock change (git stash list: $STASH_MSG)"
  elif [[ -n "$CHANGED" ]]; then
    echo "error: $ROOT has uncommitted changes; commit or stash them before --channel $CHANNEL" >&2
    echo "changed files:" >&2
    printf '%s\n' "$CHANGED" >&2
    exit 1
  fi
  # Single-branch / shallow / fork clones often lack origin/$BRANCH until we
  # fetch it by refspec. Add the refspec when missing; never drop others.
  if ! git config --get-all remote.origin.fetch 2>/dev/null | grep -Fq "refs/heads/$BRANCH:"; then
    git config --add remote.origin.fetch "+refs/heads/$BRANCH:refs/remotes/origin/$BRANCH"
  fi
  git fetch origin "+refs/heads/$BRANCH:refs/remotes/origin/$BRANCH"
  if ! git show-ref --verify --quiet "refs/remotes/origin/$BRANCH"; then
    echo "error: origin/$BRANCH not found after fetch — is the remote a GrokHub fork with that branch?" >&2
    exit 1
  fi
  # -B recreates or resets the local branch onto the tip and sets upstream.
  # Works when the local branch is missing (single-branch clone) or stale.
  git checkout -B "$BRANCH" --track "origin/$BRANCH"
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

# Overlay-safe package install. Never fail the cabin overlay if sudo/pkg is
# missing. Never call sudo without a TTY; remember a declined prompt.
PKG_SKIP_FILE="${GROKHUB_CONFIG:-$HOME/.config/GrokHub}/skip-pkg-prompt"
try_pkgs() {
  local kind="$1"
  shift
  if [[ "$#" -eq 0 ]]; then
    return 0
  fi
  if [[ "$(id -u)" -eq 0 ]]; then
    "$@" || echo "pkgs: $kind $*"
    return 0
  fi
  if [[ ! -t 0 ]]; then
    echo "pkgs: skipped (no TTY). Run: sudo $*"
    return 0
  fi
  if [[ -f "$PKG_SKIP_FILE" ]]; then
    echo "pkgs: skipped (you declined earlier). Run: sudo $*"
    return 0
  fi
  echo "GrokHub needs packages: $*"
  echo -n "Install with sudo now? [y/N] "
  local ans=
  read -r ans || ans=
  case "$ans" in
    y|Y|yes|YES)
      sudo "$@" || echo "pkgs: sudo $kind $*"
      ;;
    *)
      mkdir -p "$(dirname "$PKG_SKIP_FILE")"
      : >"$PKG_SKIP_FILE"
      echo "pkgs: skipped. Run: sudo $*"
      ;;
  esac
}

# Voice / Imagine still need ffmpeg and alsa. Skip when already installed.
if command -v pacman >/dev/null; then
  if pacman -Q ffmpeg alsa-utils >/dev/null 2>&1; then
    :
  else
    try_pkgs pacman pacman -S --needed ffmpeg alsa-utils
  fi
elif command -v apt-get >/dev/null; then
  if dpkg -s ffmpeg alsa-utils >/dev/null 2>&1; then
    :
  else
    try_pkgs apt-get apt-get install -y ffmpeg alsa-utils
  fi
elif command -v dnf >/dev/null; then
  if rpm -q ffmpeg alsa-utils >/dev/null 2>&1; then
    :
  else
    try_pkgs dnf dnf install -y ffmpeg alsa-utils
  fi
fi

if command -v systemctl >/dev/null && [[ "$SYSTEM" -eq 0 ]]; then
  systemctl --user daemon-reload >/dev/null 2>&1 || true
  systemctl --user enable grokhub.service >/dev/null 2>&1 || true
  systemctl --user enable --now grokhub-hub.service >/dev/null 2>&1 || true
fi

mkdir -p "$CONFIG_DIR"
printf '%s\n' "$ROOT" > "$CONFIG_DIR/source"
printf '%s\n' "$CHANNEL" > "$RECEIPT"
if [[ "$SWITCH" -eq 1 ]]; then
  # A switch drops the Beta auto-off baseline: turning Beta on starts a fresh
  # one on the next check, and stable has none.
  rm -f "$CONFIG_DIR/channel.beta-since"
fi

echo "installed $PREFIX/bin/grokhub ($("$PREFIX/bin/grokhub" --version 2>/dev/null || echo "channel $CHANNEL"))"
echo "installed $PREFIX/bin/grokhub-hub"
if [[ "$SYSTEM" -eq 0 ]]; then
  echo "ensure $PREFIX/bin is on PATH"
fi
