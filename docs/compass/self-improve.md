# Compass: self-improvement (Spike-7)

## Owns
- Per-task outcomes: one `TaskOutcome` per finished task in `{config}/outcomes.jsonl`, keyed by signature (`skill:<name>` or `topic:<words>`). Success is VERIFY_OK, accepted, or no undo in `UNDO_WINDOW_MS`; failure is a detector finding, a deny, an undo, a correction, or the ladder's pause. A late fact is a superseding line; `fold_outcomes` keeps the newest per task.
- The weekly self-review: `self_review_due` (Sunday at or after the `dreamHour` setting, Settings → Behavior → Dream time, default `REVIEW_NIGHT_HOUR` 21; `night_passes` runs it, then AMR dream, once a night), at most `SELF_REVIEW_CAP` (5) Pulse Suggestion cards, skill drafts (`DRAFT_MIN_RUNS` in `DRAFT_WINDOW_MS`), revert offers (`REVERT_MIN_RUNS` after a self patch), and the replay gate in front of every skill patch, nightly or weekly.
## Quick commands
- `cargo test -p grokhub-core outcome::` and `cargo test -p grokhub-core self_review::` (pure rules, fake clock)
- `cargo test -p grokhub-agent self_improve`, then `cargo test -p grokhub-app self_review_tests -- --test-threads=1` (isolate `GROKHUB_CONFIG`)
## Key files
- `crates/grokhub-core/src/outcome.rs`: `TaskOutcome`, `classify`, `append_outcome` (rotated like the trajectory file), `undo_supersedes`, `is_correction`.
- `crates/grokhub-core/src/self_review.rs`: `skill_stats`, `rank_proposals`, `check_target`, `draft_candidates`, `revert_candidates`, `line_diff`, card source ids.
- `crates/grokhub-agent/src/harness/self_improve.rs`: `outcome_from_spans`, `replay_patch` / `replay_gate`, `patch_marks`, `run_weekly` (`call_model`, class `background:review`).
- `crates/grokhub-app/src/app/self_review_ui.rs`: turn-end record, correction and undo lines, `night_passes` (self-review, then dream, at `dream_hour`), cards and their Apply / Revert.
## Change recipe
- New outcome signal: add it to `OutcomeSignals` and `classify` (failures first), set it where the cabin learns it, and add a literal test. Never put reply text or args on a record.
- New proposal kind: a `CardTarget` variant with its source prefix, a branch in `apply_self_review_card`, and a scope check before anything is written.
## What breaks it
- Writing a skill patch without `replay_gate`, or a replay that runs a step, calls a model, or writes the host (`the_weekly_pass_makes_one_routed_call_and_replays_make_none`).
- Letting `check_target` pass anything but `skill:<name>`: policy, consent, egress, the hard-class list, Access, and the gate stay refused with a finding.
- More than five cards a pass, or a pass outside Sunday night (`due_only_on_sunday_after_the_hour_once`).
## What depends on it
- `harness_turn_end` (one record per stepped turn), `send_from_composer` (corrections), `finish_skill_revert` (undo lines), `apply_review_skill_patches` (the nightly patch passes the replay gate too).
- The Pulse row menu: Apply change and Revert are click-only items that go through `pulse_accept`.
## Non-obvious
- A skill with no recorded successful run can't be patched: the replay has nothing to replay, so it refuses.
- Revert builds its `UndoAsk` from the click in `apply_self_review_card`; the source-scan test lists that one site.
- The 6a card budget isn't in the base yet; the pass keeps its own cap of five and skips a source that already has a live card.
## See also
- [harness](harness.md), [heartbeat](heartbeat.md), [app-module](app-module.md), [grokhub-core](grokhub-core.md)
