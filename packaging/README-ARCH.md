# GrokHub on Arch Linux

Prebuilt AppImage (self-updates from Settings → Updates; no `dpkg`):

```bash
curl -fsSL https://raw.githubusercontent.com/blackviperxiii-ui/GrokHub/main/scripts/install.sh | bash
grokhub-shell
```

That installs the signed AppImage into `~/.local/bin`. First launch is Get Started, which signs in with SuperGrok OAuth. CachyOS and other Arch hosts without Debian tooling use this path; the `.deb` updater does not apply here.

From a clone (rebuilds; does not self-update in the app):

```bash
sudo pacman -S --needed git rustup base-devel pkgconf nodejs npm webkit2gtk-4.1 libayatana-appindicator
rustup default stable
git clone https://github.com/blackviperxiii-ui/GrokHub.git
cd GrokHub
./scripts/install.sh --user --from-source
grokhub-shell
```

To update a clone install, pull and run `./scripts/install.sh --user --from-source` again. `~/.config/GrokHub` stays.

See [aur/README.md](./aur/README.md) for makepkg (package manager updates; not the in-app updater).
