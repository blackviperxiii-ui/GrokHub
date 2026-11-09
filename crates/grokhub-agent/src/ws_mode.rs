//! xAI WebSocket mode for the agent loop (round two, item 29).
//!
//! One socket per chat to `wss://api.x.ai/v1/responses`. The first request of
//! a chain sends the whole input, as HTTP does. Each later one sends only the
//! items that came after the last reply, with `previous_response_id`: the
//! server keeps the chain in memory on the open socket, so this works with
//! `store: false`. xAI measured up to about 20% less end-to-end time on
//! tool-heavy runs.
//!
//! The chain continues only when the new request repeats the last one
//! exactly: the same fields, the same input, then the reply's own items as
//! the loop stored them. Anything else (a compaction, a trim, a new model or
//! tool list) starts a fresh chain with the whole input on the same socket.
//! A socket that can't open, errors before any reply text, or is busy with
//! another turn hands the call back to HTTP, so the chat never depends on it.
//!
//! The socket is replaced before xAI's 59-minute limit, and after a quiet
//! spell. Switch it off with `enabled = false` in `websocket.toml`.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::client::{fold_events, ClientError, InputItem, StreamEvent, TurnOutput};
use crate::sse::SseParser;
use crate::{AuthKind, CancelToken};

/// The switch, in the config folder.
pub const CONFIG_NAME: &str = "websocket.toml";
/// xAI's WebSocket endpoint for the Responses API.
pub const WS_URL: &str = "wss://api.x.ai/v1/responses";
/// xAI closes a socket at 59 minutes; a new one opens before that.
const MAX_AGE: Duration = Duration::from_secs(55 * 60);
/// A socket quiet this long is closed on its next use and opened again.
const IDLE_CLOSE: Duration = Duration::from_secs(10 * 60);
/// Chats with a socket open at once; the least recently used idle one goes.
const MAX_SOCKETS: usize = 8;
/// After a socket fails to open, HTTP carries every call this long.
const DOWN_FOR: Duration = Duration::from_secs(10 * 60);
/// The whole open (name lookup, TCP, TLS, upgrade) gets this long before the
/// call goes over HTTP instead, so a bad network costs seconds, not tens.
#[cfg(not(test))]
const CONNECT_DEADLINE: Duration = Duration::from_secs(3);
#[cfg(test)]
const CONNECT_DEADLINE: Duration = Duration::from_millis(600);
/// Addresses tried for one open.
const CONNECT_ADDRS: usize = 2;
/// How often a pending open wakes to check Stop and the deadline.
const CONNECT_POLL: Duration = Duration::from_millis(50);
/// How often a read wakes to check Stop and the idle limit.
const READ_POLL: Duration = Duration::from_millis(250);

/// Whether WebSocket mode is on (it is unless `websocket.toml` says
/// `enabled = false`).
pub fn enabled_at(config_dir: &Path) -> bool {
    std::fs::read_to_string(config_dir.join(CONFIG_NAME))
        .map(|text| {
            crate::config_file::parse(CONFIG_NAME, &text, &["enabled"]).flag("enabled", true)
        })
        .unwrap_or(true)
}

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

/// Where the last reply on a socket left the chat.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Chain {
    /// The reply to continue from.
    id: String,
    /// The fields besides `input` that request sent.
    fields: u64,
    /// One hash per input item the server now holds: what was sent, then the
    /// reply's items as the loop stores them.
    held: Vec<u64>,
}

struct Conn {
    socket: Socket,
    opened: Instant,
    used: Instant,
    chain: Option<Chain>,
}

impl Conn {
    fn stale(&self, now: Instant) -> bool {
        now.duration_since(self.opened) >= MAX_AGE || now.duration_since(self.used) >= IDLE_CLOSE
    }
}

type Slot = Arc<Mutex<Option<Conn>>>;

/// How one turn on the socket ended.
pub enum WsTurn {
    Done(TurnOutput),
    Failed(ClientError),
    /// Nothing reached the chat: send it over HTTP instead.
    Http,
}

/// The sockets of one client, one per chat.
pub struct WsPool {
    url: String,
    slots: Mutex<HashMap<String, (Slot, Instant)>>,
    down_until: Mutex<Option<Instant>>,
    /// Requests the socket refused in a row that HTTP then carried. A few in
    /// a row means the socket path itself is refused: it rests a while.
    refused: std::sync::atomic::AtomicU32,
}

impl WsPool {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            slots: Mutex::new(HashMap::new()),
            down_until: Mutex::new(None),
            refused: std::sync::atomic::AtomicU32::new(0),
        }
    }

    fn down(&self) -> bool {
        let mut until = self.down_until.lock().unwrap_or_else(|e| e.into_inner());
        match *until {
            Some(t) if Instant::now() < t => true,
            Some(_) => {
                *until = None;
                false
            }
            None => false,
        }
    }

    fn mark_down(&self) {
        *self.down_until.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(Instant::now() + DOWN_FOR);
    }

    /// This chat's slot, made on first use; `None` when every slot is busy.
    fn slot(&self, chat: &str) -> Option<Slot> {
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        // A socket nobody used for IDLE_CLOSE is shut now, not when a ninth
        // chat needs its place: the server would close it anyway.
        slots.retain(|key, (slot, used)| {
            key == chat || Arc::strong_count(slot) > 1 || now.duration_since(*used) < IDLE_CLOSE
        });
        if let Some((slot, used)) = slots.get_mut(chat) {
            *used = now;
            return Some(slot.clone());
        }
        if slots.len() >= MAX_SOCKETS {
            // Drop the least recently used chat whose socket nobody holds.
            let idle = slots
                .iter()
                .filter(|(_, (slot, _))| Arc::strong_count(slot) == 1)
                .min_by_key(|(_, (_, used))| *used)
                .map(|(key, _)| key.clone());
            slots.remove(&idle?);
        }
        let slot: Slot = Arc::new(Mutex::new(None));
        slots.insert(chat.to_string(), (slot.clone(), now));
        Some(slot)
    }

    /// Sockets open now.
    #[cfg(test)]
    fn open_count(&self) -> usize {
        let slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        slots
            .values()
            .filter(|(slot, _)| slot.try_lock().map(|s| s.is_some()).unwrap_or(true))
            .count()
    }

    /// Run one turn on this chat's socket. `body` is the HTTP body
    /// ([`crate::client::responses_body`]); `wrote` the model the reasoning
    /// items are stamped with.
    #[allow(clippy::too_many_arguments)]
    pub fn turn(
        &self,
        bearer: &str,
        auth_kind: AuthKind,
        chat: &str,
        body: &Value,
        wrote: &str,
        cancel: &CancelToken,
        idle: Duration,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> WsTurn {
        let chat = chat.trim();
        if chat.is_empty() || self.down() {
            return WsTurn::Http;
        }
        let Some(slot) = self.slot(chat) else {
            return WsTurn::Http;
        };
        // One turn at a time per socket: a second caller goes over HTTP.
        let Ok(mut guard) = slot.try_lock() else {
            return WsTurn::Http;
        };
        let Some(input) = body.get("input").and_then(Value::as_array) else {
            return WsTurn::Http;
        };
        let fields = fields_hash(body);
        let sent: Vec<u64> = input.iter().map(item_hash).collect();
        // Up to two tries: a lost chain or an expired socket starts again
        // with the whole input on a fresh chain, once.
        for _ in 0..2 {
            let now = Instant::now();
            if guard
                .as_mut()
                .is_some_and(|c| c.stale(now) || !still_open(&mut c.socket))
            {
                close(guard.take());
            }
            if guard.is_none() {
                match connect_within(
                    &self.url,
                    bearer,
                    chat,
                    auth_kind == AuthKind::OAuth,
                    cancel,
                ) {
                    Ok(socket) => {
                        *guard = Some(Conn {
                            socket,
                            opened: now,
                            used: now,
                            chain: None,
                        });
                    }
                    Err(Opened::Cancelled) => return WsTurn::Failed(ClientError::Cancelled),
                    Err(Opened::Failed) => {
                        self.mark_down();
                        return WsTurn::Http;
                    }
                }
            }
            let Some(conn) = guard.as_mut() else {
                return WsTurn::Http;
            };
            conn.used = now;
            let chain = conn.chain.take();
            let chained = chain.as_ref().is_some_and(|c| continues(c, fields, &sent));
            let message = create_message(body, input, fields, &sent, chain.as_ref());
            if conn
                .socket
                .send(Message::text(message.to_string()))
                .is_err()
            {
                close(guard.take());
                return WsTurn::Http;
            }
            let mut shown = false;
            // Set once the server sent anything for this request: it took the
            // request and is billing it, so a drop is a retry, not a quiet
            // resend over HTTP.
            let mut accepted = false;
            let read = read_turn(conn, cancel, idle, auth_kind, &mut accepted, &mut |ev| {
                if matches!(&ev, StreamEvent::TextDelta(t) if !t.is_empty()) {
                    shown = true;
                }
                sink(ev)
            });
            match read {
                Read::Done(mut turn, id) => {
                    self.refused.store(0, std::sync::atomic::Ordering::Relaxed);
                    for item in &mut turn.reasoning_items {
                        if let InputItem::Reasoning { model, .. } = item {
                            *model = wrote.to_string();
                        }
                    }
                    if let Some(conn) = guard.as_mut() {
                        conn.used = Instant::now();
                        conn.chain = id.map(|id| Chain {
                            id,
                            fields,
                            held: held_after(&sent, &turn, wrote),
                        });
                    }
                    return WsTurn::Done(turn);
                }
                Read::Lost { retry } => {
                    // The chain is gone (an old id, or the socket's time ran
                    // out). Nothing was shown: start again in full.
                    if !retry {
                        close(guard.take());
                    }
                    if shown {
                        return WsTurn::Failed(ClientError::Disconnect);
                    }
                    if !chained && retry {
                        // A full request can't lose its chain: don't loop.
                        return WsTurn::Http;
                    }
                }
                Read::Failed(err, keep) => {
                    if !keep {
                        close(guard.take());
                    }
                    if !shown
                        && !accepted
                        && matches!(err, ClientError::Transport(_) | ClientError::Disconnect)
                    {
                        // The socket broke before the server took the
                        // request: HTTP carries it.
                        return WsTurn::Http;
                    }
                    if !shown && accepted && matches!(err, ClientError::Transport(_)) {
                        // Taken, then dropped: the retry policy decides.
                        return WsTurn::Failed(ClientError::Disconnect);
                    }
                    if !shown && refused_shape(&err) {
                        // A request HTTP may take: it goes there, and after
                        // three in a row the socket rests.
                        let n = self
                            .refused
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if n + 1 >= 3 {
                            self.mark_down();
                            self.refused.store(0, std::sync::atomic::Ordering::Relaxed);
                        }
                        return WsTurn::Http;
                    }
                    return WsTurn::Failed(err);
                }
            }
        }
        WsTurn::Http
    }
}

/// A refusal of the request's shape (400, 404, 422) rather than of the
/// account or the load: HTTP may take what the socket would not.
fn refused_shape(err: &ClientError) -> bool {
    matches!(err, ClientError::Protocol(m)
        if ["HTTP 400:", "HTTP 404:", "HTTP 422:"].iter().any(|p| m.starts_with(p)))
        && !err.is_context_length()
}

/// One `response.create`: the HTTP body's fields without `stream`, then
/// either the new items after the chain or the whole input.
fn create_message(
    body: &Value,
    input: &[Value],
    fields: u64,
    sent: &[u64],
    chain: Option<&Chain>,
) -> Value {
    let mut message = body.clone();
    let map = message.as_object_mut().expect("the body is an object");
    map.remove("stream");
    map.insert("type".into(), Value::from("response.create"));
    if let Some(chain) = chain.filter(|c| continues(c, fields, sent)) {
        map.insert("previous_response_id".into(), Value::from(chain.id.clone()));
        map.insert(
            "input".into(),
            Value::Array(input[chain.held.len()..].to_vec()),
        );
    }
    message
}

/// Whether a request with these fields and input hashes extends `chain`.
fn continues(chain: &Chain, fields: u64, sent: &[u64]) -> bool {
    chain.fields == fields && sent.len() > chain.held.len() && sent.starts_with(&chain.held)
}

/// What the server holds after `turn`: the input it was sent, then the
/// reply's items in the order and form the agent loop stores them (sealed
/// reasoning, the reply text without turn labels, the tool calls). If the loop stores them any
/// other way, the next request won't match and starts a fresh chain.
fn held_after(sent: &[u64], turn: &TurnOutput, model: &str) -> Vec<u64> {
    let mut items: Vec<InputItem> = turn.reasoning_items.clone();
    if !turn.text.is_empty() {
        items.push(InputItem::Message {
            role: "assistant".into(),
            // As run.rs stores it: with the turn labels taken out.
            content: vec![crate::client::ContentPart::InputText(
                grokhub_core::strip_turn_labels(&turn.text),
            )],
        });
    }
    for call in &turn.calls {
        items.push(InputItem::FunctionCall {
            call_id: call.call_id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
        });
    }
    let (wire, _) = crate::client::input_wire(&items, Some(model));
    let mut held = sent.to_vec();
    held.extend(wire.iter().map(item_hash));
    held
}

fn item_hash(item: &Value) -> u64 {
    let mut h = DefaultHasher::new();
    item.to_string().hash(&mut h);
    h.finish()
}

/// Every field but `input` and `stream`, hashed: a new model, effort or tool
/// list starts a fresh chain.
fn fields_hash(body: &Value) -> u64 {
    let mut rest = body.clone();
    if let Some(map) = rest.as_object_mut() {
        map.remove("input");
        map.remove("stream");
    }
    let mut h = DefaultHasher::new();
    rest.to_string().hash(&mut h);
    h.finish()
}

enum Read {
    /// The reply, with its id when the server sent one.
    Done(TurnOutput, Option<String>),
    /// The chain is gone; `retry` when the socket itself is still good.
    Lost { retry: bool },
    /// `keep` when the socket can carry the next turn.
    Failed(ClientError, bool),
}

fn read_turn(
    conn: &mut Conn,
    cancel: &CancelToken,
    idle: Duration,
    auth_kind: AuthKind,
    accepted: &mut bool,
    sink: &mut dyn FnMut(StreamEvent),
) -> Read {
    let mut parser = SseParser::new();
    let mut out = TurnOutput::default();
    let mut id: Option<String> = None;
    let mut last = Instant::now();
    loop {
        if cancel.is_cancelled() {
            // The reply is still coming on this socket: drop it.
            return Read::Failed(ClientError::Cancelled, false);
        }
        let text = match conn.socket.read() {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(_)) => return Read::Failed(ClientError::Disconnect, false),
            Ok(_) => continue,
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                if last.elapsed() >= idle {
                    return Read::Failed(ClientError::IdleTimeout, false);
                }
                continue;
            }
            Err(e) => return Read::Failed(ClientError::Transport(e.to_string()), false),
        };
        last = Instant::now();
        let Ok(event) = serde_json::from_str::<Value>(text.as_str()) else {
            continue;
        };
        let kind = event.get("type").and_then(Value::as_str).unwrap_or("");
        if kind != "error" && !kind.is_empty() {
            *accepted = true;
        }
        if kind == "error" {
            let code = event
                .pointer("/error/code")
                .and_then(Value::as_str)
                .unwrap_or("");
            match code {
                "previous_response_not_found" => return Read::Lost { retry: true },
                "websocket_connection_limit_reached" => return Read::Lost { retry: false },
                _ => {}
            }
            let status = event
                .get("status")
                .and_then(Value::as_u64)
                .and_then(|s| u16::try_from(s).ok())
                .unwrap_or(500);
            let message = event
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("model stream error");
            let message = grokhub_core::redact_secrets(message);
            if auth_kind == AuthKind::OAuth && crate::plan_room::plan_refusal(status, &message) {
                crate::plan_room::note_refused(grokhub_core::now_ms());
            }
            return Read::Failed(
                crate::client::map_http_status(status, &message, None, auth_kind),
                true,
            );
        }
        if matches!(
            kind,
            "response.created" | "response.completed" | "response.incomplete"
        ) {
            if let Some(found) = event.pointer("/response/id").and_then(Value::as_str) {
                id = Some(found.to_string());
            }
        }
        let frame = format!("data: {}\n\n", text.as_str());
        if let Err(err) = fold_events(parser.push(&frame), sink, &mut out) {
            return Read::Failed(err, true);
        }
        if matches!(kind, "response.completed" | "response.incomplete") {
            return match parser
                .finish()
                .and_then(|rest| fold_events(rest, sink, &mut out))
            {
                Ok(()) => Read::Done(out, id),
                Err(err) => Read::Failed(err, true),
            };
        }
    }
}

/// Open a socket: TCP with a timeout, TLS (rustls, ring, webpki roots) for
/// `wss`, then the upgrade with the bearer and the chat's cache key.
/// `signed_in`: the bearer is a Sign in with Grok token, so the upgrade's
/// rate-limit headers update the SuperGrok room ([`crate::plan_room`]).
fn connect(url: &str, bearer: &str, chat: &str, signed_in: bool) -> Result<Socket, String> {
    let mut request = url.into_client_request().map_err(|e| e.to_string())?;
    let headers = request.headers_mut();
    let header =
        |v: &str| tungstenite::http::HeaderValue::from_str(v).map_err(|_| "bad header".to_string());
    headers.insert("Authorization", header(&format!("Bearer {bearer}"))?);
    headers.insert("User-Agent", header(crate::client::USER_AGENT)?);
    headers.insert("x-grok-conv-id", header(&crate::subagent::cache_key(chat))?);
    let uri = request.uri();
    let host = uri.host().ok_or("no host")?.to_string();
    let tls = uri.scheme_str() == Some("wss");
    let port = uri.port_u16().unwrap_or(if tls { 443 } else { 80 });
    let addrs: Vec<_> = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .take(CONNECT_ADDRS)
        .collect();
    let mut last_err = String::from("no address");
    let mut stream = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, CONNECT_DEADLINE) {
            Ok(s) => {
                stream = Some(s);
                break;
            }
            Err(e) => last_err = e.to_string(),
        }
    }
    let stream = stream.ok_or(last_err)?;
    let _ = stream.set_nodelay(true);
    stream
        .set_read_timeout(Some(CONNECT_DEADLINE))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(CONNECT_DEADLINE))
        .map_err(|e| e.to_string())?;
    let connector = if tls {
        Some(tungstenite::Connector::Rustls(tls_config()?))
    } else {
        None
    };
    let (socket, upgrade) = tungstenite::client_tls_with_config(request, stream, None, connector)
        .map_err(|e| e.to_string())?;
    if signed_in {
        let headers = upgrade.headers();
        crate::plan_room::note_reply(
            upgrade.status().as_u16(),
            &|name| {
                headers
                    .get(name)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string)
            },
            grokhub_core::now_ms(),
        );
    }
    tcp_of(&socket)
        .ok_or("unknown stream")?
        .set_read_timeout(Some(READ_POLL))
        .map_err(|e| e.to_string())?;
    Ok(socket)
}

fn tcp_of(socket: &Socket) -> Option<&TcpStream> {
    match socket.get_ref() {
        MaybeTlsStream::Plain(s) => Some(s),
        MaybeTlsStream::Rustls(s) => Some(s.get_ref()),
        _ => None,
    }
}

/// Why a socket did not open.
enum Opened {
    /// Stop was pressed while it was opening.
    Cancelled,
    /// It failed, or missed [`CONNECT_DEADLINE`].
    Failed,
}

/// [`connect`] on its own thread, so Stop and the deadline are checked every
/// [`CONNECT_POLL`] while a name lookup or a handshake blocks. An open that
/// misses the deadline is left to end on its own timeouts.
fn connect_within(
    url: &str,
    bearer: &str,
    chat: &str,
    signed_in: bool,
    cancel: &CancelToken,
) -> Result<Socket, Opened> {
    use std::sync::mpsc::RecvTimeoutError;
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let (url, bearer, chat) = (url.to_string(), bearer.to_string(), chat.to_string());
    std::thread::Builder::new()
        .name("ws-connect".into())
        .spawn(move || {
            let _ = tx.send(connect(&url, &bearer, &chat, signed_in));
        })
        .map_err(|_| Opened::Failed)?;
    let deadline = Instant::now() + CONNECT_DEADLINE;
    loop {
        if cancel.is_cancelled() {
            return Err(Opened::Cancelled);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Opened::Failed);
        }
        match rx.recv_timeout(left.min(CONNECT_POLL)) {
            Ok(Ok(socket)) => return Ok(socket),
            Ok(Err(_)) | Err(RecvTimeoutError::Disconnected) => return Err(Opened::Failed),
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

/// Between turns nobody reads a socket. Before the next turn, read what came
/// meanwhile without waiting: pings get their pong, and a socket the server
/// closed is reported, so the turn opens a fresh one instead of failing.
fn still_open(socket: &mut Socket) -> bool {
    let Some(tcp) = tcp_of(socket) else {
        return false;
    };
    if tcp.set_nonblocking(true).is_err() {
        return false;
    }
    let open = loop {
        match socket.read() {
            Ok(Message::Close(_)) => break false,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                break true
            }
            Err(_) => break false,
        }
    };
    let restored = tcp_of(socket).is_some_and(|tcp| tcp.set_nonblocking(false).is_ok());
    // Send any pong the reads queued.
    open && restored && socket.flush().is_ok()
}

fn tls_config() -> Result<Arc<rustls::ClientConfig>, String> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| e.to_string())?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(Arc::new(config))
}

fn close(conn: Option<Conn>) {
    if let Some(mut conn) = conn {
        let _ = conn.socket.close(None);
        let _ = conn.socket.flush();
    }
}

#[cfg(test)]
#[path = "ws_mode_tests.rs"]
mod tests;
