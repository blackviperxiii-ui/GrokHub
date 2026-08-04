#!/usr/bin/env bash
# Sync launcher, desktop entry, icons, and optional systemd user unit for a
# ~/.local GrokHub install. Safe to re-run after updates.
#
# Usage:
#   ./scripts/sync-user-integration.sh           # menu + launcher only
#   ./scripts/sync-user-integration.sh --agent    # also enable systemd user unit
#   ./scripts/sync-user-integration.sh --agent --now
set -euo pipefail

HOME_DIR="${HOME:-}"
if [[ -z "$HOME_DIR" ]]; then
  echo "error: HOME is not set" >&2
  exit 1
fi

APP_LIB="${GROKHUB_HOME:-$HOME_DIR/.local/lib/grokhub}"
BIN="$HOME_DIR/.local/bin/grokhub"
DESKTOP_DIR="$HOME_DIR/.local/share/applications"
ICON_ROOT="$HOME_DIR/.local/share/icons/hicolor"
UNIT_SRC=""
ENABLE_AGENT=0
START_NOW=0
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

for arg in "$@"; do
  case "$arg" in
    --agent) ENABLE_AGENT=1 ;;
    --now) START_NOW=1 ;;
    --help|-h)
      echo "Usage: $0 [--agent] [--now]"
      echo "  Refresh ~/.local/bin/grokhub + app menu entry from install tree"
      echo "  --agent  install/enable systemd --user grokhub-agent.service"
      echo "  --now    start the agent unit immediately (with --agent)"
      exit 0
      ;;
  esac
done

if [[ ! -f "$APP_LIB/desktop/main.mjs" ]]; then
  echo "error: no GrokHub install at $APP_LIB" >&2
  echo "hint: ./scripts/install-arch.sh --user   or in-app update" >&2
  exit 1
fi

LAUNCHER_SRC=""
for cand in "$APP_LIB/packaging/aur/grokhub.sh" "$REPO_ROOT/packaging/aur/grokhub.sh"; do
  if [[ -f "$cand" ]]; then
    LAUNCHER_SRC="$cand"
    break
  fi
done

mkdir -p "$(dirname "$BIN")"
if [[ -n "$LAUNCHER_SRC" ]]; then
  install -Dm755 "$LAUNCHER_SRC" "$BIN"
else
  cat >"$BIN" <<WRAP
#!/bin/bash
export GROKHUB_HOME="$APP_LIB"
export HOME="\${HOME:-$HOME_DIR}"
exec electron --class=grokhub --name=grokhub "\$GROKHUB_HOME/desktop/main.mjs" "\$@"
WRAP
  chmod 755 "$BIN"
fi
echo "launcher → $BIN"

mkdir -p "$DESKTOP_DIR"
DESK_SRC=""
for cand in "$APP_LIB/packaging/grokhub.desktop" "$REPO_ROOT/packaging/grokhub.desktop"; do
  if [[ -f "$cand" ]]; then
    DESK_SRC="$cand"
    break
  fi
done

if [[ -n "$DESK_SRC" ]]; then
  sed -e "s|/usr/bin/grokhub|${BIN}|g" \
      -e "s|^TryExec=.*|TryExec=${BIN}|" \
      "$DESK_SRC" >"${DESKTOP_DIR}/grokhub.desktop"
else
  cat >"${DESKTOP_DIR}/grokhub.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=GrokHub
Exec=${BIN} %U
TryExec=${BIN}
Icon=grokhub
Terminal=false
Categories=Utility;Development;
StartupWMClass=grokhub
EOF
fi
chmod 644 "${DESKTOP_DIR}/grokhub.desktop"
echo "menu    → ${DESKTOP_DIR}/grokhub.desktop"

if [[ -f "$APP_LIB/packaging/grokhub.svg" ]]; then
  install -Dm644 "$APP_LIB/packaging/grokhub.svg" \
    "${ICON_ROOT}/scalable/apps/grokhub.svg"
fi
if [[ -d "$APP_LIB/packaging/icons/hicolor" ]]; then
  while IFS= read -r -d '' png; do
    rel="${png#"$APP_LIB/packaging/icons/hicolor/"}"
    install -Dm644 "$png" "${ICON_ROOT}/${rel}"
  done < <(find "$APP_LIB/packaging/icons/hicolor" -type f -name '*.png' -print0 2>/dev/null)
fi

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t "$ICON_ROOT" 2>/dev/null || true
fi

# Name collision: standalone xAI "grok" CLI under ~/.grok (not GrokHub)
if compgen -G "$HOME_DIR/.grok/downloads/grok-*" >/dev/null 2>&1; then
  echo ""
  echo "note: standalone xAI CLI files under ~/.grok/downloads/ are NOT GrokHub."
  echo "      Command 'grok' ≠ 'grokhub'. High-CPU 'grok' processes are usually that CLI."
  echo "      GrokHub never installs a binary named 'grok'."
fi

if [[ "$ENABLE_AGENT" -eq 1 ]]; then
  UNIT_DIR="$HOME_DIR/.config/systemd/user"
  mkdir -p "$UNIT_DIR"
  for cand in \
    "$APP_LIB/packaging/systemd/grokhub-agent.service" \
    "$REPO_ROOT/packaging/systemd/grokhub-agent.service"
  do
    if [[ -f "$cand" ]]; then
      UNIT_SRC="$cand"
      break
    fi
  done
  if [[ -z "$UNIT_SRC" ]]; then
    echo "error: grokhub-agent.service not found" >&2
    exit 1
  fi
  # Absolute launcher + install home for agent mode
  {
    echo "[Unit]"
    echo "Description=GrokHub always-on agent core"
    echo "After=graphical-session.target"
    echo "PartOf=graphical-session.target"
    echo ""
    echo "[Service]"
    echo "Type=simple"
    echo "WorkingDirectory=${APP_LIB}"
    echo "ExecStart=${BIN} --agent"
    echo "Restart=on-failure"
    echo "RestartSec=5"
    echo "Environment=GROKHUB_AGENT=1"
    echo "Environment=GROKHUB_TRAY=1"
    echo "Environment=GROKHUB_HOME=${APP_LIB}"
    echo "Environment=HOME=${HOME_DIR}"
    echo ""
    echo "[Install]"
    echo "WantedBy=default.target"
  } >"${UNIT_DIR}/grokhub-agent.service"
  if command -v systemctl >/dev/null 2>&1; then
    systemctl --user daemon-reload
    systemctl --user enable grokhub-agent.service
    if [[ "$START_NOW" -eq 1 ]]; then
      systemctl --user restart grokhub-agent.service
      systemctl --user --no-pager --full status grokhub-agent.service || true
    fi
    echo "agent   → systemd --user grokhub-agent.service (enabled)"
  else
    echo "note: systemctl not found — unit written to ${UNIT_DIR}/grokhub-agent.service"
  fi
fi

echo ""
echo "User integration OK"
echo "  App:     $APP_LIB"
echo "  Launch:  $BIN"
echo "  Menu:    ${DESKTOP_DIR}/grokhub.desktop"
if [[ "$ENABLE_AGENT" -eq 1 ]]; then
  echo "  Agent:   systemctl --user status grokhub-agent"
fi
