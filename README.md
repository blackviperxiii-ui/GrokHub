# GrokHub

Native Rust cabin for **Arch Linux / CachyOS**. No Electron. No Tauri.

**v2.6.19** — Grok-native unsandboxed control plane. You sit down in the cabin. It already knows the night.

| Platform | Repository | Latest |
|----------|------------|--------|
| **Linux** (this) | [Grok-Hub](https://github.com/blackviperxiii-ui/Grok-Hub) | **v2.6.19** |
| **Windows** | [Grok-Hub-Windows](https://github.com/blackviperxiii-ui/Grok-Hub-Windows) | sibling — same `grokhub-core` |
| **Android** | [Grok-Hub-Android](https://github.com/blackviperxiii-ui/Grok-Hub-Android) | key-fob — pair, task, JPEG |

## Run

```bash
sudo pacman -S --needed git rustup base-devel pkgconf gtk3 libxkbcommon libxkbcommon-x11 cmake meson ninja wayland wayland-protocols pixman libpng libx11 libxtst libxinerama glib2 libxmu
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

Slash: `/help` · `/new` · `/scratch` · `/clear` · `/undo` · `/retry` · `/stop` · `/sh` · `/host` · `/project` · `/memory` · `/recall` · `/forget` · `/board` · `/imagine` · `/skill` · `/compact` · `/learn reflect` · `/update` · `/send` · `/sync` · `/hub` · `/inhabit` · `/rewind` · `/room` · `/export` · `/rename` · `/pin` · `/delete` · `/mode auto|fast|balance|think|max` · `/dream` · `/palette`. Type `/help` in the cabin for the rest. `/skill <name>` runs that skill. `/compact` keeps the last 8 visible turns. `/context` counts visible turns, not host rows. `/scratch` blocks `/forget` and Memory Save. `/rewind` shows Restoring until host finishes, refuses an empty dest and `~/.ssh`, and restores only the bound project root. `/sync` merges chats and memory with paired computers (it does not replace). `/project` also takes `bind`, `new`, `folder`, `rename`, `move`, `delete`, `clear`. Right-click a sidebar project to rename or remove it — Delete drops the row, not the files.

Composer modes (mode pill on the composer, or `/mode`):

| UI | Sends |
|----|--------|
| **Auto** | Picks Fast / Balance / Think / Max from the ask. A Settings chat-model pin that is **not** a ladder default (`grok-3-mini-fast` / `grok-4.3` / `grok-4.6`) skips this. `/mode` and the mode pill do not write that pin. |
| **Fast** | `grok-3-mini-fast` |
| **Balance** | `grok-4.3` |
| **Think** | `grok-4.6` at `high` |
| **Max** | `grok-4.6` at `xhigh` (`xhigh` is 4.6 only) |

If the request returns 401 / 403 / 429 / 5xx, the cabin retries once down the ladder: Grok 4.6 → 4.3 → Fast. Fast has no further drop.

Projects sit in the left rail. `+` makes a project (`~/GrokHub-Work/<slug>`) or a one-level folder. Double-click or right-click to rename (display name only — the path stays). Right-click a project to add it to a folder or remove it. Folders are sidebar only; they do not move files. Click a project to bind it. Click the bound project again to open the Workboard. Bound tree is the world.

History tabs pin, rename, and delete (right-click, or `/pin` `/rename` `/delete`). A manual rename is locked. After each turn Fast names the tab from the first topic (max 16 characters) unless that lock is set. Scratch stays unnamed. The Chat rail opens the last-accessed thread (scratch is skipped when another thread exists). Each thread stores `accessed_ms`; sitting on Chat stamps it.

Imagine stills use dedicated **`grok-imagine-image-2.0`** (chat model is ignored; retired `grok-2-image` names are rewritten). Image 2.0 sends `quality` (`low` / `medium`) and prefers a `url` body; an empty URL still accepts `b64_json`. OAuth / older keys fall back to `grok-imagine-image`. Video kind calls **`grok-imagine-video-1.5`** (fallback `grok-imagine-video`) and saves an mp4. Media downloads send the same Bearer. The toolbox docks mid-pane, then to the floor once a prompt or still is live. Aspect / kind / quality / style chips map to the Imagine API (and still-prompt suffixes). Plus opens **Upload file / Paste clipboard**. Hey Grok: with a **console API key**, duplex Voice streams 24 kHz PCM on `wss://api.x.ai/v1/realtime?model=grok-voice-think-fast-2.0`. **OAuth** (no console key) is 4s push-to-talk STT + TTS (whisper fallback if neither OAuth nor a console key is present). Dead OAuth does not beat a live console key. Consumer OAuth does not grant the realtime socket. Eyes walks AT-SPI (`pyatspi`, installed with the cabin) then wmctrl + cursor. Capture prefers **grim** on Wayland (`grim -o` when the window already sits on another output) and skips blank frames. Clicks map JPEG pixels through xrandr outputs to global coords. A full-desktop frame stays at 0,0 — it does not inherit a single-monitor origin. Left-of-primary monitors stay on the virtual desktop. `act` picks the smallest AT-SPI name match. `COMPUTER_CMD: tab list|close|focus` talks to optional localhost CDP (`127.0.0.1:9222` / `9223`); the windshield reports `browser: cdp N tabs` or `browser: cdp down`. If CDP is down, `act` / `wait_for` / `key` stay. Everyday GUI help (`close that tab`, `turn this on`, `in Settings`, `how do I`, `for me`) attaches eyes and hands even when Cabin eyes is off. `just tell me` / `don't click` / `walk me through` attach eyes only. After each `COMPUTER_CMD`, HostDone re-arms both so the next kick gets a fresh JPEG and windshield. GUI-help turns show a Hands chip and a how-to; raw `COMPUTER_RESULT` stays off the pane. Hung desktop tools (grim, ffmpeg, AT-SPI, systemctl, clipboard, whisper) time out. GitHub search/error pages and CDP `/json/list` stop at a body cap. Chat JSON and SSE stop at `MEDIA_FILE_CAP`. Stream buffers and bound host/consult receipts are capped. Chat-complete text is clipped before strip/merge so a huge worker body cannot freeze the pane. Live thought+stream merge stays inside `IMAGE_FILE_CAP`; leftover deltas after the cap skip upsert. Chat bubble layout clips paint to `TEXT_FILE_CAP` so an 8MB stream cannot freeze the pane. Stream deltas refresh only the trailing chat-view stretch from borrowed refs; ChatView bodies clip to `TEXT_FILE_CAP`. Eyes last-frame URLs drop above `FRAME_CAP` so grim cannot keep an 8MB data URL. OAuth discovery/token JSON stops at `TEXT_FILE_CAP`; a pixel-bomb Settings avatar is rejected before decode. Desk and webcam JPEGs reject huge pixel counts before decode. Live presence drops a huge Eyes JPEG instead of decoding it. Host stdout and pipes are capped, including a newline-free dump; the live presence ring keeps 32 frames. Hub HTTP drops the lock before I/O so persist cannot freeze the cabin. Attaches and wall stills reject files over 8MB and huge-pixel images. Older `HOST_RESULT` / `COMPUTER_RESULT` / `CONNECTOR_RESULT` dumps shrink to head+tail when context is over 50% budget (last 4 hops and any `GOAL PIN` stay). Each HostDone appends a redacted line to `trajectory.jsonl`; nightly review can emit `SUGGEST_SKILL_PATCH` for an existing skill. Hands use **ydotool** on Wayland (`ydotoold` + uinput) and **xdotool** on X11. `./scripts/install.sh --user` builds `ydotool` `grim` `xdotool` `wmctrl` into `~/.local/lib/grokhub/bin` (AUR the same under `/usr/lib/grokhub/bin`; pointer tools are optdepends) and installs `python-atspi`, writes a user `ydotoold` unit, installs the uinput udev rule, and offers the `input` group. Lookup walks that sidecar prefix plus `PATH` / `~/.local/bin` (no `which`) and starts `ydotoold` when the socket is dead. Receipts distinguish not installed vs uinput vs daemon. A short Responses `output_text.done` does not wipe a longer streamed `COMPUTER_CMD`. Unknown ydotool keys fail closed. **Take over** / “take control” attaches a frame plus the accessibility windshield; Eyes **Install hands** retries the daemon (no silent sudo). The status bar shows the real reason if the desk is down. The model must not pkill as a stand-in for the mouse. After a hands run the cabin saves `~/.config/GrokHub/recipes/last.json` and a skill; Eyes **Replay** or a night job `replay last` runs it again. Halt / Stop / Ctrl+Shift+Esc kill a running job. Halt is a failed host receipt. Lock/password screens are skipped. Titlebar × unmaps to tray; a pinned taskbar click (or a second `grokhub`) raises the running cabin.

Settings → **Connect Grok OAuth** (or `grokhub --oauth`) is the sign-in. Device-code against `auth.x.ai` — same public client as Grok CLI. The footer paints the Grok profile photo when the session has one. A console API key is the fallback for chat and the only path for duplex Voice. Tokens live in `~/.config/GrokHub/secrets.json` (mode 0600), never in markdown. Settings → Appearance is **Dark**, **Light**, or **System**.

Settings → **Update** (or `grokhub --update` / `/update`) does `git pull --ff-only origin main` in the source clone, then `./scripts/install.sh --user`. The clone must be on `main` with an `origin`. Overlay only — config stays. User install enables `grokhub.service` (no `--now`), `enable --now` hub, rebuilds sidecars into `~/.local/lib/grokhub/bin` (skip only when that prefix already has the file), and restarts `ydotoold`. Overlay-safe `pacman` / `apt-get` / `dnf` for build tools, `python-atspi`, ffmpeg, alsa-utils. Progress stays on Settings (bar + percent). After a clean overlay, **Restart** reloads live sidecar units (`ydotoold` → hub), drops the cabin pid lock, starts a new overlay `grokhub`, and exits this process. It does not `systemctl restart grokhub.service` from inside the cabin.

The cabin drives. Host plans run without a confirm. A saved desktop recipe whose `screen=` does not match the current desktop reshoots and skips coordinate clicks. Night `replay last` runs the last GUI recipe without a chat hop. A 15s heartbeat always runs housekeep, inbox, night, review, wall, mid-thought, reflect, and anticipate. Hidden idle cabins wait for that pulse instead of spinning every 400ms. Review defers if Night just fired or chat is running. Last-night context folds into the empty-chat greeting — no fake “You sit down” turn. Quiet MidThought can fold `Continue {title}` into that greeting when there is no last-night receipt. After 21:00 a quiet Balanced review writes learned tiles to `suggestions.json` (Suggested Automations / Skills / Connectors show Reviewed today / Review due tonight). Yesterday's redacted host/hands lines in `trajectory.jsonl` can become a `SUGGEST_SKILL_PATCH` on an existing skill. Idle ≥ 10 minutes (or `/learn reflect`) does a surgical `MEMORY.md` edit with a diff and `.prev` restore. Halt / Stop / Ctrl+Shift+Esc kill a running job. Host follow-up stays on the origin thread. A truncated stream, a promised-work reply with no `HOST_CMD`, or a diagnostic that hands `sudo apt` / “not found” back to the user can quiet-continue up to four times (`FOLLOWUP:`). Follow-up scores only visible assistant prose — thinking and empty replies do not start another turn. An empty `goal_pin` falls back to the last real user task; goal continue stays on the origin thread and does not `send_chat` while host is live. Night usage shares the daily cap. Night checks parse the receipt `exit N` line; a forbidden check is skipped. Phone dispatch completes on halt / error and persists that completion so a claimed inbox row does not stay claimed forever; `GOAL_BLOCKED` is failed, not done; boot requeues leftover claimed rows and will not claim a second inbox row while one is pending. Ack of a finished inbox row does not hide the result. Host context (goal steps, consult, compact, reflect) stays on the origin thread. `/rewind` restores only the bound project root.

Android / Windows: link `libgrokhub_ffi` and include `crates/grokhub-ffi/include/grokhub.h`.

| Binary | Crate | Job |
|--------|-------|-----|
| `grokhub` | `crates/grokhub-app` | Cabin — chat, board, imagine, skills, eyes, host |
| `grokhub-hub` | `crates/grokhub-hub` | Standalone LAN `/v1` hub (port **18766**) |
| `libgrokhub_ffi` | `crates/grokhub-ffi` | C ABI for Android / Windows |

Config and memory: `~/.config/GrokHub` (`app.json`, `projects.json`, `suggestions.json`, `secrets.json` mode 0600, `memory/SOUL.md`, `USER.md`, `MEMORY.md`).

## First run

1. Land in **chat**. Banner: **Connect Grok in Settings**.
2. Settings → **Connect Grok OAuth** (or paste a console key from [console.x.ai](https://console.x.ai)). Save.
3. Optional: Devices → **Start share** for the Android key-fob.
4. The cabin drives — host, skills, learning, and anticipation run at full autonomy. Halt stops a running job.

Tokens stay in `secrets.json`. Never in markdown.

Composer is a pill: **What do you want to know?** Five quick chips sit centered over the bar — no selected first pill. An empty chat can show a faint Fast blurb. Plus opens Upload / Paste. The mode pill is Auto / Fast / Balance / Think / Max. `/mode` only sets the pill — it does not overwrite Settings → Chat model. Leftover pages (Eyes, Devices, Memory, History, Night, Workboard, Command) use the same catalog chrome: framed cards, pills, empty states. Devices paints tiles plus a real LAN pair URL. Command is a full-width field. Mic is Hey Grok. Enter or the arrow sends; Ctrl+Enter starts a new line. While a reply (or Imagine / host job) is running, Send is Stop. Chat streams tokens onto the thread that started the job; opening New chat does not steal the live reply. User and assistant turns sit in bubbles that hug the text and wrap with the chat pane. Long lines stay in the window. User bubbles sit on the right. Chat shows each thought as its own bubble and the final reply; host hops stay off the pane. Visible messages have **Copy** and **Reply** (Reply quotes into the composer). Host, hands, and connector work stay off the pane — including `HOST_CMD` heredoc bodies. The model still sees them. Thought does not announce that an image is attached. Imagine stills sit above the composer. Grok may emit `HOST_CMD:` lines (unsandboxed `bash -lc`) and `COMPUTER_CMD:` lines (mouse, keyboard, `act` / `wait_for` via ydotool or xdotool). Host runs immediately — the cabin drives. `/stop`, tray **Halt hands**, the Stop square, and Ctrl+Shift+Esc actually kill those workers. When you ask it to take over or drive the UI, a JPEG frame and the windshield object list are attached. Clicks map JPEG pixels through xrandr outputs to global coords (`grim -o` on the monitor that already has the window); a full-desktop frame stays at 0,0. `act` picks the smallest AT-SPI name match. `tab list|close|focus` uses localhost CDP when it is up. GUI help wakes eyes and hands; `just tell me` looks only. A leftover cabin frame is not attached on ordinary chat. Lock/password screens are skipped. `/rewind` refuses `~/.ssh`, `~/.gnupg`, `~/.aws`, `~/.kube`, and `~/.config/GrokHub`. Forbidden host paths stay blocked. Host hour cap still applies. `app.json` is mode 0600 like `secrets.json`.

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
| `~/.local/lib/grokhub/bin` | Sidecar ydotool / ydotoold / grim / xdotool / wmctrl |
| `/usr/bin/grokhub` | System / makepkg |
| `/usr/lib/grokhub/bin` | System sidecar hands |
| `~/.config/GrokHub` | User data (`app.json`, `projects.json`, `secrets.json`, memory) |

Release tarball: `grokhub-linux-v*.tar.gz` from `./scripts/make-release-bundle.sh`.

Arch notes: [`packaging/README-ARCH.md`](packaging/README-ARCH.md).

## Uninstall

```bash
rm -f ~/.local/bin/grokhub ~/.local/bin/grokhub-hub
rm -rf ~/.local/lib/grokhub
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
