# GrokHub — Arch Linux packaging

Native Rust binaries. No Electron.

## From a clone

```bash
sudo pacman -S --needed git rustup base-devel pkgconf gtk3 libxkbcommon libxkbcommon-x11 cmake meson ninja wayland wayland-protocols pixman libpng libx11 libxtst libxinerama glib2 libxmu
rustup default stable
git clone https://github.com/blackviperxiii-ui/Grok-Hub.git
cd Grok-Hub
./scripts/install.sh --user
grokhub
```

`install.sh --user` builds ydotool, grim, xdotool, and wmctrl next to the cabin and installs `python-atspi` for Eyes / `act` / `wait_for`. One password for build deps, AT-SPI, the uinput udev rule, and the `input` group.

## makepkg (system)

```bash
cd packaging/aur
makepkg -si
```

`makepkg -si` / `yay -S grokhub` compile ydotool, grim, xdotool, and wmctrl into `/usr/lib/grokhub/bin` and depend on `python-atspi`. Pointer sidecars stay optdepends if the build is skipped. First cabin launch or Eyes → Install hands starts `ydotoold`.

## Layout

| Path | Role |
|------|------|
| `/usr/bin/grokhub` | Cabin |
| `/usr/lib/grokhub/bin` | Sidecar ydotool / ydotoold / grim / xdotool / wmctrl |
| `/usr/bin/grokhub-hub` | Standalone LAN hub |
| `/usr/share/applications/grokhub.desktop` | App menu |
| `~/.config/GrokHub` | Config + memory (`app.json`, `projects.json`, `secrets.json`) |
