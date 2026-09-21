# GrokHub

Native Rust cabin. No Electron. No Tauri. One repo, one `main`, one version — two ship artifacts.

**v2.9.11** — Grok Build **1.0.38** alpha. Shared buttons hover-scale to 1.035 over 120ms, shrink on press, and scale plus fill on keyboard focus (same paint on Linux and Windows). Off-screen chat rows skip paint; row height follows the pane width and each row keeps a stable id. Voice is Ara; Hey Grok is PTT STT then TTS of the reply body (not thinking). A live Voice · Listening strip with Stop sits above the composer, and the line stays open until Stop, the live mic, or Ctrl+G / Super+G. Composer Stop / grok -p errors / Consult re-arm listen; failed STT holds instead of retrying every frame. Chat shows a green live dot plus Thinking / Running / Waiting (hover is the current action). First launch and reinstall install CLI alpha when grok is missing; wait / Get Started paint as the only pane and show live OAuth / device-code failures (not leftover wall or install status). Titlebar **Update available** when GitHub Latest is newer. Settings → Update adds **Update Grok Build CLI** (`grok update --alpha`, Linux PATH prepends `~/.grok/bin` and `~/.local/bin`). Windows leftover clones (not `main`) use the GitHub zip, same as Setup with no clone. Imagine videos play in-pane. Avatar menu is Settings / Help / Connect. Empty-home greeting and chips rank from the situation. Windows taskbar/tray icon and tray Quit. Settings is OAuth Account, quiet-hours dropdown, overlay Update. Chat rail reuses one empty draft (no New chat button). `/update` runs `grok update --alpha` on Linux and Windows. History is `grok sessions` 1:1.

| Platform | Artifact | Latest |
|----------|----------|--------|
| **Linux** (Arch / CachyOS) | `grokhub-linux-v2.9.11.tar.gz`, AUR | **v2.9.11** |
| **Windows** (x86_64) | `GrokHub-Setup-2.9.11.exe`, `grokhub-windows-v2.9.11.zip` | **v2.9.11** |
| **Android** | [Grok-Hub-Android](https://github.com/blackviperxiii-ui/Grok-Hub-Android) | key-fob — pair, task, JPEG |

Windows vs Linux in the cabin is `cfg(windows)` / `cfg(unix)`. The older [GrokHub-Windows](https://github.com/blackviperxiii-ui/GrokHub-Windows) fork is an archive source — new cabin work lands here.

## Run

```bash
sudo pacman -S --needed git rustup base-devel pkgconf gtk3 libxkbcommon libxkbcommon-x11 ffmpeg alsa-utils
rustup default stable
git clone https://github.com/blackviperxiii-ui/GrokHub.git
cd GrokHub
cargo test --workspace
./scripts/install.sh --user
grokhub
grok --version
```

Or without installing:

```bash
cargo run -p grokhub-app
cargo run -p grokhub-app -- --agent
cargo run -p grokhub-app -- --hub
cargo run -p grokhub-app -- --doctor
GROKHUB_HUB_PORT=18766 cargo run -p grokhub-hub
```

The tray icon is there from launch. Close / titlebar × hides the cabin — the window unmaps and stays unmapped until it loses focus (then a pinned taskbar click, tray **Show cabin**, or a second `grokhub` raises it). It does not minimize to the taskbar. Drag the titlebar body to move the undecorated window. Size and position come back on the next launch. Jobs, hub, and idle reflect keep running. Tray: **Show cabin**, **Halt**, **Quit**. One ping when it first hides; it does not spam the desktop. `grokhub --agent` starts already hidden. `GROKHUB_TRAY=0` quits on close.

`./scripts/install.sh --user` installs [Grok Build](https://x.ai/cli) **alpha** (`GROK_CHANNEL=alpha`, `grok`) next to the cabin. First launch Get Started connects Super Grok and signs in `grok` too. Chat is headless **`grok -p --output-format streaming-json`** (Grok Build 1.0.21+) with `--sandbox off` and a desktop rule so Grok uses this machine. Ask/Allow/Deny is ACP (`grok agent stdio`) when the permission pill is Ask. MCP elicitation (form or URL) uses the same ACP session. Night and phone `/v1/task` stay on `grok -p`. Bound project is `--cwd`; unbound uses `~/GrokHub-Work` (never the cabin process cwd). Overlay `/update` updates the GUI, then runs `grok update --alpha`. If `grok` is on stable after an upgrade, the cabin switches it back to alpha and does not yank a working alpha install. `/context` and `/usage` show Grok Build server tokens (including reasoning); `/usage` also totals today's cabin spend and, when a session is attached, `grok usage <id>` (1.0.14 per-turn cost). `/compact` and `/rewind` talk to Grok. Halt kills the `grok -p` child (`session/cancel` on ACP). Truncated replies and transient 5xx retries stay on the same turn. Credit-limit errors offer Try Again.

Slash: `/help` · `/new` · `/scratch` · `/clear` · `/undo` · `/retry` · `/stop` · `/sh` · `/host` · `/plan` · `/always-approve` · `/sessions` · `/inspect` · `/project` · `/memory` · `/recall` · `/forget` · `/board` · `/imagine` · `/skill` · `/compact` · `/learn` · `/update` · `/send` · `/sync` · `/hub` · `/inhabit` · `/rewind` · `/room` · `/export` · `/rename` · `/pin` · `/delete` · `/effort` · `/dream` · `/palette`. Type `/help` in the cabin for the rest. `/skill <name>` runs that skill. Skills and Connectors lists **Cabin skills** (`~/.config/GrokHub/skills`) next to the Grok Build catalog, with the run count and Use in chat. `/compact` keeps the last 8 visible turns. `/context` counts visible turns. `/scratch` blocks `/forget` and Memory Save. `/rewind` restores the bound project root (or Grok conversation rewind when mapped). `/sync` merges chats and memory with paired computers. `/project` also takes `bind`, `new`, `folder`, `rename`, `move`, `delete`, `clear`. Right-click a sidebar project to rename or remove it — Delete drops the row, not the files.

Composer session pills: **Chat** / **Plan** / **Ask**. Permission: **Ask** / **Auto** / **Always**. Hover a pill for a short tip. Both pills are remembered across launches — Always-approve is the exception and resets to Ask, same as the leftover `yolo` reset. On a permission card, **Enter** allows and **Esc** denies while the composer is empty; a half-typed follow-up still sends on Enter. Ask is fail-closed on headless (no permission card); Auto/Always map to `--permission-mode auto` / `--always-approve`. Effort dropdown: **None** / **Minimal** / **Low** / **Medium** / **High** / **Extra High** (`grok -p --reasoning-effort`). `/effort` sets the same. A saved Max effort loads as Extra High. Default model is **Grok 4.7** (`grok -p --model grok-4.7`). Greeting and chips use `grok-4.7` through `grok login`.

The **Automations** page holds both schedulers. **Scheduled** is the cabin's own clock list (`automations.json`) — pause, **Run** now, or **Remove** a job, and each row shows its schedule, next run and run count. **Loops** is the Grok Build `/loop` interval list. **New job** takes either shape: `/loop 30m check deploy` or `every weekday at 9, summarize the board`.

The left-rail **Chat** button is the new-chat control — there is no separate New chat row. Click Chat on an empty draft (no dialogue) to pull that same chat up. After a reply has started, Chat opens a new draft. One empty draft at a time. Old convos stay in sidebar History (`grok sessions`).

Projects sit in the left rail. `+` makes a project (`~/GrokHub-Work/<slug>`) or a one-level folder. Double-click or right-click to rename (display name only — the path stays). Right-click a project to add it to a folder or remove it. Folders are sidebar only; they do not move files. Click a project to bind it. Click the bound project again to open the Workboard. Bound tree is the world.

History search runs as you type across SOUL/USER/MEMORY and every chat; a new query drops the previous needle's hits so a late walk cannot open the wrong thread. A hit is a door — click a memory line to open that file in the editor, a chat line to open that thread. Re-opening the memory file already in the editor keeps unsaved typing. Below the search, History is the same list as `grok sessions list` (newest first). Right-click **Delete** or the History-page Delete button runs `grok sessions delete` against `~/.grok`, then refreshes from the CLI — a row cannot come back until Grok Build says it is gone. Cabin chats use the user Grok home so they show up in that list. Transcripts load via `grok export`. The cabin stays on Grok Build CLI **alpha** (`grok update --alpha`). It does not switch off alpha.

Imagine stills use dedicated **`grok-imagine-image-2.0`** (falls back to `grok-imagine-image` on timeout). Video kind calls **`grok-imagine-video-1.5`**. Auth is `grok login` first, then a console key / cabin OAuth. Hey Grok: push-to-talk STT into chat, then TTS of the reply body (not thinking). Same on Linux and Windows. Desktop control is **Grok Build computer-use** — the cabin keeps tool cards, diffs, and computer-use frames in a collapsed Work tree in chat. No Desk / Take over menu. Halt / Stop / tray Halt / Ctrl+Shift+Esc SIGTERM the `grok -p` child. Stream buffers clip at `IMAGE_FILE_CAP` / `TEXT_FILE_CAP`. Desk frames drop above `FRAME_CAP`. Titlebar × unmaps to tray. Plus-button stills ride `--prompt-json` image blocks.

Settings → **Account** is Super Grok device-code OAuth only — **Connect** / **Sign out** (or `grokhub --oauth`). That also writes `~/.grok/auth.json` when the Grok Build CLI is not already connected. Tokens live in `~/.config/GrokHub/secrets.json` (mode 0600), never in markdown. Settings → Appearance is **Dark**, **Light**, or **System**. Settings → Behavior holds close-to-tray, the living wall, and one **quiet hours** dropdown (Off / common windows). Picking a window saves it. GitHub PAT is not a Settings page — the connector owns that.

Windows Setup and first cabin launch (also Linux tarball / AUR first launch) **automatically** install Grok Build CLI **alpha** (`GROK_CHANNEL=alpha` / `https://x.ai/cli/alpha`) when `grok` is missing or unusable. Reinstall does the same. A working alpha install is left alone. The Install control is hidden when grok is already present or an alpha install is already running. A leftover `grok.exe` that cannot start (missing DLL) shows one cabin error, not a looping Windows dialog.

Settings → **Update** (or `grokhub --update` / `/update`) is **Install Grok Build CLI** only when grok is still missing after the automatic install failed, **Update Grok Build CLI** (`grok update --alpha`) when grok is already installed, the install-overlay control, **Update**, progress, and **Restart** after a clean overlay. When GitHub Latest is a newer cabin than the running version, the titlebar shows **Update available** (in-app — not a web page). Click it to open this same Update overlay. It retargets a leftover Origin or `GrokHub-Windows` clone to GitHub (`https://github.com/blackviperxiii-ui/GrokHub.git`), then `git pull --ff-only origin main`. Linux overlays with `./scripts/install.sh --user` and `grok update --alpha`. If a leftover `grok` is on stable, overlay / first launch switches it to alpha. Windows Setup users do not need a clone: Update downloads the latest `grokhub-windows-v*.zip` from GitHub into `%LOCALAPPDATA%\Programs\GrokHub`, then runs `grok update --alpha` (PATH includes `%USERPROFILE%\.grok\bin`). A leftover Windows clone (wrong branch, detached HEAD) uses that same zip. A Windows source clone on `main` still overlays with `scripts/install-windows.ps1`. Linux overlay / Update CLI prepends `$HOME/.grok/bin:$HOME/.local/bin` so `grok update --alpha` finds the official install. The clone path is not a Settings field — empty uses `GROKHUB_SRC` or the install receipt. After a clean overlay, **Restart** reloads hub, drops the cabin pid lock, starts a new overlay `grokhub`, and exits this process.

Settings → **About** is the app name, version, Grok Build line, doctor, and a redacted diagnostics copy. It does not list today's usage buckets or the model catalog (`/usage` and `/models` still do).

Chat is headless `grok -p` on this desktop (full filesystem and shell; Grok is told not to claim it lacks computer access). Night and phone `/v1/task` enqueue the same on the bound project. Halt / Stop / Ctrl+Shift+Esc kill the child. Chat only saves a night job when you asked to schedule one — a reply that mentions “every day at” or “heartbeat every” as advice does not. A clock ask (“every weekday at 9pm, summarize the board”) is saved as a cabin automation in `automations.json` with its hour intact; an interval ask (`/loop 30m`, “every 2 hours”) is saved as a Grok Build loop. The pulse fires both. Anticipate only fires a `Follow skill` on a real `need to` / `remind me` insight that matches a skill, not polite “if you need” chit-chat. A 15s heartbeat runs housekeep, inbox, night, review, wall, mid-thought, reflect, and anticipate. Hidden idle cabins wait for that pulse. Phone dispatch completes on halt / error. `/rewind` restores only the bound project root.

Android / Windows: link `libgrokhub_ffi` and include `crates/grokhub-ffi/include/grokhub.h`.

| Binary | Crate | Job |
|--------|-------|-----|
| `grokhub` | `crates/grokhub-app` | Cabin GUI around Grok Build `grok -p` |
| `grokhub-hub` | `crates/grokhub-hub` | Standalone LAN `/v1` hub (port **18766**) |
| `grok` | xAI Grok Build CLI | Official coding-agent CLI (`https://x.ai/cli`) — installed with the cabin |
| `libgrokhub_ffi` | `crates/grokhub-ffi` | C ABI for Android / Windows (pair/port/models; no HOST_CMD) |

Config and memory: `~/.config/GrokHub` (`app.json`, `projects.json`, `suggestions.json`, `secrets.json` mode 0600, `memory/SOUL.md`, `USER.md`, `MEMORY.md`).

## First run

1. The installer already put **Grok Build CLI alpha** on PATH (`GROK_CHANNEL=alpha` / Windows `x.ai/cli/alpha`).
2. Launch `grokhub`. **Get Started** asks to connect Super Grok (existing device-code OAuth). That also writes `~/.grok/auth.json` when the CLI has no session.
3. Settings → Connect Grok OAuth repeats the CLI write if `grok` is still logged out.
4. Optional: Devices → **Start share** for the Android key-fob. Chat is `grok -p`. Halt stops the child. Ask shows Allow / Deny when ACP is up.

Tokens stay in `secrets.json`. Never in markdown.

Composer is a pill: **Ask anything**. Five quick chips sit centered under the bar and rank the next likely move (last slash, last mode, Imagine vs chat, unfinished work, skills). The empty-home greeting is time + name + last project, not a memory dump. Plus opens Upload / Paste. Session pills are Chat / Plan / Ask; permission is Ask / Auto / Always. Hover a pill for what it does. Mic is Hey Grok (Ara). While voice is live, Voice · Listening sits above the composer with Stop. After a listen/speak turn the line stays open and listens again until Stop, the live mic, or Ctrl+G / Super+G. Enter sends; Ctrl+Enter is a newline. Send becomes Stop while a reply runs. A green live dot plus Thinking / Running / Waiting sits on the turn (and above the composer); hover shows the current action. Shared buttons hover-scale to 1.035 over 120ms, shrink on press, and scale plus fill on keyboard focus. Off-screen chat rows skip paint; height follows the pane width and each row keeps its id. Chat streams Grok Build tokens onto the thread that started the job. Follow-ups queue instead of killing the turn. Tool cards, diffs, permission prompts, and desk frames render in the pane. Leftover pages (Devices, Memory, History, Automations, Workboard, Command) use the same catalog chrome. Command is a user `/sh` field, not the agent. `/v1/frame.jpg` serves the last ACP computer-use image when one exists.

## Always-on hub

The cabin embeds the hub when you start share. For a headless box:

```bash
mkdir -p ~/.config/systemd/user
cp packaging/systemd/grokhub-hub.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now grokhub-hub.service
```

## Devices (phone / other PC)

Pair code `ABC-234`. Devices paints a real LAN IPv4 (`http://192.168.x.x:18766`), not a `<lan>` placeholder. Expired pair codes hide and rotate; New / rotated codes persist. The pair tile hides when the hub is not sharing. Android talks HTTP. Do not inhabit onto the phone. Hub `complete` is owner-only.

Contract: [`docs/superpowers/plans/2026-08-14-dispatch-android-notes.md`](docs/superpowers/plans/2026-08-14-dispatch-android-notes.md).

| Method | Path | Auth |
|--------|------|------|
| `GET` | `/v1/health` | none |
| `POST` | `/v1/pair` | pairing code |
| `POST` | `/v1/task` | Bearer |
| `GET` | `/v1/task/:id` | Bearer |
| `GET` | `/v1/results` | Bearer |
| `GET` | `/v1/frame.jpg` | Bearer (`?since=` → 304) |
| `POST` | `/v1/voice/client-secret` | Bearer — mints a 5-minute xAI realtime secret from the cabin console key. Android/browser use `wsProtocol` (`xai-client-secret.<token>`). OAuth cannot mint this. |

## Packaging

| Path | Role |
|------|------|
| `~/.local/bin/grokhub` | User install (`./scripts/install.sh --user`) |
| `~/.local/bin/grok` | Grok Build CLI (official xAI installer; also `~/.grok/bin/grok`) |
| `/usr/bin/grokhub` | System / makepkg |
| `/usr/bin/grok` | System Grok Build CLI (AUR `post_install`) |
| `~/.config/GrokHub` | Linux user data (`app.json`, `projects.json`, `secrets.json`, memory) |
| `%LOCALAPPDATA%\Programs\GrokHub` | Windows cabin (`grokhub.exe`) |
| `%APPDATA%\GrokHub` | Windows user data |

Linux tarball: `grokhub-linux-v*.tar.gz` from `./scripts/make-release-bundle.sh`.  
Windows installer: `GrokHub-Setup-<version>.exe` from `./scripts/make-windows-release.ps1` (Inno Setup). A tag publishes both.

Arch notes: [`packaging/README-ARCH.md`](packaging/README-ARCH.md).

## Uninstall

```bash
rm -f ~/.local/bin/grokhub ~/.local/bin/grokhub-hub
rm -rf ~/.local/lib/grokhub
rm -f ~/.local/share/applications/grokhub.desktop
# optional: rm -rf ~/.config/GrokHub
# optional Grok Build CLI: rm -f ~/.local/bin/grok ~/.local/bin/agent; rm -rf ~/.grok
sudo rm -f /usr/bin/grokhub /usr/bin/grokhub-hub
sudo rm -f /usr/share/applications/grokhub.desktop
```

## Development

```bash
cargo test --workspace
cargo run -p grokhub-app
cargo run -p grokhub-app -- --agent
cargo run -p grokhub-hub
cargo run -p grokhub-app -- --update
```

Spec: [`docs/superpowers/specs/2026-08-14-rust-parity-design.md`](docs/superpowers/specs/2026-08-14-rust-parity-design.md).

## License

MIT
