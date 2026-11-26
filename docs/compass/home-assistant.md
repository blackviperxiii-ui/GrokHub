# Compass: Home Assistant local devices (Order 973)

## Owns
- First-class Home Assistant path: list entities, read state, list services, call services over the REST API with explicit consent and a keychain token. MCP remains available separately; this is not an MCP-only workaround.
- Pure validation, formatting, and HTTP in `crates/grokhub-core/src/home_assistant.rs`; chat tools in `crates/grokhub-agent/src/tools/home_assistant.rs`; shell I/O in `src-tauri/src/home_assistant.rs` and Settings → PC helpers (`ui/src/PcFiles.tsx`).

## Quick commands
- `cargo test -p grokhub-core home_assistant --locked`
- `cargo test -p grokhub-agent home_assistant --locked`
- `cargo test -p grokhub-shell home_assistant --locked`

## Key files
- `crates/grokhub-core/src/home_assistant.rs`: settings shape, URL/entity gates, blocked domains, REST helpers.
- `crates/grokhub-agent/src/tools/home_assistant.rs`: host trait and the four tools.
- `src-tauri/src/home_assistant.rs`: `app.json` + keychain + host registration.
- `ui/src/PcFiles.tsx`: consent toggle, address, token, test connection.

## Change recipe
- New tool: schema + defs + gate list + dispatch arm + card-line row; bump `native.len` in `run.rs`.
- New blocked domain: add to `BLOCKED_DOMAINS` and a unit test.

## What breaks it
- Shipping tokens or LAN IPs in the repo or changelog.
- Offering tools when `enabled` is false or the token is missing.
- Dialing without `saved_host` / `with_saved_host` (SSRF).

## What depends on it
- Chat tool registry; Settings → Your PC → PC helpers.

## Non-obvious
- Token never leaves the keychain into `app.json` or the view payload (`has_token` only).
- `home_call_service` is CONTROL (asks on Auto/Ask); the three list/get tools are READONLY.

## See also
- [desktop-mcp](desktop-mcp.md), [print-alerts](print-alerts.md), [harness](harness.md)
