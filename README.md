# GrokHub

Native Rust cabin for **Arch Linux / CachyOS**. No Electron. No Tauri.

**v2.6.42** — New chat does not reuse the last ACP session. Imagine prefers `grok login`. History is in the avatar menu. Handshake no longer mixes load-replay into the live turn. Default permission is Ask.

| Platform | Repository | Latest |
|----------|------------|--------|
| **Linux** (this) | [GrokHub](https://github.com/blackviperxiii-ui/GrokHub) | **v2.6.42** |
| **Windows** | [Grok-Hub-Windows](https://github.com/blackviperxiii-ui/Grok-Hub-Windows) | sibling — same `grokhub-core` |
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

`./scripts/install.sh --user` installs [Grok Build](https://x.ai/cli) (`grok`) next to the cabin. Then `grok login`. The cabin spawns `grok --no-auto-update agent stdio` over ACP. Bound project is the ACP cwd; unbound uses `~/GrokHub-Work` (never the cabin process cwd). Overlay `/update` updates the GUI and installs `grok` if it is missing; `grok update` updates the agent.

Slash: `/help` · `/new` · `/scratch` · `/clear` · `/undo` · `/retry` · `/stop` · `/sh` · `/host` · `/plan` · `/always-approve` · `/sessions` · `/inspect` · `/project` · `/memory` · `/recall` · `/forget` · `/board` · `/imagine` · `/skill` · `/compact` · `/learn reflect` · `/update` · `/send` · `/sync` · `/hub` · `/inhabit` · `/rewind` · `/room` · `/export` · `/rename` · `/pin` · `/delete` · `/effort` · `/dream` · `/palette`. Type `/help` in the cabin for the rest. `/skill <name>` runs that skill. `/compact` keeps the last 8 visible turns. `/context` counts visible turns. `/scratch` blocks `/forget` and Memory Save. `/rewind` restores the bound project root (or Grok conversation rewind when mapped). `/sync` merges chats and memory with paired computers. `/project` also takes `bind`, `new`, `folder`, `rename`, `move`, `delete`, `clear`. Right-click a sidebar project to rename or remove it — Delete drops the row, not the files.

Composer session pills: **Chat** / **Plan** / **Ask**. Permission: **Ask** / **Auto** / **Always**. `/effort low|medium|high|xhigh` maps leftover Fast / Balance / Think / Max pins. Grok Build chooses models. Empty-home greeting and quick chips use **Grok 4.1 Fast** (`grok-4-1-fast-non-reasoning`) through `grok login`.

Projects sit in the left rail. `+` makes a project (`~/GrokHub-Work/<slug>`) or a one-level folder. Double-click or right-click to rename (display name only — the path stays). Right-click a project to add it to a folder or remove it. Folders are sidebar only; they do not move files. Click a project to bind it. Click the bound project again to open the Workboard. Bound tree is the world.

History tabs pin, rename, and delete (right-click, or `/pin` `/rename` `/delete`). A manual rename is locked. After each turn Fast names the tab from the first topic (max 16 characters) unless that lock is set. Scratch stays unnamed. The Chat rail opens the last-accessed thread (scratch is skipped when another thread exists). Each thread stores `accessed_ms`; sitting on Chat stamps it.

History lists cabin chats plus `grok sessions` (and `~/.grok/sessions`). Click a Grok session to resume it. Each cabin thread stores the ACP session id so history survives the grok CLI backend.

Imagine stills use dedicated **`grok-imagine-image-2.0`** (falls back to `grok-imagine-image` on timeout). Video kind calls **`grok-imagine-video-1.5`**. Auth is `grok login` first, then a console key / cabin OAuth. Hey Grok: console API key for duplex Voice; OAuth is PTT STT + TTS. Desktop control is **Grok Build computer-use** — the cabin renders tool cards, diffs, and computer-use frames in chat. No Desk / Take over menu. Halt / Stop / tray Halt / Ctrl+Shift+Esc cancel the ACP turn (`session/cancel`). Stream buffers clip at `IMAGE_FILE_CAP` / `TEXT_FILE_CAP`. Desk frames drop above `FRAME_CAP`. Titlebar × unmaps to tray.

Settings → **Connect Grok OAuth** (or `grokhub --oauth`) is cabin sign-in for Voice. Agent auth and Imagine use `grok login` (or `XAI_API_KEY`). Tokens live in `~/.config/GrokHub/secrets.json` (mode 0600), never in markdown. Settings → Appearance is **Dark**, **Light**, or **System**.

Settings → **Update** (or `grokhub --update` / `/update`) retargets a leftover Origin clone to GitHub (`https://github.com/blackviperxiii-ui/GrokHub.git`), then `git pull --ff-only origin main` and `./scripts/install.sh --user`. Overlay updates the GUI and installs Grok Build CLI (`grok`) if it is missing. `grok update` updates the agent. Progress stays on Settings. After a clean overlay, **Restart** reloads hub, drops the cabin pid lock, starts a new overlay `grokhub`, and exits this process.

Chat is ACP. Night and phone `/v1/task` enqueue ACP prompts on the bound project. Halt / Stop / Ctrl+Shift+Esc cancel the turn. Chat only saves a night job when you asked to schedule one — a reply that mentions “every day at” or “heartbeat every” as advice does not. Anticipate only fires a `Follow skill` on a real `need to` / `remind me` insight that matches a skill, not polite “if you need” chit-chat. A 15s heartbeat runs housekeep, inbox, night, review, wall, mid-thought, reflect, and anticipate. Hidden idle cabins wait for that pulse. Phone dispatch completes on halt / error. `/rewind` restores only the bound project root.

Android / Windows: link `libgrokhub_ffi` and include `crates/grokhub-ffi/include/grokhub.h`.

| Binary | Crate | Job |
|--------|-------|-----|
| `grokhub` | `crates/grokhub-app` | Cabin GUI around Grok Build ACP |
| `grokhub-hub` | `crates/grokhub-hub` | Standalone LAN `/v1` hub (port **18766**) |
| `grok` | xAI Grok Build CLI | Official coding-agent CLI (`https://x.ai/cli`) — installed with the cabin |
| `libgrokhub_ffi` | `crates/grokhub-ffi` | C ABI for Android / Windows (pair/port/models; no HOST_CMD) |

Config and memory: `~/.config/GrokHub` (`app.json`, `projects.json`, `suggestions.json`, `secrets.json` mode 0600, `memory/SOUL.md`, `USER.md`, `MEMORY.md`).

## First run

1. Land in **chat**. Banner: `grok login` (install.sh already put `grok` on PATH), then Connect Grok in Settings for Voice/Imagine.
2. `grok login` (or paste `XAI_API_KEY`). Optional cabin OAuth for media.
3. Optional: Devices → **Start share** for the Android key-fob.
4. Chat is ACP. Halt cancels the Grok Build turn.

Tokens stay in `secrets.json`. Never in markdown.

Composer is a pill: **Ask anything**. Five quick chips sit centered under the bar. Plus opens Upload / Paste. Session pills are Chat / Plan / Ask; permission is Ask / Auto / Always. Mic is Hey Grok. Enter sends; Ctrl+Enter is a newline. Send becomes Stop while a reply runs. Chat streams ACP tokens onto the thread that started the job. Tool cards, diffs, permission prompts, and desk frames render in the pane. Leftover pages (Desk, Devices, Memory, History, Night, Workboard, Command) use the same catalog chrome. Command is a user `/sh` field, not the agent. `/v1/frame.jpg` serves the last ACP computer-use image when one exists.

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
