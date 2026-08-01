# GrokHub Arch packages (v0.1)

**GitHub:** https://github.com/blackviperxiii-ui/Grok-Hub

## Install

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
cp PKGBUILD-bin PKGBUILD   # prebuilt
makepkg -si
```

Binary: `grokhub` · Icon: Grok mark · Version: **0.1**
