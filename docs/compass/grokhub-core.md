# Compass: grokhub-core (shared brain)

## Owns
- The pure logic every surface shares, so Linux and Windows do not grow a second protocol: chat routing and models, slash parsing, chips, history, projects, automations, Pulse and the update feed, pairing and hub state, desktop MCP protocol, channels, AMR memory, motion constants.
- No HTTP and no UI: serde stack plus minisign/toml/quick-xml/rusqlite; Windows adds `win_acl` (`crates/grokhub-core/Cargo.toml`).
## Quick commands
- `cargo test -p grokhub-core --locked -- --test-threads=1`
- `cargo test -p grokhub-core --test compass_paths` (these compass files)
## Key files
- `crates/grokhub-core/src/lib.rs`: the `pub mod` list and the flat `pub use` re-exports that other crates import.
- `crates/grokhub-core/src/state.rs` (`HubState`, `state_for_disk`) and `crates/grokhub-core/src/pair.rs`.
- `crates/grokhub-core/src/host_safety.rs`: `forbidden_reason` (leaf match plus assignment/`+`/`$(echo)`/`chr()` deobfuscation and `base64|sh` rebuild refuse) and `recall_hits`. `crates/grokhub-core/src/ssrf.rs`: `ip_blocked` / `host_name_blocked`; `public_http_url` uses them (dialers add `public_only_resolve`, never env proxy).
- Also: `panel_spec.rs` (generative UI validate-only), `watch_learn.rs` (pure SD1 step extract), `connectivity.rs`, `fix_plan.rs` (SC2).
- `crates/grokhub-core/src/feel.rs`: `ALWAYS_SETTLE_SECS`, `GLOW_SETTLE_SECS`, and the other motion timings.
- `crates/grokhub-core/src/appearance.rs`: Settings theme ids (`ops-dark`, `oled-black`, `soft-charcoal`, classic `dark` / `light` / `system`) and `resolve_palette`.
- `crates/grokhub-core/src/paths.rs`: `user_home`. `diagnostics.rs`: About clipboard + Labs Desktop zip (`DiagnosticsExport`, `write_diagnostics_zip`).
- `crates/grokhub-core/src/wire.rs`: chat wire types (`AcpEvent`, `ToolCard`, `PermissionAsk`, `ElicitAsk`, `SessionMode`, `PermissionMode`, `GrokUsage`). `crates/grokhub-core/src/proc_util.rs`: `hide_windows_console`, `kill_pid`, `is_sigterm_status`.
## Change recipe
- New helper: write it in its module with a literal-value test, then add it to that module's `pub use` block in `lib.rs`; grokhub-app imports flat names (`use grokhub_core::{...}`).
- New module: `pub mod x;` in `lib.rs` plus its re-exports. Keep it free of network and egui; outside tests only `update.rs` spawns a process (`git`).
## What breaks it
- Renaming a re-exported item: it breaks grokhub-app, grokhub-hub, and grokhub-agent in one go. Grep the workspace first.
- Editing scripts or packaging: `hands.rs`, `desktop_entry.rs`, `update.rs`, and `channel.rs` tests read `scripts/` and `packaging/` and assert on their text.
## What depends on it
- Every other crate in the workspace `Cargo.toml`. The hub serves `HubState`.
## Non-obvious
- `cabin_static_rules` / `cabin_learned_tail` (`cabin_rules.rs`) split the prompt-cache prefix from the volatile brief; the app joins them via `system_prompt_parts`.
- `now_ms`, `user_home`, `redact_secrets`, and `redact_recall` (secrets + PII, `crates/grokhub-core/src/pii.rs`) live here; reuse them instead of re-deriving HOME, time, or redaction.
- `crates/grokhub-core/src/goal.rs` stores per-chat intentions next to `ThreadGoal`. `set_intention_status` changes status only.
- `state_for_disk` strips the live frame before `hub-state.json` is written (`disk_omits_frame`); writes are 0600 / user-only DACL (`win_acl` on Windows).
- Some modules assert on their own source with `include_str!` (`chips.rs`, `state.rs`, `update.rs`); a reworded string or moved fn can fail a test.
- `crates/grokhub-core/src/imagine_auth.rs` only re-exports `xai_signin`; keychain storage for Imagine is `crates/grokhub-app/src/imagine_auth.rs`.
- The harness is not here: it lives in `crates/grokhub-agent/src/harness/mod.rs`.
## See also
- [amr](amr.md), [slash](slash.md), [desktop-mcp](desktop-mcp.md), [grokhub-hub](grokhub-hub.md), [install-scripts](install-scripts.md)
