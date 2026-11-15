# GrokHub on Arch Linux

```bash
sudo pacman -S --needed git rustup base-devel pkgconf nodejs npm webkit2gtk-4.1 libayatana-appindicator
rustup default stable
git clone https://github.com/blackviperxiii-ui/GrokHub.git
cd GrokHub
./scripts/install.sh --user
grokhub-shell
```

`./scripts/install.sh --user` builds and installs the desktop app (`grokhub-shell`) and the LAN hub (`grokhub-hub`) into `~/.local/bin`. First launch is Get Started, which signs in with SuperGrok OAuth.

To update a clone install, pull and run `./scripts/install.sh --user` again. Only the AppImage updates itself from Settings → Updates. `~/.config/GrokHub` stays.

See [aur/README.md](./aur/README.md) for makepkg.
