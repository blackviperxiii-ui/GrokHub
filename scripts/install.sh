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

HANDS_BIN="$PREFIX/lib/grokhub/bin"
write_ydotoold_unit() {
  local dest="$1"
  local bin="$2"
  mkdir -p "$(dirname "$dest")"
  cat >"$dest" <<EOF
[Unit]
Description=ydotool daemon for GrokHub hands
PartOf=graphical-session.target
After=graphical-session.target
ConditionPathExists=${bin}

[Service]
Type=simple
ExecStart=${bin} --socket-path=%t/ydotool.sock
Restart=on-failure
RestartSec=2
Environment=YDOTOOL_SOCKET=%t/ydotool.sock

[Install]
WantedBy=graphical-session.target
EOF
}

if [[ "$SYSTEM" -eq 0 ]]; then
  install -Dm644 "$ROOT/packaging/systemd/grokhub-hub.service" \
    "$HOME/.config/systemd/user/grokhub-hub.service"
  install -Dm644 "$ROOT/packaging/systemd/grokhub.service" \
    "$HOME/.config/systemd/user/grokhub.service"
  write_ydotoold_unit "$HOME/.config/systemd/user/ydotoold.service" \
    "$HANDS_BIN/ydotoold"
else
  write_ydotoold_unit "$PREFIX/lib/systemd/user/ydotoold.service" \
    "$HANDS_BIN/ydotoold"
fi

install_uinput_rule() {
  local src="$ROOT/packaging/udev/60-grokhub-uinput.rules"
  if [[ ! -f "$src" ]]; then
    return 0
  fi
  local dest="/etc/udev/rules.d/60-grokhub-uinput.rules"
  if [[ "$SYSTEM" -eq 1 ]]; then
    dest="/usr/lib/udev/rules.d/60-grokhub-uinput.rules"
  fi
  if [[ "$(id -u)" -eq 0 ]]; then
    install -Dm644 "$src" "$dest" || echo "hands: install $dest"
    modprobe uinput || echo "hands: modprobe uinput"
    udevadm control --reload-rules 2>/dev/null || true
    udevadm trigger --subsystem-match=misc --attr-match=name=uinput 2>/dev/null || true
  else
    sudo install -Dm644 "$src" "$dest" || echo "hands: sudo install $dest"
    sudo modprobe uinput || echo "hands: sudo modprobe uinput"
    sudo udevadm control --reload-rules 2>/dev/null || true
    sudo udevadm trigger --subsystem-match=misc --attr-match=name=uinput 2>/dev/null || true
  fi
}
install_uinput_rule

# Build-time compilers stay pacman packages. Overlay must not die if a sidecar fails.
BUILD_PKGS=(cmake meson ninja wayland wayland-protocols pixman libpng libx11 libxtst libxinerama libxkbcommon glib2 libxmu)
if command -v pacman >/dev/null; then
  missing=0
  for p in "${BUILD_PKGS[@]}"; do
    if ! pacman -Q "$p" >/dev/null 2>&1; then
      missing=1
      break
    fi
  done
  if [[ "$missing" -eq 1 ]]; then
    if [[ "$(id -u)" -eq 0 ]]; then
      pacman -S --needed "${BUILD_PKGS[@]}" || echo "hands: pacman -S --needed ${BUILD_PKGS[*]}"
    else
      sudo pacman -S --needed "${BUILD_PKGS[@]}" || echo "hands: sudo pacman -S --needed ${BUILD_PKGS[*]}"
    fi
  fi
fi
PREFIX="$PREFIX" HANDS_SRC="${HANDS_SRC:-$ROOT/target/hands-src}" \
  bash "$ROOT/scripts/build-hands.sh" || echo "hands: build-hands.sh continued"

if command -v systemctl >/dev/null && [[ -x "$HANDS_BIN/ydotoold" || -x "$(command -v ydotoold 2>/dev/null || true)" ]]; then
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
echo "hands sidecars $HANDS_BIN"
if [[ "$SYSTEM" -eq 0 ]]; then
  echo "ensure $PREFIX/bin is on PATH"
fi
