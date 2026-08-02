# GrokHub on Windows

## Quick install (recommended)

### A) Installer (NSIS)

1. Download `GrokHub-*-win-x64.exe` from [Releases](https://github.com/blackviperxiii-ui/Grok-Hub/releases).
2. Run the setup wizard (per-user, Start Menu + Desktop shortcuts).
3. Launch **GrokHub** → **Settings** → connect access (API key / OAuth / website link).

### B) Portable

1. Download `GrokHub-*-portable.exe`.
2. Double-click — no install required. User data still lives under `%APPDATA%\GrokHub`.

### C) From source (developer)

```powershell
git clone https://github.com/blackviperxiii-ui/Grok-Hub.git
cd Grok-Hub
powershell -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1
```

Requires [Node.js LTS](https://nodejs.org/).

Uninstall source install:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\install-windows.ps1 -Uninstall
```

---

## Build Windows packages

From the repo (Windows **or** Linux with electron-builder):

```bash
npm install
npm run desktop:win
```

Artifacts land in `dist-desktop/`:

| File | Description |
|------|-------------|
| `GrokHub-<ver>-win-x64.exe` | NSIS setup |
| `GrokHub-<ver>-portable.exe` | Portable single exe |

Faster iteration:

```bash
npm run desktop:win:dir        # unpacked dir only
npm run desktop:win:portable   # portable only
```

---

## Paths

| Item | Location |
|------|----------|
| Install (NSIS default) | `%LOCALAPPDATA%\Programs\GrokHub` |
| Source install | `%LOCALAPPDATA%\GrokHub` |
| User data / secrets | `%APPDATA%\GrokHub` |
| UI runtime logs | `%LOCALAPPDATA%\GrokHub\runtime\ui.log` |

Taskbar pin identity uses `AppUserModelId = com.grokhub.app`.

---

## Host tools

Desktop host runs **unsandboxed** as your Windows user:

- Shell: PowerShell (`-NoProfile -ExecutionPolicy Bypass`)
- Files: full filesystem under your account
- Apps: Start Menu `.lnk` / `.exe` discovery + launch

---

## Troubleshooting

| Symptom | Fix |
|---------|-----|
| Blank window | Check `%LOCALAPPDATA%\GrokHub\runtime\ui.log`; reinstall / rebuild |
| Generic Electron icon | Pin the **GrokHub** Start Menu entry, not a raw electron process |
| SmartScreen warning | Unsigned build — choose “More info → Run anyway” or sign with your cert |
| `npm run desktop:dev` | Start `npm run dev` in another terminal first (UI on :8080) |
