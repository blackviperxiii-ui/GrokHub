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
(off by default). The agent takes screenshots via Electron and injects input with
system tools — Playwright is **not** used for this (it remains a test-only dependency).

```bash
# X11 or XWayland
sudo pacman -S --needed xdotool

# Native Wayland (user must be able to write /dev/uinput — typically the `input` group)
sudo pacman -S --needed ydotool
```

Computer use sends screenshots to Grok as vision, so it needs **Grok OAuth or an xAI API key**.
The website-free fallback cannot see the screen.
