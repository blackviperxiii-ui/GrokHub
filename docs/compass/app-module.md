# Compass: grokhub-app app/ (the Cabin UI)

## Owns
- `Cabin`, the eframe app: one struct in `crates/grokhub-app/src/app/mod.rs` plus `impl Cabin` blocks split by page or feature (`chat_ui`, `chat_kick`, `acp`, `settings`, `sidebar`, `pages`, `pulse_ui`, `feed_ui`, `board_ui`, `night`, `harness_ui`, `background`, `native_*`, ...). Each submodule starts with `use super::*;`.
- The frame loop (`impl eframe::App for Cabin`: `logic`, `ui`, `on_exit`) and the background job polls it drives.
## Quick commands
- `GROKHUB_CONFIG=$(mktemp -d) cargo test -p grokhub-app app::tests::<name> -- --test-threads=1`
- `cargo test -p grokhub-app -- --test-threads=1 --skip cabin_signed_in_false_when_idle --skip board_cards_open_on_click_fold_on_drag_and_chat_like_the_chat_page` (on a box with a logged-in `grok`)
## Key files
- `crates/grokhub-app/src/app/mod.rs`: `Cabin` fields, `new`, `quiet_for_test`, the frame loop.
- `crates/grokhub-app/src/app/tests.rs`: most cabin tests, plus `cabin_src`, `fn_src`, `isolated_cabin`, `release_isolated`.
- `crates/grokhub-app/src/app/chat_ui.rs` (pane and composer) and `crates/grokhub-app/src/app/chat_kick.rs` (send, `grok -p` / ACP kick).
- `crates/grokhub-app/src/app/persist.rs`: off-thread writes behind `persist_io`.
## Change recipe
- New UI state: add a `Cabin` field and initialize it in both `new` and `quiet_for_test`.
- New behavior: put logic in a pure fn (often in grokhub-core) and unit-test it; keep the egui code thin. Then add a cabin test with `isolated_cabin` and finish with `release_isolated`.
- Disk work never runs on the UI thread: spawn it and poll a channel, as `/recall` and persist do.
## What breaks it
- Growing the `ComposerStackSlot::Pill` arm in `chat_ui.rs`: `chat_composer_pins_stop_on_the_right` reads a fixed 12,000-byte window after it (`CLAUDE.md`). Move code into helpers.
- Moving a fn between files or renaming it: `fn_src` finds `fn name(` in `cabin_src()` (a fixed list of submodules) and ends at the next same-indent fn, and many tests assert on that slice.
- Reordering `paint_approval_stack` / `paint_perm_ask` / `paint_elicit_ask` in `chat_ui.rs`: `needs_attention_summary_is_only_on_the_stack` slices between those signatures.
- Blocking the frame on disk, network, or `grok`.
## What depends on it
- `crates/grokhub-app/src/main.rs` launches it for the window and `--agent` (tray) modes.
## Non-obvious
- `cabin_src()` does not include every submodule (for example `harness_ui.rs`, `feed_ui.rs`, `pulse_ui.rs`); those tests `include_str!` the file directly.
- `on_exit` kills background `grok -p`, halts native sessions, and waits on `persist_io` because two writers of `app.json` share one temp file.
- The composer glow and Always ring timings come from `GLOW_SETTLE_SECS` / `ALWAYS_SETTLE_SECS` in grokhub-core, and motion is skipped when `motion_ok` is false (AGENTS.md UI theme).
- `cabin_signed_in_false_when_idle` and `board_cards_open_on_click_fold_on_drag_and_chat_like_the_chat_page` fail on a box with a logged-in Grok CLI; isolate HOME, PATH, and config or skip them.
## See also
- [grokhub-app](grokhub-app.md), [app-config](app-config.md), [slash](slash.md), [harness](harness.md)
