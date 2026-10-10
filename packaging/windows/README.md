# GrokHub Windows packaging

Per-user install (no admin): `%LOCALAPPDATA%\Programs\GrokHub`

## Prerequisites

- Rust toolchain with Windows target (`x86_64-pc-windows-msvc` or gnu)
- [Inno Setup 6](https://jrsoftware.org/isinfo.php) — `ISCC.exe` at:
  - `%ProgramFiles%\Inno Setup 6\ISCC.exe`
  - `%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe`

## Build

```powershell
pwsh -File scripts/make-windows-release.ps1
```

Stages `target/release/grokhub.exe` + `grokhub-hub.exe`, then runs ISCC. Setup installs only the cabin and hub. It does not install the Grok Build CLI or change PATH for it.

## Outputs (`dist-release/`)

| Artifact | Name |
|----------|------|
| Inno installer | `GrokHub-Setup-<version>.exe` |
| Portable zip | `grokhub-windows-v<version>.zip` |

Missing `grokhub.exe` / `grokhub-hub.exe` is fatal.

In-app **Settings → Update**, `/update`, and `grokhub --update` update only the cabin. When the cabin is newer and there is no source clone, they download that zip from the latest GitHub Release into `%LOCALAPPDATA%\Programs\GrokHub`. A source clone on `main` overlays with `scripts/install-windows.ps1` instead.

## Lock test (no Windows build)

```powershell
pwsh -File scripts/make-windows-release.tests.ps1
```
