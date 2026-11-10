# Compass: the learning engine

## Owns
- One stream of what happened (`Signal`: a time, a `Kind`, a `Subject` id, a number) and one store of what the app concluded (`Belief`: counters or a running average, a confidence, the user's Forget and Wrong).
- The files under `{config}/learn` (signal log, one rotated copy, belief file, lock), local only. The one engine a running app shares (`hub`), and what the chip, card and run logs mean as signals (`sources`).

## Quick commands
- `cargo test -p grokhub-core --lib learn:: --locked`

## Key files
- `crates/grokhub-core/src/learn/mod.rs`: `Engine` (`record`, `tick`, `flush`, `forget`, `mark_wrong`, `forget_all`), `BELIEF_SAVE_MS`, `UNSAVED_MAX`.
- `crates/grokhub-core/src/learn/hub.rs` and `sources.rs`: `start`, `note_all`, `flush`; `chips_shown`, `chip_outcome`, `card`, `run`.
- `crates/grokhub-core/src/learn/view.rs`: `view` (beliefs as lines), `text`, `run` (the cabin's `/learned`), `act` (the desktop app's buttons). `ui/src/Learned.tsx` shows them in Settings, Privacy & data.
- `crates/grokhub-core/src/learn/signal.rs`: `Subject`, `Kind`, `Signal`, `Stream`, `FLUSH_AT`, `FLUSH_MS`.
- `crates/grokhub-core/src/learn/belief.rs`: `Beliefs`, `Shape`, `Standing`, `HALF_LIFE_MS`, `BELIEF_CAP`; `BREAK_RUN` votes in a row against a settled belief drop most of its old side.
- `crates/grokhub-core/src/learn/ladder.rs` and `store.rs`: `step`, `Undo`, `ACT_RESTORE_AT`, `RESTORE_MIN_EVIDENCE`; `append_signals`, `hold_lock`, `SIGNAL_BYTES`, `DISK_BUDGET`. `crates/grokhub-core/src/learn/decide.rs`: the decision engine. `Proposal` in, `Verdict` and `Reason` out (`State::decide`, `decide_all`, `outcome`), with shadow passes, the per-`Area` `Dial`, the watchdog `Alarm`, `ACTS_PER_DAY` and the log, in `decisions.json`. `/learned decisions` and `/learned trust <area> <level>` show and set it; the desktop page shows the same from `view::trust` (`Act::Trust`, `Act::CarryOn`). `learn/score.rs`: the scoreboard (`Board`, `Score`, `Trend`); an area that got worse only offers for `SLIP_REST_MS` (`Reason::Slipped`). `Goal` and `State::worth`: the user's goals rank proposals (`/learned goals`). `learn/now.rs`: `Snapshot`, the one picture of right now (`hub::set_now`, `hub::now`). The router's tuner is its first learner outside habits (`crates/grokhub-agent/src/route/trust.rs`): `Proposal.tested` skips the three-yes shadow rule for a change that had its own trial, and `Outcome::RolledBack` scores a change the app took back itself without counting it as the user's undo. Auto setup's plan is weighed too (`setup::weigh`, `hub::weigh_setup` after `plan_pass` in both apps; rule `rule:settings/<rule>`): act keeps a change, offer makes it a suggestion, wait drops it for this pass; first run and the user's own answers are not weighed. `plan::accept`, `AutoStore::dismiss` and `AutoStore::undo` report the outcome, and `manage_again` forgives the rule (`State::forgive`). The local model's launch tuner reads its dial (`hub::dial(Area::LocalAi)` in `route/local_tune.rs` `current`): off act, no canary runs.

## Change recipe
- A new learner records signals and reads beliefs: no new log file or confidence rule. A new source is a pure function in `sources.rs` called from the log's own writer.

## What breaks it
- Disk work in `Engine::record` (`recording_never_touches_the_disk`), or a longer `Subject` or higher cap (`the_files_stay_inside_the_disk_budget`).

## What depends on it
- Writers: `log_chips_shown`, `log_chip_outcome`, `append_signal`, `append_run`, `remember_reply_pick`. `append_run` also votes `habit:<skill>/<part of day>` (`habit.rs`; `.gaming` on the end while a game runs, `habit::tail`), in the local hour the app set with `hub::set_utc_offset_min`; a due habit becomes a chip through `hub::habit_chips` (the cabin's `feature_chips`), and its tap or hide answers its rule in `log_chip_outcome`. Both apps call `start` at launch and `flush` on exit. Readers: the learned list (`view.rs`), the plan reply row (`reply_chips.rs`) and auto setup's `Usage` (`setup.rs`, both apps).
- The ladder reads `ACT_AT` and `SUGGEST_AT` from `crates/grokhub-core/src/autotune/plan.rs`, so auto setup and the engine can't drift apart.

## Non-obvious
- A `Subject` is an id, never words the user typed. The guard runs on file reads too, so one bad key drops the whole belief file.
- Two apps share the folder: a save reads the file, replays this app's unsaved signals and writes it back under the lock. Never save the in-memory beliefs whole.
- Before `start`, `note_all` does nothing, so tests of the logs record nothing. `crates/grokhub-core/tests/learn_hub.rs` is its own binary because the hub is one per process.
- Counts fade from `last_ms` with no timer. Forget lets later signals start the line fresh; Wrong never does. `Pinned` (`/learned always`) is the user's own rule: confidence 1, no fading, no votes. The `paused` file stops `note` in both apps within one tick. `Undo::Never` stops at an offer.

## See also
- [grokhub-core](grokhub-core.md), [self-improve](self-improve.md), [ask-offers](ask-offers.md), [heartbeat](heartbeat.md), [router](router.md)
