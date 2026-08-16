#!/usr/bin/env bash
# Build dist-release/grokhub-linux-vX.Y.Z.tar.gz (native binaries).
# Hands sidecars build on the machine via install.sh — keep the tar small.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VER="$(sed -n '/\[workspace.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p;}' "$ROOT/Cargo.toml" | head -1)"
if [[ -z "$VER" ]]; then
  echo "error: could not read workspace version" >&2
  exit 1
fi

cd "$ROOT"
cargo build --release --locked -p grokhub-app -p grokhub-hub

STAGE="$ROOT/dist-release/grokhub-linux"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp -a "$ROOT/target/release/grokhub" "$STAGE/grokhub"
cp -a "$ROOT/target/release/grokhub-hub" "$STAGE/grokhub-hub"
cp -a "$ROOT/packaging/grokhub.desktop" "$STAGE/grokhub.desktop"
cp -a "$ROOT/packaging/grokhub.svg" "$STAGE/grokhub.svg"
cp -a "$ROOT/LICENSE" "$STAGE/LICENSE"
cp -a "$ROOT/packaging/systemd/ydotoold.service" "$STAGE/ydotoold.service"
cp -a "$ROOT/packaging/udev/60-grokhub-uinput.rules" "$STAGE/60-grokhub-uinput.rules"
cp -a "$ROOT/scripts/build-hands.sh" "$STAGE/build-hands.sh"
cat >"$STAGE/install.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
HANDS_BIN="$PREFIX/lib/grokhub/bin"
install -Dm755 "$HERE/grokhub" "$PREFIX/bin/grokhub"
install -Dm755 "$HERE/grokhub-hub" "$PREFIX/bin/grokhub-hub"
install -Dm644 "$HERE/grokhub.desktop" "$PREFIX/share/applications/grokhub.desktop"
install -Dm644 "$HERE/grokhub.svg" "$PREFIX/share/icons/hicolor/scalable/apps/grokhub.svg"
mkdir -p "$(dirname "$HOME/.config/systemd/user/ydotoold.service")"
cat >"$HOME/.config/systemd/user/ydotoold.service" <<UNIT
[Unit]
Description=ydotool daemon for GrokHub hands
PartOf=graphical-session.target
After=graphical-session.target
ConditionPathExists=${HANDS_BIN}/ydotoold

[Service]
Type=simple
ExecStart=${HANDS_BIN}/ydotoold --socket-path=%t/ydotool.sock
Restart=on-failure
RestartSec=2
Environment=YDOTOOL_SOCKET=%t/ydotool.sock

[Install]
WantedBy=graphical-session.target
UNIT
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
PREFIX="$PREFIX" HANDS_SRC="${HANDS_SRC:-$HERE/hands-src}" \
  bash "$HERE/build-hands.sh" || echo "hands: build-hands.sh continued"
if command -v pacman >/dev/null; then
  if pacman -Q python-atspi >/dev/null 2>&1; then
    echo "hands: python-atspi already installed"
  elif [[ "$(id -u)" -eq 0 ]]; then
    pacman -S --needed python-atspi || echo "hands: pacman -S --needed python-atspi"
  else
    sudo pacman -S --needed python-atspi || echo "hands: sudo pacman -S --needed python-atspi"
  fi
fi
UDEV_SRC="$HERE/60-grokhub-uinput.rules"
UDEV_DEST="/etc/udev/rules.d/60-grokhub-uinput.rules"
if [[ "$(id -u)" -eq 0 ]]; then
  install -Dm644 "$UDEV_SRC" "$UDEV_DEST" || echo "hands: install $UDEV_DEST"
  modprobe uinput || echo "hands: modprobe uinput"
  udevadm control --reload-rules 2>/dev/null || true
  udevadm trigger --subsystem-match=misc --attr-match=name=uinput 2>/dev/null || true
else
  sudo install -Dm644 "$UDEV_SRC" "$UDEV_DEST" || echo "hands: sudo install $UDEV_DEST"
  sudo modprobe uinput || echo "hands: sudo modprobe uinput"
  sudo udevadm control --reload-rules 2>/dev/null || true
  sudo udevadm trigger --subsystem-match=misc --attr-match=name=uinput 2>/dev/null || true
fi
if command -v systemctl >/dev/null && [[ -x "$HANDS_BIN/ydotoold" ]]; then
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
echo "installed $PREFIX/bin/grokhub"
echo "hands sidecars $HANDS_BIN"
EOF
chmod 755 "$STAGE/install.sh" "$STAGE/grokhub" "$STAGE/grokhub-hub" "$STAGE/build-hands.sh"

OUT="$ROOT/dist-release/grokhub-linux-v${VER}.tar.gz"
tar -C "$ROOT/dist-release" -czf "$OUT" grokhub-linux
echo "$OUT"
