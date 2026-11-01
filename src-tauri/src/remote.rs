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
//! - Only [`WEB_COMMANDS`] run. The API key, settings, Local AI and desktop
//!   control stay on this computer, and web chats never get the desktop
//!   tools. Pulse works the same as on the desktop; its images come from
//!   this computer's cache, never from the source sites.

use std::collections::{HashSet, VecDeque};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::Path;
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
/// Recent chat frames kept for a tab that reconnects; older ones are a gap.
const REPLAY: usize = 4096;
/// Sent instead of the missed frames when they are older than [`REPLAY`].
pub const GAP_EVENT: &str = "remote://gap";
/// How long the pairing code lasts with no paired browser calling, opening a
/// stream or getting a chat event. Keep in step with `WEB_OFF_NOTE` in
/// ui/src/WebAccess.tsx.
pub const IDLE: Duration = Duration::from_secs(30 * 60);

pub fn refused() -> String {
    format!(
        "The web app can't do that. Use {} on your computer.",
        grokhub_core::brand::product_name()
    )
}
pub const NOT_YOURS: &str = "That chat isn't open in this browser.";

/// The web app's commands: chat with approvals, automations, memory,
/// connectors, Pulse and Imagine, and reading the settings.
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
    "get_pulse",
    "pulse_act",
    "pulse_discuss",
    "pulse_why",
    "get_feed_prompt",
    "save_feed_prompt",
    "pulse_image",
    "get_imagine",
    "imagine_generate",
    "imagine_cancel",
    "imagine_image",
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

/// One server-sent event frame, with its id so a reconnecting tab can ask
/// for what came after it (`Last-Event-ID`).
pub fn frame(id: u64, event: &str, payload: &Value) -> String {
    format!(
        "id: {id}\ndata: {}\n\n",
        json!({ "event": event, "payload": payload })
    )
}

/// The pairing link a browser opens: `https` on the LAN, `http` on loopback.
pub fn link(scheme: &str, ip: IpAddr, port: u16, code: &str) -> String {
    format!("{scheme}://{}/#code={code}", SocketAddr::new(ip, port))
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
    /// SHA-256 of the HTTPS certificate, `AB:CD:…`, to compare with the
    /// browser's. Empty while off.
    pub fingerprint: String,
}

/// Runs a web command (one of [`WEB_COMMANDS`]) with its JSON arguments.
pub type Dispatch = Arc<dyn Fn(&str, Value) -> Result<Value, String> + Send + Sync>;
/// A UI file by path: its bytes and MIME type.
pub type Assets = Arc<dyn Fn(&str) -> Option<(Vec<u8>, String)> + Send + Sync>;

struct Running {
    pairing: Shared,
    /// The plain server's port: on loopback only when `front` serves the LAN.
    port: u16,
    server: Arc<Server>,
    /// The HTTPS front on the LAN, with its certificate's fingerprint.
    front: Option<(crate::remote_tls::Front, String)>,
}

type Streams = Arc<Mutex<Hub>>;
type Chats = Arc<Mutex<HashSet<String>>>;
type Shared = Arc<Mutex<Pairing>>;

/// The open event streams and the recent frames they were sent, numbered,
/// so a tab whose stream dropped gets what it missed when it reopens.
#[derive(Default)]
struct Hub {
    txs: Vec<Sender<String>>,
    /// The last [`REPLAY`] frames, oldest first, by id.
    log: VecDeque<(u64, String)>,
    /// The id of the newest frame; 0 before the first.
    last: u64,
}

impl Hub {
    /// Ends every stream and forgets the frames; ids keep counting.
    fn clear(&mut self) {
        self.txs.clear();
        self.log.clear();
    }

    /// Numbers a frame, keeps it for replay and sends it to every stream.
    /// True when some stream took it.
    fn send(&mut self, event: &str, payload: &Value) -> bool {
        self.last += 1;
        let text = frame(self.last, event, payload);
        if self.log.len() == REPLAY {
            self.log.pop_front();
        }
        self.log.push_back((self.last, text.clone()));
        self.txs.retain(|tx| tx.send(text.clone()).is_ok());
        !self.txs.is_empty()
    }

    /// What a tab that last saw `since` missed: the frames after it, or a
    /// gap frame (carrying the newest id) when some are no longer kept.
    fn missed(&self, since: u64) -> Vec<String> {
        if since == self.last {
            return Vec::new();
        }
        let kept_from = self.log.front().map_or(self.last + 1, |(id, _)| *id);
        if since > self.last || since + 1 < kept_from {
            return vec![frame(self.last, GAP_EVENT, &Value::Null)];
        }
        self.log
            .iter()
            .filter(|(id, _)| *id > since)
            .map(|(_, text)| text.clone())
            .collect()
    }
}

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
                fingerprint: String::new(),
            },
            Some(run) => {
                let code = live(&run.pairing, &self.streams, &self.chats, Instant::now())
                    .code
                    .clone();
                let lan = match &run.front {
                    Some((front, _)) => ("https", front.port),
                    None => ("http", run.port),
                };
                let mut links: Vec<String> = lan_ip()
                    .map(|ip| link(lan.0, ip, lan.1, &code))
                    .into_iter()
                    .collect();
                links.push(link(
                    "http",
                    IpAddr::V4(Ipv4Addr::LOCALHOST),
                    run.port,
                    &code,
                ));
                WebAccessView {
                    on: true,
                    code,
                    links,
                    fingerprint: run
                        .front
                        .as_ref()
                        .map(|(_, print)| print.clone())
                        .unwrap_or_default(),
                }
            }
        }
    }

    /// Serves on `bind:port` with a new pairing code. Already on: no change.
    /// With `tls`, the certificate's folder, `bind:port` is HTTPS
    /// (`remote_tls`) and the plain server listens on a loopback port only.
    pub fn start(
        &self,
        bind: IpAddr,
        port: u16,
        tls: Option<&Path>,
        dispatch: Dispatch,
        assets: Assets,
    ) -> Result<WebAccessView, String> {
        {
            let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
            if running.is_none() {
                let plain = match tls {
                    Some(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                    None => SocketAddr::new(bind, port),
                };
                let server = Server::http(plain)
                    .map_err(|err| format!("Web access couldn't start on port {port}: {err}"))?;
                let inner = server.server_addr().to_ip().unwrap_or(plain);
                let front = match tls {
                    Some(dir) => {
                        let mut hosts = vec!["localhost".to_string(), "127.0.0.1".to_string()];
                        hosts.extend(lan_ip().map(|ip| ip.to_string()));
                        let cert = crate::remote_tls::Cert::load_or_make(dir, &hosts)?;
                        let print = cert.fingerprint()?;
                        let front =
                            crate::remote_tls::serve(SocketAddr::new(bind, port), inner, &cert)?;
                        Some((front, print))
                    }
                    None => None,
                };
                let port = inner.port();
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
                    front,
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

    /// Hands an event to every paired browser, and keeps it for a tab that
    /// is reconnecting.
    pub fn broadcast(&self, event: &str, payload: &str) {
        let mut streams = self.streams.lock().unwrap_or_else(|e| e.into_inner());
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
        let delivered = streams.send(event, &payload);
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
                &json!({
                    "ok": false,
                    "error": format!("{} is busy. Try again.", grokhub_core::brand::product_name())
                }),
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
            let since = req
                .headers()
                .iter()
                .find(|h| h.field.equiv("Last-Event-ID"))
                .and_then(|h| h.value.as_str().trim().parse::<u64>().ok());
            stream(req, &job.streams, since);
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
            let mut reply = match args {
                Err(err) => json!({ "ok": false, "error": format!("Bad arguments: {err}") }),
                Ok(_) if !web_command(&command) => json!({ "ok": false, "error": refused() }),
                Ok(args) => match (job.dispatch)(&command, args) {
                    Ok(value) => json!({ "ok": true, "value": value }),
                    Err(error) => json!({ "ok": false, "error": error }),
                },
            };
            // The newest event id once the command ran: the page waits for
            // its stream to reach it, so a chat's events land before the
            // call that ran the chat returns.
            reply["seq"] = json!(job.streams.lock().unwrap_or_else(|e| e.into_inner()).last);
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

/// An event stream. `since` is the `Last-Event-ID` of a tab reconnecting:
/// what it missed goes out first, under the same lock as new frames, so
/// nothing is skipped or sent twice.
fn stream(req: Request, streams: &Streams, since: Option<u64>) {
    let (tx, rx) = mpsc::channel::<String>();
    {
        let mut hub = streams.lock().unwrap_or_else(|e| e.into_inner());
        hub.txs
            .retain(|tx| tx.send(": ping\n\n".to_string()).is_ok());
        if hub.txs.len() >= MAX_STREAMS {
            drop(hub);
            return send_json(
                req,
                503,
                &json!({ "ok": false, "error": "Too many open tabs." }),
            );
        }
        // A new tab starts from the newest id; a returning one catches up.
        let first = match since {
            Some(id) => hub.missed(id),
            None => vec![format!("id: {}\n\n", hub.last)],
        };
        for text in first {
            let _ = tx.send(text);
        }
        hub.txs.push(tx);
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
        streams.lock().unwrap().txs.push(tx);
        chats.lock().unwrap().insert("web-chat".into());

        let old = live(
            &pairing,
            &streams,
            &chats,
            start + IDLE - Duration::from_secs(1),
        )
        .code
        .clone();
        assert_eq!(streams.lock().unwrap().txs.len(), 1);
        assert!(chats.lock().unwrap().contains("web-chat"));

        let new = live(&pairing, &streams, &chats, start + IDLE).code.clone();
        assert_ne!(new, old);
        assert!(streams.lock().unwrap().txs.is_empty());
        assert!(chats.lock().unwrap().is_empty());
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(1)),
            Err(RecvTimeoutError::Disconnected)
        );
    }

    #[test]
    fn the_web_gets_chat_memory_automations_connectors_and_pulse_only() {
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
            "get_pulse",
            "pulse_act",
            "pulse_discuss",
            "pulse_why",
            "get_feed_prompt",
            "save_feed_prompt",
            "pulse_image",
            "get_imagine",
            "imagine_generate",
            "imagine_cancel",
            "imagine_image",
        ] {
            assert!(web_command(name), "{name}");
        }
        for name in [
            "save_settings",
            "set_theme",
            "set_api_key",
            "clear_api_key",
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
            "set_explicit_personalities",
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
            "start_local_engine_download",
            "cancel_local_engine_download",
            "start_local_drafter_download",
            "cancel_local_drafter_download",
            "get_capabilities",
            "get_web_access",
            "set_web_access",
            "phone_smoke_report",
        ] {
            assert!(!web_command(name), "{name}");
        }
    }

    #[test]
    fn frames_carry_their_id_the_event_name_and_its_payload() {
        assert_eq!(
            frame(
                7,
                "chat://event",
                &json!({ "conversationId": "c1", "event": { "kind": "text", "text": "a\nb" } })
            ),
            "id: 7\ndata: {\"event\":\"chat://event\",\"payload\":{\"conversationId\":\"c1\",\"event\":{\"kind\":\"text\",\"text\":\"a\\nb\"}}}\n\n"
        );
    }

    fn text(n: u64) -> Value {
        json!({ "conversationId": "c1", "event": { "kind": "text", "text": n.to_string() } })
    }

    #[test]
    fn a_reconnecting_tab_gets_exactly_the_frames_it_missed() {
        let mut hub = Hub::default();
        for n in 1..=3 {
            assert!(!hub.send("chat://event", &text(n)), "no stream open yet");
        }
        assert_eq!(hub.last, 3);
        assert_eq!(
            hub.missed(1),
            vec![
                frame(2, "chat://event", &text(2)),
                frame(3, "chat://event", &text(3))
            ]
        );
        assert_eq!(hub.missed(3), Vec::<String>::new());
    }

    #[test]
    fn frames_older_than_the_replay_window_are_a_gap() {
        let mut hub = Hub::default();
        for n in 1..=(REPLAY as u64 + 2) {
            hub.send("chat://event", &text(n));
        }
        let newest = REPLAY as u64 + 2;
        assert_eq!(hub.log.len(), REPLAY);
        // Frames 1 and 2 are gone, so a tab that saw only frame 1 missed one.
        let gap = vec![frame(newest, GAP_EVENT, &Value::Null)];
        assert_eq!(hub.missed(1), gap);
        // A tab that saw frame 2 needs only what is still kept.
        assert_eq!(hub.missed(2).len(), REPLAY);
        assert_eq!(
            hub.missed(newest - 1),
            vec![frame(newest, "chat://event", &text(newest))]
        );
        // An id from before a restart (ahead of this run's count) is a gap too.
        assert_eq!(hub.missed(newest + 5), gap);
        hub.clear();
        assert_eq!(
            hub.missed(newest - 1),
            vec![frame(newest, GAP_EVENT, &Value::Null)]
        );
    }

    #[test]
    fn links_carry_the_code_in_the_fragment() {
        assert_eq!(
            link("https", "192.168.1.20".parse().expect("ip"), 4620, "abc"),
            "https://192.168.1.20:4620/#code=abc"
        );
        assert_eq!(
            link("http", "fe80::1".parse().expect("ip"), 4620, "abc"),
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
            .start(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, None, dispatch, assets)
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
            (200, json!({ "ok": true, "value": "got hi", "seq": 0 }))
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
            json!({ "ok": false, "error": "run_automation failed", "seq": 0 })
        );
        let key = r#"{"key":"sk-abcdefghijklmnopqrstuv"}"#;
        let (_, _, body) = http(
            port,
            "POST",
            "/remote/v1/call/set_api_key",
            Some(&code),
            key,
        );
        assert_eq!(
            parse(&body),
            json!({ "ok": false, "error": refused(), "seq": 0 })
        );
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

    #[test]
    fn a_stream_reopened_with_last_event_id_gets_what_it_missed_first() {
        let access = WebAccess::default();
        let (port, code) = serve(&access);
        access.adopt("c1");
        // The tab saw event 1, then its stream dropped while 2 and 3 went out.
        for n in 1..=3 {
            access.broadcast("chat://event", &text(n).to_string());
        }
        let mut sock = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let _ = sock.set_read_timeout(Some(Duration::from_secs(10)));
        write!(
            sock,
            "GET /remote/v1/events HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {code}\r\nLast-Event-ID: 1\r\n\r\n"
        )
        .expect("send");
        let mut lines = BufReader::new(sock);
        let mut seen = Vec::new();
        while seen.len() < 4 {
            let mut line = String::new();
            lines.read_line(&mut line).expect("a line");
            if line.starts_with("id:") || line.starts_with("data:") {
                seen.push(line);
            }
        }
        let want: Vec<String> = [2, 3]
            .into_iter()
            .flat_map(|n| {
                let f = frame(n, "chat://event", &text(n));
                let (id, data) = f.trim_end().split_once('\n').unwrap();
                [format!("{id}\n"), format!("{data}\n")]
            })
            .collect();
        assert_eq!(seen, want);
        // A call's reply names the newest id, so the page can wait for it.
        let (_, _, body) = http(
            port,
            "POST",
            "/remote/v1/call/get_settings",
            Some(&code),
            "",
        );
        assert_eq!(parse(&body)["seq"], json!(3));
        access.stop();
    }
}
