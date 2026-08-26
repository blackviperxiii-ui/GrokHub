# Windows cabin installer

**Version:** 2.6.42  
**Repo:** new public `blackviperxiii-ui/GrokHub-Windows` (not the deleted Electron app)

GrokHub on Windows is the same native egui cabin. Grok Build (`grok.exe` over ACP) is the agent, host shell, and computer-use. The first Windows ship is cabin-first: UI, hub, tray, Settings, and Grok Build. No Windows AT-SPI / screenshot / click backends.

## Split

Cabin owns: window, tray, project sidebar as cwd, LAN hub, Hey Grok voice, Imagine toolbox, `/host` and `/sh` in the cabin (PowerShell on Windows).

Grok Build owns: coding tools, sandbox, permissions, plan mode, skills/plugins/MCP, sessions, and desktop computer-use.

Transport: spawn `grok.exe --no-auto-update agent stdio`. Do not vendor grok-build crates. Do not wrap the cabin in Electron or Tauri.

## Repository

- Source: copy of Linux `GrokHub` committed `main` at v2.6.42 (`790d8e40`). Do not include uncommitted local cabin WIP.
- Linux `GrokHub` stays the Arch/CachyOS repo. Windows packaging, CI, and `cfg(windows)` live in `GrokHub-Windows`.
- Public. Default branch `main`.
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

**Bundle if possible.** Release packaging downloads the official Windows x86_64 artifact (`https://x.ai/cli/grok-<ver>-windows-x86_64.exe`, version from `https://x.ai/cli/stable`) as `grok.exe` and `agent.exe`. The installer copies them to `%USERPROFILE%\.grok\bin` and adds that directory to the user PATH.

**If the download fails, it is not a ship blocker.** The installer still installs the cabin. First launch or a post-install step may run `irm https://x.ai/cli/install.ps1 | iex`. Settings / doctor say when `grok.exe` is missing and how to install it.

Cabin overlay on Windows does not run Linux `install.sh`. Agent updates stay `grok update`.

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
- Merging Windows CI back into the Linux repo in this ship
- Android

## Success

A Windows user runs `GrokHub-Setup.exe` without admin, gets Start Menu **GrokHub**, sees the cabin, can hide to tray and Quit from the tray, and can talk to Grok Build when `grok.exe` is bundled or installed. Computer-use is Grok Build’s job.
