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

`install.sh --user` installs the cabin GUI and the official Grok Build CLI **alpha** (`GROK_CHANNEL=alpha`, `grok` from https://x.ai/cli). First launch is Get Started — Super Grok OAuth also signs in `grok`.

## makepkg (system)

```bash
cd packaging/aur
makepkg -si
```

`makepkg -si` / `yay -S grokhub` install the cabin GUI and run the Grok Build **alpha** installer from `post_install`. Computer-use is Grok Build — no grim/ydotool sidecars.

## Layout

| Path | Role |
|------|------|
| `/usr/bin/grokhub` | Cabin |
| `/usr/bin/grokhub-hub` | Standalone LAN hub |
| `/usr/bin/grok` | Grok Build CLI alpha (official xAI installer, `GROK_CHANNEL=alpha` `post_install`) |
| `/usr/lib/grokhub/install-grok-cli.sh` | Helper that runs `GROK_CHANNEL=alpha` `https://x.ai/cli/install.sh` |
| `/usr/share/applications/grokhub.desktop` | App menu |
| `~/.config/GrokHub` | Config + memory (`app.json`, `projects.json`, `secrets.json`) |
