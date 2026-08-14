# Rust cabin — remaining Electron mechanisms

**Date:** 2026-08-14  
**Version:** 2.0.0 (do not bump)  
**Override:** Native Rust only. No Electron, no Tauri, no new TypeScript.

## Why

OAuth, slash, threads, tray, and overlay update already sit in the Rust cabin. The leftover Electron tree still had a night shift that fired, a host that streamed, chat as a product, and cabin organs. Those mechanisms come back here.

## Night shift

File-backed `automations.json`. NL parse stays (“every weekday at 9…”, heartbeat). A timer marks due rows and runs them as a chat turn, inheriting YOLO / supervised.

Script gate: skip when `checkCommand` exits non-zero or prints nothing. Quiet hours block destructive host. Daily unit budget blocks further night runs after the cap.

Cabin nav: **Night**.

## Host product

Host jobs run off the UI thread. Last stdout line is the status pill. Write commands cite a path. Screenshot hygiene drops lock / password / polkit frames before they reach the model. `/rewind` snapshots the bound project under config `rewind/` (never `$HOME` unbound, never config). Keep last 5.

Eyes: windshield objects as an overlay, not only a text dump. Live JPEG while Eyes is open (cabin eyes), still never `lastFrame` on disk.

## Chat as a product

Slash autocomplete while the composer starts with `/` or `$`. `/rename`, `/context`, `/health`, `/fix`, `/remember`, `/mode`, `/dream`, `/tools on|off`. `/host` alone is status.

History search across threads. Goal pin survives compact. Send while a turn is running is interrupt-and-redirect. Assistant markdown is rendered (headers, bold, code, lists). Failover drops max → balanced → fast on 401/429/5xx.

## Cabin organs

Mid-thought greet + `/dream` (Imagine prompt from last night’s receipts). `/room` is slug + bound project + WM host script. Passenger labels for autonomy 0–4; wheel-grab (Ctrl+Shift+Esc) halts. Presence orb in the top bar. In-memory presence ring while eyes are on — no JPEG film on disk.

`/send` enqueues a hub task when sharing. `/sync` writes a hub snapshot (chats, memory, board, skills, automations — no secrets).

GitHub `CONNECTOR_CMD` runs against the PAT. Website connectors stay status-only.

Doctor adds last host receipt and skill count.

Skills: hard-run detect, trigger match, propose / patch SKILL.md.

## Honest limits

- Duplex Voice opens `wss://api.x.ai/v1/realtime` when a bearer is present. Connect/auth/mic failure falls back to record + STT + TTS.
- Super+G / Super+Shift+Esc register through the compositor when it allows a grab. Ctrl+G and Ctrl+Shift+Esc always work in-window.
- Webcam fuse is ffmpeg v4l2 `/dev/video0` when that node exists. Presence ring stays in memory — never `lastFrame` on disk.
- `/sync` last-write-wins merges hub snapshot memory files. Not a CRDT.
- Models catalog is the focused Grok set, not the website dump.
- No Telegram, no provider zoo, no WASM, no hook YAML
- Inhabit still refuses the phone
