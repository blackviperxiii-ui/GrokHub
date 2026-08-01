#!/usr/bin/env bash
# Build source tarball + checksums for packaging/aur
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PKGVER="$(node -p "require('./package.json').version" 2>/dev/null || echo 0.1.0)"
NAME="grokclaw"
STAGE="$(mktemp -d)"
OUT_DIR="$ROOT/packaging/aur"
TARBALL="${OUT_DIR}/${NAME}-${PKGVER}.tar.gz"

mkdir -p "$OUT_DIR"
rm -f "$TARBALL"

echo "Staging ${NAME}-${PKGVER} …"
DEST="${STAGE}/${NAME}-${PKGVER}"
mkdir -p "$DEST"

# Copy source needed to build (exclude heavy/generated trees)
tar -C "$ROOT" \
  --exclude=node_modules \
  --exclude=.git \
  --exclude=.output \
  --exclude=.vercel \
  --exclude=dist \
  --exclude=screenshots \
  --exclude=attachments \
  --exclude='packaging/aur/*.tar.gz' \
  --exclude='packaging/aur/*.pkg.tar*' \
  --exclude='packaging/aur/pkg' \
  --exclude='packaging/aur/src' \
  --exclude='*.log' \
  -cf - \
  package.json package-lock.json tsconfig.json vite.config.ts eslint.config.mjs \
  LICENSE desktop src migrations scripts packaging \
  2>/dev/null | tar -C "$DEST" -xf -

# Ensure critical files exist
test -f "$DEST/package.json"
test -f "$DEST/desktop/main.mjs"
test -f "$DEST/packaging/aur/grokclaw.sh"

echo "Creating $TARBALL"
tar -C "$STAGE" -czf "$TARBALL" "${NAME}-${PKGVER}"

SUM="$(sha256sum "$TARBALL" | awk '{print $1}')"
SIZE="$(du -h "$TARBALL" | awk '{print $1}')"

if [[ -f "$OUT_DIR/PKGBUILD" ]]; then
  sed -i "s/^sha256sums=.*/sha256sums=('${SUM}')/" "$OUT_DIR/PKGBUILD"
  sed -i "s/^pkgver=.*/pkgver=${PKGVER}/" "$OUT_DIR/PKGBUILD"
fi

if command -v makepkg >/dev/null 2>&1; then
  (
    cd "$OUT_DIR"
    makepkg --printsrcinfo > .SRCINFO
  )
else
  cat > "$OUT_DIR/.SRCINFO" <<EOF
pkgbase = ${NAME}
	pkgdesc = Grok-native agent control plane (modes, Imagine, connectors, unsandboxed desktop host)
	pkgver = ${PKGVER}
	pkgrel = 1
	url = https://github.com/blackviperxiii-ui/spring-dove-reef-apple

	arch = x86_64
	license = MIT
	makedepends = npm
	depends = electron
	depends = nodejs
	depends = curl
	options = !strip
	options = !debug
	source = ${NAME}-${PKGVER}.tar.gz
	sha256sums = ${SUM}

pkgname = ${NAME}
EOF
fi

rm -rf "$STAGE"

echo ""
echo "AUR release artifacts:"
echo "  tarball : $TARBALL ($SIZE)"
echo "  sha256  : $SUM"
echo "  PKGBUILD: $OUT_DIR/PKGBUILD"
echo "  SRCINFO : $OUT_DIR/.SRCINFO"
echo ""
echo "Install locally:"
echo "  cd packaging/aur && makepkg -si"
