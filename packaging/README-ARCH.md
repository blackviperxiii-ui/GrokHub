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
(off by default). It uses a silent picture loop (`grim` / `maim` / `scrot`) plus
mouse/keyboard injectors — Playwright is **not** used for this (it remains a test-only
dependency). Electron `desktopCapturer` is a last-resort fallback and may open a
screenshot picker on Wayland.

```bash
# Silent capture (avoids the portal / screenshot app)
sudo pacman -S --needed grim      # Wayland
# sudo pacman -S --needed maim    # X11 alternative

# X11 or XWayland clicks
sudo pacman -S --needed xdotool

# Native Wayland clicks (user must be able to write /dev/uinput — typically the `input` group)
sudo pacman -S --needed ydotool
```

Computer use sends screenshots to Grok as vision, so it needs **Grok OAuth or an xAI API key**.
The website-free fallback cannot see the screen.
