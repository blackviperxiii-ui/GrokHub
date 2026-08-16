#!/usr/bin/env bash
# Build dist-release/grokhub-linux-vX.Y.Z.tar.gz (native binaries).
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
cat >"$STAGE/install.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
install -Dm755 "$HERE/grokhub" "$PREFIX/bin/grokhub"
install -Dm755 "$HERE/grokhub-hub" "$PREFIX/bin/grokhub-hub"
install -Dm644 "$HERE/grokhub.desktop" "$PREFIX/share/applications/grokhub.desktop"
install -Dm644 "$HERE/grokhub.svg" "$PREFIX/share/icons/hicolor/scalable/apps/grokhub.svg"
install -Dm644 "$HERE/ydotoold.service" "$HOME/.config/systemd/user/ydotoold.service"
HANDS_PKGS=(ydotool xdotool grim wmctrl python-atspi)
if command -v pacman >/dev/null; then
  if pacman -Q "${HANDS_PKGS[@]}" >/dev/null 2>&1; then
    echo "hands packages present"
  elif [[ "$(id -u)" -eq 0 ]] && pacman -S --needed "${HANDS_PKGS[@]}"; then
    echo "installed hands packages"
  elif sudo pacman -S --needed "${HANDS_PKGS[@]}"; then
    echo "installed hands packages"
  else
    echo "hands: sudo pacman -S --needed ${HANDS_PKGS[*]}"
  fi
fi
if [[ "$(id -u)" -eq 0 ]]; then
  install -Dm644 "$HERE/60-grokhub-uinput.rules" \
    /usr/lib/udev/rules.d/60-grokhub-uinput.rules
  udevadm control --reload-rules 2>/dev/null || true
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
echo "installed $PREFIX/bin/grokhub"
EOF
chmod 755 "$STAGE/install.sh" "$STAGE/grokhub" "$STAGE/grokhub-hub"

OUT="$ROOT/dist-release/grokhub-linux-v${VER}.tar.gz"
tar -C "$ROOT/dist-release" -czf "$OUT" grokhub-linux
echo "$OUT"
