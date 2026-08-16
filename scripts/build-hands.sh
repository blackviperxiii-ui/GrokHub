#!/usr/bin/env bash
# Fetch and compile pinned ydotool + grim into $PREFIX/lib/grokhub/bin.
# Sidecars only — do not link these into the grokhub ELF (ydotool is AGPL).
# Overlay-safe: a failed tool prints the command and continues (exit 0).

set -u

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
if [[ -d "$SCRIPT_DIR/../crates" ]]; then
  ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
else
  ROOT="$SCRIPT_DIR"
fi

PREFIX="${PREFIX:-$HOME/.local}"
DEST="$PREFIX/lib/grokhub/bin"
HANDS_SRC="${HANDS_SRC:-$ROOT/target/hands-src}"
HANDS_FORCE="${HANDS_FORCE:-0}"

YDTOOL_TAG="${YDTOOL_TAG:-v1.0.4}"
GRIM_TAG="${GRIM_TAG:-v1.5.0}"

YDTOOL_URL="https://github.com/ReimuNotMoe/ydotool/archive/refs/tags/${YDTOOL_TAG}.tar.gz"
GRIM_URL="https://gitlab.freedesktop.org/emersion/grim/-/archive/${GRIM_TAG}/grim-${GRIM_TAG}.tar.gz"
GRIM_FALLBACK_URL="https://github.com/emersion/grim/archive/refs/tags/v1.4.0.tar.gz"

need() { command -v "$1" >/dev/null 2>&1; }

on_path() {
  need "$1"
}

skip_fetch() {
  local name="$1"
  if [[ "$HANDS_FORCE" == "1" ]]; then
    return 1
  fi
  if on_path "$name"; then
    echo "hands: $name already on PATH — skip fetch"
    return 0
  fi
  return 1
}

run_or_continue() {
  local label="$1"
  shift
  echo "hands: $*"
  if "$@"; then
    return 0
  fi
  echo "hands: $label failed — $*"
  return 1
}

fetch_tar() {
  local url="$1"
  local dest="$2"
  local tmp
  mkdir -p "$HANDS_SRC" "$dest"
  tmp="$(mktemp "${HANDS_SRC}/fetch.XXXXXX.tar.gz")"
  if need curl; then
    if ! curl -fsSL "$url" -o "$tmp"; then
      rm -f "$tmp"
      return 1
    fi
  elif need wget; then
    if ! wget -qO "$tmp" "$url"; then
      rm -f "$tmp"
      return 1
    fi
  else
    echo "hands: need curl or wget to fetch $url"
    rm -f "$tmp"
    return 1
  fi
  if ! tar -tzf "$tmp" >/dev/null 2>&1; then
    rm -f "$tmp"
    return 1
  fi
  rm -rf "$dest"
  mkdir -p "$dest"
  tar -xzf "$tmp" -C "$dest" --strip-components=1
  local rc=$?
  rm -f "$tmp"
  return "$rc"
}

install_bin() {
  local src="$1"
  local name="$2"
  if [[ ! -x "$src" ]]; then
    echo "hands: missing built $name at $src"
    return 1
  fi
  mkdir -p "$DEST"
  install -Dm755 "$src" "$DEST/$name"
}

build_ydotool() {
  if skip_fetch ydotool && on_path ydotoold; then
    return 0
  fi
  if ! need cmake; then
    echo "hands: ydotool needs cmake — pacman -S --needed cmake"
    return 1
  fi
  local src="$HANDS_SRC/ydotool-${YDTOOL_TAG}"
  if [[ ! -f "$src/CMakeLists.txt" ]]; then
    if ! run_or_continue "ydotool fetch" fetch_tar "$YDTOOL_URL" "$src"; then
      return 1
    fi
  fi
  # Sidecars only: skip man pages (scdoc) and upstream systemd unit.
  if grep -q 'add_subdirectory(manpage)' "$src/CMakeLists.txt" 2>/dev/null; then
    sed -i '/add_subdirectory(manpage)/d' "$src/CMakeLists.txt"
  fi
  if grep -q 'add_subdirectory(Daemon)' "$src/CMakeLists.txt" 2>/dev/null; then
    sed -i '/add_subdirectory(Daemon)/d' "$src/CMakeLists.txt"
  fi
  local bld="$src/build"
  mkdir -p "$bld"
  if ! run_or_continue "ydotool cmake" cmake -S "$src" -B "$bld" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="$PREFIX"; then
    return 1
  fi
  if ! run_or_continue "ydotool build" cmake --build "$bld" -j"$(nproc 2>/dev/null || echo 2)"; then
    return 1
  fi
  local ydo="$bld/ydotool"
  local ydod="$bld/ydotoold"
  [[ -x "$ydo" ]] || ydo="$(find "$bld" -name ydotool -type f -perm -111 | head -1)"
  [[ -x "$ydod" ]] || ydod="$(find "$bld" -name ydotoold -type f -perm -111 | head -1)"
  install_bin "$ydo" ydotool || return 1
  install_bin "$ydod" ydotoold || return 1
  echo "hands: installed $DEST/ydotool $DEST/ydotoold"
}

build_grim() {
  if skip_fetch grim; then
    return 0
  fi
  if ! need meson || ! need ninja; then
    echo "hands: grim needs meson ninja — pacman -S --needed meson ninja wayland wayland-protocols pixman libpng"
    return 1
  fi
  local src="$HANDS_SRC/grim-${GRIM_TAG}"
  if [[ ! -f "$src/meson.build" ]]; then
    if ! run_or_continue "grim fetch" fetch_tar "$GRIM_URL" "$src"; then
      echo "hands: grim ${GRIM_TAG} fetch failed — trying v1.4.0 fallback"
      src="$HANDS_SRC/grim-v1.4.0"
      if ! run_or_continue "grim fallback fetch" fetch_tar "$GRIM_FALLBACK_URL" "$src"; then
        return 1
      fi
    fi
  fi
  local bld="$src/build"
  if [[ ! -f "$bld/build.ninja" ]]; then
    rm -rf "$bld"
    if ! run_or_continue "grim meson" meson setup "$bld" "$src" \
      --prefix="$PREFIX" \
      --bindir=lib/grokhub/bin \
      -Dman-pages=disabled \
      -Djpeg=disabled; then
      # Older grim has fewer meson options.
      if ! run_or_continue "grim meson" meson setup "$bld" "$src" \
        --prefix="$PREFIX" \
        --bindir=lib/grokhub/bin; then
        return 1
      fi
    fi
  fi
  if ! run_or_continue "grim ninja" ninja -C "$bld"; then
    return 1
  fi
  local grim_bin="$bld/grim"
  [[ -x "$grim_bin" ]] || grim_bin="$(find "$bld" -name grim -type f -perm -111 | head -1)"
  install_bin "$grim_bin" grim || return 1
  echo "hands: installed $DEST/grim"
}

mkdir -p "$DEST" "$HANDS_SRC"
echo "hands: prefix $DEST"
build_ydotool || true
build_grim || true
echo "hands: done"
exit 0
