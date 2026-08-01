# GrokHub

**v0.1** — Grok-native agent control plane. OpenClaw-style shell with **Grok modes** (Auto / Fast / Expert / Heavy / Build), **Imagine**, connectors, skills, automations, usage meter, and an **unsandboxed Arch desktop host** (CLI · files · apps).

**Repo:** https://github.com/blackviperxiii-ui/Grok-Hub

---

## Install on Arch Linux / CachyOS

```bash
sudo pacman -S --needed git electron nodejs npm curl base-devel

git clone https://github.com/blackviperxiii-ui/Grok-Hub.git
cd Grok-Hub

sudo ./scripts/install-arch.sh
grokhub
```

Or makepkg (prebuilt `.output`):

```bash
cd packaging/aur
cp PKGBUILD-bin PKGBUILD
makepkg -si
grokhub
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

Launcher: `/usr/bin/grokhub` → UI on `127.0.0.1:18765` → Electron shell with **Grok** mark.

---

## Dev

```bash
npm install
npm run dev            # UI :8080
npm run desktop:dev    # Electron → :8080
npm run desktop:build  # GROKHUB_DESKTOP=1 → .output/
```

---

## License

MIT
