# GrokHub

**v0.8.10** — Grok-native agent control plane for **Windows** and **Arch Linux / CachyOS**.

Modes (Auto / Fast / Expert / Heavy / Build) · Imagine · connectors · skills · automations · usage meter · free-Grok fallback · setup sync · unsandboxed host (CLI · files · apps).

**Repository:** [github.com/blackviperxiii-ui/Grok-Hub](https://github.com/blackviperxiii-ui/Grok-Hub)

---

## Install — Windows

### Installer (recommended)

1. Grab the latest **`GrokHub-*-win-x64.exe`** from [Releases](https://github.com/blackviperxiii-ui/Grok-Hub/releases).
2. Run the setup wizard (Start Menu + Desktop shortcuts).
3. Open **GrokHub** → **Settings** → connect access.

### Portable

Download **`GrokHub-*-portable.exe`** and run it — no install step.  
User data still lives under `%APPDATA%\GrokHub`.

### From source

```powershell
# Requires Node.js LTS: https://nodejs.org/
git clone https://github.com/blackviperxiii-ui/Grok-Hub.git
cd Grok-Hub
powershell -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1
```

| After install | Path |
|---------------|------|
| Launcher | `%LOCALAPPDATA%\GrokHub\grokhub.cmd` |
| CLI | `grokhub` (user PATH) |
| Start Menu | **GrokHub** |
| User data | `%APPDATA%\GrokHub` |

Uninstall source install:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1 -Uninstall
```

More detail: [packaging/windows/README.md](packaging/windows/README.md)

---

## Install — Arch Linux / CachyOS

```bash
sudo pacman -S --needed git electron nodejs npm curl base-devel

git clone https://github.com/blackviperxiii-ui/Grok-Hub.git
cd Grok-Hub

sudo ./scripts/install-arch.sh
grokhub
```

**User install (no root, recommended if dual-install issues):**

```bash
./scripts/install-arch.sh --user
# ensure ~/.local/bin is on PATH
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
3. Optional: **Install app menu entry** (Start Menu on Windows / `.desktop` on Linux).
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
| **Desktop host** | Unsandboxed shell · files · apps (PowerShell on Windows, bash on Linux) |
| **Usage** | Plan meter · poll from website |
| **Updates** | Check / install / rollback / self-test from this GitHub repo |
| **Host safe mode** | Optional block of dangerous shell patterns |

---

## Repair (broken / blank window)

Preserves chats & secrets; rebuilds UI and reinstalls the shell:

```bash
git pull
./scripts/repair-install.sh
grokhub
```

Release tarball **with** `.output` (for GitHub Releases / updater):

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

### Package Windows builds

```bash
npm run desktop:win              # NSIS installer + portable
npm run desktop:win:portable     # portable only
npm run desktop:win:dir          # unpacked dir (fast iterate)
```

Artifacts → `dist-desktop/`.

| Script | Purpose |
|--------|---------|
| `npm run desktop` | Electron against `GROKHUB_URL` |
| `npm run desktop:arch` | Arch helper launcher |
| `npm run desktop:win` | Build Windows NSIS + portable |
| `npm run aur:release` | AUR release helper |

---

## Uninstall

**Windows (NSIS):** Settings → Apps → GrokHub → Uninstall  

**Windows (source):**

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1 -Uninstall
```

**Linux:**

```bash
sudo rm -f /usr/bin/grokhub /usr/share/applications/grokhub.desktop
sudo rm -rf /usr/lib/grokhub
# optional user data:
# rm -rf ~/.config/GrokHub ~/.local/share/applications/grokhub.desktop
```

---

## License

MIT
