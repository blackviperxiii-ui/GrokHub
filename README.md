# GrokClaw

Grok-native agent control plane — OpenClaw-style shell with **Grok modes** (Auto / Fast / Expert / Heavy / Build), **Imagine**, connectors, skills, automations, usage meter, and an **unsandboxed Arch desktop host** (CLI · files · apps).

**Repo:** https://github.com/blackviperxiii-ui/spring-dove-reef-apple

---

## Install on Arch Linux / CachyOS

### Option A — one-shot from GitHub (recommended)

```bash
sudo pacman -S --needed git electron nodejs npm curl base-devel

git clone https://github.com/blackviperxiii-ui/spring-dove-reef-apple.git
cd spring-dove-reef-apple

# If the clone is private, make sure you're logged in:
#   gh auth login
#   # or use a deploy key / SSH remote

sudo ./scripts/install-arch.sh
grokclaw
```

### Option B — makepkg (prebuilt, uses committed `.output`)

```bash
sudo pacman -S --needed base-devel electron nodejs curl git
git clone https://github.com/blackviperxiii-ui/spring-dove-reef-apple.git
cd spring-dove-reef-apple/packaging/aur
cp PKGBUILD-bin PKGBUILD
makepkg -si
grokclaw
```

### Option C — makepkg from source (rebuild UI)

```bash
cd spring-dove-reef-apple/packaging/aur
# use PKGBUILD (not PKGBUILD-bin)
makepkg -si
```

---

## What you get

| Surface | Capability |
|---------|------------|
| Agent | Mode-aware chat · `$ shell` host commands |
| Command | Dashboard · activity · usage meter |
| Desktop | Unsandboxed CLI / files / apps |
| Imagine | Local image studio |
| Modes | Auto · Fast · Expert · Heavy · Build (Grok 4.5) |
| Usage | SuperGrok Pro-style unit budget |

Launcher: `/usr/bin/grokclaw` → starts UI on `127.0.0.1:18765` → Electron shell.

---

## Dev

```bash
npm install
npm run dev            # UI :8080
npm run desktop:dev    # Electron → :8080
# or production-shaped desktop build:
npm run desktop:build  # GROKCLAW_DESKTOP=1 → .output/
```

---

## AUR publish notes

Repo is currently **private**. AUR helpers need a **public** source URL.

1. Settings → make the repo public (or mirror to a public `grokclaw` repo)
2. Optionally rename to `grokclaw`
3. Tag `v0.1.0` and push packaging/aur `PKGBUILD` + `.SRCINFO` to `aur.archlinux.org`
4. Then: `yay -S grokclaw-bin`

Details: [`packaging/aur/README.md`](packaging/aur/README.md)

---

## License

MIT
