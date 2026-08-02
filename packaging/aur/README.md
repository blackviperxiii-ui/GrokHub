# GrokHub — Arch Linux packaging

## System install (from clone)

```bash
sudo pacman -S --needed git electron nodejs npm curl base-devel
git clone https://github.com/blackviperxiii-ui/Grok-Hub.git
cd Grok-Hub
sudo ./scripts/install-arch.sh
grokhub
```

## makepkg

```bash
cd packaging/aur
cp PKGBUILD-bin PKGBUILD
makepkg -si
```

## Layout

| Path | Role |
|------|------|
| `/usr/bin/grokhub` | Launcher (`packaging/aur/grokhub.sh`) |
| `/usr/lib/grokhub` | `.output` + `desktop/` |
| `/usr/share/applications/grokhub.desktop` | Menu entry (`StartupWMClass=grokhub`) |
