# Compass: grokhub-agent (native Lab engine)

## Owns
- The native xAI Responses engine: sync HTTP client, agent loop, tools, permission gate and rule engine, MCP client, hooks, skills, plugins, subagents, memory, sessions, unattended runs, and the dry-run eval.
- It is the only engine: every chat, background, and scheduled turn runs here; nothing launches the Grok Build CLI.
## Quick commands
- `cargo test -p grokhub-agent --locked -- --test-threads=1`
- `cargo test -p grokhub-agent --test mcp_transport` (spawns the `fake_mcp` bin)
- `cargo run -p grokhub-agent --example eval` (dry-run; `--live` is refused)
## Key files
- `crates/grokhub-agent/src/lib.rs`: module list and the public surface the app uses.
- `crates/grokhub-agent/src/run.rs` (`run_loop`) and `crates/grokhub-agent/src/events.rs` (loop events to the `AcpEvent`s the cabin polls in `poll_acp`).
- `crates/grokhub-agent/src/gate.rs` (gate v0) and `crates/grokhub-agent/src/perm/mod.rs` (rule engine).
- `crates/grokhub-agent/src/tools/mod.rs`: `tool_schemas`, `schemas_for`, `execute` (read-only), `dispatch` (gated set).
## Change recipe
- New tool: add a module under `crates/grokhub-agent/src/tools/`, register its schema in `schemas_for` on the right side of the `readonly_session` / `desktop` checks, dispatch it, and make sure `gate::decide` and the harness see it.
- Ported code: keep the `// Portions derived from xai-org/grok-build ...` header and add a line to `crates/grokhub-agent/NOTICE`.
## What breaks it
- Loosening a deny: perm rules are deny > ask > allow; hooks can deny or ask but never turn a gate deny or ask into allow; a hook timeout is no decision.
- Reading the Grok CLI's credentials (`~/.grok/auth.json`): the engine uses GrokHub's own sign-in (`XAI_NEED_SIGNIN` in `crates/grokhub-core/src/xai_signin.rs`).
## What depends on it
- grokhub-app: `crates/grokhub-app/src/app/native_engine.rs`, `crates/grokhub-app/src/app/native_sessions.rs`, `crates/grokhub-app/src/app/native_unattended.rs`, `crates/grokhub-app/src/native_mcp.rs`, `crates/grokhub-app/src/native_plugins.rs`, plus the harness and `ToolOutput` in the desktop MCP.
## Non-obvious
- Gate v0 still treats Auto like Ask; Plan and btw stay read-only (`READ_ONLY_PHASE`).
- Unattended runs (`run_unattended`): Ask denies every non-read-only tool; Auto uses the Phase 5 judge and fails closed.
- Subagents: Explore is read-only, General copies the parent gate, depth 2 stops grandchildren, a worktree never falls back to the parent tree.
- Its `memory.rs` keeps `MEMORY.md` plus a rebuildable `index.sqlite` (bundled rusqlite); it never touches USER.md or SOUL.md and is not AMR; its recall pack (`first_turn_injection`) goes through `redact_recall` before the model sees it.
- No tokio: MCP uses one worker thread per stdio child or HTTP read.
- MCP browser sign-in (`crates/grokhub-agent/src/mcp/oauth.rs`, pure parts in `crates/grokhub-core/src/mcp_oauth.rs`, PKCE in `crates/grokhub-core/src/pkce.rs`): a remote server with no `Authorization` on its entry signs in from its Native MCP row. The sign-in is sealed by `seal_mcp_signin` (keyring key, next to the connection tokens), refreshed a minute before expiry, and refreshed once on a 401 before the row asks for a new sign-in. An entry that carries its own header or `tokenRef` never uses it.
## See also
- [harness](harness.md), [desktop-mcp](desktop-mcp.md), [slash](slash.md), [app-config](app-config.md)
