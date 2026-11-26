# Compass: self-manage tools (`grokhub-self`, Spike-5c)

## Owns
- Grok's own tools for its skills, connections, and automations: `grokhub --mcp-self` (an MCP client sees `grokhub-self__<tool>`, path A) and the same tools on the native engine (path E).
- One class table, `SELF_TOOLS` (read / soft / delete; a connection with `needs_token` is credentials), the scope guard (`scope_guard`), and the skill-create cap (`SKILL_CREATE_DAY_CAP`).
## Quick commands
- `cargo test -p grokhub-agent self_manage` (table, classes on every path, guard, caps, ledger writes)
- `cargo test --manifest-path src-tauri/Cargo.toml change_rows` (shell change rows / Undo; isolate `GROKHUB_CONFIG`)
## Key files
- `crates/grokhub-agent/src/self_manage/mod.rs`: `SELF_TOOLS`, `self_class`, `needs_token`, `scope_guard`, `mcp_tools`, `native_schemas`, `run_native`.
- `crates/grokhub-agent/src/self_manage/ops.rs`: what each tool does after the gate (`run`, `SelfCtx`). Skills write through `record_skill_change`; connections and automations reuse the Spike-5b writers in `crates/grokhub-agent/src/tools/connections.rs` and `crates/grokhub-agent/src/tools/control.rs`.
- the retired cabin's self_mcp.rs: `SelfServer`, `SelfIo`, `run_stdio`.
- the retired cabin's change_undo.rs: LedgerWatch reads ledger lines the server process wrote.
## Change recipe
- New tool: add a `SELF_TOOLS` row and its `mcp_tools` entry (same order), its arm in `run`, and a test that asks `harness::decide` for both the bare and `grokhub-self__` names.
- New guarded target: add it to `GUARDED` in `crates/grokhub-agent/src/self_manage/mod.rs`; the guard runs as a hard floor, before any ledger write.
## What breaks it
- A side path around `harness::decide`: the classes live in `hard_class` / `hard_floor` (`crates/grokhub-agent/src/harness/hard.rs`), so paths A and E agree.
- A token in args, a span, the ledger, a park file, or `mcp.json`. It comes only from the elicit card and is sealed by `seal_connection_token`; the entry keeps `tokenRef`.
- Writing into the user's `~/.grok`: GrokHub never touches the Grok Build CLI's config or credentials.
- Building an `UndoAsk` here: no tool can undo; Undo stays a click or typing (only_typing_or_a_click_builds_an_undo_ask).
## What depends on it
- The cabin's hard cards and park files (the retired cabin's harness_ui.rs), the Work-tree rows (poll_self_changes), and the native loop (`dispatch` in `crates/grokhub-agent/src/tools/mod.rs`).
## Non-obvious
- The stdio server runs soft calls and only parks hard class; soft calls rely on the caller's own gate. Under Always, delete and credentials still park, Enter never approves, and TTL, Halt, or a closed cabin is Deny.
- Connections land in the cabin's native `mcp.json`, never the Grok Build CLI's config, so Undo rows stay one file.
- A token-needing `connection_add` is hard class credentials by its args (`needs_token`), not its name; the server's own park is what holds it.
- `catalog_id` (or a name that matches the bundled catalog) reuses the Connectors install recipe. OAuth shows a URL elicit card and never opens the browser; Decline or a failed sign-in removes the staged entry. Catalog key connectors are credentials like `needs_token`. Tools join on the next turn (`begin_turn` refreshes frozen MCP schemas).
## See also
- [harness](harness.md), [desktop-mcp](desktop-mcp.md), [grokhub-agent](grokhub-agent.md)
