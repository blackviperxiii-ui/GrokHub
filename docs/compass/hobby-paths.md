# Compass: hobby learning paths

## Owns
- Opt-in bite-sized tips from Know me hobby tags (3D print, Blender, Rust, modding, art, CAD).
- Pure catalog, progress, difficulty, and Pulse card shape in `hobby_paths.rs`.
- Settings toggles and tick in `hobby_paths_ui.rs`; ledger in `hobby-paths.json`.

## Quick commands
- `cargo test -p grokhub-core hobby_paths --locked -- --test-threads=1`
- `cargo test -p grokhub-app hobby_paths_ui --locked -- --test-threads=1`

## Key files
- `crates/grokhub-core/src/hobby_paths.rs`: tags → hobbies, catalog, feedback, `tick`.
- `crates/grokhub-core/src/know_me.rs`: fixture tags via `load_tags`.
- `crates/grokhub-core/src/focus_app.rs`: last app prefers a matching hobby tip.
- `crates/grokhub-app/src/app/hobby_paths_ui.rs`: Settings UI, tick, Done/Skip feedback.
- `crates/grokhub-app/src/config.rs`: `hobby_paths` in `app.json`.

## Change recipe
- New hobby: extend `HobbyId`, settings fields, `catalog` steps, tag/app words, Settings checkbox.
- Feedback stays local (`PathFeedback`); do not raise difficulty past 0..=2.
- Posting uses `post_feed_card`; Pulse paints pills from `HobbyPathMeta`.

## What breaks it
- Cooldown keys use `HobbyId::key`; renaming re-nags users. `HobbyPathMeta` needs serde defaults; keep `app/mod.rs` at budget.

## What depends on it
- Pulse For You / Feed only; no harness auto-act. Needs Know me tags before tips fire.

## Non-obvious
- `tick_hobby_paths` runs from `tick_feed_pulse` at most every `TICK_EVERY_MS`.
- Focus preference maps FocusApp display names back to process fragments.

## See also
- [nudge-watchers](nudge-watchers.md), [grokhub-core](grokhub-core.md), [app-module](app-module.md)
