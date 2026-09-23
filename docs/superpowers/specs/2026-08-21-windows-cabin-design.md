# Windows cabin installer

**Version:** 2.10.12  
 
**Repo:** `blackviperxiii-ui/GrokHub` (one cabin, two artifacts — not the deleted Electron app)

Default chat model is `grok-4.7`. Effort is None / Minimal / Low / Medium / High / Extra High. A saved Max effort loads as Extra High. GrokHub on Windows is the same native egui cabin as Linux. Grok Build (`grok.exe` over ACP) is the agent, host shell, and computer-use. The Windows artifact is `GrokHub-Setup-<version>.exe` (Inno) plus `grokhub-windows-v<version>.zip`. Same tag and cabin version as Linux `grokhub-linux-v*.tar.gz`. No Windows AT-SPI / screenshot / click backends. Button feel and the chat transcript match Linux: hover scale 1.035 over 120ms, press shrink, focus scale plus fill, and off-screen rows skip paint without dropping scroll identity or resize heights. Composer Stop is a disc with a small rounded mark. Idle Stop and the idle mic sit still; they ease while hovered, pressed, listening, speaking, or a reply is running. The transcript Running row has no Stop. The changing status text above the composer is gone. The context usage bar stays. A green live dot plus Thinking / Running / Waiting sits on the turn; hover shows the current action. That line does not sit above the composer. The Ask card names the command, path, or site. History lists grok sessions from the chat cwd (USERPROFILE session home). Skills cards, Automations cards, and the Imagine wall keep hover inside the slot. A long user bubble wraps inside the row and keeps its leading gap. A thought starts expanded; collapse leaves one short row that opens again; that fold survives the live-to-stored handoff; the reply stays. Account sets a display name and a local profile picture; the avatar menu, the rail, and the connected hint hide the email. Same paint as Linux.

## Split

Cabin owns: window, tray, project sidebar as cwd, LAN hub, Hey Grok voice (Ara, PTT STT then TTS of the reply body not thinking, live Voice · Listening + Stop, line stays open until Stop / live mic / Ctrl+G), Imagine toolbox, `/host` and `/sh` in the cabin (PowerShell on Windows).

Grok Build owns: coding tools, sandbox, permissions, plan mode, skills/plugins/MCP, sessions, and desktop computer-use.

Transport: spawn `grok.exe --no-auto-update agent stdio`. Do not vendor grok-build crates. Do not wrap the cabin in Electron or Tauri.

## Repository

- Source: this repo. Windows vs Linux is `cfg(windows)` / `cfg(unix)` on the same `main`.
- Packaging: `packaging/windows/` (Inno `grokhub.iss`), `scripts/make-windows-release.ps1`, and `.github/workflows/release.yml` `windows-installer`.
- The older [GrokHub-Windows](https://github.com/blackviperxiii-ui/GrokHub-Windows) fork is an archive. New cabin work lands here.
- Do not recreate or push the old Electron `Grok-Hub-Windows` history.

## Architecture

Same workspace crates: `grokhub-app` (`grokhub.exe`), `grokhub-hub` (`grokhub-hub.exe`), `grokhub-core`, `grokhub-acp`, `grokhub-ffi`.

| Area | Linux today | Windows |
|------|-------------|---------|
| UI | eframe glow + x11/wayland | eframe glow + Windows winit (`win32`) |
| Tray | `ksni` (StatusNotifierItem) | `tray-icon` (or equivalent Win32 tray). Close hides; Quit from tray. |
| Host `/sh` | `bash -lc` | `powershell.exe -NoProfile -Command` |
| Config | `$HOME/.config/GrokHub` | `%APPDATA%\GrokHub` (`GROKHUB_CONFIG` still wins) |
| Grok home | `$HOME/.grok` | `%USERPROFILE%\.grok` |
| Cabin-isolated GROK_HOME | `~/.config/GrokHub/grok-home` | `%APPDATA%\GrokHub\grok-home` |
| Leader socket | unix socket + `--leader-socket` | same flags if `grok.exe` accepts a filesystem path; otherwise isolate via `GROK_HOME` only |
| Process spawn | `setsid` / close extra fds / `kill -- -pid` | skip unix `pre_exec`; `child.kill()` is enough |
| Computer-use desktop.rs | x11/wayland helpers | compile stubs / unused; Grok Build owns this |
| Install | `install.sh` + systemd user units | per-user `GrokHub-Setup.exe` |

`HOME` reads must fall back to `USERPROFILE` on Windows so project roots, Grok locate, and memory paths work in a stock user session.

## Grok Build CLI

Locate order: `GROKHUB_GROK`, PATH (`grok.exe`), `%USERPROFILE%\.grok\bin\grok.exe`, then `grok.exe` next to `grokhub.exe`.

**Bundle if possible.** Release packaging downloads the official Windows x86_64 artifact (`https://x.ai/cli/grok-<ver>-windows-x86_64.exe`, version from `https://x.ai/cli/alpha`, currently **1.0.38**) as `grok.exe` and `agent.exe`. The installer copies them to `%USERPROFILE%\.grok\bin` and adds that directory to the user PATH. Setup also runs the official installer (`GROK_CHANNEL=alpha` / `https://x.ai/cli/install.ps1`) so grok is not assumed on PATH.

**If the download fails, it is not a ship blocker.** The installer still installs the cabin. First launch and reinstall automatically install Grok Build CLI **alpha** (`GROK_CHANNEL=alpha` / `https://x.ai/cli/alpha`) when `grok` is missing or unusable (stub MZ or `STATUS_DLL_NOT_FOUND`). UAC on first run is expected. The Install control is hidden when grok is already present or an alpha install is already running. A leftover `grok.exe` that cannot start shows one cabin error (Windows loader dialogs silenced), not a looping MessageBox.

Cabin overlay on Windows does not run Linux `install.sh`. `grokhub --update` / `/update` use the same pending rules as the titlebar chip and Settings → Update. When the cabin is newer they download the latest `grokhub-windows` zip from GitHub when there is no source clone, or when a leftover clone is not a usable `main` checkout (wrong branch, detached HEAD). A source clone on `main` still overlays with `install-windows.ps1`. When only the CLI alpha is newer, they run `grok update --alpha` and leave the cabin. When both are newer, the CLI step runs first, and a cabin-plan failure does not drop it. Settings → **Update** stays visible when the probe found nothing; that click still overlays CLI then cabin. `grokhub --update` still exits when nothing is newer. After a CLI install, `%USERPROFILE%\.grok\bin` is preferred over a leftover PATH `grok`. Every 2 hours, and at launch, the cabin checks GitHub Latest against the running cabin and the published Grok Build CLI alpha against `grok --version`. The titlebar chip is the in-app notice and says **Update CLI**, **Update cabin**, or **Update CLI and cabin**. A working alpha install is updated only when a newer alpha exists unless that Settings click overlays. It does not switch the CLI to stable. Linux uses the same pending cases.

## Installer

- Per-user, no admin.
- Prefix: `%LOCALAPPDATA%\Programs\GrokHub`
- Files: `grokhub.exe`, `grokhub-hub.exe`, license, icon. Bundled `grok.exe` / `agent.exe` when present at pack time.
- Start Menu shortcut. Desktop shortcut optional.
- Uninstall via Windows Apps. Packager is **Inno Setup 6** (`GrokHub-Setup-<version>.exe`).
- First launch: cabin window, tray icon, hub process as today. `--agent` starts hidden to tray.

Hub: same process model as Linux (cabin can spawn hub). No systemd. A Windows scheduled task is out of scope for v1.

## CI and build

GitHub Actions `windows-latest`:

1. `cargo test --workspace --locked`
2. `cargo build --release --locked -p grokhub-app -p grokhub-hub`
3. Attempt Grok Windows artifact download into the stage dir
4. Build Inno Setup `GrokHub-Setup-<version>.exe` and a portable zip of the same files
5. Upload artifacts; attach them to a git tag release

Cross-compile from CachyOS is not the release path.

## Tests

- `config_dir` / Grok locate / `which("grok.exe")` with fake `USERPROFILE` and `PATH`
- Host runner: PowerShell `echo` on Windows, bash on Unix (existing `echo_ok` stays Unix-gated or dual)
- Tray hide/show and second-instance raise: keep Linux tests; add Windows pid-alive without `/proc`
- ACP spawn: unix `pre_exec` not compiled on Windows; stdio handshake tests stay platform-neutral
- Packaging script fails closed if `grokhub.exe` is missing; Grok download failure is a warning

## Errors

- Missing `grok.exe`: cabin still opens; chat/doctor names the missing binary and the official install command.
- Host spawn failure: existing `spawn failed:` receipt, from PowerShell.
- Two cabins: existing `cabin.pid` / `cabin.raise` with a Windows-alive check (not `/proc`).

## Out of scope

- Electron / Tauri / old NSIS Setup.exe history
- Windows computer-use (screenshots, clicks, window enumeration) in the cabin
- MSIX / Store listing
- ARM64 Windows in v1 (x86_64 only)
- A second Windows-only source repo
- Android

## Success

A Windows user runs `GrokHub-Setup.exe` without admin, gets Start Menu **GrokHub**, sees the cabin, can hide to tray and Quit from the tray, and can talk to Grok Build when `grok.exe` is bundled or installed. Computer-use is Grok Build’s job.
