# Spike-4a trust floor — 2026-10-07

First slice of Spike-4 (privacy and consent). Every check goes through `harness::decide`; there is no new executor. No version bump, no new dependency, no new network call.

## Split

4a (this slice): `ConsentLedger`, the scope framework with every scope off, `guard_egress` with `egress.jsonl`, span `origin` / `consent_ref`, the `/privacy` view, and the hub sync gate. The single Settings → Permissions row for the hub grant ships here too, because grants are click-only: slash text can come from model-written chips, Pulse or queued turns, so `/privacy` cannot be the place that grants. Without the row, `/sync` could only ever be approved once per run.

4b: AEAD at rest for the learned tier (key in the OS keyring, fail closed), PII redaction of recall packs, and the per-scope Settings rows. Those need a keyring dependency decision and a recall-pack format pass, and nothing in 4a waits on them.

## ConsentLedger

`{config}/consent.jsonl`, append-only, mode 0600, every line through `redact_secrets`. A `Grant` names a destination and data classes, or a scope. Only `by: "user"` lines count, and they are written only by `grant_destination` / `grant_scope`, which take a `UserClick` minted by the Settings click (`only_a_settings_click_writes_a_grant`). Revoke appends a copy with `revoked_at`; the last line per id wins. A file over 1 MiB reads as empty, which denies. The hard floor refuses agent writes and shell commands that touch `consent.jsonl`, and headless `grok -p` gets `Bash(*consent.jsonl*)` (47 deny rules).

## Scopes

Files in one folder, installed apps, browser history, calendar, mail, system state. All off. `Step::Scope` refuses until a grant exists; the home root, relative paths and hard-excluded paths are refused even with a click. Screen stays the existing desktop switch. Nothing reads a scope yet.

## EgressGuard

`Step::Egress { dest, data, ledger }`, in order: loopback allows; no personal data allows; an xAI model host (`grok.com`, `x.ai`, `api.x.ai` and subdomains) allows chat and personal data by default; a matching grant allows; anything else parks a hard Send card (no Always, Enter does not approve, Esc or 5 min timeout denies). A sensitive class never rides the model-host default.

Guarded: cabin xAI calls (`grok_json`, STT, TTS in `crates/grokhub-app/src/xai.rs`, and `XaiClient::once` for native Lab turns) and `/sync`, whose snapshot goes to `HubState.snapshot` and then to any paired computer through `/v1/snapshot`. The hub counts as a new destination with personal data, so `/sync` without a grant parks a card; Approve sends once and writes no grant.

`{config}/egress.jsonl` gets one line per allowed non-loopback send: time, host only, data classes, node ids, redaction count, grant id or `approved-once`, span ref, basis, origin. Never content, never a raw secret. It rolls once to `egress.1.jsonl` past 1 MiB.

## Not covered in 4a

- Grok Build's own traffic (owner decision D1): `grok -p`, ACP and GB's MCP servers talk to the network outside the cabin. GrokHub does not guard or log them and does not claim to.
- Other cabin fetches: Pulse feeds, update and GitHub checks, Labs `web_fetch`, native HTTP MCP, media GETs, and the realtime voice secret mint.
- `origin` is always `user` for now; Pulse and automation tagging comes later. xAI calls outside a harness turn log an empty span ref.
- A snapshot already in `hub-state.json` from before the upgrade stays served until the next sync or a revoke, which clears it.
- `egress.jsonl` is a log, not a lock, so it is not on the hard floor.

## Spans

`origin` (`user` default, `proactive`, `automation`, `self_manage`, `repair`) and `consent_ref` are `#[serde(default)]`; old span files still read. A send span with a user grant in `consent_ref` is clean for `approval_gate_violation`.
