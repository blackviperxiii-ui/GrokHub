# GrokHub native Rust — product, not sidecar

**Date:** 2026-08-14  
**Version:** 2.0.0 (do not bump)  
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

First run stays in chat. Banner: **Connect Grok in Settings**. Settings holds the xAI API key, YOLO (`/approve off`), device name, model.

Composer placeholder: **What do you want to know?** Mode combo: Auto / Fast / Balance / Think / Max. Auto routes from the ask (`grok-3-mini-fast` / `grok-4.3` / `grok-4.6` `high` / `grok-4.6` `xhigh`). A Settings chat-model pin skips Auto only when it is not a ladder default — `/mode` does not write that pin. Fast pins mini. Balance is Grok 4.3. Think is Grok 4.6 at high. Max is Grok 4.6 at xhigh. Failover on 401/403/429/5xx: 4.6 → 4.3 → Fast. `/mode auto|fast|balance|think|max`.

Grok may emit `HOST_CMD:` lines. The cabin confirms unless YOLO, then runs `bash -lc` and sends stdout back.

Memory files live under `~/.config/GrokHub/memory/` (`SOUL.md`, `USER.md`, `MEMORY.md`). Config: `~/.config/GrokHub/app.json`. Project tree: `~/.config/GrokHub/projects.json`. Tokens: `~/.config/GrokHub/secrets.json` (mode 0600).

Projects sit in the left rail. `+` creates a project under `~/GrokHub-Work/<slug>` or a one-level sidebar folder. Rename is the display name; the path stays. Folders do not move files. Click a project to bind it. Bound tree is the world. `/project bind|new|folder|rename|move|clear`.

Devices pane starts the in-process hub on `GROKHUB_HUB_PORT` or `18766` and shows the pair code.

## Hub contract

Unchanged. See `docs/superpowers/plans/2026-08-14-dispatch-android-notes.md`.

## In this repo (Rust)

Cabin panes: Chat, Devices, Memory, Board, Imagine, Skills, Eyes, Settings. Left rail also holds the project tree.
Core: pair, hub, slash, host rails, workboard, project tree, SKILL.md, dedicated Imagine, windshield, Hey Grok (xAI STT + TTS, whisper fallback), persist. Cabin eyes captures on each chat send.
C ABI: `crates/grokhub-ffi` + `include/grokhub.h` — pair, port, dedicated imagine/voice models, forbidden host, slash kind.

## Sibling repos

- Android links `libgrokhub_ffi` (or UniFFI later)
- Windows builds the same `grokhub` binary
