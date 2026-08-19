# Rust cabin — Grok OAuth + Electron parity

**Date:** 2026-08-14  
**Version:** 2.6.32  
**Override:** Grok OAuth is required. The old “no OAuth” line in the Rust-parity spec is void.

## Why

The Electron cabin signed in with xAI device-code OAuth (same public client as Grok CLI). SuperGrok / X Premium+ get an API access token without pasting a console key. The Rust cabin only had a key field. That is not enough.

The Electron sit-down also had threads, slash coverage, bound project, connectors, automations, host rails, and a desktop shell. Those come back as Rust modules — not Electron, not Tauri.

## OAuth (non-negotiable)

Device-code against `https://auth.x.ai` (OIDC discovery). Public client id stays the Electron/Grok CLI id. Scopes: `openid profile email offline_access grok-cli:access api:access`.

Flow: Settings → **Connect Grok OAuth** (or `grokhub --oauth`) → show user code → open `verification_uri` → poll token endpoint → store tokens.

Bearer for `api.x.ai` is: console API key if set, else OAuth access token. Refresh 30 minutes before expiry. Tokens live in `~/.config/GrokHub/secrets.json` mode `0600`. Never in markdown. The cabin footer paints the Grok profile photo when the session has one.

Trusted hosts only: `x.ai` and `*.x.ai`. Connector fetches: `grok.com`, `x.ai`, `api.x.ai`, plus user extras.

`CONNECTOR_CMD:` lines parse the same as Electron. GitHub runs when a PAT is in secrets. Website-linked connectors report status only.

## Cabin parity that ships in this pass

Slash: `/help` `/new` `/scratch` `/clear` `/undo` `/retry` `/stop` `/sh` `/host` `/project` `/send` `/sync` `/hub` `/inhabit` `/rewind` `/room` `/export` `/forget <topic>` `/approve risky|all` plus the ones already wired.

Threads: named chats, scratch (no memory write), persist under config.

Bound project: Settings + `/project bind`. Sidebar tree: create, rename, one-level folders (`/project new|folder|rename|move`). Folders do not move files. Persist `projects.json`. Host paths outside the tree need the same confirm as destructive. Unbound = full desktop.

Host: `/sh`, host on/off, hourly cap, `notify-send` when a job >30s finishes, `systemd-inhibit` while a host job runs.

Automations: parse “every weekday at 9…” / heartbeat; file-backed list; inherit YOLO/supervised.

Doctor / banner: auth = key **or** OAuth.

## Honest limits

- Duplex Voice still needs a console API key (OAuth covers STT/TTS). With a key, the cabin streams 24 kHz PCM and live captions on the realtime socket.
- Chat streams via `POST /v1/responses` (`store: false`). Tokens stay on the thread that started the job.
- Tray pings are one-shot on hide (`notify-send`); titlebar × pins a real `unix:path=` session bus so the icon appears
- Inhabit still refuses the phone
- Website connectors stay status-only unless a local token exists
- No Telegram, no provider zoo, no WASM, no hook YAML
- Hands are unsandboxed (`COMPUTER_CMD` / xdotool / grim). Halt sets `host_halt` so the worker actually stops.
