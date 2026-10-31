# Compass: print finished/failed alerts

## Owns
- Opt-in LAN discovery and read-only polling of Moonraker/Klipper and OctoPrint printers; Pulse + OS notify on finish, fail, pause, or filament runout; optional camera snapshot; pause/cancel/resume gated by the hard approval card.
- Pure parsers and transition rules in `crates/grokhub-core/src/print_alerts.rs`; cabin I/O in `print_alerts_ui.rs`.

## Quick commands
- `cargo test -p grokhub-core print_alerts --locked -- --test-threads=1`

## Key files
- `crates/grokhub-core/src/print_alerts.rs`: parse, `observe`, `alert_card`, control paths.
- `crates/grokhub-app/src/app/print_alerts_ui.rs`: discover, poll, Settings, approval park.
- `crates/grokhub-app/src/app/heartbeat_gate.rs`: `tick_housekeep` calls `tick_print_alerts`.

## Change recipe
- New printer API: add a pure parse fn + fixture test, then a cabin fetch branch.
- Controls always `requires_approval`; park `CONTROL_TOOL` before any POST.

## What breaks it
- Shipping host IPs or API keys in the repo or changelog.
- Calling poll/discover when `print_alerts.enabled` is false.
- Raising `mod.rs` past the ci-guards budget — keep Housekeep in `tick_housekeep`.

## What depends on it
- Pulse Feed via `post_feed_card`; Settings → Pulse; hard park Approve runs `execute_print_control`.

## Non-obvious
- First sample never alerts (missed mid-print); cooldown is `ALERT_COOLDOWN_MS`.
- Discovery probes localhost plus a capped /24 slice (not a full subnet storm).

## See also
- [nudge-watchers](nudge-watchers.md), [harness](harness.md), [heartbeat](heartbeat.md)
