# Compass: desktop MCP (`grokhub-desktop` server)

## Owns
- `grokhub --mcp-desktop`: a stdio JSON-RPC MCP server that Grok Build registers as `grokhub-desktop` (tools appear as `grokhub-desktop__<tool>`): `list_monitors`, `screenshot`, `click`, `move`, `drag`, `scroll`, `type`, `key`.
- Pure protocol, geometry, and key parsing in grokhub-core; the OS backends (X11, Wayland/KDE portal + libei, uinput, ydotool fallback, Windows SendInput + xcap) in grokhub-app.
## Quick commands
- `cargo test -p grokhub-core desktop_mcp` (pure JSON-RPC, geometry, key combos)
- `cargo test -p grokhub-app desktop_mcp` (backend routes with fakes; never opens a portal or `/dev/uinput`)
## Key files
- `crates/grokhub-core/src/desktop_mcp.rs`: `DesktopServer`, `DesktopBackend`, `CallGate`, `DESKTOP_MCP_SERVER`, `DESKTOP_MCP_RULE`, `OFF_MSG` / `HALT_MSG` / `LOCK_MSG`.
- `crates/grokhub-app/src/desktop_mcp/mod.rs`: `run_stdio`, `LiveBackend`, halt stamp, registration.
- `crates/grokhub-app/src/desktop_mcp/harness_gate.rs`: path A pre-check, park, and spans.
- `crates/grokhub-app/src/desktop_mcp/wayland.rs`, `crates/grokhub-app/src/desktop_mcp/x11.rs`, `crates/grokhub-app/src/desktop_mcp/windows.rs`; Linux broker in `crates/grokhub-app/src/desktop_mcp/broker.rs`.
## Change recipe
- New tool: add its schema in `tool_schemas` and its arm in `DesktopServer` dispatch (core), add it to `is_input_tool` if it moves input, extend `DesktopBackend` if the OS must act, implement it in each backend, then classify it in `desk_classify` (harness).
- New Wayland route: add it to the chain in `crates/grokhub-app/src/desktop_mcp/fallback.rs` and drive it with the existing fakes.
## What breaks it
- Any `println!` on the server path: stdout is JSON-RPC only; logs go to stderr.
- Skipping the pre-check: `run_stdio` calls `harness_gate::precheck` before `handle_line` on every OS, even under Always.
- Registering into the user's `~/.grok`: `register_desktop_mcp` targets the cabin `GROK_HOME` only.
## What depends on it
- Grok Build computer use (via `grok mcp add grokhub-desktop -- <exe> --mcp-desktop`), Settings → Let Grok control the desktop, and the harness spans and parked cards in the cabin.
## Non-obvious
- `desktop_control` is re-read from `app.json` for every call, so the Settings switch takes effect without restarting the server. Halt is a stamp file compared with the process start time (`stamp_halts`).
- Coordinates are pixels in the last screenshot of that monitor (or `all`), not physical pixels.
- On Linux the cabin holds the portal session and lock; the MCP process talks to it over `desk.sock` (broker), and a second process cannot open another session.
- An update can move the exe, so startup re-registers unless this exact binary path is in the cabin `config.toml` (TOML doubles Windows backslashes).
- Desktop tools are on the unwatched-Ask deny list (`ASK_DENY_RULES`); Plan and btw deny them even on Auto or Always.
## See also
- [harness](harness.md), [grokhub-acp](grokhub-acp.md), [grokhub-app](grokhub-app.md)
