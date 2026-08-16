# GrokHub native Rust — product, not sidecar

**Date:** 2026-08-14  
**Version:** 2.5.0  
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

Plus is Upload / Paste. Five chips sit centered over the composer with no selected first pill. Enter sends; Ctrl+Enter is a newline. Send becomes Stop while a job runs. Empty chats can show a faint Fast blurb. Chat streams Responses SSE tokens onto the thread that started the job. User and assistant turns hug the text in rounded bubbles and wrap in a tight column (hard cap 440px). Imagine toolbox docks mid-pane, then the floor; stills sit above the composer. Appearance is Dark, Light, or System. OAuth paints the Grok profile photo in the footer and covers STT/TTS; duplex Voice streams 24 kHz PCM with a console key. Settings → Update shows a percent bar on the Settings page; **Restart** reloads the new binary after a clean overlay. Chat shows thought / tool steps; Thought does not announce an attach; host receipts stay off the bubble and on the origin thread. `COMPUTER_CMD` drives mouse/keyboard/vision unsandboxed via ydotool (Wayland) or xdotool (X11). Take over attaches a grim JPEG plus the AT-SPI windshield; a hands run saves `recipes/last.json`. `/stop` / tray Halt / Ctrl+Shift+Esc flip `host_halt` so those workers actually die. Window size and position persist. The tray icon is registered from launch. Titlebar × unmaps the cabin (X11; Wayland cannot hide). Tray pings once on hide. A 15s heartbeat runs every organ, including a 21:00 Balanced review that writes `suggestions.json`. Review defers if Night just fired or chat is running. Hidden idle waits for the pulse. Last-night context folds into the empty-chat greeting. The Chat rail opens the last-accessed thread (`accessed_ms`; scratch skipped when another thread exists). Quiet MidThought can fold `Continue {title}` into that greeting when there is no last-night receipt. Imagine stills use `grok-imagine-image-2.0`; video kind calls `grok-imagine-video-1.5`. `/rewind` refuses secret dirs and restores only the bound project root. Empty Auto still routes. Host follow-up stays on the origin thread. Night usage shares the daily cap.

Devices pane starts the in-process hub on `GROKHUB_HUB_PORT` or `18766` and shows catalog tiles plus a real LAN IPv4. Hub `complete` requires the owning peer. Command uses the same chrome with a full-width field.

## Hub contract

Unchanged. See `docs/superpowers/plans/2026-08-14-dispatch-android-notes.md`.

## In this repo (Rust)

Cabin panes: Chat, Devices, Memory, Board, Imagine, Skills, Eyes, Settings. Left rail Chat sits above Imagine and opens the last-accessed thread. Left rail also holds the project tree.
Core: pair, hub, slash, host rails, workboard, project tree, SKILL.md, dedicated Imagine, windshield, Hey Grok (xAI STT + TTS, whisper fallback), persist, 15s heartbeat, always-on autonomy, saved desktop recipes, nightly learned suggestions. Cabin eyes stay dormant until asked (or hands need a frame); capture prefers Wayland-native tools and skips blank frames.
C ABI: `crates/grokhub-ffi` + `include/grokhub.h` — pair, port, dedicated imagine/voice models, forbidden host, slash kind.

## Sibling repos

- Android links `libgrokhub_ffi` (or UniFFI later)
- Windows builds the same `grokhub` binary
