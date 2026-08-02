# GrokHub

**v0.5.2** — Grok-native agent control plane for **Arch Linux / CachyOS**.

Modes (Auto / Fast / Expert / Heavy / Build) · Imagine · connectors · skills · automations · usage meter · free-Grok fallback · setup sync · unsandboxed host (CLI · files · apps).

**Repository:** [github.com/blackviperxiii-ui/Grok-Hub](https://github.com/blackviperxiii-ui/Grok-Hub)

---

## Install (Arch Linux / CachyOS)

```bash
sudo pacman -S --needed git electron nodejs npm curl base-devel

git clone https://github.com/blackviperxiii-ui/Grok-Hub.git
cd Grok-Hub

sudo ./scripts/install-arch.sh
grokhub
```

**AUR-style local package:**

```bash
cd packaging/aur
cp PKGBUILD-bin PKGBUILD
makepkg -si
grokhub
```

| After install | Path |
|---------------|------|
| Launcher | `/usr/bin/grokhub` |
| App files | `/usr/lib/grokhub` |
| Desktop entry | `/usr/share/applications/grokhub.desktop` |
| User data | `~/.config/GrokHub` |

**Pin to taskbar:** open from the app menu (**GrokHub**), then pin that icon — not a generic Electron process.

More detail: [packaging/aur/README.md](packaging/aur/README.md)

---

## First-run setup

1. Open **Settings**.
2. Connect access (any one works):
   - **Link Grok website** — free grok.com account works (free-tier fallback)
   - **Connect with Grok OAuth** — SuperGrok / X Premium+
   - **xAI API key** — from [console.x.ai](https://console.x.ai)
3. Optional: **Install app menu entry**.
4. Optional: GitHub token + **Setup sync** across machines.

Secrets and chat history stay on the device; clean installs do not ship personal data.

---

## What you get

| Surface | Capability |
|---------|------------|
| **Agent** | Streaming chat · stop · mode router · host tools |
| **History** | Threads · rename / delete |
| **Imagine** | Image / video · aspect · quality · reference |
| **Connectors** | Website-linked status · tools where available |
| **Skills / Automations** | Heartbeat schedules · multi-time runs |
| **Desktop host** | Unsandboxed shell · files · apps |
| **Usage** | Plan meter · poll from website |
| **Updates** | Check / install from this GitHub repo |

---

## Development

```bash
npm install
npm run dev              # UI on 0.0.0.0:8080
npm run desktop:dev      # Electron → dev UI
npm run desktop:build    # production .output for packaging
npm run typecheck
```

| Script | Purpose |
|--------|---------|
| `npm run desktop` | Electron against `GROKHUB_URL` |
| `npm run desktop:arch` | Arch helper launcher |
| `npm run aur:release` | AUR release helper |

---

## Uninstall

```bash
sudo rm -f /usr/bin/grokhub /usr/share/applications/grokhub.desktop
sudo rm -rf /usr/lib/grokhub
# optional user data:
# rm -rf ~/.config/GrokHub ~/.local/share/applications/grokhub.desktop
```

---

## License

MIT
