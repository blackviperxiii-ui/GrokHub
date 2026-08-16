use grokhub_core::frame::{get_jpeg, FrameGet};
use grokhub_core::inhabit::InhabitBundle;
use grokhub_core::task::Receipt;
use grokhub_core::{HubState, HUB_KIND};
use serde_json::{json, Value};
use std::io::Read;
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const MAX_BODY: usize = 8 * 1024 * 1024;

pub fn serve(state: Arc<Mutex<HubState>>, port: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let server = Server::http(("0.0.0.0", port))?;
    for req in server.incoming_requests() {
        let _ = handle(&state, req);
    }
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
        for req in server.incoming_requests() {
            let _ = handle(&state, req);
        }
    });
    Ok(bound)
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
        return match st.pair_with(code, device_id, device_name) {
            Ok(peer) => send_json(
                req,
                200,
                json!({
                    "ok": true,
                    "token": peer.token,
                    "deviceId": peer.id,
                    "hub": { "id": st.device_id, "name": st.device_name }
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
        return send_json(
            req,
            200,
            json!({
                "ok": true,
                "hub": { "id": st.device_id, "name": st.device_name },
                "you": { "id": peer.id, "name": peer.name },
                "peers": peers
            }),
        );
    }

    if method == Method::Get && path == "/v1/snapshot" {
        return send_json(req, 200, json!({ "ok": true, "snapshot": st.snapshot }));
    }
    if method == Method::Put && path == "/v1/snapshot" {
        let body = read_json(&mut req);
        let snap = body.get("snapshot").cloned().unwrap_or(body);
        return match st.put_snapshot(snap) {
            Ok(()) => send_json(req, 200, json!({ "ok": true })),
            Err(e) => send_json(req, 400, json!({ "ok": false, "error": e })),
        };
    }

    if method == Method::Post && path == "/v1/task" {
        let body = read_json(&mut req);
        let prompt = body.get("prompt").and_then(|v| v.as_str()).unwrap_or("");
        let title = body.get("title").and_then(|v| v.as_str()).unwrap_or("");
        let target = body.get("targetDeviceId").and_then(|v| v.as_str()).unwrap_or("");
        return match st.enqueue_task(&peer, target, title, prompt) {
            Ok(t) => send_json(
                req,
                200,
                json!({ "ok": true, "task": { "id": t.id, "targetDeviceId": t.target_device_id } }),
            ),
            Err(e) => send_json(req, 400, json!({ "ok": false, "error": e })),
        };
    }

    if method == Method::Get && path == "/v1/inbox" {
        return send_json(req, 200, json!({ "ok": true, "tasks": st.queued_for(&peer_id) }));
    }

    if let Some(id) = strip_prefix_suffix(&path, "/v1/inbox/", "/ack") {
        if method == Method::Post {
            st.ack_inbox(id, &peer_id);
            return send_json(req, 200, json!({ "ok": true }));
        }
    }

    if let Some(id) = strip_prefix_suffix(&path, "/v1/task/", "/complete") {
        if method == Method::Post {
            let body = read_json(&mut req);
            let result = body.get("result").and_then(|v| v.as_str()).unwrap_or("");
            let status = body.get("status").and_then(|v| v.as_str());
            let receipts: Vec<Receipt> = body
                .get("receipts")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let t = st.complete_task(id, result, receipts, status);
            return send_json(req, 200, json!({ "ok": t.is_some(), "task": t }));
        }
    }

    if let Some(id) = path.strip_prefix("/v1/task/") {
        if method == Method::Get && !id.contains('/') {
            return match st.get_task(id, &peer_id) {
                Some(t) => send_json(req, 200, json!({ "ok": true, "task": t })),
                None => send_json(req, 404, json!({ "ok": false, "error": "task not found" })),
            };
        }
    }

    if method == Method::Get && path == "/v1/results" {
        return send_json(req, 200, json!({ "ok": true, "tasks": st.claim_results(&peer_id) }));
    }

    if method == Method::Post && path == "/v1/inhabit" {
        let body = read_json(&mut req);
        let raw = body.get("bundle").cloned().unwrap_or(body);
        let bundle: InhabitBundle = serde_json::from_value(raw).unwrap_or_default();
        st.store_inhabit(bundle, &peer);
        return send_json(req, 200, json!({ "ok": true }));
    }
    if method == Method::Get && path == "/v1/inhabit" {
        return send_json(req, 200, json!({ "ok": true, "bundle": st.claim_inhabit() }));
    }

    if method == Method::Post && path == "/v1/frame" {
        let body = read_json(&mut req);
        let url = body
            .get("dataUrl")
            .or_else(|| body.get("jpeg"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        st.store_frame(url);
        return send_json(req, 200, json!({ "ok": true }));
    }
    if method == Method::Get && path == "/v1/frame" {
        return send_json(req, 200, json!({ "ok": true, "frame": st.last_frame }));
    }
    if method == Method::Get && path == "/v1/frame.jpg" {
        let since = query
            .split('&')
            .find_map(|p| p.strip_prefix("since="))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        match get_jpeg(st.last_frame.as_ref(), since) {
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
            Ok(v) => {
                let secret = grokhub_core::parse_client_secret(&v).unwrap_or_default();
                send_json(
                    req,
                    200,
                    json!({
                        "ok": true,
                        "value": secret,
                        "wsProtocol": grokhub_core::client_secret_ws_protocol(&secret),
                        "url": grokhub_core::voice_session_url(""),
                        "clientSecret": v
                    }),
                )
            }
            Err(e) => send_json(req, 502, json!({ "ok": false, "error": e })),
        };
    }

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

fn send_json(req: Request, status: u16, body: Value) -> Result<(), ()> {
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
}
