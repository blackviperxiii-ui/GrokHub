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
cat >"$STAGE/install.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"
install -Dm755 "$HERE/grokhub" "$PREFIX/bin/grokhub"
install -Dm755 "$HERE/grokhub-hub" "$PREFIX/bin/grokhub-hub"
install -Dm644 "$HERE/grokhub.desktop" "$PREFIX/share/applications/grokhub.desktop"
install -Dm644 "$HERE/grokhub.svg" "$PREFIX/share/icons/hicolor/scalable/apps/grokhub.svg"
echo "installed $PREFIX/bin/grokhub"
EOF
chmod 755 "$STAGE/install.sh" "$STAGE/grokhub" "$STAGE/grokhub-hub"

OUT="$ROOT/dist-release/grokhub-linux-v${VER}.tar.gz"
tar -C "$ROOT/dist-release" -czf "$OUT" grokhub-linux
echo "$OUT"
