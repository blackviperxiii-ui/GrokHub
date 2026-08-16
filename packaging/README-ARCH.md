# GrokHub on Arch Linux

```bash
sudo pacman -S --needed git rustup base-devel pkgconf gtk3 libxkbcommon libxkbcommon-x11 ydotool xdotool grim wmctrl python-atspi
rustup default stable
git clone https://github.com/blackviperxiii-ui/Grok-Hub.git
cd Grok-Hub
./scripts/install.sh --user
grokhub
```

`./scripts/install.sh --user` is one sudo: it installs the cabin and the hands packages (`ydotool` `xdotool` `grim` `wmctrl` `python-atspi`), enables the user `ydotoold` unit, and offers the `input` group (log out once). `makepkg -si` / `yay -S grokhub` pulls the same packages as hard depends.

Later updates: Settings → **Update**, `/update`, or `grokhub --update`. The clone must be on `main` with an `origin`. Overlay only — `~/.config/GrokHub` stays. Progress stays on Settings. After a clean overlay, **Restart** reloads the new binary.

See [aur/README.md](./aur/README.md) for makepkg.
