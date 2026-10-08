use grokhub_core::inhabit::InhabitBundle;
use grokhub_core::{HubState, HUB_KIND};
use serde::Serialize;
use serde_json::{json, Value};
use std::io::Read;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

const MAX_BODY: usize = 8 * 1024 * 1024;
/// Requests handled at once. Past this a LAN flood gets a 503, not one more thread each.
const MAX_IN_FLIGHT: usize = 64;

pub fn serve(state: Arc<Mutex<HubState>>, port: u16) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let server = Server::http(("0.0.0.0", port))?;
    accept_loop(state, server);
    Ok(())
}

/// Loopback bind for tests. Returns the bound port.
pub fn serve_background(state: Arc<Mutex<HubState>>, port: u16) -> Result<u16, String> {
    serve_bind(state, "127.0.0.1", port)
}

/// LAN bind for the native cabin. Other computers pair against this.
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
    let in_flight = Arc::new(AtomicUsize::new(0));
    for req in server.incoming_requests() {
        let Some(slot) = InFlight::enter(&in_flight, MAX_IN_FLIGHT) else {
            let _ = send_json(
                req,
                503,
                json!({ "ok": false, "error": "Hub is busy. Try again." }),
            );
            continue;
        };
        let state = state.clone();
        std::thread::spawn(move || {
            let _slot = slot;
            let _ = handle(&state, req);
        });
    }
}

/// One request being handled. The slot frees when the handler ends, even on a panic.
struct InFlight(Arc<AtomicUsize>);

impl InFlight {
    fn enter(count: &Arc<AtomicUsize>, max: usize) -> Option<Self> {
        let mut now = count.load(Ordering::SeqCst);
        loop {
            if now >= max {
                return None;
            }
            match count.compare_exchange(now, now + 1, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => return Some(Self(count.clone())),
                Err(seen) => now = seen,
            }
        }
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn handle(state: &Arc<Mutex<HubState>>, mut req: Request) -> Result<(), ()> {
    let method = req.method().clone();
    let url = req.url().to_string();
    let path = url_path(&url);
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
    if st.peer_for_token(&token).is_none() {
        drop(st);
        return send_json(
            req,
            401,
            json!({ "ok": false, "error": "Pair this computer first (Settings → Devices)." }),
        );
    }
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
    drop(st);
    send_json(req, 404, json!({ "ok": false, "error": "unknown hub route" }))
}

fn url_path(url: &str) -> String {
    let raw = url.split('#').next().unwrap_or(url);
    let p = raw.split_once('?').map_or(raw, |(p, _)| p);
    let path = p.trim_end_matches('/').to_string();
    if path.is_empty() {
        "/".into()
    } else {
        path
    }
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
    let mut headers = cors_headers();
    if let Ok(h) = Header::from_bytes(&b"content-type"[..], ctype.as_bytes()) {
        headers.push(h);
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
    fn pair_frame_contract() {
        let mut st = HubState::empty();
        let code = st.rotate_pair().code;
        let state = Arc::new(Mutex::new(st));
        let port = serve_background(state.clone(), 0).expect("bind");
        std::thread::sleep(std::time::Duration::from_millis(40));

        let (st_h, _, body) = http(
            port,
            "GET /v1/health HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n",
        );
        assert_eq!(st_h, 200);
        assert!(String::from_utf8_lossy(&body).contains(HUB_KIND));

        let pair_body = format!(
            r#"{{"code":"{code}","deviceId":"d-test","deviceName":"cabin-2"}}"#
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

        let png = r#"{"dataUrl":"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=="}"#;
        let req = format!(
            "POST /v1/frame HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{png}",
            png.len()
        );
        assert_eq!(http(port, &req).0, 200);
        let (st_f, _, body) = http(
            port,
            &format!("GET /v1/frame HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\n\r\n"),
        );
        assert_eq!(st_f, 200, "{}", String::from_utf8_lossy(&body));
        let frame: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(frame["ok"], true);
        assert!(frame["frame"].is_object(), "{frame}");
        assert_eq!(hub_id, state.lock().unwrap().device_id);
    }

    /// The phone routes are gone. A paired caller gets the unknown-route 404 for each.
    #[test]
    fn phone_routes_are_gone() {
        let mut st = HubState::empty();
        let code = st.rotate_pair().code;
        let state = Arc::new(Mutex::new(st));
        let port = serve_background(state, 0).expect("bind");
        std::thread::sleep(std::time::Duration::from_millis(40));
        let pair_body = format!(r#"{{"code":"{code}","deviceId":"d-cabin","deviceName":"cabin-2"}}"#);
        let req = format!(
            "POST /v1/pair HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{pair_body}",
            pair_body.len()
        );
        let (_, _, body) = http(port, &req);
        let v: Value = serde_json::from_slice(&body).unwrap();
        let token = v["token"].as_str().unwrap();
        for (method, path) in [
            ("POST", "/v1/task"),
            ("GET", "/v1/task/task-1"),
            ("POST", "/v1/task/task-1/complete"),
            ("GET", "/v1/inbox"),
            ("POST", "/v1/inbox/task-1/ack"),
            ("GET", "/v1/results"),
            ("GET", "/v1/frame.jpg"),
            ("POST", "/v1/voice/client-secret"),
        ] {
            let req = format!(
                "{method} {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nContent-Length: 0\r\n\r\n"
            );
            let (status, _, body) = http(port, &req);
            assert_eq!(status, 404, "{method} {path}: {}", String::from_utf8_lossy(&body));
            assert_eq!(
                String::from_utf8_lossy(&body),
                r#"{"error":"unknown hub route","ok":false}"#,
                "{method} {path}"
            );
        }
    }

    #[test]
    fn wrong_pair_code_is_forbidden() {
        let mut st = HubState::empty();
        let _code = st.rotate_pair();
        let state = Arc::new(Mutex::new(st));
        let port = serve_background(state, 0).expect("bind");
        std::thread::sleep(std::time::Duration::from_millis(40));
        let pair_body = r#"{"code":"ZZZ-999","deviceId":"d-bad","deviceName":"Laptop"}"#;
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
        // code knows it. Claiming it would let them pose as the hub to every other peer.
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
        let body = format!(r#"{{"code":"{code}","deviceId":"d-cabin","deviceName":"cabin-2"}}"#);
        let req = format!(
            "POST /v1/pair HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        assert_eq!(http(port, &req).0, 200);
    }

    #[test]
    fn paired_computer_claims_inhabit() {
        let mut st = HubState::empty();
        let code = st.rotate_pair().code;
        let state = Arc::new(Mutex::new(st));
        let port = serve_background(state, 0).expect("bind");
        std::thread::sleep(std::time::Duration::from_millis(40));
        let pair_body = format!(
            r#"{{"code":"{code}","deviceId":"d-cabin","deviceName":"cabin-2"}}"#
        );
        let req = format!(
            "POST /v1/pair HTTP/1.0\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{pair_body}",
            pair_body.len()
        );
        let (_, _, body) = http(port, &req);
        let v: Value = serde_json::from_slice(&body).unwrap();
        let cabin = v["token"].as_str().unwrap();
        let inhabit = r#"{"bundle":{"soul":"stay kind"}}"#;
        let req = format!(
            "POST /v1/inhabit HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {cabin}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{inhabit}",
            inhabit.len()
        );
        assert_eq!(http(port, &req).0, 200);
        let (st_ok, _, body) = http(
            port,
            &format!("GET /v1/inhabit HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {cabin}\r\n\r\n"),
        );
        assert_eq!(st_ok, 200, "{}", String::from_utf8_lossy(&body));
        let got: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(got["bundle"]["soul"], "stay kind");
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
            .and_then(|s| s.split("Method::Post && path == \"/v1/inhabit\"").next())
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
            .and_then(|s| s.split("unknown hub route").next())
            .expect("GET /v1/frame");
        assert!(
            get_frame.contains("drop(st)"),
            "GET frame must release the hub lock before send_json: {get_frame}"
        );
        assert!(
            get_frame.contains("as_deref()"),
            "GET frame must serialize the Arc after drop, not clone a 400KB JPEG under hub.lock(): {get_frame}"
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
        let unknown = src
            .split("frame: frame.as_deref(),")
            .nth(1)
            .and_then(|s| s.split("fn url_path").next())
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
            "a slow request must not stall pair or sync on the accept thread: {accept}"
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

    #[test]
    fn in_flight_slots_cap_and_free() {
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let a = InFlight::enter(&count, 2).expect("first");
        let b = InFlight::enter(&count, 2).expect("second");
        assert!(InFlight::enter(&count, 2).is_none(), "a third waits for a free slot");
        drop(a);
        let c = InFlight::enter(&count, 2).expect("a freed slot is reused");
        let panicked = std::thread::spawn(move || {
            let _slot = c;
            panic!("handler blew up");
        })
        .join();
        assert!(panicked.is_err());
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1, "a panic still frees its slot");
        drop(b);
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
    }
}
