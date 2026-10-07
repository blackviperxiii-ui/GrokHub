# Compass: heartbeat, throttle, and scheduled jobs

## Owns
- The 15 s pulse (`HEARTBEAT_MS`, `heartbeat_acts`) and `tick_heartbeat`, which wakes every organ: Housekeep, Inbox, Night (automations and `/loop`), Review, Wall, MidThought, Reflect, Anticipate.
- The throttle in front of the proactive acts (`ProactiveAct`: anticipate, the automatic ideas ask, the nightly review): min interval, hour and day caps, backoff after empty or dismissed acts, no act while busy, and the Halt hold.
## Quick commands
- `cargo test -p grokhub-core heartbeat` (pure rules, fake clock)
- `cargo test -p grokhub-app heartbeat -- --test-threads=1` (cabin wiring; isolate `GROKHUB_CONFIG`)
## Key files
- `crates/grokhub-core/src/heartbeat.rs`: `HeartbeatAct`, `runs_while_halted`, the pulse timing.
- `crates/grokhub-core/src/heartbeat_throttle.rs`: `HeartbeatPace` (`app.json` key `heartbeat`), `HeartbeatThrottle::gate`, `PaceHold` reasons, `PACE_NORMAL` (the defaults; config only, no Settings control).
- `crates/grokhub-app/src/app/heartbeat_gate.rs`: `heartbeat_busy`, `heartbeat_may`, `heartbeat_halt`, `heartbeat_outcome`, and the pace spans.
- `crates/grokhub-app/src/app/mod.rs` (`tick_heartbeat`, `tick_anticipate`) and `crates/grokhub-app/src/app/night.rs` (`tick_night`, `tick_loops`, `tick_review`, `start_scheduled_run`).
## Change recipe
- New proactive act: add a `ProactiveAct` variant, call `heartbeat_may` right before the work starts (after its own due checks), and report `heartbeat_outcome` when the result lands.
- New pace knob: a `#[serde(default = ...)]` field on `HeartbeatPace`, set in `PACE_NORMAL`, with a fake-clock test in `heartbeat_throttle.rs`.
## What breaks it
- Calling `heartbeat_may` before an act's own checks: it spends a slot for nothing and traces a misleading hold.
- Budgeting the Night slot: automations and loops have their own clock and `daily_auto_cap`, and run as `BgOrigin::Scheduled` runs that never take the composer (`heartbeat_halt_skips_every_organ_that_starts_work`).
- Tests that slice `tick_heartbeat` up to `fn tick_anticipate` (`the_night_slot_runs_loops_and_clock_time_automations`, `periodic_persist_leaves_the_ui_thread`): keep that span free of `self.persist()`.
## What depends on it
- Pulse ideas (`maybe_suggest_ideas`), the nightly review, anticipate, and `pulse_dismiss` / `pulse_not_this`, which feed backoff.
- `halt_everything` (tray Halt, hotkeys) calls `heartbeat_halt` first; `send_from_composer` ends the hold.
## Non-obvious
- The throttle is a pre-check only. `harness::decide` and the hard-card rules are unchanged and still see every act on its own path.
- Pace spans go to `{config_dir}/spans/heartbeat.jsonl` (session `HEARTBEAT_TRACE`, origin proactive, path `heartbeat`): decision `allow` or `hold` and a reason tag, never content. A repeated hold is traced once.
- Throttle state is in memory: a relaunch starts a fresh budget. An act with no outcome after `ENGAGE_WINDOW_MS` counts as empty.
- Halt drops an ideas or review reply still in flight; the HTTP call itself finishes in its thread.
## See also
- [app-module](app-module.md), [app-config](app-config.md), [harness](harness.md), [grokhub-core](grokhub-core.md)
