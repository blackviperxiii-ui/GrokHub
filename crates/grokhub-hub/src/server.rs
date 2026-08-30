use grokhub_core::frame::{get_jpeg, FrameGet};
use grokhub_core::inhabit::InhabitBundle;
use grokhub_core::task::Receipt;
use grokhub_core::{CompleteError, HubState, HUB_KIND};
use serde::Serialize;
use serde_json::{json, Value};
use std::io::Read;
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const MAX_BODY: usize = 8 * 1024 * 1024;

pub fn serve(state: Arc<Mutex<HubState>>, port: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let server = Server::http(("0.0.0.0", port))?;
    accept_loop(state, server);
    Ok(())
}

/// Loopback bind for tests. Returns the bound port.
pub fn serve_background(state: Arc<Mutex<HubState>>, port: u16) -> Result<u16, String> {
    serve_bind(state, "127.0.0.1", port)
}

/// LAN bind for the native cabin. Android pairs against this.
pub fn serve_lan(state: Arc<Mutex<HubState>>, port: u16) -> Result<u16, String> {
    serve_bind(state, "0.0.0.0", port)
}

fn serve_bind(state: Arc<Mutex<HubState>>, host: &str, port: u16) -> Result<u16, String> {
    let server = Server::http((host, port)).map_err(|e| e.to_string())?;
    let bound = server.server_addr().to_ip().map(|a| a.port()).unwrap_or(port);
    std::thread::spawn(move || {
        accept_loop(state, server);
    });
    Ok(bound)
}

fn accept_loop(state: Arc<Mutex<HubState>>, server: Server) {
    for req in server.incoming_requests() {
        let state = state.clone();
        std::thread::spawn(move || {
            let _ = handle(&state, req);
        });
    }
}

fn handle(state: &Arc<Mutex<HubState>>, mut req: Request) -> Result<(), ()> {
    let method = req.method().clone();
    let url = req.url().to_string();
    let (path, query) = split_url(&url);
    if method == Method::Options {
        return send(req, 204, "text/plain", b"");
    }
    if method == Method::Get && (path == "/v1/health" || path == "/health") {
        let name = state.lock().ok().map(|s| s.device_name.clone()).unwrap_or_default();
        return send_json(req, 200, json!({ "ok": true, "kind": HUB_KIND, "name": name }));
    }
    if method == Method::Post && path == "/v1/pair" {
        let body = read_json(&mut req);
        let mut st = state.lock().map_err(|_| ())?;
        let code = body.get("code").and_then(|v| v.as_str()).unwrap_or("");
        let device_id = body.get("deviceId").and_then(|v| v.as_str()).unwrap_or("");
        let device_name = body.get("deviceName").and_then(|v| v.as_str()).unwrap_or("Computer");
        let result = st.pair_with(code, device_id, device_name);
        let hub_id = st.device_id.clone();
        let hub_name = st.device_name.clone();
        drop(st);
        return match result {
            Ok(peer) => send_json(
                req,
                200,
                json!({
                    "ok": true,
                    "token": peer.token,
                    "deviceId": peer.id,
                    "hub": { "id": hub_id, "name": hub_name }
                }),
            ),
            Err(grokhub_core::state::PairError::NoCode) => send_json(
                req,
                400,
                json!({ "ok": false, "error": "No active pairing code — generate one on the host." }),
            ),
            Err(grokhub_core::state::PairError::Mismatch) => send_json(
                req,
                403,
                json!({ "ok": false, "error": "Pairing code does not match." }),
            ),
            Err(grokhub_core::state::PairError::ReservedId) => send_json(
                req,
                403,
                json!({ "ok": false, "error": "That device id is reserved by the hub." }),
            ),
        };
    }

    let token = bearer(&req);
    let mut st = state.lock().map_err(|_| ())?;
    let Some(peer_id) = st.peer_for_token(&token).map(|p| p.id.clone()) else {
        drop(st);
        return send_json(
            req,
            401,
            json!({ "ok": false, "error": "Pair this computer first (Settings → Devices)." }),
        );
    };
    if let Some(p) = st.peer_for_token_mut(&token) {
        p.last_seen = grokhub_core::now_ms();
    }
    let peer = st.peer_for_token(&token).cloned().ok_or(())?;

    if method == Method::Get && path == "/v1/status" {
        let peers: Vec<Value> = std::iter::once(json!({
            "id": st.device_id, "name": st.device_name, "role": "hub"
        }))
        .chain(st.peers.iter().map(|p| json!({ "id": p.id, "name": p.name, "role": "peer" })))
        .collect();
        let body = json!({
            "ok": true,
            "hub": { "id": st.device_id, "name": st.device_name },
            "you": { "id": peer.id, "name": peer.name },
            "peers": peers
        });
        drop(st);
        return send_json(req, 200, body);
    }

    if method == Method::Get && path == "/v1/snapshot" {
        let snapshot = st.snapshot.clone();
        drop(st);
        #[derive(Serialize)]
        struct Body<'a> {
            ok: bool,
            snapshot: Option<&'a Value>,
        }
        return send_json(req, 200, Body {
            ok: true,
            snapshot: snapshot.as_deref(),
        });
    }
    if method == Method::Put && path == "/v1/snapshot" {
        drop(st);
        let body = read_json(&mut req);
        let snap = body.get("snapshot").cloned().unwrap_or(body);
        let local = {
            let st = state.lock().map_err(|_| ())?;
            st.snapshot.clone()
        };
        match grokhub_core::merge_put_snapshot(local.as_deref(), snap) {
            Ok(merged) => {
                let mut st = state.lock().map_err(|_| ())?;
                st.snapshot = Some(std::sync::Arc::new(merged));
                st.last_incoming_at = grokhub_core::now_ms();
                drop(st);
                return send_json(req, 200, json!({ "ok": true }));
            }
            Err(e) => return send_json(req, 400, json!({ "ok": false, "error": e })),
        }
    }

    if method == Method::Post && path == "/v1/task" {
        drop(st);
        let body = read_json(&mut req);
        let prompt = body.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
        let title = body.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let target = body.get("targetDeviceId").and_then(|v| v.as_str()).unwrap_or("");
        let mut st = state.lock().map_err(|_| ())?;
        let result = st.enqueue_task(&peer, target, title, prompt);
        drop(st);
        return match result {
            Ok(t) => send_json(
                req,
                200,
                json!({ "ok": true, "task": { "id": t.id, "targetDeviceId": t.target_device_id } }),
            ),
            Err(e) => send_json(req, 400, json!({ "ok": false, "error": e })),
        };
    }

    if method == Method::Get && path == "/v1/inbox" {
        let tasks = st.queued_for(&peer_id);
        drop(st);
        return send_json(req, 200, json!({ "ok": true, "tasks": tasks }));
    }

    if let Some(id) = strip_prefix_suffix(&path, "/v1/inbox/", "/ack") {
        if method == Method::Post {
            let result = st.ack_inbox(id, &peer_id);
            drop(st);
            return match result {
                Ok(()) => send_json(req, 200, json!({ "ok": true })),
                Err(CompleteError::NotFound) => send_json(
                    req,
                    404,
                    json!({ "ok": false, "error": "task not found" }),
                ),
                Err(CompleteError::Forbidden) => send_json(
                    req,
                    403,
                    json!({ "ok": false, "error": "not the task target" }),
                ),
            };
        }
    }

    if let Some(id) = strip_prefix_suffix(&path, "/v1/task/", "/complete") {
        if method == Method::Post {
            drop(st);
            let body = read_json(&mut req);
            let result = body.get("result").and_then(|v| v.as_str()).unwrap_or("");
            let status = body.get("status").and_then(|v| v.as_str());
            let receipts: Vec<Receipt> = body
                .get("receipts")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let mut st = state.lock().map_err(|_| ())?;
            let done = st.complete_task(&peer_id, id, result, receipts, status);
            drop(st);
            return match done {
                Ok(t) => send_json(req, 200, json!({ "ok": true, "task": t })),
                Err(CompleteError::NotFound) => send_json(
                    req,
                    404,
                    json!({ "ok": false, "error": "task not found" }),
                ),
                Err(CompleteError::Forbidden) => send_json(
                    req,
                    403,
                    json!({ "ok": false, "error": "not the task target" }),
                ),
            };
        }
    }

    if let Some(id) = path.strip_prefix("/v1/task/") {
        if method == Method::Get && !id.contains('/') {
            let task = st.get_task(id, &peer_id).cloned();
            drop(st);
            return match task {
                Some(t) => send_json(req, 200, json!({ "ok": true, "task": t })),
                None => send_json(req, 404, json!({ "ok": false, "error": "task not found" })),
            };
        }
    }

    if method == Method::Get && path == "/v1/results" {
        let tasks = st.claim_results(&peer_id);
        drop(st);
        return send_json(req, 200, json!({ "ok": true, "tasks": tasks }));
    }

    if method == Method::Post && path == "/v1/inhabit" {
        drop(st);
        let body = read_json(&mut req);
        let raw = body.get("bundle").cloned().unwrap_or(body);
        let bundle: InhabitBundle = match serde_json::from_value(raw) {
            Ok(b) if grokhub_core::inhabit_bundle_usable(&b) => b,
            _ => {
                return send_json(
                    req,
                    400,
                    json!({ "ok": false, "error": "invalid inhabit bundle" }),
                );
            }
        };
        let mut st = state.lock().map_err(|_| ())?;
        st.store_inhabit(bundle, &peer);
        drop(st);
        return send_json(req, 200, json!({ "ok": true }));
    }
    if method == Method::Get && path == "/v1/inhabit" {
        if !grokhub_core::inhabit_claim_allowed(&peer.name) {
            drop(st);
            return send_json(
                req,
                403,
                json!({ "ok": false, "error": "inhabit is not for the phone" }),
            );
        }
        let bundle = st.claim_inhabit(&peer);
        drop(st);
        return send_json(req, 200, json!({ "ok": true, "bundle": bundle }));
    }

    if method == Method::Post && path == "/v1/frame" {
        drop(st);
        let body = read_json(&mut req);
        let url = body
            .get("dataUrl")
            .or_else(|| body.get("jpeg"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let frame = grokhub_core::store_frame(&url, grokhub_core::now_ms());
        let mut st = state.lock().map_err(|_| ())?;
        if let Some(f) = frame {
            st.install_frame(f);
        }
        drop(st);
        return send_json(req, 200, json!({ "ok": true }));
    }
    if method == Method::Get && path == "/v1/frame" {
        let frame = st.last_frame.clone();
        drop(st);
        #[derive(Serialize)]
        struct Body<'a> {
            ok: bool,
            frame: Option<&'a grokhub_core::PresenceFrame>,
        }
        return send_json(req, 200, Body {
            ok: true,
            frame: frame.as_deref(),
        });
    }
    if method == Method::Get && path == "/v1/frame.jpg" {
        let since = query
            .split('&')
            .find_map(|p| p.strip_prefix("since="))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let frame = st.last_frame.clone();
        drop(st);
        match get_jpeg(frame.as_deref(), since) {
            FrameGet::Missing => {
                return send_json(req, 404, json!({ "ok": false, "error": "no frame" }));
            }
            FrameGet::NotModified { at } => {
                return send_raw(req, 304, "text/plain", b"", &[("x-grokhub-frame-at", &at.to_string())]);
            }
            FrameGet::Bytes { mime, buf, at } => {
                return send_raw(req, 200, &mime, &buf, &[("x-grokhub-frame-at", &at.to_string())]);
            }
        }
    }

    if method == Method::Post && path == "/v1/voice/client-secret" {
        if let Some(err) = grokhub_core::voice_client_secret_denied(grokhub_core::realtime_can_connect(
            &st.console_api_key,
        )) {
            drop(st);
            return send_json(req, 400, json!({ "ok": false, "error": err }));
        }
        let key = st.console_api_key.clone();
        let mint = st.mint_realtime.clone();
        drop(st);
        let minted = match mint {
            Some(f) => (f.0)(&key),
            None => Err("Cabin mint not wired".into()),
        };
        return match minted {
            Ok(v) => match grokhub_core::parse_client_secret(&v).filter(|s| !s.is_empty()) {
                Some(secret) => send_json(
                    req,
                    200,
                    json!({
                        "ok": true,
                        "value": secret,
                        "wsProtocol": grokhub_core::client_secret_ws_protocol(&secret),
                        "url": grokhub_core::voice_session_url(""),
                        "clientSecret": v
                    }),
                ),
                None => send_json(
                    req,
                    502,
                    json!({ "ok": false, "error": "empty client secret" }),
                ),
            },
            Err(e) => send_json(req, 502, json!({ "ok": false, "error": e })),
        };
    }

    drop(st);
    send_json(req, 404, json!({ "ok": false, "error": "unknown hub route" }))
}

fn split_url(url: &str) -> (String, String) {
    let raw = url.split('#').next().unwrap_or(url);
    let (p, q) = raw.split_once('?').unwrap_or((raw, ""));
    let path = p.trim_end_matches('/').to_string();
    let path = if path.is_empty() { "/".into() } else { path };
    (path, q.to_string())
}

fn strip_prefix_suffix<'a>(path: &'a str, pre: &str, suf: &str) -> Option<&'a str> {
    path.strip_prefix(pre)?.strip_suffix(suf).filter(|s| !s.is_empty() && !s.contains('/'))
}

fn bearer(req: &Request) -> String {
    req.headers()
        .iter()
        .find(|h| h.field.equiv("Authorization"))
        .map(|h| h.value.as_str().to_string())
        .and_then(|v| {
            v.strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))
                .map(|s| s.to_string())
        })
        .unwrap_or_default()
}

fn read_json(req: &mut Request) -> Value {
    let mut buf = Vec::new();
    let _ = req.as_reader().take(MAX_BODY as u64).read_to_end(&mut buf);
    serde_json::from_slice(&buf).unwrap_or(json!({}))
}

fn cors_headers() -> Vec<Header> {
    vec![
        Header::from_bytes(&b"access-control-allow-origin"[..], &b"*"[..]).unwrap(),
        Header::from_bytes(
            &b"access-control-allow-headers"[..],
            &b"authorization, content-type"[..],
        )
        .unwrap(),
        Header::from_bytes(
            &b"access-control-allow-methods"[..],
            &b"GET,POST,PUT,OPTIONS"[..],
        )
        .unwrap(),
        Header::from_bytes(&b"cache-control"[..], &b"no-store"[..]).unwrap(),
    ]
}

fn send_json(req: Request, status: u16, body: impl Serialize) -> Result<(), ()> {
    let s = serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec());
    send(req, status, "application/json; charset=utf-8", &s)
}

fn send(req: Request, status: u16, ctype: &str, body: &[u8]) -> Result<(), ()> {
    send_raw(req, status, ctype, body, &[])
}

fn send_raw(
    req: Request,
    status: u16,
    ctype: &str,
    body: &[u8],
    extra: &[(&str, &str)],
) -> Result<(), ()> {
    let mut headers = cors_headers();
    if let Ok(h) = Header::from_bytes(&b"content-type"[..], ctype.as_bytes()) {
        headers.push(h);
    }
    for (k, v) in extra {
        if let Ok(h) = Header::from_bytes(k.as_bytes(), v.as_bytes()) {
            headers.push(h);
        }
    }
    let resp = Response::new(StatusCode(status), headers, body, Some(body.len()), None);
    req.respond(resp).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::HubState;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::{Arc, Mutex};

    fn http(port: u16, req: &str) -> (u16, String, Vec<u8>) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(req.as_bytes()).unwrap();
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).unwrap();
        let text = String::from_utf8_lossy(&buf).into_owned();
        let status = text
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let body = if let Some(i) = text.find("\r\n\r\n") {
            buf[i + 4..].to_vec()
        } else {
            buf
        };
        (status, text, body)
    }

    #[test]
    fn pair_task_frame_contract() {
        let mut st = HubState::empty();
        let code = st.rotate_pair().code;
        let state = Arc::new(Mutex::new(st));
        let port = serve_background(state, 0).expect("bind");
        std::thread::sleep(std::time::Duration::from_millis(40));

        let (st_h, _, body) = http(
            port,
            "GET /v1/health HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n",
        );
        assert_eq!(st_h, 200);
        assert!(String::from_utf8_lossy(&body).contains(HUB_KIND));

        let pair_body = format!(
            r#"{{"code":"{code}","deviceId":"d-test","deviceName":"Pixel"}}"#
        );
        let req = format!(
            "POST /v1/pair HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{pair_body}",
            pair_body.len()
        );
        let (st_p, _, body) = http(port, &req);
        assert_eq!(st_p, 200, "{}", String::from_utf8_lossy(&body));
        let v: Value = serde_json::from_slice(&body).unwrap();
        let token = v["token"].as_str().unwrap();
        let hub_id = v["hub"]["id"].as_str().unwrap();

        let task_body = format!(r#"{{"targetDeviceId":"{hub_id}","prompt":"flash the pi"}}"#);
        let req = format!(
            "POST /v1/task HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{task_body}",
            task_body.len()
        );
        let (st_t, _, body) = http(port, &req);
        assert_eq!(st_t, 200, "{}", String::from_utf8_lossy(&body));
        let task: Value = serde_json::from_slice(&body).unwrap();
        let tid = task["task"]["id"].as_str().unwrap();

        let (st_g, _, body) = http(
            port,
            &format!("GET /v1/task/{tid} HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\n\r\n"),
        );
        assert_eq!(st_g, 200);
        assert!(String::from_utf8_lossy(&body).contains("flash the pi"));

        let complete = r#"{"result":"nope","status":"done"}"#;
        let req = format!(
            "POST /v1/task/{tid}/complete HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{complete}",
            complete.len()
        );
        let (st_c, _, body) = http(port, &req);
        assert_eq!(
            st_c, 403,
            "sender must not complete a hub-targeted task: {}",
            String::from_utf8_lossy(&body)
        );

        let png = r#"{"dataUrl":"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="}"#;
        let req = format!(
            "POST /v1/frame HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{png}",
            png.len()
        );
        assert_eq!(http(port, &req).0, 200);
        let (st_j, headers, _) = http(
            port,
            &format!("GET /v1/frame.jpg HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\n\r\n"),
        );
        assert_eq!(st_j, 200);
        assert!(headers.to_ascii_lowercase().contains("x-grokhub-frame-at"));

        let req = format!(
            "POST /v1/voice/client-secret HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nContent-Length: 0\r\n\r\n"
        );
        let (st_v, _, body) = http(port, &req);
        assert_eq!(st_v, 400, "{}", String::from_utf8_lossy(&body));
        let msg = String::from_utf8_lossy(&body).to_ascii_lowercase();
        assert!(
            msg.contains("console") || msg.contains("api key"),
            "{}",
            String::from_utf8_lossy(&body)
        );
    }

    #[test]
    fn mints_ephemeral_without_hitting_xai() {
        let mut st = HubState::empty();
        let code = st.rotate_pair().code;
        st.console_api_key = "xai-test-key".into();
        st.mint_realtime = Some(grokhub_core::MintRealtimeFn(std::sync::Arc::new(
            |_key: &str| {
                Ok(json!({
                    "value": "ek_test_secret",
                    "expires_at": 1
                }))
            },
        )));
        let state = Arc::new(Mutex::new(st));
        let port = serve_background(state, 0).expect("bind");
        std::thread::sleep(std::time::Duration::from_millis(40));
        let pair_body = format!(
            r#"{{"code":"{code}","deviceId":"d-voice","deviceName":"Pixel"}}"#
        );
        let req = format!(
            "POST /v1/pair HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{pair_body}",
            pair_body.len()
        );
        let (_, _, body) = http(port, &req);
        let v: Value = serde_json::from_slice(&body).unwrap();
        let token = v["token"].as_str().unwrap();
        let req = format!(
            "POST /v1/voice/client-secret HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nContent-Length: 0\r\n\r\n"
        );
        let (st_v, _, body) = http(port, &req);
        assert_eq!(st_v, 200, "{}", String::from_utf8_lossy(&body));
        let secret: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(secret["ok"], true);
        assert_eq!(secret["value"], "ek_test_secret");
        assert_eq!(secret["wsProtocol"], "xai-client-secret.ek_test_secret");
        assert!(secret["url"].as_str().unwrap().contains("grok-voice-think-fast-2.0"));
    }

    #[test]
    fn voice_mint_requires_pair_token() {
        let state = Arc::new(Mutex::new(HubState::empty()));
        let port = serve_background(state, 0).expect("bind");
        std::thread::sleep(std::time::Duration::from_millis(40));
        let (st_v, _, body) = http(
            port,
            "POST /v1/voice/client-secret HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\n\r\n",
        );
        assert_eq!(st_v, 401, "{}", String::from_utf8_lossy(&body));
        assert!(
            String::from_utf8_lossy(&body)
                .to_ascii_lowercase()
                .contains("pair"),
            "{}",
            String::from_utf8_lossy(&body)
        );
    }

    #[test]
    fn voice_mint_reports_upstream_failure() {
        let mut st = HubState::empty();
        let code = st.rotate_pair().code;
        st.console_api_key = "xai-test-key".into();
        st.mint_realtime = Some(grokhub_core::MintRealtimeFn(std::sync::Arc::new(|_key: &str| {
            Err("xAI refused".into())
        })));
        let state = Arc::new(Mutex::new(st));
        let port = serve_background(state, 0).expect("bind");
        std::thread::sleep(std::time::Duration::from_millis(40));
        let pair_body = format!(
            r#"{{"code":"{code}","deviceId":"d-fail","deviceName":"Pixel"}}"#
        );
        let req = format!(
            "POST /v1/pair HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{pair_body}",
            pair_body.len()
        );
        let (_, _, body) = http(port, &req);
        let v: Value = serde_json::from_slice(&body).unwrap();
        let token = v["token"].as_str().unwrap();
        let req = format!(
            "POST /v1/voice/client-secret HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nContent-Length: 0\r\n\r\n"
        );
        let (st_v, _, body) = http(port, &req);
        assert_eq!(st_v, 502, "{}", String::from_utf8_lossy(&body));
        assert!(
            String::from_utf8_lossy(&body).contains("xAI refused"),
            "{}",
            String::from_utf8_lossy(&body)
        );
    }

    #[test]
    fn voice_mint_rejects_empty_secret() {
        let mut st = HubState::empty();
        let code = st.rotate_pair().code;
        st.console_api_key = "xai-test-key".into();
        st.mint_realtime = Some(grokhub_core::MintRealtimeFn(std::sync::Arc::new(|_key: &str| {
            Ok(serde_json::json!({ "unexpected": true }))
        })));
        let state = Arc::new(Mutex::new(st));
        let port = serve_background(state, 0).expect("bind");
        std::thread::sleep(std::time::Duration::from_millis(40));
        let pair_body = format!(
            r#"{{"code":"{code}","deviceId":"d-empty","deviceName":"Pixel"}}"#
        );
        let req = format!(
            "POST /v1/pair HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{pair_body}",
            pair_body.len()
        );
        let (_, _, body) = http(port, &req);
        let v: Value = serde_json::from_slice(&body).unwrap();
        let token = v["token"].as_str().unwrap();
        let req = format!(
            "POST /v1/voice/client-secret HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nContent-Length: 0\r\n\r\n"
        );
        let (st_v, _, body) = http(port, &req);
        assert_eq!(st_v, 502, "{}", String::from_utf8_lossy(&body));
        assert!(
            String::from_utf8_lossy(&body).contains("empty client secret"),
            "{}",
            String::from_utf8_lossy(&body)
        );
    }

    #[test]
    fn wrong_pair_code_is_forbidden() {
        let mut st = HubState::empty();
        let _code = st.rotate_pair();
        let state = Arc::new(Mutex::new(st));
        let port = serve_background(state, 0).expect("bind");
        std::thread::sleep(std::time::Duration::from_millis(40));
        let pair_body = r#"{"code":"ZZZ-999","deviceId":"d-bad","deviceName":"Pixel"}"#;
        let req = format!(
            "POST /v1/pair HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{pair_body}",
            pair_body.len()
        );
        let (st_p, _, body) = http(port, &req);
        assert_eq!(st_p, 403, "{}", String::from_utf8_lossy(&body));
    }

    #[test]
    fn pair_cannot_claim_the_hub_id() {
        let mut st = HubState::empty();
        let hub_id = st.device_id.clone();
        let code = st.rotate_pair().code;
        let state = Arc::new(Mutex::new(st));
        let port = serve_background(state.clone(), 0).expect("bind");
        std::thread::sleep(std::time::Duration::from_millis(40));

        // `/v1/pair` returns the hub id and `/v1/status` lists it, so a caller holding the
        // code knows it. Claiming it would let them read tasks addressed to the hub and
        // forge their completion.
        let body = format!(r#"{{"code":"{code}","deviceId":"{hub_id}","deviceName":"Impostor"}}"#);
        let req = format!(
            "POST /v1/pair HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let (status, _, out) = http(port, &req);
        assert_eq!(status, 403, "{}", String::from_utf8_lossy(&out));
        assert!(
            !String::from_utf8_lossy(&out).contains("\"token\""),
            "a rejected pair must not hand back a token: {}",
            String::from_utf8_lossy(&out)
        );
        assert!(
            state.lock().unwrap().peers.is_empty(),
            "the impostor must not be registered as a peer"
        );

        // The code is still good for an honest device.
        let body = format!(r#"{{"code":"{code}","deviceId":"d-phone","deviceName":"Pixel"}}"#);
        let req = format!(
            "POST /v1/pair HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        assert_eq!(http(port, &req).0, 200);
    }

    #[test]
    fn phone_cannot_claim_inhabit() {
        let mut st = HubState::empty();
        let code = st.rotate_pair().code;
        let state = Arc::new(Mutex::new(st));
        let port = serve_background(state.clone(), 0).expect("bind");
        std::thread::sleep(std::time::Duration::from_millis(40));
        let pair_body = format!(
            r#"{{"code":"{code}","deviceId":"d-phone","deviceName":"Pixel phone"}}"#
        );
        let req = format!(
            "POST /v1/pair HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{pair_body}",
            pair_body.len()
        );
        let (_, _, body) = http(port, &req);
        let v: Value = serde_json::from_slice(&body).unwrap();
        let phone = v["token"].as_str().unwrap();
        let inhabit = r#"{"bundle":{"soul":"stay kind"}}"#;
        let req = format!(
            "POST /v1/inhabit HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {phone}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{inhabit}",
            inhabit.len()
        );
        assert_eq!(http(port, &req).0, 200);
        let (st_g, _, body) = http(
            port,
            &format!("GET /v1/inhabit HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {phone}\r\n\r\n"),
        );
        assert_eq!(st_g, 403, "{}", String::from_utf8_lossy(&body));
        let code2 = state.lock().unwrap().rotate_pair().code;
        let pair_body = format!(
            r#"{{"code":"{code2}","deviceId":"d-cabin","deviceName":"cabin-2"}}"#
        );
        let req = format!(
            "POST /v1/pair HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{pair_body}",
            pair_body.len()
        );
        let (_, _, body) = http(port, &req);
        let v: Value = serde_json::from_slice(&body).unwrap();
        let cabin = v["token"].as_str().unwrap();
        let (st_ok, _, body) = http(
            port,
            &format!("GET /v1/inhabit HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {cabin}\r\n\r\n"),
        );
        assert_eq!(st_ok, 200, "{}", String::from_utf8_lossy(&body));
        assert!(String::from_utf8_lossy(&body).contains("stay kind"));
    }

    #[test]
    fn snapshot_and_frame_drop_hub_lock_before_io() {
        let src = include_str!("server.rs");
        let get_snap = src
            .split("Method::Get && path == \"/v1/snapshot\"")
            .nth(1)
            .and_then(|s| s.split("Method::Put && path == \"/v1/snapshot\"").next())
            .expect("GET /v1/snapshot");
        assert!(
            get_snap.contains("drop(st)"),
            "GET snapshot must release the hub lock before send_json or persist freezes: {get_snap}"
        );
        assert!(
            get_snap.contains("snapshot.clone()"),
            "GET snapshot must clone before drop: {get_snap}"
        );
        assert!(
            get_snap.contains("as_deref()"),
            "GET snapshot must serialize the Arc after drop, not deep-clone Value under hub.lock(): {get_snap}"
        );
        let put_snap = src
            .split("Method::Put && path == \"/v1/snapshot\"")
            .nth(1)
            .and_then(|s| s.split("Method::Post && path == \"/v1/task\"").next())
            .expect("PUT /v1/snapshot");
        let drop_at = put_snap.find("drop(st)").expect("PUT snapshot must drop before read_json");
        let read_at = put_snap.find("read_json").expect("PUT snapshot reads a body");
        assert!(
            drop_at < read_at,
            "PUT snapshot must not read the body while holding the hub lock: {put_snap}"
        );
        let clone_at = put_snap.find("snapshot.clone()").expect("PUT clones the Arc");
        let merge_at = put_snap.find("merge_put_snapshot").expect("PUT merges off lock");
        assert!(
            read_at < clone_at && clone_at < merge_at && !put_snap.contains(".put_snapshot("),
            "PUT snapshot must not from_value 8MB under hub.lock(): {put_snap}"
        );
        let get_frame = src
            .split("Method::Get && path == \"/v1/frame\"")
            .nth(1)
            .and_then(|s| s.split("Method::Get && path == \"/v1/frame.jpg\"").next())
            .expect("GET /v1/frame");
        assert!(
            get_frame.contains("drop(st)"),
            "GET frame must release the hub lock before send_json: {get_frame}"
        );
        assert!(
            get_frame.contains("as_deref()"),
            "GET frame must serialize the Arc after drop, not clone a 400KB JPEG under hub.lock(): {get_frame}"
        );
        let get_jpg = src
            .split("Method::Get && path == \"/v1/frame.jpg\"")
            .nth(1)
            .and_then(|s| s.split("Method::Post && path == \"/v1/voice/client-secret\"").next())
            .expect("GET /v1/frame.jpg");
        assert!(
            get_jpg.contains("drop(st)"),
            "GET frame.jpg must release the hub lock before send_raw: {get_jpg}"
        );
        let post_frame = src
            .split("Method::Post && path == \"/v1/frame\"")
            .nth(1)
            .and_then(|s| s.split("Method::Get && path == \"/v1/frame\"").next())
            .expect("POST /v1/frame");
        let drop_at = post_frame.find("drop(st)").expect("POST frame must drop before read_json");
        let read_at = post_frame.find("read_json").expect("POST frame reads a body");
        assert!(
            drop_at < read_at,
            "POST frame must not read a JPEG while holding the hub lock: {post_frame}"
        );
        let parse = post_frame.find("store_frame").expect("POST parses the JPEG");
        let relock = post_frame.rfind("state.lock()").expect("POST re-locks to install");
        assert!(
            read_at < parse && parse < relock && post_frame.contains("install_frame"),
            "POST frame must not decode a 400KB JPEG under hub.lock(): {post_frame}"
        );
        let post_task = src
            .split("Method::Post && path == \"/v1/task\"")
            .nth(1)
            .and_then(|s| s.split("path == \"/v1/inbox\"").next())
            .expect("POST /v1/task");
        let drop_at = post_task.find("drop(st)").expect("POST task must drop before read_json");
        let read_at = post_task.find("read_json").expect("POST task reads a body");
        assert!(
            drop_at < read_at,
            "POST task must not read the body while holding the hub lock: {post_task}"
        );
        let complete = src
            .split("/v1/task/")
            .nth(1)
            .and_then(|s| s.split("path.strip_prefix(\"/v1/task/\")").next())
            .expect("POST complete");
        let drop_at = complete.find("drop(st)").expect("complete must drop before read_json");
        let read_at = complete.find("read_json").expect("complete reads a body");
        assert!(
            drop_at < read_at,
            "task complete must not read receipts while holding the hub lock: {complete}"
        );
        let inhabit = src
            .split("Method::Post && path == \"/v1/inhabit\"")
            .nth(1)
            .and_then(|s| s.split("Method::Get && path == \"/v1/inhabit\"").next())
            .expect("POST /v1/inhabit");
        let drop_at = inhabit.find("drop(st)").expect("POST inhabit must drop before read_json");
        let read_at = inhabit.find("read_json").expect("POST inhabit reads a body");
        assert!(
            drop_at < read_at,
            "POST inhabit must not read the bundle while holding the hub lock: {inhabit}"
        );
        let inbox = src
            .split("path == \"/v1/inbox\"")
            .nth(1)
            .and_then(|s| s.split("/v1/inbox/").next())
            .expect("GET /v1/inbox");
        assert!(
            inbox.contains("drop(st)"),
            "GET inbox must release the hub lock before send_json: {inbox}"
        );
        let results = src
            .split("path == \"/v1/results\"")
            .nth(1)
            .and_then(|s| s.split("path == \"/v1/inhabit\"").next())
            .expect("GET /v1/results");
        assert!(
            results.contains("drop(st)"),
            "GET results must release the hub lock before send_json: {results}"
        );
        let get_inhabit = src
            .split("Method::Get && path == \"/v1/inhabit\"")
            .nth(1)
            .and_then(|s| s.split("Method::Post && path == \"/v1/frame\"").next())
            .expect("GET /v1/inhabit");
        assert!(
            get_inhabit.contains("drop(st)"),
            "GET inhabit must release the hub lock before send_json: {get_inhabit}"
        );
        let status = src
            .split("path == \"/v1/status\"")
            .nth(1)
            .and_then(|s| s.split("path == \"/v1/snapshot\"").next())
            .expect("GET /v1/status");
        assert!(
            status.contains("drop(st)"),
            "GET status must release the hub lock before send_json: {status}"
        );
        let pair = src
            .split("path == \"/v1/pair\"")
            .nth(1)
            .and_then(|s| s.split("fn bearer").next().or_else(|| s.split("let token = bearer").next()))
            .expect("POST /v1/pair");
        let drop_at = pair.find("drop(st)").expect("POST pair must drop before send_json");
        let send_at = pair.find("send_json").expect("POST pair sends");
        assert!(
            drop_at < send_at,
            "POST pair must not send while holding the hub lock: {pair}"
        );
        let get_task = src
            .split("path.strip_prefix(\"/v1/task/\")")
            .nth(1)
            .and_then(|s| s.split("path == \"/v1/results\"").next())
            .expect("GET /v1/task");
        assert!(
            get_task.contains("drop(st)"),
            "GET task must release the hub lock before send_json: {get_task}"
        );
        let ack = src
            .split("/v1/inbox/")
            .nth(1)
            .and_then(|s| s.split("/v1/task/").next())
            .expect("POST inbox ack");
        let drop_at = ack.find("drop(st)").expect("ack must drop before send_json");
        let send_at = ack.find("send_json").expect("ack sends");
        assert!(
            drop_at < send_at,
            "inbox ack must not send while holding the hub lock: {ack}"
        );
        let voice_deny = src
            .split("voice_client_secret_denied")
            .nth(1)
            .and_then(|s| s.split("let key = st.console_api_key").next())
            .expect("voice deny");
        let drop_at = voice_deny
            .find("drop(st)")
            .expect("voice 400 must drop before send_json");
        let send_at = voice_deny.find("send_json").expect("voice 400 sends");
        assert!(
            drop_at < send_at,
            "voice deny must not send while holding the hub lock: {voice_deny}"
        );
        let unknown = src
            .split("Err(e) => send_json(req, 502")
            .nth(1)
            .and_then(|s| s.split("fn split_url").next())
            .expect("unknown hub route");
        let drop_at = unknown
            .find("drop(st)")
            .expect("unknown route must drop before send_json");
        let send_at = unknown.find("send_json").expect("unknown route sends");
        assert!(
            drop_at < send_at,
            "unknown route must not send while holding the hub lock: {unknown}"
        );
        let accept = src
            .split("fn accept_loop(")
            .nth(1)
            .and_then(|s| s.split("fn handle(").next())
            .expect("accept_loop");
        let spawn = accept.find("thread::spawn").expect("per-request spawn");
        let handle_at = accept.find("handle(").expect("handle");
        assert!(
            spawn < handle_at,
            "voice mint must not stall pair/inbox on the accept thread: {accept}"
        );
        let serve = src
            .split("pub fn serve(")
            .nth(1)
            .and_then(|s| s.split("pub fn serve_background(").next())
            .expect("serve");
        assert!(
            serve.contains("accept_loop") && !serve.contains("handle("),
            "LAN serve must not handle on the accept thread: {serve}"
        );
    }
}
