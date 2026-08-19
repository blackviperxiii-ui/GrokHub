# GrokHub on Arch Linux

```bash
sudo pacman -S --needed git rustup base-devel pkgconf gtk3 libxkbcommon libxkbcommon-x11 cmake meson ninja wayland wayland-protocols pixman libpng libx11 libxtst libxinerama glib2 libxmu
rustup default stable
origin auth login
git clone https://github.com/blackviperxiii-ui/GrokHub.git
cd GrokHub
./scripts/install.sh --user
grokhub
```

`./scripts/install.sh --user` installs the cabin, hub, and official Grok Build CLI (`grok` from https://x.ai/cli). Computer-use is Grok Build — the overlay does not build grim/ydotool sidecars.

Later updates: Settings → **Update**, `/update`, or `grokhub --update`. The clone must be on `main` with an `origin`. Overlay only — `~/.config/GrokHub` stays. Progress stays on Settings. After a clean overlay, **Restart** reloads the new binary.

See [aur/README.md](./aur/README.md) for makepkg.
