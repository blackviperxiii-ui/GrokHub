# GrokHub on Arch Linux

## Install

```bash
sudo pacman -S --needed git electron nodejs npm curl base-devel
git clone https://github.com/blackviperxiii-ui/Grok-Hub.git
cd Grok-Hub
sudo ./scripts/install-arch.sh
grokhub
```

See [aur/README.md](./aur/README.md) for PKGBUILD options.

## Optional: computer use (screenshot + mouse/keyboard)

GrokHub can drive the Linux desktop when **Settings → Agent → Computer use** is enabled
(off by default). Capture is the same silent stack Cursor uses: **ffmpeg x11grab**
(or **grim** on native Wayland) plus **xdotool** / **ydotool**. It will not open a
screenshot or portal app. Playwright stays test-only.

```bash
# Wayland (KDE/GNOME) — required on CachyOS Plasma
sudo pacman -S --needed ydotool grim xdotool ffmpeg
# or: ./scripts/provision-computer-use.sh

# X11-only session
sudo pacman -S --needed ffmpeg xdotool
```

See [COMPUTER_USE.md](../COMPUTER_USE.md). Multi-monitor clicks use the full virtual desktop (`xdpyinfo` / `xrandr` if xdotool is missing). `/dev/uinput` must be writable for ydotool (KDE `uaccess` ACL is often enough).

Computer use sends screenshots to Grok as vision, so it needs **Grok OAuth or an xAI API key**.
The website-free fallback cannot see the screen.
