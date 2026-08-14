#!/usr/bin/env bash
# Install the system tools GrokHub computer-use needs on Arch / CachyOS.
# Run in a real terminal (needs sudo/pacman). Does not modify GrokHub user data.
set -euo pipefail
if ! command -v pacman >/dev/null 2>&1; then
  echo "This helper is for Arch / CachyOS (pacman)." >&2
  echo "Install ydotool grim xdotool ffmpeg with your package manager." >&2
  exit 1
fi
echo "==> GrokHub computer-use tools"
sudo pacman -S --needed ydotool grim xdotool ffmpeg
echo ""
echo "Wayland (KDE/GNOME): grim captures the full framebuffer; ydotool injects via /dev/uinput."
echo "  /dev/uinput must be writable (KDE uaccess ACL is enough on many boxes; else: usermod -aG input \$USER && re-login)."
if command -v systemctl >/dev/null 2>&1 && systemctl --user list-unit-files 2>/dev/null | grep -q ydotoold; then
  echo "  This package ships ydotoold — enable with: systemctl --user enable --now ydotoold.service"
fi
echo "Then relaunch GrokHub and Desktop host → Computer → Probe."
