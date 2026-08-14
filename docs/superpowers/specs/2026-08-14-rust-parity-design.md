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

Composer placeholder: **Message Grok**.

Grok may emit `HOST_CMD:` lines. The cabin confirms unless YOLO, then runs `bash -lc` and sends stdout back.

Memory files live under `~/.config/GrokHub/memory/` (`SOUL.md`, `USER.md`, `MEMORY.md`). Config: `~/.config/GrokHub/app.json`.

Devices pane starts the in-process hub on `GROKHUB_HUB_PORT` or `18766` and shows the pair code.

## Hub contract

Unchanged. See `docs/superpowers/plans/2026-08-14-dispatch-android-notes.md`.

## In this repo (Rust)

Cabin panes: Chat, Devices, Memory, Board, Imagine, Skills, Eyes, Settings.
Core: pair, hub, slash, host rails, workboard, SKILL.md, dedicated Imagine, windshield, Hey Grok (xAI STT + TTS, whisper fallback), persist. Cabin eyes captures on each chat send.
C ABI: `crates/grokhub-ffi` + `include/grokhub.h` — pair, port, dedicated imagine/voice models, forbidden host, slash kind.

## Sibling repos

- Android links `libgrokhub_ffi` (or UniFFI later)
- Windows builds the same `grokhub` binary
