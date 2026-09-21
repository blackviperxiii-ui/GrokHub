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

Later updates: Settings → **Update**, `/update`, or `grokhub --update`. Only what is newer runs. CLI first (`grok update --alpha`) when a newer alpha exists, then the cabin when GitHub Latest is newer. The clone must be on `main` with an `origin` for the cabin half. `~/.config/GrokHub` stays. If `grok` is on stable, the overlay switches it to alpha. A current alpha is left alone. Progress stays on Settings. After a clean overlay, **Restart** reloads the new binary.

See [aur/README.md](./aur/README.md) for makepkg.
