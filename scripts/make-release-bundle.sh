#!/usr/bin/env bash
# Build dist-release/grokhub-linux-vX.Y.Z.tar.gz (native binaries).
# Hands sidecars are not bundled. Computer-use is Grok Build.
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
cp -a "$ROOT/packaging/systemd/grokhub.service" "$STAGE/grokhub.service"
cp -a "$ROOT/packaging/systemd/grokhub-hub.service" "$STAGE/grokhub-hub.service"
cp -a "$ROOT/scripts/install-grok-cli.sh" "$STAGE/install-grok-cli.sh"
cat >"$STAGE/install.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
install -Dm755 "$HERE/grokhub" "$PREFIX/bin/grokhub"
install -Dm755 "$HERE/grokhub-hub" "$PREFIX/bin/grokhub-hub"
install -Dm644 "$HERE/grokhub.desktop" "$PREFIX/share/applications/grokhub.desktop"
install -Dm644 "$HERE/grokhub.svg" "$PREFIX/share/icons/hicolor/scalable/apps/grokhub.svg"
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
if command -v pacman >/dev/null; then
  try_pkgs pacman pacman -S --needed ffmpeg alsa-utils
elif command -v apt-get >/dev/null; then
  try_pkgs apt-get apt-get install -y ffmpeg alsa-utils
elif command -v dnf >/dev/null; then
  try_pkgs dnf dnf install -y ffmpeg alsa-utils
fi
install -Dm644 "$HERE/grokhub-hub.service" \
  "$HOME/.config/systemd/user/grokhub-hub.service"
install -Dm644 "$HERE/grokhub.service" \
  "$HOME/.config/systemd/user/grokhub.service"
if command -v systemctl >/dev/null; then
  systemctl --user daemon-reload >/dev/null 2>&1 || true
  systemctl --user enable grokhub.service >/dev/null 2>&1 || true
  systemctl --user enable --now grokhub-hub.service >/dev/null 2>&1 || true
fi
PREFIX="$PREFIX" bash "$HERE/install-grok-cli.sh" \
  || echo "grok: install-grok-cli.sh continued"
echo "installed $PREFIX/bin/grokhub"
if [[ -x "$PREFIX/bin/grok" || -x "$HOME/.grok/bin/grok" ]] || command -v grok >/dev/null 2>&1; then
  echo "installed Grok Build CLI (grok)"
else
  echo "grok: Grok Build CLI not on PATH — curl -fsSL https://x.ai/cli/install.sh | bash"
fi
EOF
chmod 755 "$STAGE/install.sh" "$STAGE/grokhub" "$STAGE/grokhub-hub" \
  "$STAGE/install-grok-cli.sh"

OUT="$ROOT/dist-release/grokhub-linux-v${VER}.tar.gz"
tar -C "$ROOT/dist-release" -czf "$OUT" grokhub-linux
echo "$OUT"
