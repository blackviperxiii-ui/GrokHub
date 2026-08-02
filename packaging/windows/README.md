# GrokHub on Windows

## Requirements

- Windows 10/11 **x64**
- [Node.js 20+ LTS](https://nodejs.org/) (includes npm) — install, then open a **new** PowerShell
- PowerShell 5.1+ (built-in)

**Work from your user folder** (`Downloads`, `Documents`, etc.) — not `C:\Windows\System32`.

## Install without Git (recommended)

If `git clone` fails with `ca-bundle.crt` / trust anchors errors, skip Git entirely:

```powershell
cd $env:USERPROFILE\Downloads
irm https://raw.githubusercontent.com/blackviperxiii-ui/Grok-Hub/main/packaging/windows/download-and-install.ps1 -OutFile download-and-install.ps1
powershell -ExecutionPolicy Bypass -File .\download-and-install.ps1
```

Uses Windows TLS certificates (not Git’s mingw CA file).

## Install with Git

```powershell
cd $env:USERPROFILE\Downloads
git clone https://github.com/blackviperxiii-ui/Grok-Hub.git
cd Grok-Hub
powershell -ExecutionPolicy Bypass -File .\packaging\windows\install.ps1 -Build
```

### Fix: `error adding trust anchors from file: .../ca-bundle.crt`

Git for Windows is using a missing/corrupt CA bundle. Pick one:

1. **Use the ZIP installer** (above) — no Git needed  
2. Point Git at Windows Schannel:
   ```powershell
   git config --global http.sslBackend schannel
   git clone https://github.com/blackviperxiii-ui/Grok-Hub.git
   ```
3. Reinstall [Git for Windows](https://git-scm.com/download/win)  
4. Manual ZIP from the browser:  
   https://github.com/blackviperxiii-ui/Grok-Hub/archive/refs/heads/main.zip  
   Extract, then:
   ```powershell
   cd $env:USERPROFILE\Downloads\Grok-Hub-main
   powershell -ExecutionPolicy Bypass -File .\packaging\windows\install.ps1 -Build
   ```

## Launch

| Method | How |
|--------|-----|
| Start Menu | **GrokHub** or **GrokHub (direct)** |
| Run dialog | `%LOCALAPPDATA%\GrokHub\grokhub.cmd` |

## If install fails

| Symptom | Fix |
|---------|-----|
| `Node.js is required` | Install Node LTS, **close and reopen** PowerShell |
| `ca-bundle.crt` on clone | Use ZIP / `download-and-install.ps1` |
| `ExecutionPolicy` | `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned` |
| `electron binary missing` | Re-run `install.ps1 -Build` |
| Blank window | Check `%LOCALAPPDATA%\GrokHub\runtime\ui.log` |
| Running from System32 | `cd $env:USERPROFILE\Downloads` first |

## Uninstall

```powershell
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\GrokHub"
Remove-Item -Force "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\GrokHub.lnk" -ErrorAction SilentlyContinue
Remove-Item -Force "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\GrokHub (direct).lnk" -ErrorAction SilentlyContinue
```
