# Android dispatch consume API (Linux hub)

The Android app is a sibling repo. This is the live HTTP contract on GrokHub 2.0 (`crates/grokhub-hub`). No Android code lives here.

Base URL: `http://<lan-ip>:18766` (default port). Kind: `grokhub-hub-v1`.
Android 9+ must allow **cleartext HTTP** for that LAN origin.

## Auth

| Route | Auth |
|---|---|
| `GET /v1/health` or `GET /health` | none |
| `POST /v1/pair` | none (pairing code) |
| `OPTIONS *` | none |
| everything else | `Authorization: Bearer <token>` |

401: `{ "ok": false, "error": "Pair this computer first (Settings → Devices)." }`

CORS: `authorization, content-type` · `GET,POST,PUT,OPTIONS`. Max JSON body 8 MB.

## Pair

1. Linux: Settings → Devices → start share. Code looks like `ABC-234` (15 min, one-shot). Alphabet omits `I O 0 1`. Normalize: uppercase, strip non-alphanumerics.
2. Android: persist a stable `deviceId`, then:

`POST /v1/pair`

```json
{ "code": "ABC-234", "deviceId": "<stable-android-id>", "deviceName": "Pixel" }
```

200:

```json
{
  "ok": true,
  "token": "<hex>",
  "deviceId": "<your id>",
  "hub": { "id": "<linux-device-id>", "name": "cachyos" }
}
```

400 = no/expired code. 403 = wrong code. Re-pair with the same `deviceId` rotates the token.

Then `GET /v1/status` (Bearer) to confirm `hub` / `you` / `peers`.

`GET /v1/health` → `{ ok, kind: "grokhub-hub-v1", name }` for discovery.

## Send a task home

`POST /v1/task` (Bearer)

```json
{
  "targetDeviceId": "<hub.id from pair>",
  "title": "Flash the pi",
  "prompt": "flash the pi with last week's skill"
}
```

`prompt` required, max 16k. `title` optional, max 120.

200: `{ "ok": true, "task": { "id": "task-<hex>", "targetDeviceId": "…" } }`

Linux claims the inbox, runs the prompt on the bound project / supervised policy, then completes the task. The phone is the **sender**. Do not call `GET /v1/inbox` or `POST /v1/inbox/:id/ack` (those are for the worker box).

## Poll

Non-destructive (keep your own UI state):

`GET /v1/task/:id` (Bearer) → `{ ok, task }` if you are `fromId` or `targetDeviceId`. 404 otherwise.

Destructive (each result once — persist immediately):

`GET /v1/results` (Bearer) → `{ ok, tasks: HubTask[] }` for your `fromId` tasks in `done` or `failed` that are not yet claimed.

Completed task shape:

```json
{
  "id": "task-…",
  "fromId": "<android deviceId>",
  "fromName": "Pixel",
  "targetDeviceId": "<linux>",
  "title": "Flash the pi",
  "prompt": "…",
  "status": "done",
  "createdAt": 123,
  "result": "flash ok · exit 0",
  "receipts": [
    { "cmd": "./flash.sh", "risk": "moderate", "code": 0, "ms": 4200 }
  ]
}
```

Linux writes `status: "failed"` when the job is blocked / needs the user; otherwise `done`. Failures also appear in `result` text. `skillId` may be absent.

Show `result` + last receipt. Do not open a second chat channel.

## Live frames (Presence)

Paired remotes poll while a job is outstanding. Same Bearer as `/v1/task`.

- `GET /v1/frame` → `{ ok, frame: { dataUrl, at } | null }`
- `GET /v1/frame.jpg` → raw image bytes (`content-type` from the data URL). `404` when empty. Pass `?since=<at>` for `304` when unchanged.
- Header `x-grokhub-frame-at` is the frame timestamp. Poll every 400ms while a job is live; stop when idle.

Linux pushes frames with `POST /v1/frame` `{ dataUrl }` (memory-only on the hub, not written to `hub-state.json`). Do not `POST /v1/frame` from the phone. Do not open a second chat channel.

## Do not

- Do not send API keys or Grok OAuth through the hub.
- Do not `POST` / `GET /v1/inhabit` — inhabit is Linux-box-to-Linux-box; `GET` consumes the bundle. The phone is the key fob.
- Do not implement Telegram/Discord or a second agent chat.
- Snapshot `GET/PUT /v1/snapshot` is desktop chat/memory merge, not required for Dispatch.
