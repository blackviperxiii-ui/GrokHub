# Compass: the learning engine

## Owns
- One stream of what happened (`Signal`: a time, a `Kind`, a `Subject` id, a number) and one store of what the app concluded (`Belief`: counters or a running average, a confidence, the user's Forget and Wrong).
- The files under `{config}/learn`: the capped signal log and its one rotated copy, and the belief file. Local only.

## Quick commands
- `cargo test -p grokhub-core --lib learn:: --locked`

## Key files
- `crates/grokhub-core/src/learn/mod.rs`: `Engine` (`record`, `tick`, `flush`, `forget`, `mark_wrong`, `forget_all`), `BELIEF_SAVE_MS`.
- `crates/grokhub-core/src/learn/signal.rs`: `Subject`, `Kind`, `Signal`, `Stream`, `FLUSH_AT`, `FLUSH_MS`.
- `crates/grokhub-core/src/learn/belief.rs`: `Beliefs`, `Shape`, `Standing`, `HALF_LIFE_MS`, `BELIEF_CAP`.
- `crates/grokhub-core/src/learn/ladder.rs`: `step`, `Undo`, `ACT_RESTORE_AT`, `RESTORE_MIN_EVIDENCE`.
- `crates/grokhub-core/src/learn/store.rs`: `Log`, `SIGNAL_CAP`, `DISK_BUDGET`, `disk_used`.

## Change recipe
- A new learner records signals and reads beliefs. Don't add another log file or another confidence rule.
- A new `Kind` needs a line in `Kind::vote` and in `signals_vote_by_kind`.

## What breaks it
- Disk work in `Engine::record`: `recording_never_touches_the_disk` fails. Only `tick` and `flush` write.
- A longer `Subject`, a higher cap or a wider line: `the_files_stay_inside_the_disk_budget` fails.

## What depends on it
- Nothing yet. Both apps and the existing learners (chips, card taste, rhythm, auto setup, the router) plug in next.
- The ladder reads `ACT_AT` and `SUGGEST_AT` from `crates/grokhub-core/src/autotune/plan.rs`, so auto setup and the engine can't drift apart.

## Non-obvious
- A `Subject` is an id, never words the user typed. The guard runs on file reads too, so one bad key drops the whole belief file.
- Counts fade from `last_ms` when a belief is read or updated, with no timer. Forget lets later signals start the line fresh; Wrong never does.
- `Undo::Never` stops at an offer. What may run without asking at all is the harness's rule, not this module's.

## See also
- [grokhub-core](grokhub-core.md), [self-improve](self-improve.md), [ask-offers](ask-offers.md), [heartbeat](heartbeat.md), [router](router.md)
