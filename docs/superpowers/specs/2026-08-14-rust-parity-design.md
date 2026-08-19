# GrokHub native Rust — product, not sidecar

**Date:** 2026-08-14  
**Version:** 2.6.32  
**Decision:** The shipping Linux / Windows / Android product is Rust. No Electron. No Tauri.

## Why

Electron is a Chromium tax. Tauri is still a webview wrapping the same TypeScript cabin. Jeremy asked for a full shift to Rust so Linux, Windows, and Android share one core and one cabin, not a JS UI with a Rust sidecar.

## What ships

| Binary | Crate | Job |
|---|---|---|
| `grokhub` | `grokhub-app` | Native cabin (egui). Chat, devices, memory, settings, host. Embeds the hub. |
| `grokhub-hub` | `grokhub-hub` | Same `/v1` LAN hub as a standalone process (Android / second box). |

Shared crate: `grokhub-core` — pair codes, hub state, frames, inhabit, redaction, chat/xAI helpers.

## Forbidden

- Electron as the product launch path
- Tauri wrapping `src/`
- New TypeScript cabin features
- Provider zoo, WASM, hook YAML, Telegram/Discord
- xAI Grok OAuth is required (device-code). See `2026-08-14-cabin-oauth-parity-design.md`.
- Secrets in markdown
- `lastFrame` on disk
- Inhabit onto the phone

## Cabin (native)

First run stays in chat. Banner: **Connect Grok in Settings**. Settings holds the xAI API key, device name, model. Autonomy is locked at maximum. `/approve` is gone. Host plans run; Halt stops a job.

Composer placeholder: **What do you want to know?** Mode pill: Auto / Fast / Balance / Think / Max. Auto routes from the ask (`grok-3-mini-fast` / `grok-4.3` / `grok-4.6` `high` / `grok-4.6` `xhigh`). A Settings chat-model pin skips Auto only when it is not a ladder default — `/mode` does not write that pin. Fast pins mini. Balance is Grok 4.3. Think is Grok 4.6 at high. Max is Grok 4.6 at xhigh. Failover on 401/403/429/5xx: 4.6 → 4.3 → Fast. `/mode auto|fast|balance|think|max`.

Grok may emit `HOST_CMD:` lines. The cabin runs `bash -lc` and sends stdout back. Host hour cap and forbidden paths still apply. Destructive night jobs skip and mark ran so they do not retry every pulse.

Memory files live under `~/.config/GrokHub/memory/` (`SOUL.md`, `USER.md`, `MEMORY.md`). Config: `~/.config/GrokHub/app.json`. Project tree: `~/.config/GrokHub/projects.json`. Tokens: `~/.config/GrokHub/secrets.json` (mode 0600).

Projects sit in the left rail. `+` creates a project under `~/GrokHub-Work/<slug>` or a one-level sidebar folder. Rename is the display name; the path stays. Right-click rename or delete (delete drops the sidebar row, not the files). Folders do not move files. Click a project to bind it. Bound tree is the world. `/project bind|new|folder|rename|move|delete|clear`.

History tabs: pin, rename (locks the title), delete. Fast names the tab from the first topic (max 16 characters) unless locked. `/pin` `/rename` `/delete`.

Plus is Upload / Paste. Five chips sit centered over the composer with no selected first pill. Enter sends; Ctrl+Enter is a newline. Send becomes Stop while a job runs. Empty chats can show a faint Fast blurb. Chat streams Responses SSE tokens onto the thread that started the job. User and assistant turns hug the text in rounded bubbles and wrap with the chat pane (~84% of the measured CentralPanel, no pixel lock). Long lines stay in the window. User bubbles sit on the right. Chat shows each thought as its own bubble and the final reply; host hops stay off the pane. Visible messages have Copy and Reply (Reply quotes into the composer). Imagine toolbox docks mid-pane, then the floor; stills sit above the composer. Appearance is Dark, Light, or System. OAuth paints the Grok profile photo in the footer and covers STT/TTS; duplex Voice streams 24 kHz PCM with a console key. Settings → Update shows a percent bar on the Settings page. Overlay install enables `grokhub.service` (no `--now`), `enable --now` hub, rebuilds sidecars (skip only when `$PREFIX/lib/grokhub/bin` already has the file), and restarts `ydotoold`. Overlay-safe pacman / apt-get / dnf for build tools, python-atspi, ffmpeg, alsa-utils. **Restart** reloads live sidecar units (`ydotoold` → hub), drops the cabin pid lock, starts a new overlay `grokhub`, and exits this process. It does not `systemctl restart grokhub.service` from inside the cabin. Chat shows each thought as its own bubble and the final reply; Thought does not announce an attach; host, hands, and connector work (including `HOST_CMD` heredocs) stay off the pane. Clicks map JPEG pixels through xrandr outputs to global coords (`grim -o` on the monitor that already has the window); a full-desktop frame stays at 0,0 and does not inherit a single-monitor origin; left-of-primary monitors stay on the virtual desktop. `act` picks the smallest AT-SPI name match. `COMPUTER_CMD: tab list|close|focus` talks to optional localhost CDP; windshield reports `browser: cdp`. Everyday GUI help wakes eyes and hands even when Cabin eyes is off; `just tell me` is eyes only. After each `COMPUTER_CMD` hop both re-arm. GUI-help turns show a Hands chip and a how-to. Hung desktop tools time out; hub I/O drops the lock before HTTP. GitHub and CDP HTTP bodies are capped. Chat JSON and SSE stop at `MEDIA_FILE_CAP`. OAuth JSON is capped; Settings avatars reject huge pixel counts before decode. Desk JPEGs do the same. Mid-job result dumps trim past 50% budget. Nightly review can `SUGGEST_SKILL_PATCH` an existing skill from `trajectory.jsonl`. The model still sees them. `COMPUTER_CMD` drives mouse/keyboard/vision unsandboxed via ydotool (Wayland) or xdotool (X11). `install.sh` builds `ydotool` `grim` `xdotool` `wmctrl` into `~/.local/lib/grokhub/bin` (AUR `/usr/lib/grokhub/bin`; pointer tools are optdepends) and installs `python-atspi` as a hard depend so Eyes can import `pyatspi`. A user `ydotoold` unit ships with the cabin. Hands lookup walks that sidecar prefix plus PATH / `~/.local/bin` without spawning `which`, starts `ydotoold` when the socket is dead, and receipts distinguish missing / uinput / daemon. A short Responses `output_text.done` does not wipe a longer streamed `COMPUTER_CMD`. Unknown ydotool keys fail closed. Take over / “take control” attaches a grim JPEG plus the AT-SPI windshield; Eyes **Install hands** retries the daemon. The model must not pkill as a stand-in for hands. A hands run saves `recipes/last.json`. `/stop` / tray Halt / Ctrl+Shift+Esc flip `host_halt` so those workers actually die. Halt is a failed host receipt. Window size and position persist. The tray icon is registered from launch. Titlebar × unmaps the cabin (X11; Wayland cannot hide). A pinned taskbar click or a second `grokhub` (`cabin.raise`) raises the running cabin. Tray pings once on hide. A 15s heartbeat runs every organ, including a 21:00 Balanced review that writes `suggestions.json`. Review defers if Night just fired or chat is running. Hidden idle waits for the pulse. Last-night context folds into the empty-chat greeting. The Chat rail opens the last-accessed thread (`accessed_ms`; scratch skipped when another thread exists). Quiet MidThought can fold `Continue {title}` into that greeting when there is no last-night receipt. Imagine stills use `grok-imagine-image-2.0` (fallback `grok-imagine-image`, URL then b64, Bearer download); video kind calls `grok-imagine-video-1.5` (fallback `grok-imagine-video`). A truncated stream, a promised-work reply with no `HOST_CMD`, or a diagnostic that hands apt / “not found” back to the user can quiet-continue up to four times. Follow-up scores only visible assistant prose — thinking and empty replies do not start another turn. An empty `goal_pin` falls back to the last real user task; goal continue stays on the origin thread and does not `send_chat` while host is live. `/rewind` shows Restoring until host finishes, refuses an empty dest and secret dirs, and restores only the bound project root. Empty Auto still routes. Host follow-up stays on the origin thread. Dead OAuth does not beat a live console key. Night checks parse the receipt `exit N` line. Phone dispatch completes on halt / error and persists that completion so a claimed inbox row does not stay claimed forever; `GOAL_BLOCKED` is failed, not done; boot requeues leftover claimed rows and will not claim a second inbox row while one is pending. Host context (goal steps, consult, compact, reflect) stays on the origin thread. `/compact` keeps the last 8 visible turns; `/context` counts visible turns, not host rows. `/skill <name>` runs the skill. Scratch blocks `/forget` and Memory Save. `/sync` merges chats and memory (it does not replace). Night usage shares the daily cap.

Devices pane starts the in-process hub on `GROKHUB_HUB_PORT` or `18766` and shows catalog tiles plus a real LAN IPv4. Expired pair codes hide and rotate; New / rotated codes persist. The pair tile hides when the hub is not sharing. Hub `complete` requires the owning peer. Command uses the same chrome with a full-width field.

## Hub contract

Unchanged. See `docs/superpowers/plans/2026-08-14-dispatch-android-notes.md`.

## In this repo (Rust)

Cabin panes: Chat, Devices, Memory, Board, Imagine, Skills, Eyes, Settings. Left rail Chat sits above Imagine and opens the last-accessed thread. Left rail also holds the project tree.
Core: pair, hub, slash, host rails, workboard, project tree, SKILL.md, dedicated Imagine, windshield, Hey Grok (xAI STT + TTS, whisper fallback), persist, 15s heartbeat, always-on autonomy, saved desktop recipes, nightly learned suggestions. Cabin eyes stay dormant until asked, hands need a frame, or GUI help (`close that tab` / `turn this on`); `just tell me` looks and does not click. Capture prefers Wayland-native tools and skips blank frames.
C ABI: `crates/grokhub-ffi` + `include/grokhub.h` — pair, port, dedicated imagine/voice models, forbidden host, slash kind.

## Sibling repos

- Android links `libgrokhub_ffi` (or UniFFI later)
- Windows builds the same `grokhub` binary
