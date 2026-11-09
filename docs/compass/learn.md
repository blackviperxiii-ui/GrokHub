# Compass: the learning engine

## Owns
- One stream of what happened (`Signal`: a time, a `Kind`, a `Subject` id, a number) and one store of what the app concluded (`Belief`: counters or a running average, a confidence, the user's Forget and Wrong).
- The files under `{config}/learn` (signal log, one rotated copy, belief file, lock), local only. The one engine a running app shares (`hub`), and what the chip, card and run logs mean as signals (`sources`).

## Quick commands
- `cargo test -p grokhub-core --lib learn:: --locked`

## Key files
- `crates/grokhub-core/src/learn/mod.rs`: `Engine` (`record`, `tick`, `flush`, `forget`, `mark_wrong`, `forget_all`), `BELIEF_SAVE_MS`, `UNSAVED_MAX`.
- `crates/grokhub-core/src/learn/hub.rs` and `sources.rs`: `start`, `note_all`, `flush`; `chips_shown`, `chip_outcome`, `card`, `run`.
- `crates/grokhub-core/src/learn/signal.rs`: `Subject`, `Kind`, `Signal`, `Stream`, `FLUSH_AT`, `FLUSH_MS`.
- `crates/grokhub-core/src/learn/belief.rs`: `Beliefs`, `Shape`, `Standing`, `HALF_LIFE_MS`, `BELIEF_CAP`.
- `crates/grokhub-core/src/learn/ladder.rs`: `step`, `Undo`, `ACT_RESTORE_AT`, `RESTORE_MIN_EVIDENCE`.
- `crates/grokhub-core/src/learn/store.rs`: `append_signals`, `hold_lock`, `SIGNAL_BYTES`, `DISK_BUDGET`, `disk_used`.

## Change recipe
- A new learner records signals and reads beliefs: no new log file or confidence rule. A new source is a pure function in `sources.rs` called from the log's own writer.

## What breaks it
- Disk work in `Engine::record` (`recording_never_touches_the_disk`), or a longer `Subject` or higher cap (`the_files_stay_inside_the_disk_budget`).

## What depends on it
- Writers: `log_chips_shown`, `log_chip_outcome`, `append_signal`, `append_run`. Both apps call `start` at launch and `flush` on exit. No reader yet.
- The ladder reads `ACT_AT` and `SUGGEST_AT` from `crates/grokhub-core/src/autotune/plan.rs`, so auto setup and the engine can't drift apart.

## Non-obvious
- A `Subject` is an id, never words the user typed. The guard runs on file reads too, so one bad key drops the whole belief file.
- Two apps share the folder: a save reads the file, replays this app's unsaved signals and writes it back under the lock. Never save the in-memory beliefs whole.
- Before `start`, `note_all` does nothing, so tests of the logs record nothing. `crates/grokhub-core/tests/learn_hub.rs` is its own binary because the hub is one per process.
- Counts fade from `last_ms` with no timer. Forget lets later signals start the line fresh; Wrong never does. `Undo::Never` stops at an offer.

## See also
- [grokhub-core](grokhub-core.md), [self-improve](self-improve.md), [ask-offers](ask-offers.md), [heartbeat](heartbeat.md), [router](router.md)
