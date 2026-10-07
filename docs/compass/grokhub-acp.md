# Compass: grokhub-acp (Grok Build CLI client)

## Owns
- Everything that spawns or reads the `grok` CLI: ACP `grok agent stdio` sessions (`connect`), headless `grok -p --output-format streaming-json` turns (`spawn_grok_p_stream`), CLI discovery and first-run alpha install, the catalog (`grok inspect`, `grok mcp`, skills, plugins), `grok sessions` History, and desktop MCP registration.
- `PermissionMode` (Ask / Auto / Always) and `SessionMode` (Chat / Plan / btw) and how each maps to argv.
## Quick commands
- `cargo test -p grokhub-acp --locked -- --test-threads=1`
- `cargo test -p grokhub-acp --test fake_agent` (drives the `grokhub-fake-acp` bin; set `FAKE_ACP_*` env in the test)
- `cargo run -p grokhub-acp --example ping`
## Key files
- `crates/grokhub-acp/src/client.rs`: `SpawnOpts`, `connect`, `AcpHandle`, `spawn_grok_p_stream`, `run_single_turn`, session listing.
- `crates/grokhub-acp/src/locate.rs`: `find_grok`, `cabin_grok_home`, `cabin_leader_socket`, `with_ask_deny`, `with_hard_deny`, `CLI_CREDENTIAL_DENY`, `register_desktop_mcp`.
- `crates/grokhub-acp/src/protocol.rs`: JSON-RPC shapes, `AcpEvent`, `PermissionMode::uses_acp` / `scheduled_args`, `ASK_ACP_DOWN`.
- `crates/grokhub-acp/src/stream.rs` (streaming-json events) and `crates/grokhub-acp/src/install.rs` (alpha install).
## Change recipe
- New spawn option: add the field to `SpawnOpts`, then fill it in every struct literal, including `fake_opts` in `crates/grokhub-acp/tests/fake_agent.rs` and the app callers.
- New CLI flag: add it where argv is built in `crates/grokhub-acp/src/locate.rs` and keep the deny helpers append-only: they add `--deny` pairs, never an allow.
## What breaks it
- Sharing `~/.grok/leader.sock` or `~/.grok` for the cabin child: the CLI leader SIGTERMs it (exit 143). Use `cabin_grok_home` and `cabin_leader_socket`.
- An Ask turn that falls through to `grok -p`: Ask needs live ACP; if ACP is down the turn is denied (`ASK_ACP_DOWN`).
- Removing guarded strings: `client.rs` and `locate.rs` tests `include_str!` themselves and split on fn names like `pub fn connect(` and `fn grok_p_once(`.
## What depends on it
- grokhub-app (chat kick, background runs, settings, History, desktop MCP registration) and grokhub-agent (`CLI_CREDENTIAL_DENY`, ACP-shaped events).
## Non-obvious
- `GROKHUB_GROK` overrides CLI discovery; tests that touch it or `PATH` hold `grok_env_test_lock`.
- History and the catalog read the user's `~/.grok` (`grok_user_stdout_timeout`); the chat child runs in the cabin home. `--resume` must look where the session lives.
- An unanswered client-bound request (`fs/readTextFile`, `terminal/*`) makes Grok Build close stdio, so reply with `method_not_found`.
- On a box with a logged-in `grok`, app tests that assume "no CLI" can fail; isolate HOME, PATH and `GROKHUB_CONFIG`.
- `prepare_cabin_grok_home` symlinks the CLI login into the cabin home (a copy on Windows, stale after a later `grok login`). Agents still never read or copy `~/.grok/auth.json` (`CLAUDE.md`, Secrets).
## See also
- [harness](harness.md), [desktop-mcp](desktop-mcp.md), [install-scripts](install-scripts.md), [grokhub-app](grokhub-app.md)
