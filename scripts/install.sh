#!/usr/bin/env bash
# Install grokhub + grokhub-hub from this clone.
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
  install -Dm644 "$ROOT/packaging/systemd/ydotoold.service" \
    "$HOME/.config/systemd/user/ydotoold.service"
else
  install -Dm644 "$ROOT/packaging/systemd/ydotoold.service" \
    "$PREFIX/lib/systemd/user/ydotoold.service"
  if [[ -f "$ROOT/packaging/udev/60-grokhub-uinput.rules" ]]; then
    install -Dm644 "$ROOT/packaging/udev/60-grokhub-uinput.rules" \
      /usr/lib/udev/rules.d/60-grokhub-uinput.rules
  fi
fi

HANDS_PKGS=(ydotool xdotool grim wmctrl python-atspi)
if command -v pacman >/dev/null; then
  if pacman -Q "${HANDS_PKGS[@]}" >/dev/null 2>&1; then
    echo "hands packages present"
  else
    if [[ "$(id -u)" -eq 0 ]]; then
      if pacman -S --needed "${HANDS_PKGS[@]}"; then
        echo "installed hands packages"
      else
        echo "hands: pacman -S --needed ${HANDS_PKGS[*]}"
      fi
    elif sudo pacman -S --needed "${HANDS_PKGS[@]}"; then
      echo "installed hands packages"
    else
      echo "hands: sudo pacman -S --needed ${HANDS_PKGS[*]}"
    fi
  fi
fi
if command -v systemctl >/dev/null && command -v ydotoold >/dev/null; then
  systemctl --user daemon-reload >/dev/null 2>&1 || true
  systemctl --user enable --now ydotoold.service >/dev/null 2>&1 || true
fi
HANDS_USER="${SUDO_USER:-$USER}"
if command -v id >/dev/null && [[ -n "${HANDS_USER}" ]]; then
  if id -nG "$HANDS_USER" 2>/dev/null | grep -qw input; then
    echo "hands: $HANDS_USER already in input"
  elif sudo usermod -aG input "$HANDS_USER"; then
    echo "hands: added $HANDS_USER to input — log out once"
  else
    echo "hands: sudo usermod -aG input $HANDS_USER  # then log out once"
  fi
fi

CONFIG_DIR="${GROKHUB_CONFIG:-$HOME/.config/GrokHub}"
mkdir -p "$CONFIG_DIR"
printf '%s\n' "$ROOT" > "$CONFIG_DIR/source"

echo "installed $PREFIX/bin/grokhub"
echo "installed $PREFIX/bin/grokhub-hub"
if [[ "$SYSTEM" -eq 0 ]]; then
  echo "ensure $PREFIX/bin is on PATH"
fi
