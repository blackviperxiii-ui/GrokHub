# GrokHub — Arch Linux packaging

Native Rust binaries. No Electron.

## From a clone

```bash
sudo pacman -S --needed git rustup base-devel pkgconf gtk3 libxkbcommon libxkbcommon-x11 ydotool xdotool grim wmctrl python-atspi
rustup default stable
git clone https://github.com/blackviperxiii-ui/Grok-Hub.git
cd Grok-Hub
./scripts/install.sh --user
grokhub
```

`install.sh --user` sudo-installs the hands packages with the cabin. One password.

## makepkg (system)

```bash
cd packaging/aur
makepkg -si
```

`makepkg -si` / `yay -S grokhub` installs `ydotool` `xdotool` `grim` `wmctrl` `python-atspi` as depends. First cabin launch or Eyes → Install hands starts `ydotoold`.

## Layout

| Path | Role |
|------|------|
| `/usr/bin/grokhub` | Cabin |
| `/usr/bin/grokhub-hub` | Standalone LAN hub |
| `/usr/share/applications/grokhub.desktop` | App menu |
| `~/.config/GrokHub` | Config + memory (`app.json`, `projects.json`, `secrets.json`) |
