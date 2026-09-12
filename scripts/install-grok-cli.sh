#!/usr/bin/env bash
# Install xAI Grok Build CLI alpha (https://x.ai/cli) alongside grokhub.
# Official installer writes ~/.grok/bin/grok. This also links PREFIX/bin/grok.
# Overlay-safe: never fails the cabin install. First-time fetch is GROK_CHANNEL=alpha.
set -u

PREFIX="${PREFIX:-$HOME/.local}"
GROK_INSTALL_URL="${GROK_INSTALL_URL:-https://x.ai/cli/install.sh}"
export GROK_CHANNEL="${GROK_CHANNEL:-alpha}"

if [[ -z "${GROK_BIN_DIR:-}" ]]; then
  case "$PREFIX" in
    /usr|/usr/local) export GROK_BIN_DIR="$PREFIX/bin" ;;
  esac
fi

grok_present() {
  command -v grok >/dev/null 2>&1 \
    || [[ -x "${GROK_BIN_DIR:-}/grok" ]] \
    || [[ -x "$PREFIX/bin/grok" ]] \
    || [[ -x "$HOME/.grok/bin/grok" ]]
}

grok_bin() {
  if [[ -x "${GROK_BIN_DIR:-}/grok" ]]; then
    printf '%s\n' "${GROK_BIN_DIR}/grok"
  elif [[ -x "$HOME/.grok/bin/grok" ]]; then
    printf '%s\n' "$HOME/.grok/bin/grok"
  elif command -v grok >/dev/null 2>&1; then
    command -v grok
  fi
}

# Channel grok will follow on `grok update`. Alpha only — never --stable.
grok_channel() {
  local bin="$1"
  local cfg="${GROK_HOME:-$HOME/.grok}/config.toml"
  local ch=""
  if [[ -f "$cfg" ]]; then
    ch=$(awk '
      BEGIN { in_cli = 0 }
      /^[[:space:]]*\[cli\][[:space:]]*$/ { in_cli = 1; next }
      /^[[:space:]]*\[/ { in_cli = 0; next }
      in_cli && $0 ~ /^[[:space:]]*channel[[:space:]]*=/ {
        sub(/^[[:space:]]*channel[[:space:]]*=[[:space:]]*/, "")
        gsub(/["'\'']/, "")
        gsub(/[[:space:]]/, "")
        print
        exit
      }
    ' "$cfg")
  fi
  if [[ -z "$ch" && -n "$bin" ]]; then
    local json
    json=$("$bin" update --check --json 2>/dev/null || true)
    ch=$(printf '%s' "$json" | sed -n 's/.*"channel"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)
  fi
  printf '%s\n' "$ch"
}

grok_on_alpha() {
  local ch
  ch=$(grok_channel "$1" | tr '[:upper:]' '[:lower:]')
  [[ "$ch" == "alpha" ]]
}

link_into_prefix() {
  local src=""
  if [[ -x "${GROK_BIN_DIR:-}/grok" ]]; then
    src="${GROK_BIN_DIR}/grok"
  elif [[ -x "$HOME/.grok/bin/grok" ]]; then
    src="$HOME/.grok/bin/grok"
  elif command -v grok >/dev/null 2>&1; then
    src="$(command -v grok)"
  fi
  [[ -n "$src" ]] || return 0
  mkdir -p "$PREFIX/bin" || return 0
  if [[ "$src" != "$PREFIX/bin/grok" && ! -e "$PREFIX/bin/grok" ]]; then
    ln -sf "$src" "$PREFIX/bin/grok" 2>/dev/null \
      || install -Dm755 "$src" "$PREFIX/bin/grok" \
      || true
  fi
  local agent=""
  if [[ -x "${GROK_BIN_DIR:-}/agent" ]]; then
    agent="${GROK_BIN_DIR}/agent"
  elif [[ -x "$HOME/.grok/bin/agent" ]]; then
    agent="$HOME/.grok/bin/agent"
  fi
  if [[ -n "$agent" && "$agent" != "$PREFIX/bin/agent" && ! -e "$PREFIX/bin/agent" ]]; then
    ln -sf "$agent" "$PREFIX/bin/agent" 2>/dev/null || true
  fi
}

if grok_present; then
  link_into_prefix
  bin="$(grok_bin)"
  if grok_on_alpha "$bin"; then
    echo "grok: Grok Build CLI alpha already present"
    exit 0
  fi
  echo "grok: Grok Build CLI is not on alpha — switching with grok update --alpha"
  if [[ -n "$bin" ]]; then
    "$bin" update --alpha \
      || echo "grok: grok update --alpha failed — cabin install continues"
  else
    echo "grok: missing — run: curl -fsSL https://x.ai/cli/install.sh | GROK_CHANNEL=alpha bash"
  fi
  link_into_prefix
  exit 0
fi

if ! command -v curl >/dev/null 2>&1 && ! command -v wget >/dev/null 2>&1; then
  echo "grok: curl or wget required to install Grok Build CLI from https://x.ai/cli"
  exit 0
fi

echo "grok: installing Grok Build CLI alpha (GROK_CHANNEL=alpha) from https://x.ai/cli"
if command -v curl >/dev/null 2>&1; then
  curl -fsSL "$GROK_INSTALL_URL" | bash \
    || echo "grok: official installer failed — cabin install continues"
else
  wget -qO- "$GROK_INSTALL_URL" | bash \
    || echo "grok: official installer failed — cabin install continues"
fi

link_into_prefix
if grok_present; then
  echo "grok: installed Grok Build CLI"
else
  echo "grok: missing — run: curl -fsSL https://x.ai/cli/install.sh | GROK_CHANNEL=alpha bash"
fi
