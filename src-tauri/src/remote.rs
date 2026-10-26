//! Settings → Web access (cross-platform Phase 5b). While it's on, the shell
//! serves its own UI and the remote API on the LAN, so a browser on another
//! device can use this computer's GrokHub (`ui/src/transport.ts`). The
//! protocol is in docs/cross-platform/README.md (Phase 5).
//!
//! - Every API call and the event stream need the pairing code, a fresh
//!   random one each time web access turns on. It's never saved, so quitting
//!   GrokHub or turning web access off ends every pairing. After [`IDLE`]
//!   with no paired browser using it, the code is swapped for a new one and
//!   every browser has to pair again.
//! - Only [`WEB_COMMANDS`] run. The API key, settings, Pulse, Local AI and
//!   desktop control stay on this computer, and web chats never get the
//!   desktop tools.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, Server};

pub const PORT: u16 = 4620;
pub const PREFIX: &str = "/remote/v1";
pub const KIND: &str = "grokhub-remote";
pub const VERSION: u32 = 1;
const MAX_BODY: usize = 1024 * 1024;
/// Calls handled at once; past this a flood gets a 503.
const MAX_IN_FLIGHT: usize = 32;
/// Open event streams; one per browser tab is plenty.
const MAX_STREAMS: usize = 8;
const PING: Duration = Duration::from_secs(15);
/// How long the pairing code lasts with no paired browser calling, opening a
/// stream or getting a chat event. Keep in step with `WEB_OFF_NOTE` in
/// ui/src/WebAccess.tsx.
pub const IDLE: Duration = Duration::from_secs(30 * 60);

pub const REFUSED: &str = "The web app can't do that. Use GrokHub on your computer.";
pub const NOT_YOURS: &str = "That chat isn't open in this browser.";

/// The web app's commands: chat with approvals, automations, memory and
/// connectors, and reading the settings.
pub const WEB_COMMANDS: &[&str] = &[
    "get_settings",
    "send_chat",
    "answer_permission",
    "answer_elicit",
    "cancel_chat",
    "close_chat",
    "list_automations",
    "add_automation",
    "set_automation_enabled",
    "remove_automation",
    "run_automation",
    "get_memory",
    "save_memory",
    "add_memory",
    "search_memory",
    "edit_memory_row",
    "forget_memory_row",
    "approve_memory_row",
    "reject_memory_row",
    "set_memory_writes",
    "list_connectors",
    "check_connectors",
    "restart_connector",
    "sign_out_connector",
    "disconnect_connector",
];

pub fn web_command(name: &str) -> bool {
    WEB_COMMANDS.contains(&name)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Hello,
    Events,
    Call(String),
    /// A file of the UI, by its path.
    Page(String),
    Wrong(u16),
}

pub fn route(method: &Method, url: &str) -> Route {
    let path = url.split(['?', '#']).next().unwrap_or("/");
    if let Some(rest) = path.strip_prefix(PREFIX) {
        return match (method, rest) {
            (Method::Get, "/hello") => Route::Hello,
            (Method::Get, "/events") => Route::Events,
            (_, "/hello" | "/events") => Route::Wrong(405),
            (Method::Post, _) => match rest.strip_prefix("/call/") {
                Some(name) if !name.is_empty() => Route::Call(name.to_string()),
                _ => Route::Wrong(404),
            },
            (_, call) if call.starts_with("/call/") => Route::Wrong(405),
            _ => Route::Wrong(404),
        };
    }
    match method {
        Method::Get | Method::Head => Route::Page(path.to_string()),
        _ => Route::Wrong(405),
    }
}

/// `Authorization: Bearer <code>`, compared in constant time.
pub fn authorized(header: Option<&str>, code: &str) -> bool {
    let Some(given) = header.and_then(|h| h.strip_prefix("Bearer ")) else {
        return false;
    };
    let (given, code) = (given.trim().as_bytes(), code.as_bytes());
    given.len() == code.len()
        && given
            .iter()
            .zip(code)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
}

/// One server-sent event frame.
pub fn frame(event: &str, payload: &Value) -> String {
    format!(
        "data: {}\n\n",
        json!({ "event": event, "payload": payload })
    )
}

/// The pairing link a browser on the LAN opens.
pub fn link(ip: IpAddr, port: u16, code: &str) -> String {
    format!("http://{}/#code={code}", SocketAddr::new(ip, port))
}

/// This computer's LAN address: the one its default route leaves from.
/// Connecting a UDP socket sends nothing.
pub fn lan_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_unspecified() && !ip.is_loopback()).then_some(ip)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebAccessView {
    pub on: bool,
    /// Empty while off.
    pub code: String,
    /// Pairing links, the LAN one first.
    pub links: Vec<String>,
}

/// Runs a web command (one of [`WEB_COMMANDS`]) with its JSON arguments.
pub type Dispatch = Arc<dyn Fn(&str, Value) -> Result<Value, String> + Send + Sync>;
/// A UI file by path: its bytes and MIME type.
pub type Assets = Arc<dyn Fn(&str) -> Option<(Vec<u8>, String)> + Send + Sync>;

struct Running {
    pairing: Shared,
    port: u16,
    server: Arc<Server>,
}

type Streams = Arc<Mutex<Vec<Sender<String>>>>;
type Chats = Arc<Mutex<HashSet<String>>>;
type Shared = Arc<Mutex<Pairing>>;

/// The pairing code and when a paired browser last used it.
struct Pairing {
    code: String,
    used: Instant,
}

impl Pairing {
    fn new(now: Instant) -> Self {
        Self {
            code: grokhub_core::new_token(),
            used: now,
        }
    }

    /// Swaps in a fresh code once this one went unused for [`IDLE`]. True when it did.
    fn renew_if_idle(&mut self, now: Instant) -> bool {
        if now.saturating_duration_since(self.used) < IDLE {
            return false;
        }
        *self = Self::new(now);
        true
    }

    /// Whether `header` carries the live code; a match counts as use.
    fn admit(&mut self, header: Option<&str>, now: Instant) -> bool {
        let ok = authorized(header, &self.code);
        if ok {
            self.used = now;
        }
        ok
    }
}

/// Locks the pairing, first renewing an idle code. A renewal also ends every
/// open stream and forgets the browsers' chats, as turning web access off does.
fn live<'a>(
    pairing: &'a Shared,
    streams: &Streams,
    chats: &Chats,
    now: Instant,
) -> MutexGuard<'a, Pairing> {
    let mut live = pairing.lock().unwrap_or_else(|e| e.into_inner());
    if live.renew_if_idle(now) {
        streams.lock().unwrap_or_else(|e| e.into_inner()).clear();
        chats.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
    live
}

#[derive(Default)]
pub struct WebAccess {
    running: Mutex<Option<Running>>,
    streams: Streams,
    /// The conversations a browser started. Only their events go out, so a
    /// chat in the window stays in the window.
    chats: Chats,
}

impl WebAccess {
    pub fn view(&self) -> WebAccessView {
        let running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        match running.as_ref() {
            None => WebAccessView {
                on: false,
                code: String::new(),
                links: Vec::new(),
            },
            Some(run) => {
                let code = live(&run.pairing, &self.streams, &self.chats, Instant::now())
                    .code
                    .clone();
                let mut links: Vec<String> = lan_ip()
                    .map(|ip| link(ip, run.port, &code))
                    .into_iter()
                    .collect();
                links.push(link(IpAddr::V4(Ipv4Addr::LOCALHOST), run.port, &code));
                WebAccessView {
                    on: true,
                    code,
                    links,
                }
            }
        }
    }

    /// Serves on `bind:port` with a new pairing code. Already on: no change.
    pub fn start(
        &self,
        bind: IpAddr,
        port: u16,
        dispatch: Dispatch,
        assets: Assets,
    ) -> Result<WebAccessView, String> {
        {
            let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
            if running.is_none() {
                let server = Server::http(SocketAddr::new(bind, port))
                    .map_err(|err| format!("Web access couldn't start on port {port}: {err}"))?;
                let port = server
                    .server_addr()
                    .to_ip()
                    .map(|addr| addr.port())
                    .unwrap_or(port);
                let server = Arc::new(server);
                let pairing = Arc::new(Mutex::new(Pairing::new(Instant::now())));
                let job = Job {
                    pairing: Arc::clone(&pairing),
                    streams: Arc::clone(&self.streams),
                    chats: Arc::clone(&self.chats),
                    dispatch,
                    assets,
                };
                let accept = Arc::clone(&server);
                std::thread::spawn(move || accept_loop(&accept, &job));
                *running = Some(Running {
                    pairing,
                    port,
                    server,
                });
            }
        }
        Ok(self.view())
    }

    /// Stops serving; the code stops working and every event stream ends.
    pub fn stop(&self) {
        let stopped = self
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(run) = stopped {
            run.server.unblock();
        }
        self.chats.lock().unwrap_or_else(|e| e.into_inner()).clear();
        self.streams
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    /// Hands an event to every paired browser.
    pub fn broadcast(&self, event: &str, payload: &str) {
        let mut streams = self.streams.lock().unwrap_or_else(|e| e.into_inner());
        if streams.is_empty() {
            return;
        }
        let payload: Value = serde_json::from_str(payload).unwrap_or(Value::Null);
        let chat = payload["conversationId"].as_str().unwrap_or_default();
        if !self
            .chats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(chat)
        {
            return;
        }
        let text = frame(event, &payload);
        streams.retain(|tx| tx.send(text.clone()).is_ok());
        let delivered = !streams.is_empty();
        drop(streams);
        // A browser getting its chat's events is using the pairing.
        if delivered {
            if let Some(run) = self
                .running
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
            {
                run.pairing.lock().unwrap_or_else(|e| e.into_inner()).used = Instant::now();
            }
        }
    }

    /// A conversation a browser started.
    pub fn owns(&self, conversation_id: &str) -> bool {
        self.chats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(conversation_id)
    }

    /// A browser started (or continues) this conversation.
    pub fn adopt(&self, conversation_id: &str) {
        self.chats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(conversation_id.to_string());
    }
}

struct Job {
    pairing: Shared,
    streams: Streams,
    chats: Chats,
    dispatch: Dispatch,
    assets: Assets,
}

fn accept_loop(server: &Server, job: &Job) {
    let in_flight = Arc::new(AtomicUsize::new(0));
    for req in server.incoming_requests() {
        if in_flight.fetch_add(1, Ordering::SeqCst) >= MAX_IN_FLIGHT {
            in_flight.fetch_sub(1, Ordering::SeqCst);
            send_json(
                req,
                503,
                &json!({ "ok": false, "error": "GrokHub is busy. Try again." }),
            );
            continue;
        }
        let slot = Slot(Arc::clone(&in_flight));
        let job = Job {
            pairing: Arc::clone(&job.pairing),
            streams: Arc::clone(&job.streams),
            chats: Arc::clone(&job.chats),
            dispatch: Arc::clone(&job.dispatch),
            assets: Arc::clone(&job.assets),
        };
        std::thread::spawn(move || handle(req, &job, slot));
    }
}

/// One call being handled; frees its place when the handler ends, even on a panic.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

fn handle(mut req: Request, job: &Job, slot: Slot) {
    let auth = req
        .headers()
        .iter()
        .find(|h| h.field.equiv("Authorization"))
        .map(|h| h.value.as_str().to_string());
    let now = Instant::now();
    let paired = live(&job.pairing, &job.streams, &job.chats, now).admit(auth.as_deref(), now);
    match route(req.method(), req.url()) {
        Route::Hello => send_json(req, 200, &json!({ "kind": KIND, "version": VERSION })),
        Route::Events if !paired => refuse(req),
        Route::Events => {
            // A stream lasts as long as the tab; it doesn't hold a call's place.
            drop(slot);
            stream(req, &job.streams);
        }
        Route::Call(_) if !paired => refuse(req),
        Route::Call(command) => {
            let mut body = Vec::new();
            let read = req
                .as_reader()
                .take(MAX_BODY as u64 + 1)
                .read_to_end(&mut body);
            if read.is_err() || body.len() > MAX_BODY {
                return send_json(
                    req,
                    413,
                    &json!({ "ok": false, "error": "That request is too big." }),
                );
            }
            let args = if body.iter().all(u8::is_ascii_whitespace) {
                Ok(json!({}))
            } else {
                serde_json::from_slice::<Value>(&body).map_err(|err| err.to_string())
            };
            let reply = match args {
                Err(err) => json!({ "ok": false, "error": format!("Bad arguments: {err}") }),
                Ok(_) if !web_command(&command) => json!({ "ok": false, "error": REFUSED }),
                Ok(args) => match (job.dispatch)(&command, args) {
                    Ok(value) => json!({ "ok": true, "value": value }),
                    Err(error) => json!({ "ok": false, "error": error }),
                },
            };
            send_json(req, 200, &reply);
        }
        Route::Page(path) => page(req, &path, &job.assets),
        Route::Wrong(status) => {
            send_json(req, status, &json!({ "ok": false, "error": "Not found." }))
        }
    }
}

fn header(name: &str, value: &str) -> Option<Header> {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).ok()
}

fn send(req: Request, status: u16, body: Vec<u8>, content_type: &str, extra: &[(&str, &str)]) {
    let mut res = Response::from_data(body).with_status_code(status);
    let base = [
        ("Content-Type", content_type),
        ("Cache-Control", "no-store"),
        ("X-Content-Type-Options", "nosniff"),
        ("Referrer-Policy", "no-referrer"),
    ];
    for (name, value) in base.iter().chain(extra) {
        if let Some(h) = header(name, value) {
            res.add_header(h);
        }
    }
    let _ = req.respond(res);
}

fn send_json(req: Request, status: u16, body: &Value) {
    send(
        req,
        status,
        body.to_string().into_bytes(),
        "application/json",
        &[],
    );
}

fn refuse(req: Request) {
    send_json(
        req,
        401,
        &json!({ "ok": false, "error": "Pair this browser first." }),
    );
}

/// The page may load only its own files and talk only to this desktop.
const PAGE_CSP: &str = "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

fn page(req: Request, path: &str, assets: &Assets) {
    let file = match path.trim_start_matches('/') {
        "" => "index.html",
        file => file,
    };
    // An unknown path is the app's own route: the page handles it.
    let found = assets(file).or_else(|| assets("index.html"));
    match found {
        Some((bytes, mime)) => send(
            req,
            200,
            bytes,
            &mime,
            &[
                ("Content-Security-Policy", PAGE_CSP),
                ("X-Frame-Options", "DENY"),
            ],
        ),
        None => send_json(
            req,
            404,
            &json!({ "ok": false, "error": "This build has no web app. Build the UI first." }),
        ),
    }
}

fn stream(req: Request, streams: &Streams) {
    let (tx, rx) = mpsc::channel::<String>();
    {
        let mut all = streams.lock().unwrap_or_else(|e| e.into_inner());
        all.retain(|tx| tx.send(": ping\n\n".to_string()).is_ok());
        if all.len() >= MAX_STREAMS {
            drop(all);
            return send_json(
                req,
                503,
                &json!({ "ok": false, "error": "Too many open tabs." }),
            );
        }
        all.push(tx);
    }
    // tiny_http's own chunked writer holds 8 KB before it sends, so the
    // stream writes its chunks itself and flushes each event.
    let mut out = req.into_writer();
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nTransfer-Encoding: chunked\r\n\r\n";
    if out.write_all(head.as_bytes()).is_err() || chunk(&mut out, ": open\n\n").is_err() {
        return;
    }
    loop {
        let text = match rx.recv_timeout(PING) {
            Ok(text) => text,
            Err(RecvTimeoutError::Timeout) => ": ping\n\n".to_string(),
            // Web access went off: end the response, so the page knows.
            Err(RecvTimeoutError::Disconnected) => {
                let _ = out.write_all(b"0\r\n\r\n").and_then(|()| out.flush());
                return;
            }
        };
        if chunk(&mut out, &text).is_err() {
            return;
        }
    }
}

fn chunk(out: &mut impl Write, text: &str) -> std::io::Result<()> {
    write!(out, "{:x}\r\n{text}\r\n", text.len())?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::net::TcpStream;

    #[test]
    fn routes_the_api_and_leaves_the_rest_to_the_page() {
        assert_eq!(route(&Method::Get, "/remote/v1/hello"), Route::Hello);
        assert_eq!(route(&Method::Get, "/remote/v1/events?x=1"), Route::Events);
        assert_eq!(
            route(&Method::Post, "/remote/v1/call/send_chat"),
            Route::Call("send_chat".into())
        );
        assert_eq!(route(&Method::Post, "/remote/v1/call/"), Route::Wrong(404));
        assert_eq!(
            route(&Method::Get, "/remote/v1/call/send_chat"),
            Route::Wrong(405)
        );
        assert_eq!(route(&Method::Post, "/remote/v1/hello"), Route::Wrong(405));
        assert_eq!(route(&Method::Get, "/remote/v1/nope"), Route::Wrong(404));
        assert_eq!(route(&Method::Get, "/"), Route::Page("/".into()));
        assert_eq!(
            route(&Method::Get, "/assets/index.js?v=2"),
            Route::Page("/assets/index.js".into())
        );
        assert_eq!(route(&Method::Post, "/"), Route::Wrong(405));
    }

    #[test]
    fn only_the_exact_code_pairs() {
        let code = "0123456789abcdef";
        assert!(authorized(Some("Bearer 0123456789abcdef"), code));
        assert!(!authorized(Some("Bearer 0123456789abcdeF"), code));
        assert!(!authorized(Some("Bearer 0123456789abcde"), code));
        assert!(!authorized(Some("0123456789abcdef"), code));
        assert!(!authorized(Some("Bearer "), code));
        assert!(!authorized(None, code));
    }

    #[test]
    fn an_unused_code_is_swapped_after_idle() {
        let start = Instant::now();
        let minute = Duration::from_secs(60);
        let mut pairing = Pairing::new(start);
        let first = pairing.code.clone();
        let bearer = format!("Bearer {first}");
        assert_eq!(first.len(), 48);

        assert!(!pairing.renew_if_idle(start + 29 * minute));
        assert!(pairing.admit(Some(&bearer), start + 29 * minute));
        // That use restarts the clock: 58 minutes in is only 29 idle.
        assert!(!pairing.renew_if_idle(start + 58 * minute));
        assert!(!pairing.admit(Some("Bearer wrong"), start + 58 * minute));
        // A refused code isn't use, so 59 minutes in is 30 idle.
        assert!(pairing.renew_if_idle(start + 59 * minute));
        assert_ne!(pairing.code, first);
        assert_eq!(pairing.code.len(), 48);
        assert!(!pairing.admit(Some(&bearer), start + 59 * minute));
        let fresh = format!("Bearer {}", pairing.code);
        assert!(pairing.admit(Some(&fresh), start + 59 * minute));
    }

    #[test]
    fn a_renewed_code_ends_streams_and_forgets_web_chats() {
        let start = Instant::now();
        let pairing: Shared = Arc::new(Mutex::new(Pairing::new(start)));
        let streams: Streams = Arc::default();
        let chats: Chats = Arc::default();
        let (tx, rx) = mpsc::channel::<String>();
        streams.lock().unwrap().push(tx);
        chats.lock().unwrap().insert("web-chat".into());

        let old = live(
            &pairing,
            &streams,
            &chats,
            start + IDLE - Duration::from_secs(1),
        )
        .code
        .clone();
        assert_eq!(streams.lock().unwrap().len(), 1);
        assert!(chats.lock().unwrap().contains("web-chat"));

        let new = live(&pairing, &streams, &chats, start + IDLE).code.clone();
        assert_ne!(new, old);
        assert!(streams.lock().unwrap().is_empty());
        assert!(chats.lock().unwrap().is_empty());
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1)),
            Err(RecvTimeoutError::Disconnected)
        );
    }

    #[test]
    fn the_web_gets_chat_memory_automations_and_connectors_only() {
        for name in [
            "send_chat",
            "answer_permission",
            "answer_elicit",
            "get_settings",
            "get_memory",
            "edit_memory_row",
            "forget_memory_row",
            "approve_memory_row",
            "reject_memory_row",
            "set_memory_writes",
            "list_connectors",
        ] {
            assert!(web_command(name), "{name}");
        }
        for name in [
            "save_settings",
            "set_theme",
            "set_api_key",
            "clear_api_key",
            "get_pulse",
            "pulse_act",
            "get_interests",
            "add_interest",
            "interest_act",
            "get_journal",
            "journal_add",
            "journal_act",
            "journal_to_mission",
            "get_permissions",
            "add_permission_rule",
            "remove_permission_rule",
            "remember_grant",
            "forget_grant",
            "get_personalities",
            "set_personality",
            "duplicate_personality",
            "save_personality",
            "delete_personality",
            "get_playbooks",
            "save_playbook",
            "duplicate_playbook",
            "reset_playbook",
            "get_plugins",
            "plugin_act",
            "install_plugin",
            "fetch_plugin_market",
            "get_local_ai",
            "save_local_ai",
            "get_capabilities",
            "get_web_access",
            "set_web_access",
            "phone_smoke_report",
        ] {
            assert!(!web_command(name), "{name}");
        }
    }

    #[test]
    fn frames_carry_the_event_name_and_its_payload() {
        assert_eq!(
            frame(
                "chat://event",
                &json!({ "conversationId": "c1", "event": { "kind": "text", "text": "a\nb" } })
            ),
            "data: {\"event\":\"chat://event\",\"payload\":{\"conversationId\":\"c1\",\"event\":{\"kind\":\"text\",\"text\":\"a\\nb\"}}}\n\n"
        );
    }

    #[test]
    fn links_carry_the_code_in_the_fragment() {
        assert_eq!(
            link("192.168.1.20".parse().expect("ip"), 4620, "abc"),
            "http://192.168.1.20:4620/#code=abc"
        );
        assert_eq!(
            link("fe80::1".parse().expect("ip"), 4620, "abc"),
            "http://[fe80::1]:4620/#code=abc"
        );
    }

    /// Plain HTTP/1.1 over a socket: (status, headers, body).
    fn http(
        port: u16,
        method: &str,
        path: &str,
        auth: Option<&str>,
        body: &str,
    ) -> (u16, String, String) {
        let mut sock = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let auth = auth
            .map(|c| format!("Authorization: Bearer {c}\r\n"))
            .unwrap_or_default();
        write!(
            sock,
            "{method} {path} HTTP/1.1\r\nHost: x\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .expect("send");
        let mut raw = String::new();
        sock.read_to_string(&mut raw).expect("read");
        let (head, body) = raw.split_once("\r\n\r\n").expect("a response");
        let status = head
            .split(' ')
            .nth(1)
            .and_then(|s| s.parse().ok())
            .expect("status");
        (status, head.to_string(), body.to_string())
    }

    fn parse(body: &str) -> Value {
        serde_json::from_str(body).expect("JSON")
    }

    fn serve(access: &WebAccess) -> (u16, String) {
        let dispatch: Dispatch = Arc::new(|command, args| match command {
            "get_settings" => Ok(json!({ "model": "grok-4.7" })),
            "send_chat" => Ok(json!(format!(
                "got {}",
                args["text"].as_str().unwrap_or("")
            ))),
            _ => Err(format!("{command} failed")),
        });
        let assets: Assets = Arc::new(|path| match path {
            "index.html" => Some((b"<html>app</html>".to_vec(), "text/html".into())),
            "assets/app.js" => Some((b"js".to_vec(), "text/javascript".into())),
            _ => None,
        });
        let view = access
            .start(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, dispatch, assets)
            .expect("starts");
        let port: u16 = view
            .links
            .last()
            .and_then(|l| l.strip_prefix("http://127.0.0.1:"))
            .and_then(|l| l.split('/').next())
            .and_then(|p| p.parse().ok())
            .expect("a localhost link");
        (port, view.code)
    }

    #[test]
    fn serves_the_page_and_runs_only_paired_web_commands() {
        let access = WebAccess::default();
        let (port, code) = serve(&access);
        assert_eq!(code.len(), 48);
        assert!(access.view().on);

        let (status, _, body) = http(port, "GET", "/remote/v1/hello", None, "");
        assert_eq!(
            (status, parse(&body)),
            (200, json!({ "kind": "grokhub-remote", "version": 1 }))
        );

        let (status, head, body) = http(port, "GET", "/", None, "");
        assert_eq!((status, body.as_str()), (200, "<html>app</html>"));
        assert!(
            head.contains("Content-Security-Policy: default-src 'self'"),
            "{head}"
        );
        assert!(head.contains("X-Frame-Options: DENY"), "{head}");
        let (_, _, body) = http(port, "GET", "/memory", None, "");
        assert_eq!(body, "<html>app</html>");
        let (_, head, body) = http(port, "GET", "/assets/app.js", None, "");
        assert_eq!(body, "js");
        assert!(head.contains("Content-Type: text/javascript"), "{head}");

        let call = "/remote/v1/call/send_chat";
        let args = r#"{"conversationId":"c1","text":"hi"}"#;
        assert_eq!(http(port, "POST", call, None, args).0, 401);
        assert_eq!(http(port, "POST", call, Some("wrong"), args).0, 401);
        let (status, _, body) = http(port, "POST", call, Some(&code), args);
        assert_eq!(
            (status, parse(&body)),
            (200, json!({ "ok": true, "value": "got hi" }))
        );
        let (_, _, body) = http(
            port,
            "POST",
            "/remote/v1/call/run_automation",
            Some(&code),
            "",
        );
        assert_eq!(
            parse(&body),
            json!({ "ok": false, "error": "run_automation failed" })
        );
        let key = r#"{"key":"sk-abcdefghijklmnopqrstuv"}"#;
        let (_, _, body) = http(
            port,
            "POST",
            "/remote/v1/call/set_api_key",
            Some(&code),
            key,
        );
        assert_eq!(parse(&body), json!({ "ok": false, "error": REFUSED }));
        let (_, _, body) = http(port, "POST", call, Some(&code), "{not json");
        let error = parse(&body)["error"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        assert!(error.starts_with("Bad arguments:"), "{body}");

        access.stop();
        assert!(!access.view().on);
        assert!(TcpStream::connect(("127.0.0.1", port)).is_err() || http_refused(port));
    }

    /// After `stop` the socket may linger briefly, but no request is served.
    fn http_refused(port: u16) -> bool {
        let Ok(mut sock) = TcpStream::connect(("127.0.0.1", port)) else {
            return true;
        };
        let _ = sock.set_read_timeout(Some(Duration::from_secs(2)));
        let _ = write!(sock, "GET /remote/v1/hello HTTP/1.1\r\nHost: x\r\n\r\n");
        let mut buf = [0u8; 16];
        !matches!(sock.read(&mut buf), Ok(n) if n > 0)
    }

    #[test]
    fn a_paired_stream_gets_broadcast_events_and_ends_on_stop() {
        let access = WebAccess::default();
        let (port, code) = serve(&access);
        let mut sock = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let _ = sock.set_read_timeout(Some(Duration::from_secs(10)));
        write!(
            sock,
            "GET /remote/v1/events HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {code}\r\n\r\n"
        )
        .expect("send");
        let mut lines = BufReader::new(sock);
        let mut line = String::new();
        let mut next = |line: &mut String| {
            line.clear();
            lines.read_line(line).expect("a line");
            line.clone()
        };
        assert_eq!(next(&mut line), "HTTP/1.1 200 OK\r\n");
        // Through the headers to the opening comment; the stream is
        // registered by then.
        while next(&mut line) != ": open\n" {}
        // A chat in the window isn't the browser's; only its own go out.
        access.broadcast(
            "chat://event",
            r#"{"conversationId":"window-1","event":{"kind":"text","text":"private"}}"#,
        );
        access.adopt("c1");
        access.broadcast(
            "chat://event",
            r#"{"conversationId":"c1","event":{"kind":"done","stop":"end_turn"}}"#,
        );
        while !next(&mut line).starts_with("data:") {}
        assert_eq!(
            line,
            "data: {\"event\":\"chat://event\",\"payload\":{\"conversationId\":\"c1\",\"event\":{\"kind\":\"done\",\"stop\":\"end_turn\"}}}\n"
        );
        access.stop();
        // The last chunk: a zero length, then the blank line.
        while next(&mut line) != "0\r\n" {}
        assert_eq!(next(&mut line), "\r\n");
    }
}
