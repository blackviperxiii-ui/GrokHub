# Compass: nudge watchers (Command Centre card 42)

## Owns
- Opt-in local watchers that post one Pulse suggestion when metadata-only signals fire (game news, GPU driver offer, disk, crashes, OS packages, peripherals).
- Pure rules in `crates/grokhub-core/src/nudge_watchers.rs`; Know me tags via `know_me.rs` when `know-me.json` exists.
- Cabin collectors and Settings toggles in `nudge_watchers_ui.rs`; ledger in `nudge-watchers.json`.

## Quick commands
- `cargo test -p grokhub-core nudge_watchers --locked -- --test-threads=1`

## Key files
- `crates/grokhub-core/src/nudge_watchers.rs`: detect, cooldown, daily cap, card shape.
- `crates/grokhub-core/src/know_me.rs`: optional tag load.
- `crates/grokhub-app/src/app/nudge_watchers_ui.rs`: `tick_nudge_watchers` (folder and device reads on a worker: `fs_snapshot`, then `finish_nudge_tick`), Settings UI.
- `crates/grokhub-app/src/config.rs`: `nudge_watchers` in `app.json`.

## Change recipe
- New watcher: extend `WatcherId`, snapshot field, `detect_*`, Settings toggle, collector, fixture test in `nudge_watchers.rs`.
- Posting uses `post_feed_card`; dismiss calls `nudge_watcher_dismissed` from `pulse_dismiss`.

## What breaks it
- Cooldown and snooze keys use `dedupe_key`; changing them re-nags users.
- `pulse::PulseMeta::nudge_watcher` is on the card schema; keep serde defaults.

## What depends on it
- Pulse Feed only; no harness auto-act. GPU offers need `nudge-gpu-offer.json` from a future feed fetch.

## Non-obvious
- `tick_nudge_watchers` runs from `tick_feed_pulse` at most every `TICK_EVERY_MS`.
- Steam patch nudges need a second news gid per app after the ledger seeds the first.

## See also
- [grokhub-core](grokhub-core.md), [app-module](app-module.md)
