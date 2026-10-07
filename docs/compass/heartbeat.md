# Compass: heartbeat, throttle, and scheduled jobs

## Owns
- The 15 s pulse (`HEARTBEAT_MS`, `heartbeat_acts`) and `tick_heartbeat`, which wakes every organ: Housekeep, Inbox, Night (automations and `/loop`), Review, Wall, MidThought, Reflect, Anticipate.
- The throttle in front of the proactive acts (`ProactiveAct`: anticipate, the automatic ideas ask, the nightly review): min interval, hour and day caps, backoff after empty or dismissed acts, no act while busy, and the Halt hold.
- Done for you (Spike-6b): `tick_auto_act`, the first step of `tick_anticipate`, takes one queued candidate and acts on it without asking only under the autonomy ceiling. That needs every term: soft class, a change-ledger target (reversibility 1.0), a granted scope, Access Supervised or Full, the pill on Auto or Always, MindCheck `p_mind < 0.2` with history and no ask-first window, `confidence >= 0.8`, not quiet, not busy, not halted, and under `AUTO_PER_DAY` (5). No setting raises it. The step then still goes through `harness::decide` (`Step::Proactive`) and the native dispatch (path E, origin proactive), and posts a `DoneForYou` update card with Undo and "Don't do this again" (pointer click only). A miss lands in `ProactiveState::asked` with its `CeilingMiss` and an `ask` span. A hard candidate is prepared, not done: it parks a hard card, and an unattended park times out to a Deny span on the next tick.
## Quick commands
- `cargo test -p grokhub-core heartbeat` (pure rules, fake clock)
- `cargo test -p grokhub-app heartbeat -- --test-threads=1` (cabin wiring; isolate `GROKHUB_CONFIG`)
## Key files
- `crates/grokhub-core/src/heartbeat.rs`: `HeartbeatAct`, `runs_while_halted`, the pulse timing.
- `crates/grokhub-core/src/heartbeat_throttle.rs`: `HeartbeatPace` (`app.json` key `heartbeat`), `HeartbeatThrottle::gate`, `PaceHold` reasons, `PACE_NORMAL` (the defaults; config only, no Settings control).
- `crates/grokhub-app/src/app/heartbeat_gate.rs`: `heartbeat_busy`, `heartbeat_may`, `heartbeat_halt`, `heartbeat_outcome`, and the pace spans.
- `crates/grokhub-core/src/proactive_auto.rs` (`ceiling_allows`, `AutoAct`, `CeilingMiss`, `AutoBudget`), `crates/grokhub-agent/src/harness/proactive.rs` (`run_auto_act`, `proactive_mind`, `answer_span`), and `crates/grokhub-app/src/app/proactive_auto.rs` (`tick_auto_act`, `ceiling_ctx`, `done_for_you_undo`, `done_for_you_never`).
- `crates/grokhub-app/src/app/mod.rs` (`tick_heartbeat`, `tick_anticipate`) and `crates/grokhub-app/src/app/night.rs` (`tick_night`, `tick_loops`, `tick_review`, `start_scheduled_run`).
## Change recipe
- New proactive act: add a `ProactiveAct` variant, call `heartbeat_may` right before the work starts (after its own due checks), and report `heartbeat_outcome` when the result lands.
- New pace knob: a `#[serde(default = ...)]` field on `HeartbeatPace`, set in `PACE_NORMAL`, with a fake-clock test in `heartbeat_throttle.rs`.
## What breaks it
- An auto-act that skips `AutoAct::admit` or `harness::decide`, runs through anything but the native dispatch, or acts on a step with no ledger target. `AutoAct` has private fields so a hard candidate can't become one (`reply_to_sam_is_a_prepared_draft_on_a_hard_card_even_under_always_and_full`, `a_hundred_soft_approvals_never_let_a_hard_send_auto_act`).
- Calling `heartbeat_may` before an act's own checks: it spends a slot for nothing and traces a misleading hold.
- Budgeting the Night slot: automations and loops have their own clock and `daily_auto_cap`, and run as `BgOrigin::Scheduled` runs that never take the composer (`heartbeat_halt_skips_every_organ_that_starts_work`).
- Tests that slice `tick_heartbeat` up to `fn tick_anticipate` (`the_night_slot_runs_loops_and_clock_time_automations`, `periodic_persist_leaves_the_ui_thread`): keep that span free of `self.persist()`.
## What depends on it
- Pulse ideas (`maybe_suggest_ideas`), the nightly review, anticipate, and `pulse_dismiss` / `pulse_not_this`, which feed backoff.
- `halt_everything` (tray Halt, hotkeys) calls `heartbeat_halt` first; `send_from_composer` ends the hold.
## Non-obvious
- Spike-6a's `ProactiveEngine` and `ProactiveBudget` are not in this base yet. `ProactiveState::queue` is where its candidates come in, `asked` is what its "I can …" cards will show, and `AutoBudget` (in memory, like the throttle) is the auto slice of its budget.
- MindCheck for auto-acts reads `{config_dir}/spans/proactive.jsonl`: approvals, a Done-for-you Undo (`undo`, prior 0.6, ask first 30 days) and "Don't do this again" (`never`, prior 1.0 for good). An auto-act's span carries `undo_ref` (`connection:12`), and its ledger line gets no Work-tree row or Changed card: the Done-for-you card holds its Undo.
- The throttle is a pre-check only. `harness::decide` and the hard-card rules are unchanged and still see every act on its own path.
- Pace spans go to `{config_dir}/spans/heartbeat.jsonl` (session `HEARTBEAT_TRACE`, origin proactive, path `heartbeat`): decision `allow` or `hold` and a reason tag, never content. A repeated hold is traced once.
- Throttle state is in memory: a relaunch starts a fresh budget. An act with no outcome after `ENGAGE_WINDOW_MS` counts as empty.
- Halt drops an ideas or review reply still in flight; the HTTP call itself finishes in its thread.
## See also
- [app-module](app-module.md), [app-config](app-config.md), [harness](harness.md), [grokhub-core](grokhub-core.md)
