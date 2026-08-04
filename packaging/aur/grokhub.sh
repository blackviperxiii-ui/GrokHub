#!/bin/bash
# GrokHub launcher — prefers a complete user install over broken/unwritable system.
set -euo pipefail

user_lib="${HOME}/.local/lib/grokhub"
user_share="${HOME}/.local/share/grokhub"
sys_lib="/usr/lib/grokhub"

# --- logging first (must exist before any log call; set -euo makes missing log fatal) ---
PORT="${GROKHUB_PORT:-18765}"
URL="${GROKHUB_URL:-http://127.0.0.1:${PORT}}"
RUNTIME="${XDG_RUNTIME_DIR:-/tmp}/grokhub"
LOG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/GrokHub/logs"
LOG="${LOG_DIR}/ui.log"
PIDFILE="${RUNTIME}/ui.pid"
LOCKFILE="${RUNTIME}/ui.lock"
INSTANCE_LOCK="${RUNTIME}/instance.lock"

mkdir -p "$RUNTIME" "$LOG_DIR" 2>/dev/null || true
chmod 700 "$LOG_DIR" 2>/dev/null || true

log() {
  printf '[%s] %s\n' "$(date -Iseconds 2>/dev/null || date)" "$*" >>"$LOG" 2>/dev/null || true
}

# Rotate diagnostic restart log if large
if [[ -f /tmp/grokhub-ui-restart.log ]]; then
  sz=$(wc -c </tmp/grokhub-ui-restart.log 2>/dev/null || echo 0)
  if [[ "${sz:-0}" -gt 200000 ]]; then
    mv -f /tmp/grokhub-ui-restart.log /tmp/grokhub-ui-restart.log.prev 2>/dev/null || true
  fi
fi

# A complete install needs main, UI build, and a bridge that exports factoryReinstall.
install_ok() {
  local root="$1"
  [[ -f "$root/desktop/main.mjs" ]] || return 1
  [[ -f "$root/.output/server/index.mjs" ]] || return 1
  [[ -f "$root/desktop/grok-bridge.cjs" ]] || return 1
  # Broken system packages historically shipped a bridge without factoryReinstall
  # which hard-crashes Electron at require() time.
  grep -q "factoryReinstall" "$root/desktop/grok-bridge.cjs" 2>/dev/null || return 1
  return 0
}

pick_app_root() {
  if [[ -n "${GROKHUB_HOME:-}" ]] && install_ok "${GROKHUB_HOME}"; then
    printf '%s' "$GROKHUB_HOME"
    return
  fi
  # Prefer complete user installs over system (avoids dual-install / broken /usr traps)
  for cand in "$user_lib" "$user_share"; do
    if install_ok "$cand"; then
      printf '%s' "$cand"
      return
    fi
  done
  if install_ok "$sys_lib"; then
    printf '%s' "$sys_lib"
    return
  fi
  # Fallbacks for partial trees (repair may still work)
  for cand in "$user_lib" "$user_share" "$sys_lib"; do
    if [[ -f "$cand/desktop/main.mjs" ]]; then
      printf '%s' "$cand"
      return
    fi
  done
  printf '%s' "${GROKHUB_HOME:-$sys_lib}"
}

APP_ROOT="$(pick_app_root)"
export GROKHUB_HOME="$APP_ROOT"
log "launcher start root=$APP_ROOT"

# Standalone xAI CLI (~/.grok/downloads/grok-*) is NOT this app.
# If you see a high-CPU process named "grok", that is usually the CLI, not GrokHub.
if [[ -d "${HOME}/.grok/downloads" ]] && command -v grok >/dev/null 2>&1; then
  grok_path="$(command -v grok 2>/dev/null || true)"
  if [[ "$grok_path" == *".grok"* ]]; then
    log "note: PATH has standalone grok CLI at $grok_path (unrelated to GrokHub)"
  fi
fi

# Dual-install safety: once a complete user install exists, never fall back to system
# without an explicit override. Pin choice for desktop entries / repair scripts.
INSTALL_PIN="${XDG_CONFIG_HOME:-$HOME/.config}/GrokHub/install-source"
if install_ok "$user_lib"; then
  if [[ "$APP_ROOT" != "$user_lib" && "$APP_ROOT" != "$user_share" ]]; then
    if [[ "${GROKHUB_ALLOW_SYSTEM:-}" != "1" ]]; then
      APP_ROOT="$user_lib"
      export GROKHUB_HOME="$APP_ROOT"
      log "forced user install over system (set GROKHUB_ALLOW_SYSTEM=1 to override)"
      echo "note: dual install detected — using $APP_ROOT (system ignored). Remove /usr/lib/grokhub to silence." >&2
    fi
  fi
  mkdir -p "$(dirname "$INSTALL_PIN")" 2>/dev/null || true
  printf '%s\n' "user:$user_lib" >"$INSTALL_PIN" 2>/dev/null || true
elif install_ok "$sys_lib"; then
  mkdir -p "$(dirname "$INSTALL_PIN")" 2>/dev/null || true
  printf '%s\n' "system:$sys_lib" >"$INSTALL_PIN" 2>/dev/null || true
fi

export GROKHUB_WAYLAND="${GROKHUB_WAYLAND:-1}"
export GROKHUB_TRAY="${GROKHUB_TRAY:-1}"

# Map this process to grokhub.desktop (basename = Wayland app_id = "grokhub")
export CHROME_DESKTOP="grokhub.desktop"
export ELECTRON_FORCE_WINDOW_MENU_BAR=0

if ! command -v electron >/dev/null 2>&1; then
  echo "error: electron not found (pacman -S electron)" >&2
  exit 1
fi

if ! command -v node >/dev/null 2>&1; then
  echo "error: node not found (pacman -S nodejs)" >&2
  exit 1
fi

if [[ ! -f "$APP_ROOT/desktop/main.mjs" ]]; then
  echo "error: GrokHub not installed at $APP_ROOT" >&2
  echo "hint: install user tree to ~/.local/lib/grokhub or system to /usr/lib/grokhub" >&2
  exit 1
fi

if ! install_ok "$APP_ROOT"; then
  echo "warning: install at $APP_ROOT looks incomplete (missing UI build or factoryReinstall in grok-bridge.cjs)" >&2
  echo "hint: run ./scripts/install-arch.sh --user  or  Settings → Factory reinstall" >&2
  log "incomplete install $APP_ROOT"
  if [[ "$APP_ROOT" == "$sys_lib" ]] && install_ok "$user_lib"; then
    APP_ROOT="$user_lib"
    export GROKHUB_HOME="$APP_ROOT"
    log "redirected to user install $APP_ROOT"
    echo "note: redirected to complete user install $APP_ROOT" >&2
  fi
fi

# Dual-install note
if [[ -f "$sys_lib/desktop/main.mjs" && -f "$user_lib/desktop/main.mjs" && "$APP_ROOT" == "$user_lib" ]]; then
  log "using user install $APP_ROOT (stale system also at $sys_lib — ignored; remove with: sudo rm -rf /usr/lib/grokhub)"
fi

# Single-instance: if another grokhub electron is live, focus path is handled by Electron lock;
# still avoid double UI spawn via pidfile.
pid_is_our_ui() {
  local pid="$1"
  [[ -n "$pid" ]] || return 1
  kill -0 "$pid" 2>/dev/null || return 1
  local cmd
  cmd="$(tr '\0' ' ' <"/proc/$pid/cmdline" 2>/dev/null || true)"
  [[ "$cmd" == *node* ]] || [[ "$cmd" == *ELECTRON_RUN_AS_NODE* ]] || return 1
  [[ "$cmd" == *".output/server"* || "$cmd" == *"index.mjs"* || "$cmd" == *grokhub* ]] || return 1
  [[ "$cmd" != *"desktop/main.mjs"* ]] || return 1
  return 0
}

ui_up() {
  curl -sf -o /dev/null --max-time 1 "${URL}/" 2>/dev/null
}

ui_healthy() {
  # HTTP 200 with real HTML is enough. Use grep -a (null bytes in SSR can make
  # GNU grep treat the body as binary and fail the match → launcher exits, app never opens).
  local code body
  code="$(curl -s -o ${RUNTIME}/ui-health.body -w '%{http_code}' --max-time 2 "${URL}/" 2>/dev/null || echo 000)"
  [[ "$code" == "200" ]] || return 1
  body="$(tr -d '\0' <${RUNTIME}/ui-health.body 2>/dev/null || true)"
  [[ -n "$body" && ${#body} -gt 200 ]] || return 1
  if printf '%s' "$body" | grep -aqiE 'GrokHub|<!DOCTYPE html>|tanstack|/assets/|root'; then
    return 0
  fi
  # Accept large 200 HTML even if markers missing (build variants)
  [[ ${#body} -gt 1500 ]]
}

start_ui() {
  if ui_healthy; then
    return 0
  fi

  if [[ -f "$PIDFILE" ]]; then
    old="$(tr -d ' \n\0' <"$PIDFILE" 2>/dev/null || true)"
    if pid_is_our_ui "${old:-}"; then
      # Wait briefly for existing UI
      for _ in $(seq 1 30); do
        if ui_healthy; then return 0; fi
        sleep 0.1
      done
      # Stale listener — only kill our verified UI pid (never fuser -k)
      log "killing stale UI pid $old"
      kill "$old" 2>/dev/null || true
      sleep 0.3
      kill -9 "$old" 2>/dev/null || true
    fi
    rm -f "$PIDFILE"
  fi
  rm -f "$LOCKFILE" 2>/dev/null || true

  if [[ ! -f "$APP_ROOT/.output/server/index.mjs" ]]; then
    echo "error: missing UI build at $APP_ROOT/.output (reinstall package / repair-install)" >&2
    exit 1
  fi

  UI_ENTRY="$APP_ROOT/.output/server/index.mjs"
  if [[ ! -f "$UI_ENTRY" ]]; then
    echo "error: missing UI build at $UI_ENTRY" >&2
    exit 1
  fi
  log "Starting UI on ${URL} from $APP_ROOT entry=$UI_ENTRY"
  echo "Starting GrokHub UI on ${URL} …"
  # Absolute path + cd — never resolve .output from \$HOME (field bug)
  (
    cd "$APP_ROOT" || exit 1
    export PORT="$PORT"
    export NITRO_PORT="$PORT"
    export HOST="127.0.0.1"
    export NITRO_HOST="127.0.0.1"
    export GROKHUB_HOME="$APP_ROOT"
    exec node "$UI_ENTRY"
  ) >>"$LOG" 2>&1 &
  echo $! >"$PIDFILE"
  echo $! >"$LOCKFILE"
  # Diagnostic mirror
  echo "[ui] $(date -Iseconds) pid=$(cat "$PIDFILE" 2>/dev/null) root=$APP_ROOT entry=$UI_ENTRY" >>/tmp/grokhub-ui-restart.log 2>/dev/null || true

  for _ in $(seq 1 150); do
    if ui_healthy; then
      log "UI healthy pid=$(cat "$PIDFILE" 2>/dev/null || echo '?')"
      return 0
    fi
    sleep 0.2
  done

  echo "warning: UI slow/unhealthy after start — see $LOG (continuing)" >&2
  tail -n 40 "$LOG" >&2 || true
  log "UI health timeout — continuing to Electron"
  return 0
}

start_ui

if ! ui_healthy; then
  echo "warning: GrokHub UI not healthy at ${URL} — launching Electron anyway (UI may recover)" >&2
  log "UI unhealthy at launch; continuing to Electron"
  # do not exit 1 — blank/recoverable window is better than app refusing to open
fi

export GROKHUB_URL="$URL"

ELECTRON_BIN="$(command -v electron)"
log "exec electron=$ELECTRON_BIN root=$APP_ROOT"

# Optional sandbox: set GROKHUB_SANDBOX=1 to try Chromium sandbox (default off for system electron).
EXTRA_FLAGS=()
if [[ "${GROKHUB_SANDBOX:-0}" != "1" ]]; then
  EXTRA_FLAGS+=(--no-sandbox)
fi

# Use a stable argv0 + WM class that match StartupWMClass=grokhub / desktop id.
exec -a grokhub "$ELECTRON_BIN" \
  --class=grokhub \
  --name=grokhub \
  "${EXTRA_FLAGS[@]}" \
  "$APP_ROOT/desktop/main.mjs" \
  "$@"
