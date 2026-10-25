#!/usr/bin/env bash
# Copies the Tauri shell's bundles from src-tauri/target/release/bundle into
# dist-release/ under their release names (cross-platform recode, Phase 6):
#   grokhub-shell-vX.Y.Z-linux-amd64.deb / .AppImage
#   GrokHub-Shell-Setup-X.Y.Z.exe
#   grokhub-shell-vX.Y.Z-macos-arm64.dmg
# Fails unless exactly one bundle of each kind exists for the VERSION.
#
#   scripts/name-shell-bundles.sh Linux|Windows|macOS
set -euo pipefail

os="${1:?usage: name-shell-bundles.sh Linux|Windows|macOS}"
root="$(cd "$(dirname "$0")/.." && pwd)"
ver="$(tr -d '[:space:]' < "$root/VERSION")"
bundle="$root/src-tauri/target/release/bundle"
out="$root/dist-release"
mkdir -p "$out"

# copy <kind dir> <file suffix> <release name>
copy() {
  local found=()
  shopt -s nullglob
  found=("$bundle/$1/"*"_${ver}_$2")
  shopt -u nullglob
  if [ "${#found[@]}" -ne 1 ]; then
    echo "name-shell-bundles: want one $bundle/$1/*_${ver}_$2, found ${#found[@]}" >&2
    exit 1
  fi
  cp "${found[0]}" "$out/$3"
  echo "name-shell-bundles: $3"
}

case "$os" in
  Linux)
    copy deb amd64.deb "grokhub-shell-v$ver-linux-amd64.deb"
    copy appimage amd64.AppImage "grokhub-shell-v$ver-linux-amd64.AppImage"
    ;;
  Windows)
    copy nsis x64-setup.exe "GrokHub-Shell-Setup-$ver.exe"
    ;;
  macOS)
    copy dmg aarch64.dmg "grokhub-shell-v$ver-macos-arm64.dmg"
    ;;
  *)
    echo "name-shell-bundles: unknown OS $os" >&2
    exit 2
    ;;
esac
