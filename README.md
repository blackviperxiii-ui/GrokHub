# GrokHub

**v0.8.13** — Grok-native agent control plane for **Arch Linux / CachyOS**.

Adaptive modes · Imagine · connectors · skills · automations · usage meter · free-Grok fallback · setup sync · unsandboxed desktop host (CLI · files · apps).

**Repository:** [github.com/blackviperxiii-ui/Grok-Hub](https://github.com/blackviperxiii-ui/Grok-Hub)

> Windows builds live in a **separate repository**. This repo is Linux-focused (Arch / CachyOS packaging, AUR, and in-app updates).

---

## Install — Arch Linux / CachyOS

### From source (recommended)

```bash
sudo pacman -S --needed git electron nodejs npm curl base-devel

git clone https://github.com/blackviperxiii-ui/Grok-Hub.git
cd Grok-Hub

sudo ./scripts/install-arch.sh
grokhub
```

### User install (no root)

Prefer this if you already have a system install under `/usr` and want updates without `pkexec`:

```bash
./scripts/install-arch.sh --user
# ensure ~/.local/bin is on PATH
grokhub
```

| After user install | Path |
|--------------------|------|
| Launcher | `~/.local/bin/grokhub` |
| App files | `~/.local/lib/grokhub` |
| User data | `~/.config/GrokHub` |

### AUR-style local package

```bash
cd packaging/aur
cp PKGBUILD-bin PKGBUILD
makepkg -si
grokhub
```

| After system install | Path |
|----------------------|------|
| Launcher | `/usr/bin/grokhub` |
| App files | `/usr/lib/grokhub` |
| Desktop entry | `/usr/share/applications/grokhub.desktop` |
| User data | `~/.config/GrokHub` |

**Pin to taskbar:** open **GrokHub** from the app menu, then pin that icon — not a generic Electron process.

More detail: [packaging/aur/README.md](packaging/aur/README.md)

### Release tarball (updater / offline)

From [Releases](https://github.com/blackviperxiii-ui/Grok-Hub/releases), download **`grokhub-desktop-v*.tar.gz`** (includes `.output`). The in-app updater uses the same asset.

---

## First-run setup

1. Open **Settings**.
2. Connect access (any one works):
   - **Link Grok website** — free grok.com account works (free-tier fallback)
   - **Connect with Grok OAuth** — SuperGrok / X Premium+
   - **xAI API key** — from [console.x.ai](https://console.x.ai)
3. Optional: **Install app menu entry** (`.desktop` launcher).
4. Optional: GitHub token + **Setup sync** across machines.

Secrets and chat history stay on the device; clean installs do not ship personal data.

---

## What you get

| Surface | Capability |
|---------|------------|
| **Agent** | Streaming chat · stop · Adaptive router · host tools · reply/copy/delete · attach · voice |
| **History** | Search · pin · folders · date groups · rename / delete |
| **Imagine** | Image / video · aspect · quality · reference · lightbox · multi-delete |
| **Connectors** | Website-linked status · tools where available |
| **Skills / Automations** | Heartbeat schedules · multi-time runs |
| **Desktop host** | Unsandboxed shell · files · apps (bash) |
| **Usage** | Plan meter · poll from grok.com every minute |
| **Updates** | Check / install / rollback / self-test from this GitHub repo |
| **Command palette** | `Ctrl+K` — jump to views, modes, recent chats |
| **Appearance** | Dark / Light / System theme |

---

## Repair (broken / blank window)

Preserves chats & secrets; rebuilds UI and reinstalls the shell:

```bash
git pull
./scripts/repair-install.sh
grokhub
```

Build a release tarball **with** `.output` (for GitHub Releases / updater):

```bash
npm run release:bundle
# upload dist-release/grokhub-desktop-v*.tar.gz as a release asset
```

---

## Development

```bash
npm install
npm run dev              # UI on 0.0.0.0:8080
npm run desktop:dev      # Electron → dev UI (run `npm run dev` first)
npm run desktop:build    # production .output for packaging
npm run typecheck
```

| Script | Purpose |
|--------|---------|
| `npm run desktop` | Electron against `GROKHUB_URL` |
| `npm run desktop:arch` | Arch helper launcher |
| `npm run aur:release` | AUR release helper |
| `npm run release:bundle` | Desktop tarball for Releases |

---

## Uninstall

**System install:**

```bash
sudo rm -f /usr/bin/grokhub /usr/share/applications/grokhub.desktop
sudo rm -rf /usr/lib/grokhub
# optional user data:
# rm -rf ~/.config/GrokHub
```

**User install:**

```bash
rm -f ~/.local/bin/grokhub ~/.local/share/applications/grokhub.desktop
rm -rf ~/.local/lib/grokhub
# optional: rm -rf ~/.config/GrokHub
```

---

## License

MIT
