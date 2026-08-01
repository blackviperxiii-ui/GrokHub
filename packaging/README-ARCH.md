# GrokClaw on Arch Linux

## Real AUR-style package (recommended)

Full package lives in **`packaging/aur/`**:

| Package | File | Notes |
|---------|------|--------|
| **grokclaw-bin** | `PKGBUILD-bin` + `grokclaw-bin-0.1.0.tar.gz` | **Prebuilt** — install in seconds |
| **grokclaw** | `PKGBUILD` + `grokclaw-0.1.0.tar.gz` | Build from source with npm |

### Install prebuilt now

```bash
sudo pacman -S --needed base-devel electron nodejs curl
cd packaging/aur
cp PKGBUILD-bin PKGBUILD
makepkg -si
grokclaw
```

See [aur/README.md](./aur/README.md) for AUR publish steps (`yay -S grokclaw-bin`).

---

## Dev run (without packaging)

```bash
sudo pacman -S --needed electron nodejs npm
npm install
npm run dev          # terminal 1
npm run desktop:dev  # terminal 2
# or:
npm run desktop:arch
```

## Security

Unsandboxed host agent: shell, files, and apps run as your Linux user.
