# GrokHub — Arch Linux packaging

Native Rust binaries. No Electron.

## From a clone

```bash
sudo pacman -S --needed git rustup base-devel pkgconf gtk3 libxkbcommon libxkbcommon-x11 cmake meson ninja wayland wayland-protocols pixman libpng libx11 libxtst libxinerama glib2 libxmu
rustup default stable
origin auth login
git clone https://github.com/blackviperxiii-ui/GrokHub.git
cd GrokHub
./scripts/install.sh --user
grokhub
```

`install.sh --user` installs the cabin GUI and the official Grok Build CLI (`grok` from https://x.ai/cli).

## makepkg (system)

```bash
cd packaging/aur
makepkg -si
```

`makepkg -si` / `yay -S grokhub` install the cabin GUI and run the Grok Build installer from `post_install`. Computer-use is Grok Build — no grim/ydotool sidecars.

## Layout

| Path | Role |
|------|------|
| `/usr/bin/grokhub` | Cabin |
| `/usr/bin/grokhub-hub` | Standalone LAN hub |
| `/usr/bin/grok` | Grok Build CLI (official xAI installer, `post_install`) |
| `/usr/lib/grokhub/install-grok-cli.sh` | Helper that runs `https://x.ai/cli/install.sh` |
| `/usr/share/applications/grokhub.desktop` | App menu |
| `~/.config/GrokHub` | Config + memory (`app.json`, `projects.json`, `secrets.json`) |
