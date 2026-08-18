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

# Overlay-safe package install. Never fail the cabin overlay if sudo/pkg is missing.
try_pkgs() {
  local kind="$1"
  shift
  if [[ "$#" -eq 0 ]]; then
    return 0
  fi
  if [[ "$(id -u)" -eq 0 ]]; then
    "$@" || echo "hands: $kind $*"
  else
    sudo "$@" || echo "hands: sudo $kind $*"
  fi
}

# Build-time compilers. Overlay must not die if a sidecar fails.
if command -v pacman >/dev/null; then
  try_pkgs pacman pacman -S --needed cmake meson ninja wayland wayland-protocols pixman libpng \
    libx11 libxtst libxinerama libxkbcommon glib2 libxmu
  try_pkgs pacman pacman -S --needed python-atspi ffmpeg alsa-utils
elif command -v apt-get >/dev/null; then
  try_pkgs apt-get apt-get install -y cmake meson ninja-build pkg-config libwayland-dev \
    wayland-protocols libpixman-1-dev libpng-dev libx11-dev libxtst-dev libxinerama-dev \
    libxkbcommon-dev libglib2.0-dev libxmu-dev
  try_pkgs apt-get apt-get install -y python3-pyatspi ffmpeg alsa-utils
elif command -v dnf >/dev/null; then
  try_pkgs dnf dnf install -y cmake meson ninja-build wayland-devel wayland-protocols-devel \
    pixman-devel libpng-devel libX11-devel libXtst-devel libXinerama-devel libxkbcommon-devel \
    glib2-devel libXmu-devel
  try_pkgs dnf dnf install -y python3-pyatspi ffmpeg alsa-utils
fi
PREFIX="$PREFIX" HANDS_SRC="${HANDS_SRC:-$ROOT/target/hands-src}" \
  bash "$ROOT/scripts/build-hands.sh" || echo "hands: build-hands.sh continued"
for tool in ydotool ydotoold grim xdotool wmctrl; do
  if [[ -x "$HANDS_BIN/$tool" ]]; then
    echo "hands: sidecar $HANDS_BIN/$tool"
  else
    echo "hands: missing sidecar $tool — Eyes Install hands after deps"
  fi
done

if command -v systemctl >/dev/null && [[ "$SYSTEM" -eq 0 ]]; then
  systemctl --user daemon-reload >/dev/null 2>&1 || true
  systemctl --user enable grokhub.service >/dev/null 2>&1 || true
  systemctl --user enable --now grokhub-hub.service >/dev/null 2>&1 || true
  if [[ -x "$HANDS_BIN/ydotoold" || -x "$(command -v ydotoold 2>/dev/null || true)" ]]; then
    systemctl --user enable --now ydotoold.service >/dev/null 2>&1 || true
    systemctl --user restart ydotoold.service >/dev/null 2>&1 || true
  fi
elif command -v systemctl >/dev/null && [[ -x "$HANDS_BIN/ydotoold" || -x "$(command -v ydotoold 2>/dev/null || true)" ]]; then
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
