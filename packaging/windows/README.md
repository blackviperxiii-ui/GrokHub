# GrokHub on Windows

## Requirements

- Windows 10/11 **x64**
- [Node.js 20+ LTS](https://nodejs.org/) (includes npm)
- PowerShell 5.1+ (built-in)

## Install (recommended)

From a clone of the repo in **PowerShell**:

```powershell
git clone https://github.com/blackviperxiii-ui/Grok-Hub.git
cd Grok-Hub

# Build UI + install Electron + copy to %LOCALAPPDATA%\GrokHub
powershell -ExecutionPolicy Bypass -File .\packaging\windows\install.ps1 -Build
```

What the installer does:

1. `npm install` (if needed)
2. Installs **Electron** binary into `node_modules/electron`
3. Runs `npm run desktop:build` → `.output/server`
4. Copies app to `%LOCALAPPDATA%\GrokHub`
5. Creates **Start Menu → GrokHub** (and a direct Electron shortcut)
6. Sets user env `GROKHUB_HOME` and adds install dir to user `PATH`
7. Smoke-launches the app

### Launch

| Method | Command / place |
|--------|------------------|
| Start Menu | **GrokHub** |
| CMD / Run | `%LOCALAPPDATA%\GrokHub\grokhub.cmd` |
| After PATH refresh | `grokhub.cmd` |

### If install fails

| Symptom | Fix |
|---------|-----|
| `Node.js is required` | Install Node LTS, **close and reopen** PowerShell |
| `electron binary missing` | `npm install electron@35.1.2 --save-dev` then re-run install |
| `UI build failed` | From repo root: `npm install` then `npm run desktop:build` and check errors |
| `ExecutionPolicy` | `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned` |
| Window opens but blank | Check `%LOCALAPPDATA%\GrokHub\runtime\ui.log` — Node must be on PATH |
| Start Menu does nothing | Use **GrokHub (direct)** shortcut, or run `grokhub.cmd` from Explorer |

Reinstall / repair:

```powershell
powershell -ExecutionPolicy Bypass -File .\packaging\windows\install.ps1 -Build
```

## How it runs

1. `grokhub.cmd` starts `electron.exe desktop\main.mjs`
2. `desktop/ui-server.cjs` starts Nitro from `.output\server\index.mjs` on port **18765** if needed
3. The window loads `http://127.0.0.1:18765`

You do **not** need a separate terminal for the UI server.

## Packaged NSIS / portable (optional)

```powershell
npm install
npm run desktop:build
npm install --save-dev electron@35.1.2 electron-builder
npm run dist:win
```

Output under `dist/`. Prefer `install.ps1` for source installs.

## Uninstall

```powershell
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\GrokHub"
Remove-Item -Force "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\GrokHub.lnk" -ErrorAction SilentlyContinue
Remove-Item -Force "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\GrokHub (direct).lnk" -ErrorAction SilentlyContinue
# Optional app data:
# Remove-Item -Recurse -Force "$env:APPDATA\GrokHub"
```

## Dev

```powershell
npm install
npm run desktop:build
# Terminal A — optional live UI
npm run dev
# Terminal B
$env:GROKHUB_URL = "http://127.0.0.1:8080"
npx electron desktop/main.mjs
```
