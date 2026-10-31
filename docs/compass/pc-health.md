# Compass: PC health while gaming

## Owns
- Opt-in sampling of CPU/GPU temps, clocks, and util while a game is running, then one Pulse summary with throttle flags, a bound guess, and tips.
- Pure rules in `crates/grokhub-core/src/pc_health.rs`; cabin collectors and Settings in `pc_health_ui.rs`.

## Quick commands
- `cargo test -p grokhub-core pc_health --locked -- --test-threads=1`

## Key files
- `crates/grokhub-core/src/pc_health.rs`: parsers, `observe`, `summarize`, `summary_card`.
- `crates/grokhub-app/src/app/pc_health_ui.rs`: `tick_pc_health`, Settings toggle.
- `crates/grokhub-core/src/local_governor.rs`: `game_in_processes` (game detection).

## Change recipe
- New sensor: add a pure parse fn + fixture test in `pc_health.rs`, then a collector branch in `pc_health_ui.rs`.
- Tips stay advisory; never write fan curves, clocks, or drivers.

## What breaks it
- Calling collectors when `pc_health.enabled` is false: tests require zero session work when off.
- Renaming `SOURCE_PREFIX` (`pchealth:`) breaks card identity.

## What depends on it
- Pulse Feed via `post_feed_card`; `tick_housekeep` in `heartbeat_gate.rs` calls `tick_pc_health`.

## Non-obvious
- Game-up uses `game_in_processes` plus GPU util ≥ `GPU_BUSY_GAME_PCT` (same idea as the local governor).
- Sessions under `MIN_SESSION_MS` never post; sample cadence is `SAMPLE_EVERY_MS`.

## See also
- [nudge-watchers](nudge-watchers.md), [grokhub-core](grokhub-core.md)
