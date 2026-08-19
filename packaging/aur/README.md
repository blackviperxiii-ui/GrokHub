# GrokHub — Arch Linux packaging

Native Rust binaries. No Electron.

## From a clone

```bash
sudo pacman -S --needed git rustup base-devel pkgconf gtk3 libxkbcommon libxkbcommon-x11 cmake meson ninja wayland wayland-protocols pixman libpng libx11 libxtst libxinerama glib2 libxmu
rustup default stable
origin auth login
git clone https://origin.cursor.com/git/viperxiii/GrokHub.git
cd GrokHub
./scripts/install.sh --user
grokhub
```

`install.sh --user` installs the cabin GUI. Install Grok Build from https://x.ai/cli for the agent.

## makepkg (system)

```bash
cd packaging/aur
makepkg -si
```

`makepkg -si` / `yay -S grokhub` install the cabin GUI. Computer-use is Grok Build — no grim/ydotool sidecars.

## Layout

| Path | Role |
|------|------|
| `/usr/bin/grokhub` | Cabin |
| `/usr/bin/grokhub-hub` | Standalone LAN hub |
| `/usr/share/applications/grokhub.desktop` | App menu |
| `~/.config/GrokHub` | Config + memory (`app.json`, `projects.json`, `secrets.json`) |
