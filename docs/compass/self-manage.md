# Compass: self-manage tools (`grokhub-self`, Spike-5c)

## Owns
- Grok's own tools for its skills, connections, and automations: `grokhub --mcp-self` (Grok Build sees `grokhub-self__<tool>`, path A) and the same tools on the native Lab engine (path E).
- One class table, `SELF_TOOLS` (read / soft / delete; a connection with `needs_token` is credentials), the scope guard (`scope_guard`), and the skill-create cap (`SKILL_CREATE_DAY_CAP`).
## Quick commands
- `cargo test -p grokhub-agent self_manage` (table, classes on every path, guard, caps, ledger writes)
- `cargo test -p grokhub-app self_mcp` (server, parks, tokens), then `cargo test -p grokhub-app changes_from_the_self_server_process_show_as_work_rows`
## Key files
- `crates/grokhub-agent/src/self_manage/mod.rs`: `SELF_TOOLS`, `self_class`, `needs_token`, `scope_guard`, `mcp_tools`, `native_schemas`, `run_native`.
- `crates/grokhub-agent/src/self_manage/ops.rs`: what each tool does after the gate (`run`, `SelfCtx`). Skills write through `record_skill_change`; connections and automations reuse the Spike-5b writers in `crates/grokhub-agent/src/tools/connections.rs` and `crates/grokhub-agent/src/tools/control.rs`.
- `crates/grokhub-app/src/self_mcp.rs`: `SelfServer`, `SelfIo`, `run_stdio`, registration (`maybe_register_on_start`).
- `crates/grokhub-app/src/app/change_undo.rs`: `LedgerWatch` reads ledger lines the server process wrote.
## Change recipe
- New tool: add a `SELF_TOOLS` row and its `mcp_tools` entry (same order), its arm in `run`, and a test that asks `harness::decide` for both the bare and `grokhub-self__` names. A delete-class tool also gets a `MCPTool(grokhub-self__…)` line in `HEADLESS_DENY_RULES` (bump the literal count).
- New guarded target: add it to `GUARDED` in `crates/grokhub-agent/src/self_manage/mod.rs`; the guard runs as a hard floor, before any ledger write.
## What breaks it
- A side path around `harness::decide`: the classes live in `hard_class` / `hard_floor` (`crates/grokhub-agent/src/harness/hard.rs`), so paths A, B, C, and E agree.
- A token in args, a span, the ledger, a park file, or `mcp.json`. It comes only from the elicit card and is sealed by `seal_connection_token`; the entry keeps `tokenRef`.
- Writing into the user's `~/.grok`: GrokHub never touches the Grok Build CLI's config or credentials.
- Building an `UndoAsk` here: no tool can undo; Undo stays a click or typing (`only_typing_or_a_click_builds_an_undo_ask`).
## What depends on it
- The cabin's hard cards and park files (`crates/grokhub-app/src/app/harness_ui.rs`), the Work-tree rows (`poll_self_changes`), and the native loop (`dispatch` in `crates/grokhub-agent/src/tools/mod.rs`).
## Non-obvious
- Soft calls follow Grok Build's own pill: GB already asked before it called the server, so the server only parks hard class. Under Always, delete and credentials still park, Enter never approves, and TTL, Halt, or a closed cabin is Deny.
- Connections land in the cabin's native `mcp.json` (the Lab engine's), not GB's config, so Undo rows stay one file.
- GB `--deny` rules can't read args, so a token-needing `connection_add` is listed in `GB_DENY_GAPS`; the server's own park still holds it.
## See also
- [harness](harness.md), [desktop-mcp](desktop-mcp.md), [grokhub-agent](grokhub-agent.md)
