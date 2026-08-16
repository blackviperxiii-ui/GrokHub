# GrokHub on Arch Linux

```bash
sudo pacman -S --needed git rustup base-devel pkgconf gtk3 libxkbcommon libxkbcommon-x11 cmake meson ninja wayland wayland-protocols pixman libpng libx11 libxtst libxinerama glib2 libxmu
rustup default stable
git clone https://github.com/blackviperxiii-ui/Grok-Hub.git
cd Grok-Hub
./scripts/install.sh --user
grokhub
```

`./scripts/install.sh --user` builds `ydotool`, `grim`, `xdotool`, and `wmctrl` into `~/.local/lib/grokhub/bin`, writes a user `ydotoold` unit that starts that sidecar, installs the uinput udev rule, and offers the `input` group (log out once). `makepkg -si` / `yay -S grokhub` do the same into `/usr/lib/grokhub/bin`. Runtime `ydotool` / `grim` / `xdotool` / `wmctrl` / `python-atspi` are optdepends, not hard depends.

Later updates: Settings → **Update**, `/update`, or `grokhub --update`. The clone must be on `main` with an `origin`. Overlay only — `~/.config/GrokHub` stays. Progress stays on Settings. After a clean overlay, **Restart** reloads the new binary.

See [aur/README.md](./aur/README.md) for makepkg.
