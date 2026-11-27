# Browser Companion (Phone Browser via Web Access)

Follow-up from [Companion Device Nodes](./README.md) (PRs #2169, #2171, #2177) so a phone or tablet browser can act as a companion device (`camera`, `location`, `talk`, `canvas`) without installing a native binary or weakening the desktop's Origin guarantees.

## Architecture Decisions (RPE)

### 1. TLS Story: Reuse Web Access HTTPS
Browsers only expose `navigator.mediaDevices.getUserMedia` (camera / microphone), `navigator.geolocation`, and Web Speech APIs inside a **secure context** (`https://` or `http://127.0.0.1` / `http://localhost`). Plain `http://<lan-ip>:18767` fails browser capability checks even on a private LAN.

Instead of terminating a second self-signed TLS certificate on port `18767`:
- Browser companions reuse **Web access's HTTPS listener** (`src-tauri/src/remote_tls.rs` + `src-tauri/src/remote.rs`, default port `4620`, persisted self-signed ECDSA certificate in `<config>/web-access/`, SHA-256 fingerprint shown in Settings -> Web access).
- Once the user verifies the fingerprint and accepts Web access in their phone browser, `/node/v1/{pair,hello,next,reply,event}` shares that same HTTPS origin (`https://<lan-ip>:4620`) and satisfies `PAGE_CSP` (`connect-src 'self'`).
- Native CLI companions (`grokhub-hub node`) continue dialing `18767` directly over the LAN with no `Origin` header.

### 2. Explicit Single-Origin Allowlist on `NodeHost`
`NodeHost` keeps `allowed_origin: None` by default, so any web page request carrying an `Origin` header continues receiving `403` without `Access-Control-*` headers.
- When configured via `NodeHost::set_allowed_origin(Some(origin))`, `normalize_origin` accepts only `https://<host>[:<port>]` or loopback `http://127.0.0.1[:<port>]` / `http://localhost[:<port>]` / `http://[::1][:<port>]` (never `*`, paths, queries, or plain non-loopback `http://`).
- Requests to `/node/v1/{health,pair,hello,next,reply,event}` whose `Origin` header equals the configured `allowed_origin` receive:
  - `204` on `OPTIONS` preflight with `Access-Control-Allow-Origin: <origin>`, `Access-Control-Allow-Methods: GET, POST, OPTIONS`, `Access-Control-Allow-Headers: Authorization, Content-Type`, `Vary: Origin`.
  - Exact `Access-Control-Allow-Origin: <origin>` and `Vary: Origin` on JSON replies.
- Any request whose `Origin` header is absent behaves identically to a native companion (no CORS headers). Any mismatched `Origin` receives `403` with no `Access-Control-*` headers.
- `/node/v1/mcp/<id>` **always** rejects every `Origin` header (`from_web_page(&req)`), even the allowlisted origin, and remains strictly loopback + `local_key`.

### 3. Web Access Proxy (`src-tauri/src/remote.rs`)
Web access only proxies the five companion client routes (`/node/v1/pair`, `/hello`, `/next`, `/reply`, `/event`) over loopback `127.0.0.1:<companion_port>` when both Web access and Companions are running:
- Requests with a mismatched `Origin` (anything other than the active Web access origin) are rejected with `403`.
- Requests to `/node/v1/mcp/*` or any unknown path return `404` and never touch the MCP relay.
- `/node/v1/next` releases its in-flight concurrency slot (`drop(slot)`) before long-polling so active companions do not starve Web access UI calls.

### 4. Two-Yes Consent Parity
Browser companions share `grokhub_core::companion_nodes::Book` and the exact pair -> approve -> per-capability rules:
1. Pairing (`platform: "browser"`) yields an unapproved record; `/next` returns `{ ok: true, pending: true }` and zero tools can run until the desktop user clicks Approve under **Devices -> Companions**.
2. Browser companions advertise nothing until the browser user opts in to specific capabilities (`camera`, `location`, `talk`, `canvas`), and only the intersection of browser offers and desktop `allowed` switches is callable via `tools/list` and `tools/call`.
