# GrokClaw Arch packages

**GitHub:** https://github.com/blackviperxiii-ui/spring-dove-reef-apple

## Install from GitHub (you already pushed)

```bash
sudo pacman -S --needed git electron nodejs npm curl base-devel

git clone https://github.com/blackviperxiii-ui/spring-dove-reef-apple.git
cd spring-dove-reef-apple
sudo ./scripts/install-arch.sh
grokclaw
```

Or with makepkg (prebuilt `.output` in the tree):

```bash
cd spring-dove-reef-apple/packaging/aur
cp PKGBUILD-bin PKGBUILD
makepkg -si
```

## Packages

| File | Name | Notes |
|------|------|--------|
| `PKGBUILD-bin` | `grokclaw-bin` | Uses git clone + committed `.output` (fast) |
| `PKGBUILD` | `grokclaw` | Builds with `GROKCLAW_DESKTOP=1 npm run build` |

Both depend on: `electron` `nodejs` `curl`

## Private repo note

This repo is **private**. That is fine for *your* machines (`git clone` with auth).  
**AUR** requires a publicly downloadable source — either:

1. Make the repo public, or  
2. Mirror / rename to a public `grokclaw` repo and point `source=` there.

## Publish to AUR (after public)

```bash
# In a clean dir
git clone ssh://aur@aur.archlinux.org/grokclaw-bin.git
cp PKGBUILD-bin PKGBUILD
# edit if needed
makepkg --printsrcinfo > .SRCINFO
git add PKGBUILD .SRCINFO grokclaw.install
git commit -m "grokclaw-bin 0.1.0"
git push
```

Then: `yay -S grokclaw-bin`
