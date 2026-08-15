# GrokHub

Native Rust cabin for **Arch Linux / CachyOS**. No Electron. No Tauri.

**v2.0.0** — Grok-native unsandboxed control plane. You sit down in the cabin. It already knows the night.

| Platform | Repository | Latest |
|----------|------------|--------|
| **Linux** (this) | [Grok-Hub](https://github.com/blackviperxiii-ui/Grok-Hub) | **v2.0.0** |
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

Close on the window hides the cabin to the tray — the window unmaps; it does not minimize to the taskbar. Jobs, hub, and idle reflect keep running. Tray: **Show cabin**, **Halt hands**, **Quit**. `grokhub --agent` starts already hidden. `GROKHUB_TRAY=0` quits on close.

Slash: `/help` · `/new` · `/scratch` · `/clear` · `/undo` · `/retry` · `/stop` · `/sh` · `/host` · `/project` · `/approve` · `/memory` · `/recall` · `/forget` · `/board` · `/imagine` · `/skill` · `/compact` · `/learn reflect` · `/update` · `/send` · `/sync` · `/hub` · `/inhabit` · `/rewind` · `/room` · `/export` · `/mode auto|fast|balance|think|max` · `/dream` · `/palette`. Type `/help` in the cabin for the rest. `/project` also takes `bind`, `new`, `folder`, `rename`, `move`, `clear`.

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

Imagine always uses dedicated **`grok-2-image`** (chat model is ignored). Hey Grok records 4s (`arecord` / `ffmpeg` / `sox`), transcribes with xAI STT when a key is present (whisper fallback), and speaks the reply via xAI TTS. Eyes walks AT-SPI (`pyatspi`) then wmctrl + cursor. With Cabin eyes on, a JPEG is captured on each chat send, stored on the hub (not disk), and attached to that turn.

Settings → **Connect Grok OAuth** (or `grokhub --oauth`) is the sign-in. Device-code against `auth.x.ai` — same public client as Grok CLI. A console API key is optional. Tokens live in `~/.config/GrokHub/secrets.json` (mode 0600), never in markdown.

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

Composer is a pill: **What do you want to know?** Plus pastes the clipboard. Mode combo is Auto / Fast / Balance / Think / Max. `/mode` only sets the combo — it does not overwrite Settings → Chat model. Mic is Hey Grok. Ctrl+Enter or the arrow sends; Enter is a newline. Grok may emit `HOST_CMD:` lines. The cabin confirms unless YOLO, then runs `bash -lc`.

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
