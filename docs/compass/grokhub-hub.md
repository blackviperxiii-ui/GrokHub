# Compass: grokhub-hub (LAN `/v1` hub)

## Owns
- The tiny_http server for phones and second boxes: `/v1/health` and `/health` (no auth), `/v1/pair`, then bearer-token routes `/v1/status`, `/v1/snapshot`, `/v1/task` (plus `POST /v1/task/{id}/complete`), `/v1/inbox` (plus `POST /v1/inbox/{id}/ack`), `/v1/results`, `/v1/inhabit`, `/v1/frame`, `/v1/frame.jpg`, `/v1/voice/client-secret`.
- Two entry points with one bootstrap (`run`): the standalone `grokhub-hub` binary and `grokhub --hub`; the cabin embeds it with `serve_lan`.
## Quick commands
- `GROKHUB_HUB_PORT=18766 cargo run -p grokhub-hub` (the `.cursor/environment.json` terminal)
- `cargo test -p grokhub-hub -- --test-threads=1` (loopback binds via `serve_background`)
## Key files
- `crates/grokhub-hub/src/server.rs`: routing, `bearer`, `MAX_BODY` (8 MiB), `MAX_IN_FLIGHT` (64), `serve` / `serve_background` / `serve_lan`.
- `crates/grokhub-hub/src/lib.rs`: `parse_args`, `run` (load state, rotate the pair code, persist every 2 s).
- `crates/grokhub-hub/src/main.rs`: port from `GROKHUB_HUB_PORT`, state path.
- `crates/grokhub-core/src/state.rs` (`HubState`, `DEFAULT_PORT`, `HUB_KIND`, `state_for_disk`) and `crates/grokhub-core/src/pair.rs` (`PAIR_TTL_MS`, `PAIR_MAX_TRIES`).
## Change recipe
- New route: add the `method` + `path` arm in `server.rs` after the token check (unless it must be public), keep hub-lock time short, and add a loopback test like `pair_task_frame_contract`.
- New persisted field: add it to `HubState` and decide in `state_for_disk` whether it may reach `hub-state.json`.
## What breaks it
- Holding `hub.lock()` across JSON parse or socket I/O (`snapshot_and_frame_drop_hub_lock_before_io`).
- Persisting the live frame or `console_api_key` (`disk_omits_frame`).
- Binding before `--version` / `--help` return (`standalone_hub_version_and_help_must_not_bind` slices `crates/grokhub-hub/src/main.rs` at `grokhub_hub::run(`) or printing the pair code (`standalone_hub_rotates_expired_and_does_not_print_the_code` slices `lib.rs` at `pub fn run(`).
## What depends on it
- Android pairing and `/v1/task`, the cabin Devices page, `packaging/systemd/grokhub-hub.service`, and `scripts/install.sh` (enables that unit).
## Non-obvious
- `serve` binds `0.0.0.0`, so the hub is LAN-visible by design; pairing (15 min codes, burned after 10 wrong guesses) is the gate.
- Standalone `main.rs` finds state via `GROKHUB_CONFIG`, then `HOME/.config/GrokHub`, then the cwd; it does not use `%APPDATA%` like `config_dir()` in grokhub-app.
- `HUB_KIND` (`grokhub-hub-v1`) is what `--doctor` and FFI clients probe; changing it is a protocol break.
- The cabin only fills `HubState.snapshot` after the `hub` destination grant or a one-time hard Send approve (`privacy_ui.rs`); revoking clears it. The `/v1/snapshot` route itself is unchanged.
- Voice minting needs a console API key that is never written to `hub-state.json`; tests stub the xAI call.
## See also
- [grokhub-core](grokhub-core.md), [grokhub-ffi](grokhub-ffi.md), [install-scripts](install-scripts.md)
