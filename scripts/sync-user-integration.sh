#!/usr/bin/env bash
# Sync launcher, desktop entry, icons, optional systemd user unit for ~/.local installs.
set -euo pipefail
HOME_DIR="${HOME:-}"
[[ -n "$HOME_DIR" ]] || { echo "error: HOME is not set" >&2; exit 1; }
APP_LIB="${GROKHUB_HOME:-$HOME_DIR/.local/lib/grokhub}"
BIN="$HOME_DIR/.local/bin/grokhub"
DESKTOP_DIR="$HOME_DIR/.local/share/applications"
ICON_ROOT="$HOME_DIR/.local/share/icons/hicolor"
ENABLE_AGENT=0
START_NOW=0
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
for arg in "$@"; do
  case "$arg" in
    --agent) ENABLE_AGENT=1 ;;
    --now) START_NOW=1 ;;
    --help|-h) echo "Usage: $0 [--agent] [--now]"; exit 0 ;;
  esac
done
[[ -f "$APP_LIB/desktop/main.mjs" ]] || { echo "error: no install at $APP_LIB" >&2; exit 1; }
LAUNCHER_SRC=""
for cand in "$APP_LIB/packaging/aur/grokhub.sh" "$REPO_ROOT/packaging/aur/grokhub.sh"; do
  [[ -f "$cand" ]] && LAUNCHER_SRC="$cand" && break
done
mkdir -p "$(dirname "$BIN")"
if [[ -n "$LAUNCHER_SRC" ]]; then
  install -Dm755 "$LAUNCHER_SRC" "$BIN"
else
  printf '#!/bin/bash\nexport GROKHUB_HOME=%q\nexec electron --class=grokhub --name=grokhub "$GROKHUB_HOME/desktop/main.mjs" "$@"\n' "$APP_LIB" >"$BIN"
  chmod 755 "$BIN"
fi
echo "launcher → $BIN"
mkdir -p "$DESKTOP_DIR"
DESK_SRC=""
for cand in "$APP_LIB/packaging/grokhub.desktop" "$REPO_ROOT/packaging/grokhub.desktop"; do
  [[ -f "$cand" ]] && DESK_SRC="$cand" && break
done
if [[ -n "$DESK_SRC" ]]; then
  sed -e "s|/usr/bin/grokhub|${BIN}|g" -e "s|^TryExec=.*|TryExec=${BIN}|" \
    "$DESK_SRC" >"${DESKTOP_DIR}/grokhub.desktop"
else
  printf '[Desktop Entry]\nType=Application\nName=GrokHub\nExec=%s %%U\nTryExec=%s\nIcon=grokhub\nTerminal=false\nStartupWMClass=grokhub\n' "$BIN" "$BIN" >"${DESKTOP_DIR}/grokhub.desktop"
fi
chmod 644 "${DESKTOP_DIR}/grokhub.desktop"
echo "menu    → ${DESKTOP_DIR}/grokhub.desktop"
[[ -f "$APP_LIB/packaging/grokhub.svg" ]] && install -Dm644 "$APP_LIB/packaging/grokhub.svg" "${ICON_ROOT}/scalable/apps/grokhub.svg" || true
if [[ -d "$APP_LIB/packaging/icons/hicolor" ]]; then
  while IFS= read -r -d '' png; do
    rel="${png#"$APP_LIB/packaging/icons/hicolor/"}"
    install -Dm644 "$png" "${ICON_ROOT}/${rel}"
  done < <(find "$APP_LIB/packaging/icons/hicolor" -type f -name '*.png' -print0 2>/dev/null)
fi
command -v update-desktop-database >/dev/null && update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
if compgen -G "$HOME_DIR/.grok/downloads/grok-*" >/dev/null 2>&1; then
  echo "note: ~/.grok/downloads has standalone xAI CLI binaries (not GrokHub). Command is 'grok' ≠ 'grokhub'."
fi
if [[ "$ENABLE_AGENT" -eq 1 ]]; then
  UNIT_DIR="$HOME_DIR/.config/systemd/user"
  mkdir -p "$UNIT_DIR"
  cat >"${UNIT_DIR}/grokhub-agent.service" <<UNIT
[Unit]
Description=GrokHub always-on agent core
After=graphical-session.target
PartOf=graphical-session.target

[Service]
Type=simple
WorkingDirectory=${APP_LIB}
ExecStart=${BIN} --agent
Restart=on-failure
RestartSec=5
Environment=GROKHUB_AGENT=1
Environment=GROKHUB_TRAY=1
Environment=GROKHUB_HOME=${APP_LIB}
Environment=HOME=${HOME_DIR}

[Install]
WantedBy=default.target
UNIT
  if command -v systemctl >/dev/null; then
    systemctl --user daemon-reload
    systemctl --user enable grokhub-agent.service
    [[ "$START_NOW" -eq 1 ]] && systemctl --user restart grokhub-agent.service || true
    echo "agent   → systemd --user grokhub-agent.service"
  fi
fi
echo "User integration OK · $APP_LIB"
