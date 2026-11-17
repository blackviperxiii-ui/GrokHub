# GrokHub — Arch Linux packaging

Loadout is built with Tauri. The desktop app and the LAN hub, built from source. No Electron.

## From a clone

```bash
sudo pacman -S --needed git rustup base-devel pkgconf nodejs npm webkit2gtk-4.1 libayatana-appindicator
rustup default stable
git clone https://github.com/blackviperxiii-ui/GrokHub.git
cd GrokHub
./scripts/install.sh --user
grokhub-shell
```

`install.sh --user` installs the desktop app and hub. First launch is Get Started, which signs in with SuperGrok OAuth.

## makepkg (system)

`packaging/PKGBUILD` builds the clone it sits in:

```bash
cd packaging
makepkg -si
```

`packaging/aur/PKGBUILD` builds the release tag `v<pkgver>` from GitHub. Both run the same steps as `install.sh`: `cargo build -p grokhub-hub`, `npm ci --prefix ui`, then `tauri build --no-bundle`. `build()` needs the network for cargo and npm downloads.

For on-device models, install optional `llama-cpp` from extra (`pacman -S llama-cpp`) so `llama-server` is on PATH, or use the in-app engine download in Settings.

## Layout

| Path | Role |
|------|------|
| `/usr/bin/grokhub-shell` | Desktop app |
| `/usr/bin/grokhub-hub` | Standalone LAN hub |
| `/usr/bin/grokhub` | Renamed-binary shim: forwards `--mcp-*`, `--doctor` and the other headless flags to `grokhub-hub` |
| `/usr/share/applications/grokhub-shell.desktop` | App menu |
| `/usr/lib/systemd/user/grokhub-hub.service` | Hub user unit: `systemctl --user enable --now grokhub-hub` |
| `~/.config/GrokHub` | Config + memory (`app.json`, `projects.json`, `secrets.json`) |
