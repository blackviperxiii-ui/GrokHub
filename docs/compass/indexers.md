# Compass: local indexers (Spike-8a, design P5)

## Owns
- `crates/grokhub-agent/src/indexers/`: one module per scope (`files`, `apps`, `browser_history`, `system_state`), the `Scheduler` that runs one scope per heartbeat tick, `scope:` AMR nodes, the in-memory `ScopeIndex`, `forget_facts` ("Forget these"), and `ScopeAsks` (in-context ask cards).
- Cabin glue in `crates/grokhub-app/src/app/indexer_ui.rs`: `tick_indexers` (Housekeep slot, low-priority worker), `paint_scope_asks` (Work tree), `ui_scope_facts` under each granted row in `crates/grokhub-app/src/app/scope_ui.rs`.
## Quick commands
- `cargo test -p grokhub-agent indexers -- --test-threads=1` (acceptance tests, counting fake FS, fake power and clock)
- `cargo test -p grokhub-app scope -- --test-threads=1` (Settings rows, ask card, Forget these; isolate `GROKHUB_CONFIG`)
## Key files
- `crates/grokhub-agent/src/indexers/mod.rs`: `Scheduler::tick`, `TickEnv`, `TickOutcome`, `index_excluded`, `node_id`, `scope_store`, `ScopeIndex`, `forget_facts`, `ScopeAsks`; tests in `crates/grokhub-agent/src/indexers/tests.rs`.
- `crates/grokhub-agent/src/indexers/browser_history.rs` (`read_history`: temp copy, read-only, SQLite authorizer), `crates/grokhub-agent/src/indexers/paths.rs` (`PlatformDirs`, `browser_root`, `app_dirs`), `crates/grokhub-agent/src/indexers/power.rs` (`on_battery`, `lower_thread_priority`).
- `crates/grokhub-agent/src/indexers/system_state.rs`: `snapshot` / `SystemSnapshot`, reused by Spike-8b's diagnose.
## Change recipe
- New scope reader: add a `gather` arm, keep every read behind `IndexFs` (so the boot test still counts zero), run `index_excluded` on every path before reading, return `Fact`s with a stable `item`, and add a fixture test that greps the config dir for the fixture's plain text.
- New hard exclude: `SCOPE_HARD_EXCLUDES` (also refuses the grant) or `INDEX_EXCLUDES` (indexers only), plus a line in `hard_excludes_cover_password_managers_and_key_files`.
## What breaks it
- Caching a consent answer: `tick` and every `BATCH` read `ConsentLedger::load` again and ask `decide` (`Step::Scope`), so a revoke lands within one tick (`revoke_means_no_new_nodes_after_one_tick`).
- A plain node or a plaintext fallback: facts are always personal or sensitive, sealed by `LearnedVault`; no keyring is `TickOutcome::Paused` (`no_keyring_pauses_and_writes_nothing`).
- Any table but `moz_places` / `urls`, opening the original history file, or a URL path in a fact (`browser_history_reads_history_tables_from_a_temp_copy_only`).
- Granting from anything but a pointer click: `ScopeAsks::ask` only queues a card; Allow goes through `grant_scope_click` (`a_scope_ask_card_grants_on_a_click_only`, `only_a_settings_click_writes_a_grant`).
## What depends on it
- Settings → Permissions scope rows and `/privacy`; the AMR store and its dream (which skips `SCOPE_SOURCE_PREFIX` nodes); Spike-6a cards (`ScopeAsks`) and Spike-8b diagnose (`snapshot`).
## Non-obvious
- Calendar and mail are grant rows only: no cabin-owned reader exists (D1), so `has_reader` is false and no ask card is offered for them.
- App launch counts come only from the cabin's own `APP_LAUNCH_LOG`; nothing writes it yet. A forgotten fact keeps its tombstone, so the same item is never written again.
- Tags and spans carry the scope kind only (`scope:files`), never the folder path; `node_id` hashes key and item. `RESCAN_MS` paces each scope; battery and quiet hours skip the tick before any read.
## See also
- [harness](harness.md), [amr](amr.md), [heartbeat](heartbeat.md), [app-module](app-module.md)
