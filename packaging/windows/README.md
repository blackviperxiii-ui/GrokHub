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

Stages `target/release/grokhub.exe` + `grokhub-hub.exe`, downloads Grok Build CLI **alpha** into the stage, then runs ISCC. Missing `grok.exe` fails the pack. Setup still runs `install-grok-alpha.ps1` (`GROK_CHANNEL=alpha` / `https://x.ai/cli/install.ps1`) so grok is not assumed on PATH. First cabin launch does the same if grok is missing or unusable. Offline:

```powershell
pwsh -File scripts/make-windows-release.ps1 -SkipGrok
```

## Outputs (`dist-release/`)

| Artifact | Name |
|----------|------|
| Inno installer | `GrokHub-Setup-<version>.exe` |
| Portable zip | `grokhub-windows-v<version>.zip` |

Missing `grokhub.exe` / `grokhub-hub.exe` is fatal. Grok download failure is fatal unless `-SkipGrok`.

In-app **Settings → Update**, `/update`, and `grokhub --update` run only what is newer. When the cabin is newer and there is no source clone, they download that zip from the latest GitHub Release into `%LOCALAPPDATA%\Programs\GrokHub`. A source clone on `main` overlays with `scripts/install-windows.ps1` instead. When the CLI alpha is newer they run `grok update --alpha` (first, when both are newer). A current alpha is left alone.

## Lock test (no Windows build)

```powershell
pwsh -File scripts/make-windows-release.tests.ps1
```
