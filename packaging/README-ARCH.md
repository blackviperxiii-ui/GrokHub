# GrokHub on Arch Linux

```bash
sudo pacman -S --needed git rustup base-devel pkgconf gtk3 libxkbcommon libxkbcommon-x11 cmake meson ninja wayland wayland-protocols pixman libpng libx11 libxtst libxinerama glib2 libxmu
rustup default stable
origin auth login
git clone https://origin.cursor.com/blackviperxiii-ui/grok-hub.git
cd grok-hub
./scripts/install.sh --user
grokhub
```

`./scripts/install.sh --user` builds `ydotool`, `grim`, `xdotool`, and `wmctrl` into `~/.local/lib/grokhub/bin`, installs `python-atspi` for the Eyes windshield, writes a user `ydotoold` unit that starts that sidecar, installs the uinput udev rule, and offers the `input` group (log out once). `makepkg -si` / `yay -S grokhub` do the same into `/usr/lib/grokhub/bin` and depend on `python-atspi`. Runtime `ydotool` / `grim` / `xdotool` / `wmctrl` stay optdepends for when a sidecar build is skipped.

Later updates: Settings → **Update**, `/update`, or `grokhub --update`. The clone must be on `main` with a clean worktree. A leftover GitHub `origin` for this repo is rewritten to Origin. Origin git may need `origin auth login`. Overlay only — `~/.config/GrokHub` stays. Progress stays on Settings. After a clean overlay, **Restart** reloads the new binary.

See [aur/README.md](./aur/README.md) for makepkg.
