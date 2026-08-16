#!/usr/bin/env bash
# Fetch and compile pinned ydotool, grim, xdotool, wmctrl into $PREFIX/lib/grokhub/bin.
# Sidecars only — do not link ydotool into the grokhub ELF (ydotool is AGPL).
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
XDTOOL_TAG="${XDTOOL_TAG:-v4.20260303.1}"
WMCTRL_TAG="${WMCTRL_TAG:-1.07}"

YDTOOL_URL="https://github.com/ReimuNotMoe/ydotool/archive/refs/tags/${YDTOOL_TAG}.tar.gz"
GRIM_URL="https://gitlab.freedesktop.org/emersion/grim/-/archive/${GRIM_TAG}/grim-${GRIM_TAG}.tar.gz"
GRIM_GIT_URL="https://gitlab.freedesktop.org/emersion/grim.git"
GRIM_FALLBACK_URL="https://github.com/emersion/grim/archive/refs/tags/v1.4.0.tar.gz"
XDTOOL_URL="https://github.com/jordansissel/xdotool/archive/refs/tags/${XDTOOL_TAG}.tar.gz"
WMCTRL_URL="https://deb.debian.org/debian/pool/main/w/wmctrl/wmctrl_1.07.orig.tar.gz"
WMCTRL_FALLBACK_URL="https://github.com/Conservatory/wmctrl/archive/refs/heads/master.tar.gz"

need() { command -v "$1" >/dev/null 2>&1; }

on_path() {
  need "$1"
}

skip_fetch() {
  local name="$1"
  if [[ "$HANDS_FORCE" == "1" ]]; then
    return 1
  fi
  if [[ -x "$DEST/$name" ]]; then
    echo "hands: $name already in $DEST — skip fetch"
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

fetch_git() {
  local url="$1"
  local tag="$2"
  local dest="$3"
  if ! need git; then
    return 1
  fi
  rm -rf "$dest"
  mkdir -p "$(dirname "$dest")"
  git clone --depth 1 --branch "$tag" "$url" "$dest"
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
  if skip_fetch ydotool && [[ -x "$DEST/ydotoold" ]]; then
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
  # Configure from the ydotool tree so `git describe` does not pick GrokHub.
  if ! (
    cd "$src" || exit 1
    run_or_continue "ydotool cmake" cmake -S . -B build \
      -DCMAKE_BUILD_TYPE=Release \
      -DCMAKE_INSTALL_PREFIX="$PREFIX"
  ); then
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
      echo "hands: grim ${GRIM_TAG} tarball failed — trying git ${GRIM_TAG}"
      if ! run_or_continue "grim git" fetch_git "$GRIM_GIT_URL" "$GRIM_TAG" "$src"; then
        echo "hands: grim ${GRIM_TAG} git failed — trying v1.4.0 fallback"
        src="$HANDS_SRC/grim-v1.4.0"
        if ! run_or_continue "grim fallback fetch" fetch_tar "$GRIM_FALLBACK_URL" "$src"; then
          return 1
        fi
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

build_xdotool() {
  if skip_fetch xdotool; then
    return 0
  fi
  if ! need make; then
    echo "hands: xdotool needs make — pacman -S --needed base-devel libx11 libxtst libxinerama libxkbcommon"
    return 1
  fi
  local src="$HANDS_SRC/xdotool-${XDTOOL_TAG}"
  if [[ ! -f "$src/Makefile" ]]; then
    if ! run_or_continue "xdotool fetch" fetch_tar "$XDTOOL_URL" "$src"; then
      return 1
    fi
  fi
  if ! run_or_continue "xdotool make" make -C "$src" static -j"$(nproc 2>/dev/null || echo 2)"; then
    if ! run_or_continue "xdotool make" make -C "$src" xdotool WITHOUT_RPATH_FIX=1 -j"$(nproc 2>/dev/null || echo 2)"; then
      return 1
    fi
  fi
  local bin="$src/xdotool.static"
  [[ -x "$bin" ]] || bin="$src/xdotool"
  [[ -x "$bin" ]] || bin="$(find "$src" -name xdotool -type f -perm -111 | head -1)"
  install_bin "$bin" xdotool || return 1
  echo "hands: installed $DEST/xdotool"
}

build_wmctrl() {
  if skip_fetch wmctrl; then
    return 0
  fi
  if ! need make; then
    echo "hands: wmctrl needs make — pacman -S --needed base-devel libx11 glib2 libxmu"
    return 1
  fi
  local src="$HANDS_SRC/wmctrl-${WMCTRL_TAG}"
  if [[ ! -f "$src/configure" && ! -f "$src/Makefile" && ! -f "$src/Makefile.am" ]]; then
    if ! run_or_continue "wmctrl fetch" fetch_tar "$WMCTRL_URL" "$src"; then
      echo "hands: wmctrl ${WMCTRL_TAG} tarball failed — trying Conservatory"
      if ! run_or_continue "wmctrl fallback fetch" fetch_tar "$WMCTRL_FALLBACK_URL" "$src"; then
        return 1
      fi
    fi
  fi
  if [[ -x "$src/configure" || -f "$src/configure" ]]; then
    chmod +x "$src/configure" 2>/dev/null || true
    if ! (
      cd "$src" || exit 1
      run_or_continue "wmctrl configure" ./configure --prefix="$PREFIX" --bindir="$DEST"
    ); then
      return 1
    fi
  elif [[ -f "$src/configure.ac" ]] && need autoreconf; then
    if ! (
      cd "$src" || exit 1
      run_or_continue "wmctrl autoreconf" autoreconf -fi
      run_or_continue "wmctrl configure" ./configure --prefix="$PREFIX" --bindir="$DEST"
    ); then
      return 1
    fi
  fi
  if ! run_or_continue "wmctrl make" make -C "$src" -j"$(nproc 2>/dev/null || echo 2)"; then
    return 1
  fi
  local bin="$src/wmctrl"
  [[ -x "$bin" ]] || bin="$src/src/wmctrl"
  [[ -x "$bin" ]] || bin="$(find "$src" -name wmctrl -type f -perm -111 | head -1)"
  install_bin "$bin" wmctrl || return 1
  echo "hands: installed $DEST/wmctrl"
}

mkdir -p "$DEST" "$HANDS_SRC"
echo "hands: prefix $DEST"
build_ydotool || true
build_grim || true
build_xdotool || true
build_wmctrl || true
echo "hands: done"
exit 0
