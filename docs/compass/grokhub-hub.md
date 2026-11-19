# Compass: grokhub-hub (LAN `/v1` hub)

## Owns
- The tiny_http server for paired computers: `/v1/health` and `/health` (no auth), `/v1/pair`, then bearer-token routes `/v1/status`, `/v1/snapshot` (`/sync`), `/v1/inhabit`, `/v1/frame`, `/v1/fleet` (machine cards, `grokhub-core` `fleet`). The phone routes (task, inbox, results, frame.jpg, voice client-secret) were removed 2026-10-07.
- Two entry points with one bootstrap (`run`): the standalone `grokhub-hub` binary and `grokhub --hub`; the shell embeds it with `serve_lan`.
- Opt-in `--mcp-bridge` stdio MCP (`grokhub-bridge`): read-only unsealed AMR `memory_search` and `automation_status` for local IDE clients. Never opens `.sealed` nodes; never listens on a port.
## Quick commands
- `GROKHUB_HUB_PORT=18766 cargo run -p grokhub-hub` (the `.cursor/environment.json` terminal)
- `cargo test -p grokhub-hub -- --test-threads=1` (loopback binds via `serve_background`)
- `cargo test -p grokhub-hub mcp_bridge` (sealed denial + bridge tools)
## Key files
- `crates/grokhub-hub/src/server.rs`: routing, `bearer`, `MAX_BODY` (8 MiB), `MAX_IN_FLIGHT` (64), `serve` / `serve_background` / `serve_lan`.
- `crates/grokhub-hub/src/lib.rs`: `parse_args`, `run` (load state, rotate the pair code, persist every 2 s).
- `crates/grokhub-hub/src/mcp_bridge.rs`: `--mcp-bridge` stdio server (`search_unsealed`, `automation_status`).
- `crates/grokhub-hub/src/main.rs`: port from `GROKHUB_HUB_PORT`, state path.
- `crates/grokhub-hub/src/bin/grokhub.rs`: shim forwards `--mcp-bridge` to `grokhub-hub`.
- `crates/grokhub-core/src/state.rs` (`HubState`, `DEFAULT_PORT`, `state_for_disk`; `HUB_KIND` lives in `crates/grokhub-proto/src/lib.rs`) and `crates/grokhub-core/src/pair.rs` (`PAIR_TTL_MS`, `PAIR_MAX_TRIES`).
## Change recipe
- New route: add the `method` + `path` arm in `server.rs` after the token check (unless it must be public), keep hub-lock time short, and add a loopback test like `pair_frame_contract`.
- New persisted field: add it to `HubState` and decide in `state_for_disk` whether it may reach `hub-state.json`.
## What breaks it
- Holding `hub.lock()` across JSON parse or socket I/O (`snapshot_and_frame_drop_hub_lock_before_io`).
- Persisting the live frame (`disk_omits_frame`), or an old `hub-state.json` failing to load and getting quarantined (`old_hub_state_with_phone_rows_still_loads`).
- Binding before `--version` / `--help` return (`standalone_hub_version_and_help_must_not_bind` slices `crates/grokhub-hub/src/main.rs` at `grokhub_hub::run(`) or printing the pair code (`standalone_hub_rotates_expired_and_does_not_print_the_code` slices `lib.rs` at `pub fn run(`).
## What depends on it
- Computer-to-computer pairing and `/sync`, the Devices page, `packaging/systemd/grokhub-hub.service`, and `scripts/install.sh` (enables that unit).
## Non-obvious
- `serve` binds `0.0.0.0`, so the hub is LAN-visible by design; pairing (15 min codes, burned after 10 wrong guesses) is the gate.
- Standalone `main.rs` finds state via `GROKHUB_CONFIG`, then `HOME/.config/GrokHub`, then the cwd; it does not use `%APPDATA%` like `config_dir()` in `crates/grokhub-core/src/paths.rs`.
- `HUB_KIND` (`grokhub-hub-v1`) is what `--doctor` and paired computers probe; changing it is a protocol break.
- The shell only fills `HubState.snapshot` when a peer is paired and after the `hub` destination grant or a one-time hard Send approve; revoking clears it. The `/v1/snapshot` route itself is unchanged.
- Local inbox helpers (`enqueue_local`) queue on this computer only; no route lets a peer add or read tasks (`phone_routes_are_gone`). Desktop `/send` starts a read-only background run, not a queued task; `enqueue_local` stays for tests.
## See also
- [grokhub-core](grokhub-core.md), [install-scripts](install-scripts.md)
