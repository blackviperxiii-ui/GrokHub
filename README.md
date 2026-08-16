# GrokHub

Native Rust cabin for **Arch Linux / CachyOS**. No Electron. No Tauri.

**v2.5.0** — Grok-native unsandboxed control plane. You sit down in the cabin. It already knows the night.

| Platform | Repository | Latest |
|----------|------------|--------|
| **Linux** (this) | [Grok-Hub](https://github.com/blackviperxiii-ui/Grok-Hub) | **v2.5.0** |
| **Windows** | [Grok-Hub-Windows](https://github.com/blackviperxiii-ui/Grok-Hub-Windows) | sibling — same `grokhub-core` |
| **Android** | [Grok-Hub-Android](https://github.com/blackviperxiii-ui/Grok-Hub-Android) | key-fob — pair, task, JPEG |

## Run

```bash
sudo pacman -S --needed git rustup base-devel pkgconf gtk3 libxkbcommon libxkbcommon-x11
rustup default stable
git clone https://github.com/blackviperxiii-ui/Grok-Hub.git
cd Grok-Hub
cargo test --workspace
./scripts/install.sh --user
grokhub
```

Or without installing:

```bash
cargo run -p grokhub-app
cargo run -p grokhub-app -- --agent
cargo run -p grokhub-app -- --hub
cargo run -p grokhub-app -- --doctor
GROKHUB_HUB_PORT=18766 cargo run -p grokhub-hub
```

The tray icon is there from launch. Close / titlebar × hides the cabin — the window unmaps; it does not minimize to the taskbar. Size and position come back on the next launch. Jobs, hub, and idle reflect keep running. Tray: **Show cabin**, **Halt hands**, **Quit**. One ping when it first hides; it does not spam the desktop. `grokhub --agent` starts already hidden. `GROKHUB_TRAY=0` quits on close.

Slash: `/help` · `/new` · `/scratch` · `/clear` · `/undo` · `/retry` · `/stop` · `/sh` · `/host` · `/project` · `/approve` · `/memory` · `/recall` · `/forget` · `/board` · `/imagine` · `/skill` · `/compact` · `/learn reflect` · `/update` · `/send` · `/sync` · `/hub` · `/inhabit` · `/rewind` · `/room` · `/export` · `/rename` · `/pin` · `/delete` · `/mode auto|fast|balance|think|max` · `/dream` · `/palette`. Type `/help` in the cabin for the rest. `/project` also takes `bind`, `new`, `folder`, `rename`, `move`, `delete`, `clear`. Right-click a sidebar project to rename or remove it — Delete drops the row, not the files.

Composer modes (combo on the pill, or `/mode`):

| UI | Sends |
|----|--------|
| **Auto** | Picks Fast / Balance / Think / Max from the ask. A Settings chat-model pin that is **not** a ladder default (`grok-3-mini-fast` / `grok-4.3` / `grok-4.6`) skips this. `/mode` and the composer combo do not write that pin. |
| **Fast** | `grok-3-mini-fast` |
| **Balance** | `grok-4.3` |
| **Think** | `grok-4.6` at `high` |
| **Max** | `grok-4.6` at `xhigh` (`xhigh` is 4.6 only) |

If the request returns 401 / 403 / 429 / 5xx, the cabin retries once down the ladder: Grok 4.6 → 4.3 → Fast. Fast has no further drop.

Projects sit in the left rail. `+` makes a project (`~/GrokHub-Work/<slug>`) or a one-level folder. Double-click or right-click to rename (display name only — the path stays). Right-click a project to add it to a folder or remove it. Folders are sidebar only; they do not move files. Click a project to bind it. Click the bound project again to open the Workboard. Bound tree is the world.

History tabs pin, rename, and delete (right-click, or `/pin` `/rename` `/delete`). A manual rename is locked. After each turn Fast names the tab from the current topics unless that lock is set. Scratch stays unnamed.

Imagine always uses dedicated **`grok-2-image`** (chat model is ignored). The toolbox docks mid-pane, then to the floor once a prompt or still is live. Aspect / kind / quality / style chips change the still prompt. Plus opens **Upload file / Paste clipboard**. Hey Grok: with a **console API key**, duplex Voice streams 24 kHz PCM on `wss://api.x.ai/v1/realtime?model=grok-voice-think-fast-2.0`. **OAuth** (no console key) is 4s push-to-talk STT + TTS (whisper fallback if neither OAuth nor a console key is present). Consumer OAuth does not grant the realtime socket. Eyes walks AT-SPI (`pyatspi`) then wmctrl + cursor. With Cabin eyes on, a JPEG is captured on each chat send, stored on the hub (not disk), and attached to that turn. Asking Grok to click, type, or otherwise drive the desktop also attaches a frame and runs `COMPUTER_CMD` through xdotool (no sandbox).

Settings → **Connect Grok OAuth** (or `grokhub --oauth`) is the sign-in. Device-code against `auth.x.ai` — same public client as Grok CLI. The footer paints the Grok profile photo when the session has one. A console API key is the fallback for chat and the only path for duplex Voice. Tokens live in `~/.config/GrokHub/secrets.json` (mode 0600), never in markdown. Settings → Appearance is **Dark**, **Light**, or **System**.

Settings → **Update** (or `grokhub --update` / `/update`) does `git pull --ff-only origin main` in the source clone, then `./scripts/install.sh --user`. The clone must be on `main` with an `origin`. Overlay only — config stays. Progress stays on Settings (bar + percent). Quit the tray and relaunch `grokhub` so the new binary is the one that runs.

`HOST_PLAN` is an editable checklist. `scripts/verify.sh` gates **done** / `GOAL_COMPLETE`. A recipe whose `screen=` does not match the current desktop reshoots and skips coordinate clicks. Idle ≥ 10 minutes (or `/learn reflect`) does a surgical `MEMORY.md` edit with a diff and `.prev` restore.

Android / Windows: link `libgrokhub_ffi` and include `crates/grokhub-ffi/include/grokhub.h`.

| Binary | Crate | Job |
|--------|-------|-----|
| `grokhub` | `crates/grokhub-app` | Cabin — chat, board, imagine, skills, eyes, host |
| `grokhub-hub` | `crates/grokhub-hub` | Standalone LAN `/v1` hub (port **18766**) |
| `libgrokhub_ffi` | `crates/grokhub-ffi` | C ABI for Android / Windows |

Config and memory: `~/.config/GrokHub` (`app.json`, `projects.json`, `secrets.json` mode 0600, `memory/SOUL.md`, `USER.md`, `MEMORY.md`).

## First run

1. Land in **chat**. Banner: **Connect Grok in Settings**.
2. Settings → **Connect Grok OAuth** (or paste a console key from [console.x.ai](https://console.x.ai)). Save.
3. Optional: Devices → **Start share** for the Android key-fob.
4. YOLO is `/approve off` — host commands run without a prompt.

Tokens stay in `secrets.json`. Never in markdown.

Composer is a pill: **What do you want to know?** Five quick chips sit above it on one row — no selected first pill. An empty chat can show a faint Fast blurb. Plus opens Upload / Paste. Mode combo is Auto / Fast / Balance / Think / Max. `/mode` only sets the combo — it does not overwrite Settings → Chat model. Mic is Hey Grok. Enter or the arrow sends; Ctrl+Enter starts a new line. While a reply (or Imagine / host job) is running, Send is Stop. Chat streams tokens onto the thread that started the job; opening New chat does not steal the live reply. User and assistant turns sit in bubbles that hug the text. Host receipts stay off the bubble. Imagine stills sit above the composer. Grok may emit `HOST_CMD:` lines (unsandboxed `bash -lc`) and `COMPUTER_CMD:` lines (mouse, keyboard, `act` / `wait_for` via xdotool). The cabin confirms unless YOLO. `/stop`, tray **Halt hands**, the Stop square, and Ctrl+Shift+Esc actually kill those workers. When you ask it to drive the UI, a JPEG frame is attached for that turn even if Cabin eyes is off. Lock/password screens are skipped. Forbidden paths (`~/.ssh`, `/etc/shadow`) stay blocked.

## Always-on hub

The cabin embeds the hub when you start share. For a headless box:

```bash
mkdir -p ~/.config/systemd/user
cp packaging/systemd/grokhub-hub.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now grokhub-hub.service
```

## Devices (phone / other PC)

Pair code `ABC-234`, URL `http://<lan>:18766`. Android talks HTTP. Do not inhabit onto the phone.

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
| `/usr/bin/grokhub` | System / makepkg |
| `~/.config/GrokHub` | User data (`app.json`, `projects.json`, `secrets.json`, memory) |

Release tarball: `grokhub-linux-v*.tar.gz` from `./scripts/make-release-bundle.sh`.

Arch notes: [`packaging/README-ARCH.md`](packaging/README-ARCH.md).

## Uninstall

```bash
rm -f ~/.local/bin/grokhub ~/.local/bin/grokhub-hub
rm -f ~/.local/share/applications/grokhub.desktop
# optional: rm -rf ~/.config/GrokHub
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
