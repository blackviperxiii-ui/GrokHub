//! The companion host: the door companions dial in to, and the relay that
//! carries this app's MCP calls out to them. Protocol and rules:
//! docs/design/companion-nodes/README.md; the paired list and the consent
//! rules are `grokhub_core::companion_nodes`.
//!
//! A companion can't be dialed (a phone behind NAT, a laptop that sleeps),
//! so it holds a long poll on `/next` and gets this app's MCP requests from
//! it, answering each on `/reply`. This app's own MCP client reaches a
//! companion at `/mcp/<id>`, which only a caller on this computer with the
//! local key may use. So every capability arrives through the MCP path the
//! app already has: tool approvals, output fencing and the Offline switch.

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use grokhub_core::companion_nodes::{self as nodes, Book, Companion};
use grokhub_core::pair::ct_eq;
use grokhub_core::state::PairCode;
use serde::Serialize;
use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

use crate::server::{bearer, from_web_page, read_json, send, url_path};

pub const PREFIX: &str = "/node/v1";
pub const KIND: &str = "companion-host";
/// How long a companion's `/next` waits for a request before it comes back
/// empty and polls again.
pub const POLL: Duration = Duration::from_secs(25);
/// A companion is online while it polls, and for this long after.
const ONLINE: Duration = Duration::from_secs(40);
/// How long an MCP call waits for the companion's answer.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(120);
/// Requests handled at once, polls included.
const MAX_IN_FLIGHT: usize = 32;
/// Polls one companion may hold at once.
const MAX_POLLS: usize = 2;
/// Requests waiting for one companion to pick them up.
const MAX_QUEUED: usize = 64;
/// Wakes and heard lines kept for the app to take.
const MAX_EVENTS: usize = 32;
const MAX_EVENT_TEXT: usize = 2000;

/// A companion woke the app or heard the user say something.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub companion: String,
    /// `wake` (the wake phrase or button) or `heard` (what the user said).
    pub kind: String,
    pub text: String,
    pub at: u64,
}

/// One companion as the app's Devices page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompanionView {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub approved: bool,
    pub offers: Vec<String>,
    pub allowed: Vec<String>,
    pub online: bool,
    pub paired_at: u64,
}

#[derive(Default)]
struct Lane {
    queue: VecDeque<Value>,
    /// Relay id → the caller's own id and where its answer goes.
    waiting: HashMap<String, (Value, mpsc::SyncSender<Value>)>,
    polling: usize,
    last_poll: Option<Instant>,
}

#[derive(Default)]
struct Relay {
    lanes: Mutex<HashMap<String, Lane>>,
    cv: Condvar,
    seq: AtomicU64,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Relay {
    fn online(&self, id: &str) -> bool {
        lock(&self.lanes).get(id).is_some_and(|lane| {
            lane.polling > 0 || lane.last_poll.is_some_and(|at| at.elapsed() < ONLINE)
        })
    }

    /// Send a request and wait for its answer. The request goes out under a
    /// relay id, so two callers that both use id 1 get their own answers.
    fn call(&self, id: &str, mut rpc: Value, timeout: Duration) -> Result<Value, String> {
        let original = rpc.get("id").cloned().unwrap_or(Value::Null);
        let rid = format!("r{}", self.seq.fetch_add(1, Ordering::Relaxed));
        rpc["id"] = json!(rid);
        let (tx, rx) = mpsc::sync_channel(1);
        {
            let mut lanes = lock(&self.lanes);
            let lane = lanes.entry(id.to_string()).or_default();
            if lane.queue.len() >= MAX_QUEUED {
                return Err("The companion has too many requests waiting.".into());
            }
            lane.waiting.insert(rid.clone(), (original, tx));
            lane.queue.push_back(rpc);
        }
        self.cv.notify_all();
        match rx.recv_timeout(timeout) {
            Ok(answer) => Ok(answer),
            Err(RecvTimeoutError::Timeout) => {
                let mut lanes = lock(&self.lanes);
                if let Some(lane) = lanes.get_mut(id) {
                    lane.waiting.remove(&rid);
                    lane.queue
                        .retain(|q| q.get("id").and_then(Value::as_str) != Some(rid.as_str()));
                }
                Err("The companion didn't answer in time.".into())
            }
            Err(RecvTimeoutError::Disconnected) => Err("The companion was removed.".into()),
        }
    }

    /// A notification: nothing comes back.
    fn notify(&self, id: &str, rpc: Value) {
        {
            let mut lanes = lock(&self.lanes);
            let lane = lanes.entry(id.to_string()).or_default();
            if lane.queue.len() < MAX_QUEUED {
                lane.queue.push_back(rpc);
            }
        }
        self.cv.notify_all();
    }

    /// The companion's long poll: the next request, or `None` after `wait`.
    /// `Err` when it already holds [`MAX_POLLS`].
    fn next(&self, id: &str, wait: Duration) -> Result<Option<Value>, ()> {
        let deadline = Instant::now() + wait;
        let mut lanes = lock(&self.lanes);
        {
            let lane = lanes.entry(id.to_string()).or_default();
            if lane.polling >= MAX_POLLS {
                return Err(());
            }
            lane.polling += 1;
        }
        let got = loop {
            // The lane is gone only when the companion was removed: answer
            // now, so its next poll hears that instead of waiting it out.
            let Some(lane) = lanes.get_mut(id) else {
                return Ok(None);
            };
            if let Some(rpc) = lane.queue.pop_front() {
                break Some(rpc);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break None;
            }
            lanes = self
                .cv
                .wait_timeout(lanes, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        };
        if let Some(lane) = lanes.get_mut(id) {
            lane.polling = lane.polling.saturating_sub(1);
            lane.last_poll = Some(Instant::now());
        }
        Ok(got)
    }

    /// The companion's answer to a relayed request, back under the caller's
    /// own id. An answer nobody waits for (late, or never asked) is dropped.
    fn reply(&self, id: &str, mut rpc: Value) -> bool {
        let Some(rid) = rpc.get("id").and_then(Value::as_str).map(str::to_string) else {
            return false;
        };
        let Some((original, tx)) = lock(&self.lanes)
            .get_mut(id)
            .and_then(|lane| lane.waiting.remove(&rid))
        else {
            return false;
        };
        rpc["id"] = original;
        tx.try_send(rpc).is_ok()
    }

    /// A removed companion's waiting callers get their answer: it's gone.
    fn drop_lane(&self, id: &str) {
        lock(&self.lanes).remove(id);
        self.cv.notify_all();
    }
}

struct Shared {
    book: Mutex<Book>,
    path: PathBuf,
    relay: Relay,
    /// What this app's MCP client sends to reach `/mcp/<id>`.
    key: String,
    events: Mutex<VecDeque<Event>>,
    host_name: String,
    /// Optional single HTTPS (or loopback HTTP) web origin allowed to pair and
    /// poll as a browser companion.
    allowed_origin: Mutex<Option<String>>,
}

/// Normalize one explicit browser companion origin. Rejects wildcards, paths,
/// credentials, and plain non-loopback `http://` origins.
pub fn normalize_origin(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed == "*"
        || trimmed.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return None;
    }
    let (scheme, rest) = if let Some(rest) = trimmed.strip_prefix("https://") {
        ("https", rest)
    } else {
        let rest = trimmed.strip_prefix("http://")?;
        ("http", rest)
    };
    if rest.is_empty() || rest.contains(['/', '?', '#', '@']) {
        return None;
    }
    if scheme == "http" {
        let host = if let Some(after) = rest.strip_prefix('[') {
            after.split_once(']')?.0
        } else {
            rest.split(':').next().unwrap_or("")
        };
        if !matches!(host, "127.0.0.1" | "localhost" | "::1") {
            return None;
        }
    }
    Some(format!("{scheme}://{rest}"))
}

impl Shared {
    fn save(&self, book: &Book) {
        if let Err(e) = book.save(&self.path) {
            eprintln!("companions: couldn't save {}: {e}", self.path.display());
        }
    }
}

/// The running companion host. Dropping it stops the listener.
pub struct NodeHost {
    shared: Arc<Shared>,
    server: Arc<Server>,
    port: u16,
}

impl Drop for NodeHost {
    fn drop(&mut self) {
        self.server.unblock();
    }
}

impl NodeHost {
    /// Listen on `bind:port` (port 0 for any free one) with the paired list
    /// in `path`. `key` is the local key this app's MCP client sends.
    pub fn start(
        path: &Path,
        bind: IpAddr,
        port: u16,
        key: String,
        host_name: &str,
    ) -> Result<NodeHost, String> {
        if key.len() < 16 {
            return Err("The companion host needs a local key of 16 or more characters.".into());
        }
        let server = Server::http(SocketAddr::new(bind, port)).map_err(|e| e.to_string())?;
        let port = server
            .server_addr()
            .to_ip()
            .map(|a| a.port())
            .unwrap_or(port);
        let shared = Arc::new(Shared {
            book: Mutex::new(Book::load(path)),
            path: path.to_path_buf(),
            relay: Relay::default(),
            key,
            events: Mutex::new(VecDeque::new()),
            host_name: host_name.to_string(),
            allowed_origin: Mutex::new(None),
        });
        let server = Arc::new(server);
        let (loop_server, loop_shared) = (server.clone(), shared.clone());
        std::thread::spawn(move || accept_loop(&loop_server, &loop_shared));
        Ok(NodeHost {
            shared,
            server,
            port,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// The key this app's MCP client sends to `/mcp/<id>`.
    pub fn local_key(&self) -> &str {
        &self.shared.key
    }

    /// Where this app's MCP client reaches a companion.
    pub fn mcp_url(&self, id: &str) -> String {
        format!("http://127.0.0.1:{}{PREFIX}/mcp/{id}", self.port)
    }

    /// A new one-time pairing code (15 minutes).
    pub fn open_pairing(&self) -> PairCode {
        lock(&self.shared.book).open_pairing(grokhub_core::now_ms())
    }

    /// The pairing code still open: not spent, burned or expired.
    pub fn pairing(&self) -> Option<PairCode> {
        let now = grokhub_core::now_ms();
        lock(&self.shared.book)
            .pair
            .clone()
            .filter(|p| p.expires_at >= now)
    }

    pub fn close_pairing(&self) {
        lock(&self.shared.book).pair = None;
    }

    pub fn companions(&self) -> Vec<CompanionView> {
        let book = lock(&self.shared.book).clone();
        book.companions
            .into_iter()
            .map(|c| CompanionView {
                online: self.shared.relay.online(&c.id),
                id: c.id,
                name: c.name,
                platform: c.platform,
                approved: c.approved,
                offers: c.offers,
                allowed: c.allowed,
                paired_at: c.paired_at,
            })
            .collect()
    }

    /// The user's yes, with what they allowed.
    pub fn approve(&self, id: &str, allowed: &[String]) -> bool {
        self.edit(|book| book.approve(id, allowed))
    }

    pub fn set_allowed(&self, id: &str, allowed: &[String]) -> bool {
        self.edit(|book| book.set_allowed(id, allowed))
    }

    pub fn remove(&self, id: &str) -> bool {
        let gone = self.edit(|book| book.remove(id));
        if gone {
            self.shared.relay.drop_lane(id);
        }
        gone
    }

    fn edit(&self, change: impl FnOnce(&mut Book) -> bool) -> bool {
        let mut book = lock(&self.shared.book);
        let changed = change(&mut book);
        if changed {
            self.shared.save(&book);
        }
        changed
    }

    /// Allow or clear the single browser origin permitted to call `/node/v1`
    /// companion endpoints (never `/mcp/<id>`).
    pub fn set_allowed_origin(&self, origin: Option<&str>) -> bool {
        match origin {
            None => {
                *lock(&self.shared.allowed_origin) = None;
                true
            }
            Some(raw) => match normalize_origin(raw) {
                Some(norm) => {
                    *lock(&self.shared.allowed_origin) = Some(norm);
                    true
                }
                None => false,
            },
        }
    }

    pub fn allowed_origin(&self) -> Option<String> {
        lock(&self.shared.allowed_origin).clone()
    }

    /// Wakes and heard lines since the last take, oldest first.
    pub fn take_events(&self) -> Vec<Event> {
        lock(&self.shared.events).drain(..).collect()
    }
}

fn accept_loop(server: &Server, shared: &Arc<Shared>) {
    let in_flight = Arc::new(AtomicUsize::new(0));
    for req in server.incoming_requests() {
        if in_flight.fetch_add(1, Ordering::SeqCst) >= MAX_IN_FLIGHT {
            in_flight.fetch_sub(1, Ordering::SeqCst);
            let _ = send_node_json(
                req,
                503,
                json!({ "ok": false, "error": "Busy. Try again." }),
                None,
            );
            continue;
        }
        let (shared, in_flight) = (shared.clone(), in_flight.clone());
        std::thread::spawn(move || {
            struct Slot(Arc<AtomicUsize>);
            impl Drop for Slot {
                fn drop(&mut self) {
                    self.0.fetch_sub(1, Ordering::SeqCst);
                }
            }
            let _slot = Slot(in_flight);
            let _ = handle(&shared, req);
        });
    }
}

fn send_node_json(
    req: Request,
    status: u16,
    body: impl Serialize,
    cors_origin: Option<&str>,
) -> Result<(), ()> {
    let bytes = serde_json::to_vec(&body).unwrap_or_else(|_| b"{}".to_vec());
    let mut headers = Vec::new();
    for (name, value) in [
        ("cache-control", "no-store"),
        ("content-type", "application/json; charset=utf-8"),
    ] {
        if let Ok(h) = Header::from_bytes(name.as_bytes(), value.as_bytes()) {
            headers.push(h);
        }
    }
    if let Some(origin) = cors_origin {
        for (name, value) in [("access-control-allow-origin", origin), ("vary", "Origin")] {
            if let Ok(h) = Header::from_bytes(name.as_bytes(), value.as_bytes()) {
                headers.push(h);
            }
        }
    }
    let resp = Response::new(
        StatusCode(status),
        headers,
        bytes.as_slice(),
        Some(bytes.len()),
        None,
    );
    req.respond(resp).map_err(|_| ())
}

fn send_preflight(req: Request, origin: &str) -> Result<(), ()> {
    let mut headers = Vec::new();
    for (name, value) in [
        ("cache-control", "no-store"),
        ("access-control-allow-origin", origin),
        ("access-control-allow-methods", "GET, POST, OPTIONS"),
        (
            "access-control-allow-headers",
            "Authorization, Content-Type",
        ),
        ("vary", "Origin"),
    ] {
        if let Ok(h) = Header::from_bytes(name.as_bytes(), value.as_bytes()) {
            headers.push(h);
        }
    }
    let empty: &[u8] = b"";
    let resp = Response::new(StatusCode(204), headers, empty, Some(0), None);
    req.respond(resp).map_err(|_| ())
}

fn error(req: Request, status: u16, text: &str, cors: Option<&str>) -> Result<(), ()> {
    send_node_json(req, status, json!({ "ok": false, "error": text }), cors)
}

fn request_origin(req: &Request) -> Option<String> {
    req.headers()
        .iter()
        .find(|h| h.field.equiv("Origin"))
        .map(|h| h.value.as_str().trim().to_string())
}

fn handle(shared: &Shared, mut req: Request) -> Result<(), ()> {
    let path = url_path(req.url());
    let Some(rest) = path.strip_prefix(PREFIX) else {
        return error(req, 404, "Unknown route.", None);
    };
    let rest = rest.to_string();
    let from = req.remote_addr().map(|a| a.ip());
    if let Some(id) = rest.strip_prefix("/mcp/") {
        return mcp(shared, req, id, from);
    }
    if !from.is_some_and(nodes::private_address) {
        return error(req, 403, "Companions pair from the same network.", None);
    }
    // A page in a browser on the network may pair/poll only from the single
    // explicitly allowlisted Web access origin. Native companions send no Origin.
    let cors_origin = match request_origin(&req) {
        None => None,
        Some(origin) => {
            let allowed = lock(&shared.allowed_origin).clone();
            if allowed.as_deref() != Some(origin.as_str()) {
                return error(
                    req,
                    403,
                    "Pairing is for the companion app, not a web page.",
                    None,
                );
            }
            Some(origin)
        }
    };
    let cors = cors_origin.as_deref();
    let method = req.method().clone();
    if method == Method::Options {
        return match (cors, rest.as_str()) {
            (Some(origin), "/health" | "/pair" | "/hello" | "/next" | "/reply" | "/event") => {
                send_preflight(req, origin)
            }
            _ => error(req, 404, "Unknown route.", cors),
        };
    }
    match (method, rest.as_str()) {
        (Method::Get, "/health") => send_node_json(
            req,
            200,
            json!({ "ok": true, "kind": KIND, "version": nodes::VERSION }),
            cors,
        ),
        (Method::Post, "/pair") => pair(shared, req, cors),
        (Method::Post, "/hello") => {
            let (c, body) = match authed_body(shared, &mut req) {
                Ok(got) => got,
                Err((status, text)) => return error(req, status, text, cors),
            };
            let offers: Vec<String> = body
                .get("offers")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let mut book = lock(&shared.book);
            book.hello(&c.id, &offers);
            shared.save(&book);
            let c = book.get(&c.id).cloned().ok_or(())?;
            drop(book);
            send_node_json(
                req,
                200,
                json!({ "ok": true, "approved": c.approved, "offers": c.offers, "allowed": c.allowed }),
                cors,
            )
        }
        (Method::Get, "/next") => {
            let Some(c) = authed(shared, &req) else {
                return error(req, 401, "Pair this companion first.", cors);
            };
            if !c.approved {
                return send_node_json(req, 200, json!({ "ok": true, "pending": true }), cors);
            }
            match shared.relay.next(&c.id, POLL) {
                Ok(Some(rpc)) => send_node_json(req, 200, json!({ "ok": true, "rpc": rpc }), cors),
                Ok(None) => send_node_json(req, 200, json!({ "ok": true }), cors),
                Err(()) => error(req, 429, "This companion is already polling.", cors),
            }
        }
        (Method::Post, "/reply") => {
            let (c, body) = match authed_body(shared, &mut req) {
                Ok(got) => got,
                Err((status, text)) => return error(req, status, text, cors),
            };
            let rpc = body.get("rpc").cloned().unwrap_or(Value::Null);
            let taken = shared.relay.reply(&c.id, rpc);
            send_node_json(req, 200, json!({ "ok": true, "taken": taken }), cors)
        }
        (Method::Post, "/event") => {
            let (c, body) = match authed_body(shared, &mut req) {
                Ok(got) => got,
                Err((status, text)) => return error(req, status, text, cors),
            };
            let kind = body.get("kind").and_then(Value::as_str).unwrap_or("");
            let needs = match kind {
                "wake" => "wake",
                "heard" => "talk",
                _ => return error(req, 400, "An event is `wake` or `heard`.", cors),
            };
            if !c.may(needs) {
                return error(
                    req,
                    403,
                    &format!("`{needs}` isn't allowed for this companion."),
                    cors,
                );
            }
            let text: String = body
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .chars()
                .filter(|ch| !ch.is_control() || *ch == '\n')
                .take(MAX_EVENT_TEXT)
                .collect();
            let mut events = lock(&shared.events);
            if events.len() >= MAX_EVENTS {
                events.pop_front();
            }
            events.push_back(Event {
                companion: c.id,
                kind: kind.to_string(),
                text,
                at: grokhub_core::now_ms(),
            });
            drop(events);
            send_node_json(req, 200, json!({ "ok": true }), cors)
        }
        _ => error(req, 404, "Unknown route.", cors),
    }
}

fn authed(shared: &Shared, req: &Request) -> Option<Companion> {
    lock(&shared.book).by_token(&bearer(req)).cloned()
}

/// The companion behind the token and the JSON body, or the status and
/// text to refuse with.
fn authed_body(
    shared: &Shared,
    req: &mut Request,
) -> Result<(Companion, Value), (u16, &'static str)> {
    let c = authed(shared, req).ok_or((401, "Pair this companion first."))?;
    let body = read_json(req).map_err(|e| (400, e))?;
    Ok((c, body))
}

fn pair(shared: &Shared, mut req: Request, cors: Option<&str>) -> Result<(), ()> {
    let body = match read_json(&mut req) {
        Ok(body) => body,
        Err(e) => return error(req, 400, e, cors),
    };
    let text = |key: &str| body.get(key).and_then(Value::as_str).unwrap_or("");
    let mut book = lock(&shared.book);
    let result = book.pair_with(
        text("code"),
        text("id"),
        text("name"),
        text("platform"),
        grokhub_core::now_ms(),
    );
    if result.is_ok() {
        shared.save(&book);
    }
    drop(book);
    match result {
        Ok(token) => send_node_json(
            req,
            200,
            json!({
                "ok": true,
                "token": token,
                "version": nodes::VERSION,
                "host": { "name": shared.host_name },
            }),
            cors,
        ),
        Err(e) => error(
            req,
            match e {
                nodes::PairError::NoCode | nodes::PairError::BadId => 400,
                nodes::PairError::Mismatch | nodes::PairError::Full => 403,
            },
            e.text(),
            cors,
        ),
    }
}

fn rpc_error(id: Value, error: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": error })
}

/// This app's MCP client calling a companion. Loopback only, with the local
/// key, never from a web page. One JSON-RPC message per POST, answered as
/// JSON; there is no server stream, so GET is 405 (streamable HTTP lets a
/// server say so).
fn mcp(shared: &Shared, mut req: Request, id: &str, from: Option<IpAddr>) -> Result<(), ()> {
    let key_ok = ct_eq(bearer(&req).as_bytes(), shared.key.as_bytes());
    if !from.is_some_and(|ip| ip.is_loopback()) || from_web_page(&req) || !key_ok {
        return error(req, 403, "Only this app reaches its companions.", None);
    }
    if *req.method() != Method::Post {
        return send(req, 405, "text/plain; charset=utf-8", b"POST only");
    }
    let body = match read_json(&mut req) {
        Ok(body) => body,
        Err(e) => return error(req, 400, e, None),
    };
    if !body.is_object() {
        let answer = rpc_error(
            Value::Null,
            json!({ "code": -32600, "message": "One message per request." }),
        );
        return send_node_json(req, 200, answer, None);
    }
    let rid = body.get("id").cloned();
    let companion = lock(&shared.book).get(id).cloned();
    let refused = match &companion {
        None => Some(json!({ "code": -32001, "message": "That companion isn't paired." })),
        Some(c) if !c.approved => Some(json!({
            "code": -32001,
            "message": format!("{} is waiting for your yes under Devices → Companions.", c.name),
        })),
        Some(c) => nodes::refusal(&body, c),
    };
    let Some(rid) = rid.filter(|v| !v.is_null()) else {
        // A notification: relayed when allowed, never answered.
        if refused.is_none() {
            shared.relay.notify(id, body);
        }
        return send(req, 202, "text/plain; charset=utf-8", b"");
    };
    if let Some(error) = refused {
        return send_node_json(req, 200, rpc_error(rid, error), None);
    }
    let Some(c) = companion else {
        return Err(());
    };
    if !shared.relay.online(id) {
        let answer = rpc_error(
            rid,
            json!({ "code": -32001, "message": format!("{} is offline. Open its companion and try again.", c.name) }),
        );
        return send_node_json(req, 200, answer, None);
    }
    let method = body
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    match shared.relay.call(id, body, CALL_TIMEOUT) {
        Ok(mut answer) => {
            if method == "tools/list" {
                // The user's switches may have changed since the call left.
                let now = lock(&shared.book).get(id).cloned().unwrap_or(c);
                nodes::filter_tools(&mut answer, &now);
            }
            send_node_json(req, 200, answer, None)
        }
        Err(text) => send_node_json(
            req,
            200,
            rpc_error(rid, json!({ "code": -32001, "message": text })),
            None,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, TcpStream};

    const KEY: &str = "local-key-0123456789";

    fn http(port: u16, method: &str, path: &str, auth: &str, body: &str) -> (u16, Value) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let auth = if auth.is_empty() {
            String::new()
        } else {
            format!("Authorization: Bearer {auth}\r\n")
        };
        let req = format!(
            "{method} {path} HTTP/1.0\r\nHost: 127.0.0.1\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        s.write_all(req.as_bytes()).expect("write");
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).expect("read");
        let text = String::from_utf8_lossy(&buf).into_owned();
        let status = text
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let body = text.split_once("\r\n\r\n").map_or("", |(_, b)| b);
        (status, serde_json::from_str(body).unwrap_or(Value::Null))
    }

    fn host() -> (NodeHost, PathBuf) {
        let dir = std::env::temp_dir().join(grokhub_core::uid("node-host"));
        let path = dir.join(nodes::FILE);
        let host = NodeHost::start(
            &path,
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            0,
            KEY.to_string(),
            "Desk",
        )
        .expect("start");
        (host, path)
    }

    fn pair(host: &NodeHost) -> String {
        let code = host.open_pairing().code;
        let body =
            format!(r#"{{"code":"{code}","id":"pixel","name":"Pixel","platform":"android"}}"#);
        let (status, v) = http(host.port(), "POST", "/node/v1/pair", "", &body);
        assert_eq!(status, 200, "{v}");
        assert_eq!(v["host"]["name"], "Desk");
        v["token"].as_str().expect("token").to_string()
    }

    /// A companion that polls once, checks the request and answers it.
    fn answer_once(port: u16, token: String, answer: Value) -> std::thread::JoinHandle<Value> {
        std::thread::spawn(move || {
            let (status, got) = http(port, "GET", "/node/v1/next", &token, "");
            assert_eq!(status, 200, "{got}");
            let rpc = got["rpc"].clone();
            let mut reply = answer;
            reply["id"] = rpc["id"].clone();
            let body = json!({ "rpc": reply }).to_string();
            let (status, v) = http(port, "POST", "/node/v1/reply", &token, &body);
            assert_eq!((status, v["taken"].clone()), (200, json!(true)));
            rpc
        })
    }

    fn wait_online(host: &NodeHost) {
        for _ in 0..200 {
            if host.shared.relay.online("pixel") {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("companion never polled");
    }

    #[test]
    fn a_paired_companion_waits_for_the_yes_then_serves_allowed_tools_only() {
        let (host, path) = host();
        let token = pair(&host);
        let port = host.port();

        // Pending: polls come back at once with nothing, and the app's
        // calls are refused before anything is relayed.
        let (status, v) = http(port, "GET", "/node/v1/next", &token, "");
        assert_eq!((status, v["pending"].clone()), (200, json!(true)));
        let list = json!({"jsonrpc":"2.0","id":7,"method":"tools/list"}).to_string();
        let (status, v) = http(port, "POST", "/node/v1/mcp/pixel", KEY, &list);
        assert_eq!(status, 200);
        assert_eq!(v["id"], 7);
        assert_eq!(
            v["error"]["message"],
            "Pixel is waiting for your yes under Devices → Companions."
        );

        let hello = json!({ "offers": ["camera", "sms", "mcp"] }).to_string();
        let (status, v) = http(port, "POST", "/node/v1/hello", &token, &hello);
        assert_eq!((status, v["approved"].clone()), (200, json!(false)));
        assert!(host.approve("pixel", &["camera".into(), "mcp".into()]));

        // tools/list goes out under a relay id and comes back under the
        // caller's, with the tool the user didn't allow taken out.
        let node = answer_once(
            port,
            token.clone(),
            json!({"jsonrpc":"2.0","result":{"tools":[
                {"name":"camera_snap"},{"name":"sms_send"},{"name":"github-search"}
            ]}}),
        );
        wait_online(&host);
        let (status, v) = http(port, "POST", "/node/v1/mcp/pixel", KEY, &list);
        let sent = node.join().expect("node");
        assert_eq!(sent["method"], "tools/list");
        assert_ne!(sent["id"], 7, "relayed under its own id");
        assert_eq!(status, 200);
        assert_eq!(v["id"], 7);
        assert_eq!(
            v["result"]["tools"],
            json!([{"name":"camera_snap"},{"name":"github-search"}])
        );

        // A call to a tool that isn't allowed never reaches the companion.
        let sms = json!({"jsonrpc":"2.0","id":"a","method":"tools/call","params":{"name":"sms_send","arguments":{}}});
        let (_, v) = http(port, "POST", "/node/v1/mcp/pixel", KEY, &sms.to_string());
        assert_eq!(v["error"]["code"], -32602);
        assert!(host.shared.relay.lanes.lock().expect("lanes")["pixel"]
            .queue
            .is_empty());

        // The file holds the approval, not the token.
        let saved = std::fs::read_to_string(&path).expect("saved");
        assert!(
            saved.contains("\"approved\": true") && !saved.contains(&token),
            "{saved}"
        );

        // Removed: a poll it holds ends at once, and its token stops working.
        let held = std::thread::spawn({
            let token = token.clone();
            move || {
                let at = Instant::now();
                (http(port, "GET", "/node/v1/next", &token, ""), at.elapsed())
            }
        });
        std::thread::sleep(Duration::from_millis(200));
        assert!(host.remove("pixel"));
        let ((status, v), took) = held.join().expect("poll");
        assert_eq!((status, v.get("rpc")), (200, None));
        assert!(took < Duration::from_secs(5), "the poll waited {took:?}");
        let (status, _) = http(port, "GET", "/node/v1/next", &token, "");
        assert_eq!(status, 401);
        let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
    }

    /// The app's own MCP client (the one every chat uses) connects to a
    /// companion through the relay and lists its tools; a call goes through.
    #[test]
    fn the_apps_mcp_client_reaches_a_companion_through_the_relay() {
        let (host, path) = host();
        let dir = path.parent().expect("dir").to_path_buf();
        let token = pair(&host);
        let port = host.port();
        http(
            port,
            "POST",
            "/node/v1/hello",
            &token,
            r#"{"offers":["camera","location"]}"#,
        );
        host.approve("pixel", &["camera".into()]);
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        // Left running: its last poll ends with the test's process.
        let _node = {
            let (stop, seen, token) = (stop.clone(), seen.clone(), token.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    let (_, got) = http(port, "GET", "/node/v1/next", &token, "");
                    let rpc = got["rpc"].clone();
                    let Some(method) = rpc["method"].as_str() else {
                        continue;
                    };
                    lock(&seen).push(method.to_string());
                    let result = match method {
                        "initialize" => json!({
                            "protocolVersion": rpc["params"]["protocolVersion"].clone(),
                            "capabilities": { "tools": {} },
                            "serverInfo": { "name": "pixel", "version": "1" }
                        }),
                        "tools/list" => json!({ "tools": [
                            { "name": "camera_snap", "description": "Take a photo", "inputSchema": { "type": "object" },
                              "annotations": { "readOnlyHint": true } },
                            { "name": "location_get", "description": "Where it is", "inputSchema": { "type": "object" } }
                        ]}),
                        "tools/call" => {
                            json!({ "content": [{ "type": "text", "text": format!("ran {}", rpc["params"]["name"]) }] })
                        }
                        _ => continue,
                    };
                    let reply = json!({ "rpc": { "jsonrpc": "2.0", "id": rpc["id"].clone(), "result": result } });
                    http(port, "POST", "/node/v1/reply", &token, &reply.to_string());
                }
            })
        };
        wait_online(&host);

        let config = dir.join("config");
        std::fs::create_dir_all(&config).expect("config");
        std::fs::write(
            config.join("mcp.json"),
            json!({ "mcpServers": { "node-pixel": {
                "url": host.mcp_url("pixel"),
                "headers": { "Authorization": format!("Bearer {KEY}") }
            }}})
            .to_string(),
        )
        .expect("mcp.json");
        // nextest gives each test its own process; under `cargo test` the
        // old value is put back below so later tests don't inherit it.
        let before = std::env::var_os("GROKHUB_CONFIG");
        // SAFETY: no other thread of this test reads the environment.
        unsafe { std::env::set_var("GROKHUB_CONFIG", &config) };
        grokhub_agent::mcp::connect_in_background(&dir);
        let mut tools = vec![];
        for _ in 0..500 {
            tools = grokhub_agent::mcp::tool_list();
            if !tools.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["camera_snap"], "location wasn't allowed");
        assert_eq!(tools[0].server, "node-pixel");
        // An allowed call goes out and its answer comes back.
        let call = json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"camera_snap","arguments":{}}});
        let (_, v) = http(port, "POST", "/node/v1/mcp/pixel", KEY, &call.to_string());
        assert_eq!(v["id"], 9);
        assert_eq!(v["result"]["content"][0]["text"], "ran \"camera_snap\"");
        stop.store(true, Ordering::SeqCst);
        grokhub_agent::mcp::shutdown_all();
        // SAFETY: as above.
        unsafe {
            match before {
                Some(v) => std::env::set_var("GROKHUB_CONFIG", v),
                None => std::env::remove_var("GROKHUB_CONFIG"),
            }
        }
        drop(host);
        let methods = lock(&seen).clone();
        assert!(
            methods.starts_with(&["initialize".to_string()]),
            "{methods:?}"
        );
        assert!(methods.contains(&"tools/call".to_string()), "{methods:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn only_this_app_with_the_key_reaches_the_relay() {
        let (host, path) = host();
        let token = pair(&host);
        let list = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}).to_string();
        let port = host.port();
        assert_eq!(
            http(port, "POST", "/node/v1/mcp/pixel", "wrong-key", &list).0,
            403
        );
        assert_eq!(http(port, "POST", "/node/v1/mcp/pixel", "", &list).0, 403);
        assert_eq!(
            http(port, "POST", "/node/v1/mcp/pixel", &token, &list).0,
            403,
            "a companion's token is not the app's key"
        );
        assert_eq!(http(port, "GET", "/node/v1/mcp/pixel", KEY, "").0, 405);
        let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
    }

    #[test]
    fn an_approved_companion_that_isnt_polling_is_offline() {
        let (host, path) = host();
        let token = pair(&host);
        let hello = json!({ "offers": ["camera"] }).to_string();
        http(host.port(), "POST", "/node/v1/hello", &token, &hello);
        host.approve("pixel", &["camera".into()]);
        let call =
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"camera_snap"}});
        let (_, v) = http(
            host.port(),
            "POST",
            "/node/v1/mcp/pixel",
            KEY,
            &call.to_string(),
        );
        assert_eq!(v["id"], 2);
        assert_eq!(
            v["error"]["message"],
            "Pixel is offline. Open its companion and try again."
        );
        let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
    }

    #[test]
    fn a_call_nobody_answers_times_out_and_leaves_nothing_queued() {
        let relay = Relay::default();
        let err = relay
            .call(
                "n",
                json!({"jsonrpc":"2.0","id":1,"method":"ping"}),
                Duration::from_millis(30),
            )
            .expect_err("no answer");
        assert_eq!(err, "The companion didn't answer in time.");
        let lanes = relay.lanes.lock().expect("lanes");
        assert!(lanes["n"].queue.is_empty() && lanes["n"].waiting.is_empty());
        drop(lanes);
        assert!(
            !relay.reply("n", json!({"jsonrpc":"2.0","id":"r0","result":{}})),
            "a late answer is dropped"
        );
    }

    #[test]
    fn events_need_the_matching_capability() {
        let (host, path) = host();
        let token = pair(&host);
        let port = host.port();
        http(
            port,
            "POST",
            "/node/v1/hello",
            &token,
            r#"{"offers":["wake","talk"]}"#,
        );
        host.approve("pixel", &["wake".into()]);
        let (status, _) = http(
            port,
            "POST",
            "/node/v1/event",
            &token,
            r#"{"kind":"wake","text":""}"#,
        );
        assert_eq!(status, 200);
        let (status, v) = http(
            port,
            "POST",
            "/node/v1/event",
            &token,
            r#"{"kind":"heard","text":"hi"}"#,
        );
        assert_eq!(status, 403, "{v}");
        let events = host.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(
            (events[0].companion.as_str(), events[0].kind.as_str()),
            ("pixel", "wake")
        );
        assert!(host.take_events().is_empty());
        let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
    }

    #[test]
    fn companions_come_from_the_network_not_the_internet() {
        let ok = [
            "127.0.0.1",
            "192.168.1.5",
            "10.0.0.2",
            "172.16.4.1",
            "169.254.1.1",
            "100.100.1.1",
            "fd00::1",
            "fe80::1",
            "::1",
            "::ffff:192.168.1.5",
        ];
        for ip in ok {
            assert!(nodes::private_address(ip.parse().expect("ip")), "{ip}");
        }
        for ip in [
            "8.8.8.8",
            "100.128.0.1",
            "2606:4700::1111",
            "::ffff:8.8.8.8",
        ] {
            assert!(!nodes::private_address(ip.parse().expect("ip")), "{ip}");
        }
    }

    #[test]
    fn a_web_page_cannot_pair() {
        let (host, path) = host();
        let code = host.open_pairing().code;
        let body = format!(r#"{{"code":"{code}","id":"pixel","name":"P","platform":"x"}}"#);
        let mut s = TcpStream::connect(("127.0.0.1", host.port())).expect("connect");
        let req = format!(
            "POST /node/v1/pair HTTP/1.0\r\nHost: x\r\nOrigin: https://evil.example\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        s.write_all(req.as_bytes()).expect("write");
        let mut out = String::new();
        s.read_to_string(&mut out).expect("read");
        assert!(
            out.starts_with("HTTP/1.0 403") || out.starts_with("HTTP/1.1 403"),
            "{out}"
        );
        assert!(host.companions().is_empty(), "the code wasn't even tried");
        let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
    }

    fn http_with_origin(
        port: u16,
        method: &str,
        path: &str,
        origin: &str,
        auth: &str,
        body: &str,
    ) -> (u16, String, Value) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let auth = if auth.is_empty() {
            String::new()
        } else {
            format!("Authorization: Bearer {auth}\r\n")
        };
        let req = format!(
            "{method} {path} HTTP/1.0\r\nHost: 127.0.0.1\r\n\
             Origin: {origin}\r\n{auth}Content-Type: application/json\r\n\
             Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        s.write_all(req.as_bytes()).expect("write");
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).expect("read");
        let text = String::from_utf8_lossy(&buf).into_owned();
        let (head, raw_body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
        let status = head
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        (
            status,
            head.to_string(),
            serde_json::from_str(raw_body).unwrap_or(Value::Null),
        )
    }

    #[test]
    fn normalize_origin_rejects_wildcards_paths_and_plain_lan_http() {
        assert_eq!(
            normalize_origin("https://192.168.1.10:4620"),
            Some("https://192.168.1.10:4620".into())
        );
        assert_eq!(
            normalize_origin("http://127.0.0.1:4620"),
            Some("http://127.0.0.1:4620".into())
        );
        assert_eq!(
            normalize_origin("http://localhost:4620"),
            Some("http://localhost:4620".into())
        );
        assert_eq!(normalize_origin("*"), None);
        assert_eq!(normalize_origin("https://192.168.1.10:4620/"), None);
        assert_eq!(normalize_origin("http://192.168.1.10:4620"), None);
    }

    #[test]
    fn allowlisted_browser_origin_can_preflight_pair_and_poll_with_same_consent_rules() {
        let (host, path) = host();
        let origin = "https://192.168.1.10:4620";
        assert!(host.set_allowed_origin(Some(origin)));
        assert_eq!(host.allowed_origin().as_deref(), Some(origin));

        let (status, head, _) =
            http_with_origin(host.port(), "OPTIONS", "/node/v1/pair", origin, "", "");
        assert_eq!(status, 204);
        assert!(
            head.to_ascii_lowercase()
                .contains(&format!("access-control-allow-origin: {origin}")),
            "{head}"
        );

        let code = host.open_pairing().code;
        let body =
            format!(r#"{{"code":"{code}","id":"phone-web","name":"Phone","platform":"browser"}}"#);
        let (status, head, v) =
            http_with_origin(host.port(), "POST", "/node/v1/pair", origin, "", &body);
        assert_eq!(status, 200, "{v}");
        assert!(
            head.to_ascii_lowercase()
                .contains(&format!("access-control-allow-origin: {origin}")),
            "{head}"
        );
        let token = v["token"].as_str().expect("token").to_string();

        // Pending consent: polling is admitted with CORS and returns pending:true,
        // and MCP calls stay refused until desktop approval.
        let (status, _, v) =
            http_with_origin(host.port(), "GET", "/node/v1/next", origin, &token, "");
        assert_eq!((status, v["pending"].clone()), (200, json!(true)));
        let hello = json!({ "offers": ["camera", "location"] }).to_string();
        let (status, _, v) = http_with_origin(
            host.port(),
            "POST",
            "/node/v1/hello",
            origin,
            &token,
            &hello,
        );
        assert_eq!((status, v["approved"].clone()), (200, json!(false)));
        assert!(host.approve("phone-web", &["camera".into()]));

        // After approval, browser origin can pick up queued RPC calls just like native nodes.
        let list = json!({"jsonrpc":"2.0","id":9,"method":"tools/list"}).to_string();
        let port = host.port();
        let poll_token = token.clone();
        let poller_origin = origin.to_string();
        let poller = std::thread::spawn(move || {
            let (status, _, got) = http_with_origin(
                port,
                "GET",
                "/node/v1/next",
                &poller_origin,
                &poll_token,
                "",
            );
            assert_eq!(status, 200, "{got}");
            let mut reply = json!({
                "jsonrpc": "2.0",
                "id": got["rpc"]["id"].clone(),
                "result": { "tools": [
                    { "name": "camera_snap" },
                    { "name": "location_get" }
                ] }
            });
            let reply_body = json!({ "rpc": reply.take() }).to_string();
            let (status, _, v) = http_with_origin(
                port,
                "POST",
                "/node/v1/reply",
                &poller_origin,
                &poll_token,
                &reply_body,
            );
            assert_eq!((status, v["taken"].clone()), (200, json!(true)));
        });
        for _ in 0..200 {
            if host.shared.relay.online("phone-web") {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            host.shared.relay.online("phone-web"),
            "phone-web never polled"
        );
        let (status, mcp_ans) = http(port, "POST", "/node/v1/mcp/phone-web", KEY, &list);
        poller.join().expect("poller");
        assert_eq!(status, 200);
        assert_eq!(
            mcp_ans["result"]["tools"],
            json!([{ "name": "camera_snap" }])
        );

        let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
    }

    #[test]
    fn unlisted_origin_and_mcp_with_origin_are_refused_with_no_cors_headers() {
        let (host, path) = host();
        let origin = "https://192.168.1.10:4620";
        assert!(host.set_allowed_origin(Some(origin)));
        let code = host.open_pairing().code;
        let body = format!(r#"{{"code":"{code}","id":"evil","name":"Bad","platform":"browser"}}"#);

        let (status, head, _) = http_with_origin(
            host.port(),
            "POST",
            "/node/v1/pair",
            "https://evil.example",
            "",
            &body,
        );
        assert_eq!(status, 403);
        assert!(
            !head.to_ascii_lowercase().contains("access-control-"),
            "{head}"
        );
        assert!(host.companions().is_empty());

        let (status, head, _) = http_with_origin(
            host.port(),
            "POST",
            "/node/v1/mcp/evil",
            origin,
            KEY,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        );
        assert_eq!(status, 403);
        assert!(
            !head.to_ascii_lowercase().contains("access-control-"),
            "{head}"
        );
        let _ = std::fs::remove_dir_all(path.parent().expect("dir"));
    }
}
