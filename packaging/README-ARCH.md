# GrokHub on Arch Linux

```bash
sudo pacman -S --needed git rustup base-devel pkgconf gtk3 libxkbcommon libxkbcommon-x11
rustup default stable
git clone https://github.com/blackviperxiii-ui/Grok-Hub.git
cd Grok-Hub
./scripts/install.sh --user
grokhub
```

Later updates: Settings → **Update**, `/update`, or `grokhub --update`. The clone must be on `main` with an `origin`. Overlay only — `~/.config/GrokHub` stays. Progress stays on Settings. After a clean overlay, **Restart** reloads the new binary.

Hands (mouse/keyboard takeover) need `ydotool` on Wayland or `xdotool` on X11, plus `grim`, `wmctrl`, and `python-atspi`. `./scripts/install.sh --user` tries `pacman -S --needed` for those and installs a user `ydotoold` unit. If `/dev/uinput` is not writable, add your user to the `input` group, load `uinput`, and log out. Eyes → **Install hands** starts the daemon without sudo.

See [aur/README.md](./aur/README.md) for makepkg.
