# GrokHub on Arch Linux

```bash
sudo pacman -S --needed git rustup base-devel pkgconf gtk3 libxkbcommon libxkbcommon-x11
rustup default stable
git clone https://github.com/blackviperxiii-ui/Grok-Hub.git
cd Grok-Hub
./scripts/install.sh --user
grokhub
```

Later updates: Settings → **Update**, `/update`, or `grokhub --update`. The clone must be on `main` with an `origin`. Overlay only — `~/.config/GrokHub` stays. Quit the tray and relaunch so the new binary runs.

See [aur/README.md](./aur/README.md) for makepkg.
