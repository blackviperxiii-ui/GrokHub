#!/usr/bin/env bash
# Install the Tauri shell (grokhub-shell) + grokhub-hub from this clone.
#   --channel beta    fetch origin, check out the beta branch, build, install
#   --channel stable  same for main (stable = main)
#   --app shell       the Tauri shell (grokhub-shell) plus the hub; the only
#                     app since the egui cabin was retired
# The channel is remembered in $GROKHUB_CONFIG/channel (the install receipt),
# so a plain ./scripts/install.sh --user and the in-app Update stay on it.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
SYSTEM=0
CHANNEL=""
DRY_RUN=0
# Which app to build: the shell and the hub. An old cabin install moves over
# to the shell (see move_cabin_login below).
APP=shell

usage() {
  echo "usage: $0 [--user|--system] [--prefix=DIR] [--channel beta|stable] [--app shell] [--dry-run]"
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

set_app() {
  case "$1" in
    shell) APP="$1" ;;
    cabin|both)
      echo "error: the cabin was retired; --app takes only shell" >&2
      exit 2
      ;;
    *)
      echo "error: --app takes shell, not '$1'" >&2
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
    --app=*) set_app "${1#--app=}" ;;
    --app)
      if [[ $# -lt 2 ]]; then
        echo "error: --app takes shell" >&2
        exit 2
      fi
      set_app "$2"
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
  echo "channel=$CHANNEL branch=$BRANCH switch=$SWITCH prefix=$PREFIX receipt=$RECEIPT app=$APP"
  exit 0
fi

if [[ "$SYSTEM" -eq 1 && "$(id -u)" -ne 0 ]]; then
  echo "error: --system needs root" >&2
  exit 1
fi

# The shell builds its web UI with npm and links the system webview. Stop
# before git or cargo when either is missing, rather than halfway through.
missing=()
command -v npm >/dev/null 2>&1 || missing+=(npm)
if command -v pkg-config >/dev/null 2>&1 && ! pkg-config --exists webkit2gtk-4.1; then
  missing+=(webkit2gtk-4.1)
fi
if [[ "${#missing[@]}" -gt 0 ]]; then
  echo "error: the shell needs: ${missing[*]}" >&2
  echo "  Arch: sudo pacman -S --needed nodejs npm webkit2gtk-4.1" >&2
  echo "  Debian/Ubuntu: sudo apt-get install nodejs npm libwebkit2gtk-4.1-dev" >&2
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
  # Missing local branch (single-branch / shallow): create it tracking origin.
  # Existing local branch: never `checkout -B` (that drops local commits). Refuse
  # when HEAD is not an ancestor of origin/$BRANCH (ahead or diverged); otherwise
  # fast-forward onto the tip.
  if git show-ref --verify --quiet "refs/heads/$BRANCH"; then
    git checkout "$BRANCH"
    if ! git merge-base --is-ancestor HEAD "origin/$BRANCH"; then
      echo "error: local $BRANCH has commits that are not on origin/$BRANCH; sort that out by hand" >&2
      exit 1
    fi
    if ! git merge --ff-only "origin/$BRANCH"; then
      echo "error: local $BRANCH has commits that are not on origin/$BRANCH; sort that out by hand" >&2
      exit 1
    fi
    git branch --set-upstream-to="origin/$BRANCH" "$BRANCH" >/dev/null 2>&1 || true
  else
    git checkout -B "$BRANCH" --track "origin/$BRANCH"
  fi
  echo "channel $CHANNEL: $(git rev-parse --short=7 HEAD) on $BRANCH"
fi

GROKHUB_CHANNEL="$CHANNEL" cargo build --release --locked -p grokhub-hub
TARGET="${CARGO_TARGET_DIR:-$ROOT/target}"
# src-tauri is its own workspace, so its build lands in src-tauri/target
# unless CARGO_TARGET_DIR points both builds at one folder.
npm ci --prefix "$ROOT/ui"
GROKHUB_CHANNEL="$CHANNEL" "$ROOT/ui/node_modules/.bin/tauri" build --no-bundle -- --locked
SHELL_TARGET="${CARGO_TARGET_DIR:-$ROOT/src-tauri/target}"

install -Dm755 "$TARGET/release/grokhub-hub" "$PREFIX/bin/grokhub-hub"
install -Dm755 "$SHELL_TARGET/release/grokhub-shell" "$PREFIX/bin/grokhub-shell"
install_desktop_entry() {
  local src="$1"
  local dst="$2"
  local bin="$3"
  local name tmp
  name="$(basename "$bin")"
  tmp="$(mktemp)"
  sed -e "s|^Exec=${name}\$|Exec=${bin}|" \
      -e "s|^TryExec=${name}\$|TryExec=${bin}|" \
      "$src" >"$tmp"
  install -Dm644 "$tmp" "$dst"
  rm -f "$tmp"
}
install_desktop_entry \
  "$ROOT/packaging/grokhub-shell.desktop" \
  "$PREFIX/share/applications/grokhub-shell.desktop" \
  "$PREFIX/bin/grokhub-shell"
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$PREFIX/share/applications" >/dev/null 2>&1 || true
fi
install -Dm644 "$ROOT/packaging/grokhub.svg" \
  "$PREFIX/share/icons/hicolor/scalable/apps/grokhub.svg"
if [[ -d "$ROOT/packaging/icons/hicolor" ]]; then
  mkdir -p "$PREFIX/share/icons/hicolor"
  cp -a "$ROOT/packaging/icons/hicolor/." "$PREFIX/share/icons/hicolor/"
fi

# A cabin install that started at login through grokhub.service starts the
# shell at login instead: the same autostart entry the shell's own Start at
# login switch writes (named after the product, Exec with --autostart), so the
# switch shows it as on. The unit is turned off, not deleted, and the cabin
# binary stays where it is. Only for a shell-only user install.
move_cabin_login() {
  local unit="$HOME/.config/systemd/user/grokhub.service"
  [[ "$SYSTEM" -eq 0 ]] || return 0
  [[ -f "$unit" ]] || return 0
  local enabled=0
  if [[ -e "$HOME/.config/systemd/user/graphical-session.target.wants/grokhub.service" ]]; then
    enabled=1
  fi
  if command -v systemctl >/dev/null 2>&1; then
    if systemctl --user is-enabled grokhub.service >/dev/null 2>&1; then
      enabled=1
    fi
    systemctl --user disable grokhub.service >/dev/null 2>&1 || true
  fi
  [[ "$enabled" -eq 1 ]] || return 0
  local product
  product="$(sed -n 's/^[[:space:]]*"productName":[[:space:]]*"\([^"]*\)".*/\1/p' "$ROOT/brand/brand.json" | head -n1)"
  [[ -n "$product" ]] || return 0
  local entry="$HOME/.config/autostart/$product.desktop"
  if [[ ! -f "$entry" ]]; then
    mkdir -p "$(dirname "$entry")"
    printf '[Desktop Entry]\nType=Application\nVersion=1.0\nName=%s\nComment=%s startup script\nExec=%s --autostart\nStartupNotify=false\nTerminal=false' \
      "$product" "$product" "$PREFIX/bin/grokhub-shell" >"$entry"
  fi
  echo "moved start at login from the cabin to the shell ($entry)"
}

if [[ "$SYSTEM" -eq 0 ]]; then
  install -Dm644 "$ROOT/packaging/systemd/grokhub-hub.service" \
    "$HOME/.config/systemd/user/grokhub-hub.service"
  # The shell starts at login through its own Settings switch (the autostart
  # plugin), so it gets no unit here.
fi

# Overlay-safe package install. Never fail the overlay if sudo/pkg is
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
  echo "Loadout needs packages: $*"
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
  systemctl --user enable --now grokhub-hub.service >/dev/null 2>&1 || true
fi

move_cabin_login

mkdir -p "$CONFIG_DIR"
printf '%s\n' "$ROOT" > "$CONFIG_DIR/source"
printf '%s\n' "$CHANNEL" > "$RECEIPT"
if [[ "$SWITCH" -eq 1 ]]; then
  # A switch drops the Beta auto-off baseline: turning Beta on starts a fresh
  # one on the next check, and stable has none.
  rm -f "$CONFIG_DIR/channel.beta-since"
fi

echo "installed $PREFIX/bin/grokhub-shell (channel $CHANNEL)"
echo "installed $PREFIX/bin/grokhub-hub"
if [[ "$SYSTEM" -eq 0 ]]; then
  echo "ensure $PREFIX/bin is on PATH"
fi
