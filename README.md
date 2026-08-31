# GrokHub

Native Rust cabin for **Arch Linux / CachyOS**. No Electron. No Tauri.

**v2.7.1** — Grok Build **1.0.14**. History is `grok sessions` 1:1. Chat is this Linux desktop. `/usage` includes persisted session spend. Retry status shows why.

| Platform | Repository | Latest |
|----------|------------|--------|
| **Linux** (this) | [GrokHub](https://github.com/blackviperxiii-ui/GrokHub) | **v2.7.1** |
| **Windows** | [GrokHub-Windows](https://github.com/blackviperxiii-ui/GrokHub-Windows) | native cabin — same crates |
| **Android** | [Grok-Hub-Android](https://github.com/blackviperxiii-ui/Grok-Hub-Android) | key-fob — pair, task, JPEG |

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

`./scripts/install.sh --user` installs [Grok Build](https://x.ai/cli) (`grok`) next to the cabin. Then `grok login`. Chat is headless **`grok -p --output-format streaming-json`** (Grok Build 1.0.14+) with `--sandbox off` and a desktop rule so Grok uses this machine. Ask/Allow/Deny is ACP (`grok agent stdio`) when the permission pill is Ask. Night and phone `/v1/task` stay on `grok -p`. Bound project is `--cwd`; unbound uses `~/GrokHub-Work` (never the cabin process cwd). Overlay `/update` updates the GUI and installs `grok` if it is missing; `grok update` updates the agent on the stable channel (`grok update --alpha` is optional). `/context` and `/usage` show Grok Build server tokens (including reasoning); `/usage` also totals today's cabin spend and, when a session is attached, `grok usage <id>` (1.0.14 per-turn cost). `/compact` and `/rewind` talk to Grok. Halt kills the `grok -p` child (`session/cancel` on ACP). Truncated replies and transient 5xx retries stay on the same turn. Credit-limit errors offer Try Again.

Slash: `/help` · `/new` · `/scratch` · `/clear` · `/undo` · `/retry` · `/stop` · `/sh` · `/host` · `/plan` · `/always-approve` · `/sessions` · `/inspect` · `/project` · `/memory` · `/recall` · `/forget` · `/board` · `/imagine` · `/skill` · `/compact` · `/learn reflect` · `/update` · `/send` · `/sync` · `/hub` · `/inhabit` · `/rewind` · `/room` · `/export` · `/rename` · `/pin` · `/delete` · `/effort` · `/dream` · `/palette`. Type `/help` in the cabin for the rest. `/skill <name>` runs that skill. Skills and Connectors lists **Cabin skills** (`~/.config/GrokHub/skills`) next to the Grok Build catalog, with the run count and Use in chat. `/compact` keeps the last 8 visible turns. `/context` counts visible turns. `/scratch` blocks `/forget` and Memory Save. `/rewind` restores the bound project root (or Grok conversation rewind when mapped). `/sync` merges chats and memory with paired computers. `/project` also takes `bind`, `new`, `folder`, `rename`, `move`, `delete`, `clear`. Right-click a sidebar project to rename or remove it — Delete drops the row, not the files.

Composer session pills: **Chat** / **Plan** / **Ask**. Permission: **Ask** / **Auto** / **Always**. Both pills are remembered across launches — Always-approve is the exception and resets to Ask, same as the leftover `yolo` reset. On a permission card, **Enter** allows and **Esc** denies while the composer is empty; a half-typed follow-up still sends on Enter. Ask is fail-closed on headless (no permission card); Auto/Always map to `--permission-mode auto` / `--always-approve`. Effort dropdown: **None** / **Minimal** / **Low** / **Medium** / **High** / **Extra High** / **Max** (`grok -p --reasoning-effort`). `/effort` sets the same. Default model is **Grok 4.6**. Greeting and chips use `grok-4.6` through `grok login`.

The **Automations** page holds both schedulers. **Scheduled** is the cabin's own clock list (`automations.json`) — pause, **Run** now, or **Remove** a job, and each row shows its schedule, next run and run count. **Loops** is the Grok Build `/loop` interval list. **New job** takes either shape: `/loop 30m check deploy` or `every weekday at 9, summarize the board`.

Projects sit in the left rail. `+` makes a project (`~/GrokHub-Work/<slug>`) or a one-level folder. Double-click or right-click to rename (display name only — the path stays). Right-click a project to add it to a folder or remove it. Folders are sidebar only; they do not move files. Click a project to bind it. Click the bound project again to open the Workboard. Bound tree is the world.

History search runs as you type across SOUL/USER/MEMORY and every chat; a hit is a door — click a memory line to open that file in the editor, a chat line to open that thread. Below it, History is the same list as `grok sessions list` (newest first). Right-click **Delete** or the History-page Delete button runs `grok sessions delete` against `~/.grok`, then refreshes from the CLI — a row cannot come back until Grok Build says it is gone. Cabin chats use the user Grok home so they show up in that list. Transcripts load via `grok export`. Stable channel: `grok update`. Optional faster builds: `grok update --alpha`.

Imagine stills use dedicated **`grok-imagine-image-2.0`** (falls back to `grok-imagine-image` on timeout). Video kind calls **`grok-imagine-video-1.5`**. Auth is `grok login` first, then a console key / cabin OAuth. Hey Grok: console API key for duplex Voice; OAuth is PTT STT + TTS. Desktop control is **Grok Build computer-use** — the cabin renders tool cards, diffs, and computer-use frames in chat. No Desk / Take over menu. Halt / Stop / tray Halt / Ctrl+Shift+Esc SIGTERM the `grok -p` child. Stream buffers clip at `IMAGE_FILE_CAP` / `TEXT_FILE_CAP`. Desk frames drop above `FRAME_CAP`. Titlebar × unmaps to tray. Plus-button stills ride `--prompt-json` image blocks.

Settings → **Connect Grok OAuth** (or `grokhub --oauth`) is cabin sign-in for Voice. Agent auth and Imagine use `grok login` (or `XAI_API_KEY`). Tokens live in `~/.config/GrokHub/secrets.json` (mode 0600), never in markdown. Settings → Appearance is **Dark**, **Light**, or **System**. Settings → Behavior holds close-to-tray, the living wall, **quiet hours**, **automations a day**, and **host commands an hour** — a clock or cap the cabin cannot read keeps the old value instead of switching the guard off.

Settings → **Update** (or `grokhub --update` / `/update`) retargets a leftover Origin clone to GitHub (`https://github.com/blackviperxiii-ui/GrokHub.git`), then `git pull --ff-only origin main` and `./scripts/install.sh --user`. Overlay updates the GUI and installs Grok Build CLI (`grok`) if it is missing. `grok update` updates the agent. Progress stays on Settings. After a clean overlay, **Restart** reloads hub, drops the cabin pid lock, starts a new overlay `grokhub`, and exits this process.

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

1. Land in **chat**. Banner: `grok login` (install.sh already put `grok` on PATH), then Connect Grok in Settings for Voice/Imagine.
2. `grok login` (or paste `XAI_API_KEY`). Optional cabin OAuth for media.
3. Optional: Devices → **Start share** for the Android key-fob.
4. Chat is `grok -p`. Halt stops the child. Ask shows Allow / Deny when ACP is up.

Tokens stay in `secrets.json`. Never in markdown.

Composer is a pill: **Ask anything**. Five quick chips sit centered under the bar. Plus opens Upload / Paste. Session pills are Chat / Plan / Ask; permission is Ask / Auto / Always. Mic is Hey Grok. Enter sends; Ctrl+Enter is a newline. Send becomes Stop while a reply runs. Chat streams Grok Build tokens onto the thread that started the job. Follow-ups queue instead of killing the turn. Tool cards, diffs, permission prompts, and desk frames render in the pane. Leftover pages (Devices, Memory, History, Automations, Workboard, Command) use the same catalog chrome. Command is a user `/sh` field, not the agent. `/v1/frame.jpg` serves the last ACP computer-use image when one exists.

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
| `~/.config/GrokHub` | User data (`app.json`, `projects.json`, `secrets.json`, memory) |

Release tarball: `grokhub-linux-v*.tar.gz` from `./scripts/make-release-bundle.sh`.

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
