# GrokHub

**v0.4.1** — Grok-native agent desktop (OpenClaw-style) for **Arch Linux** and **Windows**.

Modes (Auto / Fast / Expert / Heavy / Build) · Imagine · connectors · skills · automations · usage meter · free-Grok fallback · setup sync · unsandboxed host (CLI · files · apps).

**Repository:** [github.com/blackviperxiii-ui/Grok-Hub](https://github.com/blackviperxiii-ui/Grok-Hub)

---

## Install

### Arch Linux / CachyOS (recommended)

```bash
sudo pacman -S --needed git electron nodejs npm curl base-devel

git clone https://github.com/blackviperxiii-ui/Grok-Hub.git
cd Grok-Hub

# Build UI if needed, install to /usr
sudo ./scripts/install-arch.sh

grokhub
```

**AUR-style local package** (uses prebuilt `.output` when present):

```bash
cd packaging/aur
cp PKGBUILD-bin PKGBUILD   # or use PKGBUILD for from-source
makepkg -si
grokhub
```

| After install | Path |
|---------------|------|
| Launcher | `/usr/bin/grokhub` |
| App files | `/usr/lib/grokhub` |
| Desktop entry | `/usr/share/applications/grokhub.desktop` |
| User data | `~/.config/GrokHub` (Electron userData) |

**Pin to taskbar:** open from the app menu (**GrokHub**), then pin that icon — not a generic Electron process. After updates, unpin/re-pin once if the shell still shows a second icon.

More detail: [packaging/aur/README.md](packaging/aur/README.md)

---

### Windows 10 / 11

**Requirements:** [Node.js 20+](https://nodejs.org/) (LTS), PowerShell 5+

#### Option A — Install from source (current user)

```powershell
git clone https://github.com/blackviperxiii-ui/Grok-Hub.git
cd Grok-Hub

powershell -ExecutionPolicy Bypass -File packaging\windows\install.ps1 -Build
```

This installs to `%LOCALAPPDATA%\GrokHub`, adds a **Start Menu → GrokHub** shortcut, and puts the folder on your user `PATH`.

Launch:

```text
%LOCALAPPDATA%\GrokHub\grokhub.cmd
```

or **Start Menu → GrokHub**.

#### Option B — Packaged installer (NSIS / portable)

On a Windows machine:

```powershell
git clone https://github.com/blackviperxiii-ui/Grok-Hub.git
cd Grok-Hub
npm install
npm run desktop:build
npm install --save-dev electron electron-builder
npm run dist:win
```

Artifacts in `dist/`:

| Artifact | Use |
|----------|-----|
| `GrokHub-*-Setup.exe` | NSIS installer (Start Menu + optional desktop shortcut) |
| `GrokHub-*-portable.exe` | Portable, no install |

**Pin to taskbar:** launch from Start Menu, then pin. AppUserModelID is `com.grokhub.app`.

Full Windows notes: [packaging/windows/README.md](packaging/windows/README.md)

---

## First-run setup (all platforms)

1. Open **Settings**.
2. Connect access (any one works):
   - **Link Grok website** — free grok.com account works (free-tier fallback)
   - **Connect with Grok OAuth** — SuperGrok / X Premium+
   - **xAI API key** — from [console.x.ai](https://console.x.ai) (trial credits count as free-tier API)
3. Optional: **Free Grok fallback** card — free plan limits + website fallback when premium models are denied.
4. Optional: **Install app menu entry** / Start Menu shortcut.
5. Optional: GitHub token + **Setup sync** to push/pull skills & automations across machines.

Secrets and chat history stay on the device (`userData`); clean installs do not ship personal data.

---

## What you get

| Surface | Capability |
|---------|------------|
| **Agent** | Streaming chat · stop · mode router · host tools |
| **History** | Threads · rename / delete |
| **Imagine** | Image / video · aspect · quality · reference |
| **Connectors** | Website-linked status · tools where available |
| **Skills / Automations** | Heartbeat schedules · multi-time runs |
| **Desktop host** | Unsandboxed shell (bash / PowerShell) · files · apps |
| **Usage** | Plan meter (Free / SuperGrok / Pro) · poll from website |
| **Updates** | Check / install from this GitHub repo · factory reinstall |
| **Setup sync** | OAuth-keyed pack · optional encrypted Gist |

---

## Development

```bash
npm install
npm run dev              # UI on 0.0.0.0:8080
npm run desktop:dev      # Electron → dev UI (Linux/mac env)
npm run desktop:build    # production .output for desktop packaging
npm run typecheck
```

| Script | Purpose |
|--------|---------|
| `npm run desktop` | Electron against current env `GROKHUB_URL` |
| `npm run desktop:arch` | Arch helper launcher |
| `npm run desktop:win` | Electron on Windows |
| `npm run dist:win` | NSIS + portable (needs electron-builder) |
| `npm run aur:release` | AUR release helper |

---

## Uninstall

**Arch**

```bash
sudo rm -f /usr/bin/grokhub /usr/share/applications/grokhub.desktop
sudo rm -rf /usr/lib/grokhub
# optional user data:
# rm -rf ~/.config/GrokHub ~/.local/share/applications/grokhub.desktop
```

**Windows (user install)**

```powershell
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\GrokHub"
Remove-Item -Force "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\GrokHub.lnk" -ErrorAction SilentlyContinue
# optional user data:
# Remove-Item -Recurse -Force "$env:APPDATA\GrokHub"
```

NSIS installs: **Settings → Apps → GrokHub**.

---

## License

MIT
