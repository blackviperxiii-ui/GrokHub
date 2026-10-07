# Compass: self-manage tools (`grokhub-self`, Spike-5c)

## Owns
- Grok's own tools for its skills, connections, and automations: `grokhub --mcp-self` (Grok Build sees `grokhub-self__<tool>`, path A) and the same tools on the native Lab engine (path E).
- One class table, `SELF_TOOLS` (read / soft / delete; a connection that names `secrets` is credentials), the scope guard (`scope_guard`), the skill-create cap (`SKILL_CREATE_DAY_CAP`), and the sealed connection secrets.
## Quick commands
- `cargo test -p grokhub-agent self_manage` (table, classes on every path, guard, caps, ledger writes)
- `cargo test -p grokhub-app self_mcp` (server, parks, secrets), then `cargo test -p grokhub-app self_made_skill_undo_click_removes_it`
## Key files
- `crates/grokhub-agent/src/self_manage/mod.rs`: `SELF_TOOLS`, `self_class`, `scope_guard`, `mcp_tools`, `native_schemas`, `run_native`.
- `crates/grokhub-agent/src/self_manage/ops.rs`: what each tool does after the gate (`run`, `SelfCtx`), through the ChangeLedger writers.
- `crates/grokhub-agent/src/self_manage/secrets.rs`: `seal_secrets`, `open_secrets`, `SECRET_MARK`, `SECRET_ENV_FLAG`.
- `crates/grokhub-app/src/self_mcp.rs`: `SelfServer`, `SelfIo`, `run_stdio`, `run_secret_env`, registration.
## Change recipe
- New tool: add a `SELF_TOOLS` row and its `mcp_tools` entry (same order), its arm in `run`, and a test that asks `harness::decide` for both the bare and `grokhub-self__` names. A delete-class tool also gets a `MCPTool(grokhub-self__…)` line in `HEADLESS_DENY_RULES` (bump the literal count).
- New guarded target: add it to `GUARDED` in `crates/grokhub-agent/src/self_manage/mod.rs`; the guard runs as a hard floor, before any ledger write.
## What breaks it
- A side path around `harness::decide`: the classes live in `hard_class` / `hard_floor` (`crates/grokhub-agent/src/harness/hard.rs`), so paths A, B, C, and E agree.
- A secret value in args, a span, the ledger, a park file, or a connection's config. Values come only from the elicit card and are sealed with the learned-tier key.
- Registering into the user's `~/.grok`: `register_self_mcp` targets the cabin `GROK_HOME` only.
- Building an `UndoAsk` here: no tool can undo; Undo stays a click or typing (`only_typing_or_a_click_builds_an_undo_ask`).
## What depends on it
- The cabin's hard cards and park files (`crates/grokhub-app/src/app/harness_ui.rs`), the skill Undo rows (`crates/grokhub-app/src/app/skill_undo.rs`), and the native loop (`dispatch` in `crates/grokhub-agent/src/tools/mod.rs`).
## Non-obvious
- Soft calls follow Grok Build's own pill: GB already asked before it called the server, so the server only parks hard class. Under Always, delete and credentials still park, Enter never approves, and TTL, Halt, or a closed cabin is Deny.
- A connection with secrets runs as `grokhub --mcp-secret-env <name> -- <command>`; a locked keyring fails closed.
- GB `--deny` rules can't read args, so a secret-bearing `connection_add` is listed in `GB_DENY_GAPS`; the server's own park still holds it.
## See also
- [harness](harness.md), [desktop-mcp](desktop-mcp.md), [grokhub-agent](grokhub-agent.md)
