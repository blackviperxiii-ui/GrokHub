# Compass: desktop MCP (`grokhub-desktop` server)

## Owns
- `grokhub --mcp-desktop`: a stdio JSON-RPC MCP server the native engine reaches as `grokhub-desktop` (tools appear as `grokhub-desktop__<tool>`): `list_monitors`, `screenshot`, `click`, `move`, `drag`, `scroll`, `type`, `key`, `open_app`, `focus_window`, `delete_files`.
- Pure protocol, geometry, and key parsing in grokhub-core; the OS backends (X11, Wayland/KDE portal + libei, uinput, ydotool fallback, Windows SendInput + xcap) in grokhub-app.
- Spike-2a: `grokhub --mcp-cua` (`run_cua_stdio`), registered as `grokhub-cua` only while desktop control and the `cuaDriver` flag in `app.json` are both on (Linux; `cua_wanted`). It starts the pinned `cua-driver mcp --socket <config>/cua/driver.sock` child in `bounded` mode with the cabin manifest, and gates every call in `crates/grokhub-agent/src/harness/cua.rs` before forwarding.
## Quick commands
- `cargo test -p grokhub-core desktop_mcp` (pure JSON-RPC, geometry, key combos)
- `cargo test -p grokhub-app desktop_mcp` (backend routes with fakes; never opens a portal or `/dev/uinput`)
- `cargo test -p grokhub-agent cua` and `cargo test -p grokhub-agent --test cua_proxy` (fake Cua child `crates/grokhub-agent/src/bin/fake_cua.rs`; never a real `cua-driver`)
## Key files
- `crates/grokhub-core/src/desktop_mcp.rs`: `DesktopServer`, `DesktopBackend`, `CallGate`, `DESKTOP_MCP_SERVER`, `DESKTOP_MCP_RULE`, `OFF_MSG` / `HALT_MSG` / `LOCK_MSG`.
- `crates/grokhub-app/src/desktop_mcp/mod.rs`: `run_stdio`, `LiveBackend`, halt stamp, registration.
- `crates/grokhub-app/src/desktop_mcp/harness_gate.rs`: path A pre-check, park, spans, and the `ui_changed` probe (`handle_desk_line`).
- `crates/grokhub-app/src/desktop_mcp/apps.rs`: open an app, list and focus windows, and move files to the trash (Linux: gtk-launch / gio launch, X11 EWMH, a KWin script on KDE Wayland, gio trash / trash-put; Windows: ShellExecuteW, EnumWindows, SetForegroundWindow, SHFileOperationW with undo).
- `crates/grokhub-app/src/desktop_mcp/wayland.rs`, `crates/grokhub-app/src/desktop_mcp/x11.rs`, `crates/grokhub-app/src/desktop_mcp/windows.rs`; Linux broker in `crates/grokhub-app/src/desktop_mcp/broker.rs`.
## Change recipe
- New tool: add its schema in `tool_schemas` and its arm in `DesktopServer` dispatch (core), add it to `is_input_tool` if it moves input, extend `DesktopBackend` if the OS must act, implement it in each backend, then classify it in `desk_classify` (harness).
- New Wayland route: add it to the chain in `crates/grokhub-app/src/desktop_mcp/fallback.rs` and drive it with the existing fakes.
## What breaks it
- Any `println!` on the server path: stdout is JSON-RPC only; logs go to stderr.
- Skipping the pre-check: `run_stdio` calls `harness_gate::handle_desk_line` (pre-check, park, then `handle_line`) on every OS, even under Always.
- Deleting before the park: `delete_files` is hard class Delete and only runs after Jeremy's click; it checks every path first and deletes none if one is bad.
- Writing the Cua gate into user config: `set_cabin_cua` holds it as a native MCP slot that never lands in `mcp.json`. Handing the model the Cua socket, or running Cua in `standard` / `unrestricted` mode (`cua_spawn_env` sets `bounded` and strips `CUA_ENV_REMOVE`).
## What depends on it
- Native engine computer use (`gate::is_desktop` names the `grokhub-desktop__*` tools), Settings → Let Grok control the desktop, and the harness spans and parked cards in the cabin.
## Non-obvious
- `desktop_control` is re-read from `app.json` for every call, so the Settings switch takes effect without restarting the server. Halt is a stamp file compared with the process start time (`stamp_halts`).
- Coordinates are pixels in the last screenshot of that monitor (or `all`), not physical pixels; before any screenshot they are the monitor's native pixels (`COORD_NOTE`).
- On Linux the cabin holds the portal session and lock; the MCP process talks to it over `desk.sock` (broker), and a second process cannot open another session.
- An update can move the exe, so `sync_native_cua` re-reads the current exe path whenever the desktop switch or the engine starts.
- Unattended Ask denies desktop tools (`Gate` `attended: false`); Plan and btw deny them even on Auto or Always.
## See also
- [harness](harness.md), [grokhub-agent](grokhub-agent.md), [grokhub-app](grokhub-app.md)
