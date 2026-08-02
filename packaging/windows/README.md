# GrokHub on Windows

GrokHub is an Electron desktop app. On Windows it uses the same codebase as Linux:

- Unsandboxed **desktop host** (PowerShell by default)
- Grok OAuth / free website session / API key
- Start Menu pin + tray

## Requirements

- **Windows 10/11** x64
- **Node.js 20+** ([nodejs.org](https://nodejs.org/))
- **Electron** (installed with the app, or `npm i -D electron`)

## Quick install (from source)

In **PowerShell** from a clone of this repo:

```powershell
# Optional: build + install for current user
powershell -ExecutionPolicy Bypass -File packaging\windows\install.ps1 -Build
```

This copies the production UI + desktop shell to:

`%LOCALAPPDATA%\GrokHub`

and adds a **Start Menu → GrokHub** shortcut.

Launch:

```text
%LOCALAPPDATA%\GrokHub\grokhub.cmd
```

## Dev run

```powershell
npm install
npm run desktop:build
$env:GROKHUB_URL = "http://127.0.0.1:8080"
# terminal A
npm run dev
# terminal B
npm run desktop:win
```

Or after a production build:

```powershell
npm run desktop:win:prod
```

## Packaged installer (NSIS / portable)

On a Windows machine (or CI with Windows runners):

```powershell
npm install
npm run desktop:build
npm run dist:win
```

Artifacts land in `dist/`:

| File | Notes |
|------|--------|
| `GrokHub Setup *.exe` | NSIS installer (Start Menu, optional desktop icon) |
| `GrokHub *.exe` | Portable (no install) |

`electron-builder` config lives in `electron-builder.yml`.

## Taskbar pin

1. Launch **GrokHub** from the Start Menu (not a raw `electron.exe`).
2. Right-click the taskbar icon → **Pin to taskbar**.
3. AppUserModelID is `com.grokhub.app` so the pin survives restarts.

## Uninstall (user install)

```powershell
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\GrokHub"
Remove-Item -Force "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\GrokHub.lnk" -ErrorAction SilentlyContinue
# Optional: wipe app data
# Remove-Item -Recurse -Force "$env:APPDATA\GrokHub"
```

NSIS installs use **Settings → Apps**.

## Notes

- Host agent commands run in **PowerShell** on Windows.
- Wayland/Linux-only flags are skipped automatically.
- Secrets and chat memory stay under `%APPDATA%\GrokHub` (Electron `userData`).
