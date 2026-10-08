//! Streamable HTTP and legacy SSE. Synchronous `ureq` only.
//!
//! TODO(oauth): the browser OAuth flow (dynamic client registration, PKCE,
//! refresh) is not implemented. Requests send only the `Authorization` header
//! already stored on the server entry (`headers.Authorization` or
//! `bearerToken` / `bearer_token`). This client does not read credential
//! files, invent tokens, or open a browser.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::rpc::{self, RawTool};
use crate::harness::egress_dest;

const USER_AGENT: &str = concat!("GrokHub/", env!("CARGO_PKG_VERSION"));

enum IoMsg {
    Msg(Value),
    Endpoint(String),
    Eof(String),
}

enum Body {
    Json(Value),
    Empty,
    Stream(Box<dyn Read + Send>),
}

struct Posted {
    session: Option<String>,
    body: Body,
}

pub(crate) struct HttpConn {
    server: String,
    agent: ureq::Agent,
    /// Message URL. Legacy SSE replaces this with the `endpoint` event.
    post_url: String,
    headers: BTreeMap<String, String>,
    session_id: Option<String>,
    next_id: u64,
    legacy: bool,
    legacy_rx: Option<Receiver<IoMsg>>,
    stop: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
}

pub(crate) fn connect(
    server: &str,
    url: &str,
    sse: bool,
    headers: &BTreeMap<String, String>,
    startup: Duration,
    tool_timeout: Duration,
) -> Result<(HttpConn, Vec<RawTool>), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(format!("MCP server `{server}` url must be http or https"));
    }
    // EgressGuard (Spike-4c): the handshake (initialize, tools/list) carries
    // no user data, so it is one public line per connect.
    guard_handshake(url)?;
    let mut conn = HttpConn {
        server: server.to_string(),
        agent: agent(tool_timeout.max(startup)),
        post_url: url.to_string(),
        headers: headers.clone(),
        session_id: None,
        next_id: 0,
        legacy: sse,
        legacy_rx: None,
        stop: Arc::new(AtomicBool::new(false)),
        reader: None,
    };
    if sse {
        conn.open_legacy(startup)?;
        if egress_dest(&conn.post_url) != egress_dest(url) {
            guard_handshake(&conn.post_url)?;
        }
    }
    let init = conn.roundtrip("initialize", rpc::initialize_params(), startup)?;
    let _ = init;
    conn.notify("notifications/initialized", json!({}))?;
    let tools = list_tools(&mut conn, startup)?;
    Ok((conn, tools))
}

impl HttpConn {
    pub(crate) fn call(
        &mut self,
        name: &str,
        args: &Value,
        timeout: Duration,
        stop: &dyn Fn() -> bool,
    ) -> Result<(String, bool), String> {
        // Tool args are chat, or personal when they carry a recall-pack line.
        // A hard Send waits on the cabin's card; Deny sends nothing.
        let data = crate::harness::model_text_classes(&args.to_string());
        let req = crate::harness::EgressReq::new(&self.post_url, data);
        let tool = format!("mcp:{}", self.server);
        crate::harness::guard_or_park(&crate::perm::config_dir(), &req, &tool, &mut || stop())?;
        let result = self.roundtrip(
            "tools/call",
            json!({"name": name, "arguments": args}),
            timeout,
        )?;
        Ok(rpc::tool_output(&result))
    }

    pub(crate) fn ping(&mut self, timeout: Duration) -> Result<(), String> {
        guard_handshake(&self.post_url)?;
        self.roundtrip("ping", json!({}), timeout).map(|_| ())
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        let posted = self.exchange(&rpc::notification(method, params), Duration::from_secs(15))?;
        // A notification has no id. Drop a stream body so the POST connection closes.
        drop(posted);
        Ok(())
    }

    fn roundtrip(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        self.next_id = self.next_id.saturating_add(1);
        let id = self.next_id;
        let posted = self.exchange(&rpc::request(id, method, params), timeout)?;
        match posted {
            Body::Json(msg) => {
                if rpc::id_matches(&msg, id) {
                    return take_result(&msg);
                }
                if let Some(reply) = super::elicit::answer_elicitation(&self.server, &msg) {
                    self.exchange(&reply, timeout)?;
                }
                Err(format!("MCP server `{}` stopped", self.server))
            }
            Body::Empty => {
                if self.legacy {
                    self.wait_legacy(id, timeout)
                } else {
                    Err(format!("MCP server `{}` stopped", self.server))
                }
            }
            Body::Stream(reader) => {
                let (tx, rx) = mpsc::channel();
                let stop = self.stop.clone();
                let _reader = thread_reader(reader, tx, stop);
                self.wait_rx(id, &rx, timeout)
            }
        }
    }

    fn wait_legacy(&mut self, id: u64, timeout: Duration) -> Result<Value, String> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(format!("MCP server `{}` timed out", self.server));
            }
            let msg = {
                let rx = self
                    .legacy_rx
                    .as_ref()
                    .ok_or_else(|| format!("MCP server `{}` stopped", self.server))?;
                rx.recv_timeout(left.min(Duration::from_millis(200)))
            };
            match msg {
                Ok(IoMsg::Msg(msg)) => {
                    if rpc::id_matches(&msg, id) {
                        return take_result(&msg);
                    }
                    if let Some(reply) = super::elicit::answer_elicitation(&self.server, &msg) {
                        let _ = self.exchange(&reply, left);
                    } else if msg.get("method").is_some() && msg.get("id").is_some() {
                        let rid = msg.get("id").cloned().unwrap_or(Value::Null);
                        let _ = self.exchange(
                            &json!({
                                "jsonrpc": "2.0",
                                "id": rid,
                                "error": {"code": -32601, "message": "method not supported"}
                            }),
                            left,
                        );
                    }
                }
                Ok(IoMsg::Endpoint(_)) => continue,
                Ok(IoMsg::Eof(err)) => return Err(stopped(&self.server, &err)),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(format!("MCP server `{}` stopped", self.server));
                }
            }
        }
    }

    fn wait_rx(
        &mut self,
        id: u64,
        rx: &Receiver<IoMsg>,
        timeout: Duration,
    ) -> Result<Value, String> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(format!("MCP server `{}` timed out", self.server));
            }
            match rx.recv_timeout(left.min(Duration::from_millis(200))) {
                Ok(IoMsg::Msg(msg)) => {
                    if rpc::id_matches(&msg, id) {
                        return take_result(&msg);
                    }
                    if let Some(reply) = super::elicit::answer_elicitation(&self.server, &msg) {
                        let _ = self.exchange(&reply, left);
                    } else if msg.get("method").is_some() && msg.get("id").is_some() {
                        let rid = msg.get("id").cloned().unwrap_or(Value::Null);
                        let _ = self.exchange(
                            &json!({
                                "jsonrpc": "2.0",
                                "id": rid,
                                "error": {"code": -32601, "message": "method not supported"}
                            }),
                            left,
                        );
                    }
                }
                Ok(IoMsg::Endpoint(_)) => continue,
                Ok(IoMsg::Eof(err)) => {
                    return Err(stopped(&self.server, &err));
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(format!("MCP server `{}` stopped", self.server));
                }
            }
        }
    }

    fn exchange(&mut self, body: &Value, timeout: Duration) -> Result<Body, String> {
        let posted = post_value(
            &self.agent,
            &self.post_url,
            &self.headers,
            self.session_id.as_deref(),
            body,
            timeout,
        )
        .map_err(|err| stopped(&self.server, &err))?;
        if let Some(sid) = posted.session {
            if self.session_id.is_none() {
                self.session_id = Some(sid);
            }
        }
        Ok(posted.body)
    }

    fn open_legacy(&mut self, timeout: Duration) -> Result<(), String> {
        let base = self.post_url.clone();
        let resp = open_get(
            &self.agent,
            &base,
            &self.headers,
            self.session_id.as_deref(),
        )
        .map_err(|err| stopped(&self.server, &err))?;
        let (tx, rx) = mpsc::channel();
        let stop = self.stop.clone();
        self.reader = Some(thread_reader(resp.into_reader(), tx, stop));
        self.legacy_rx = Some(rx);
        let endpoint = {
            let rx = self.legacy_rx.as_ref().expect("legacy receiver");
            let deadline = Instant::now() + timeout;
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(format!("MCP server `{}` timed out", self.server));
                }
                match rx.recv_timeout(left) {
                    Ok(IoMsg::Endpoint(url)) => break url,
                    Ok(IoMsg::Msg(_)) => continue,
                    Ok(IoMsg::Eof(err)) => return Err(stopped(&self.server, &err)),
                    Err(RecvTimeoutError::Timeout) => {
                        return Err(format!("MCP server `{}` timed out", self.server));
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        return Err(format!("MCP server `{}` stopped", self.server));
                    }
                }
            }
        };
        self.post_url = resolve_endpoint(&base, &endpoint)?;
        Ok(())
    }
}

impl Drop for HttpConn {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Detach the reader. The socket unblocks when the peer closes or the
        // read timeout fires. Joining here would wait out that timeout.
        self.reader.take();
        self.legacy_rx.take();
    }
}

fn guard_handshake(url: &str) -> Result<(), String> {
    crate::harness::guard_quiet(&crate::perm::config_dir(), &crate::harness::EgressReq::new(url, &[]))
}

fn list_tools(conn: &mut HttpConn, timeout: Duration) -> Result<Vec<RawTool>, String> {
    let mut all = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..100 {
        let mut params = json!({});
        if let Some(token) = &cursor {
            params["cursor"] = json!(token);
        }
        let result = conn.roundtrip("tools/list", params, timeout)?;
        let (page, next) = rpc::parse_tools(&result);
        all.extend(page);
        match next {
            Some(token) => cursor = Some(token),
            None => return Ok(all),
        }
    }
    Ok(all)
}

fn agent(read: Duration) -> ureq::Agent {
    let read = if read.is_zero() {
        Duration::from_secs(60)
    } else {
        read
    };
    let builder = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(3600).max(read))
        .timeout_write(Duration::from_secs(30))
        .redirects(0);
    // Tests reach a loopback server under a `.test` name, so the guard sees a
    // real (non-loopback) destination. Production resolves as usual.
    #[cfg(test)]
    let builder = builder.resolver(tests::resolve_test_host);
    builder.build()
}

fn open_get(
    agent: &ureq::Agent,
    url: &str,
    headers: &BTreeMap<String, String>,
    session: Option<&str>,
) -> Result<ureq::Response, String> {
    let mut req = agent.get(url);
    req = apply_headers(req, headers, session);
    req = req.set("Accept", "text/event-stream");
    req = req.set("MCP-Protocol-Version", rpc::PROTOCOL);
    match req.call() {
        Ok(resp) => Ok(resp),
        Err(ureq::Error::Status(code, resp)) => Err(http_err(code, resp)),
        Err(ureq::Error::Transport(err)) => Err(err.to_string()),
    }
}

fn post_value(
    agent: &ureq::Agent,
    url: &str,
    headers: &BTreeMap<String, String>,
    session: Option<&str>,
    body: &Value,
    timeout: Duration,
) -> Result<Posted, String> {
    let text = serde_json::to_string(body).map_err(|err| err.to_string())?;
    let mut req = agent.post(url);
    req = apply_headers(req, headers, session);
    req = req
        .set("Content-Type", "application/json")
        .set("Accept", "application/json, text/event-stream")
        .set("MCP-Protocol-Version", rpc::PROTOCOL)
        .timeout(timeout.max(Duration::from_secs(1)));
    let resp = match req.send_string(&text) {
        Ok(resp) => resp,
        Err(ureq::Error::Status(code, resp)) => return Err(http_err(code, resp)),
        Err(ureq::Error::Transport(err)) => return Err(err.to_string()),
    };
    let session = resp.header("mcp-session-id").map(str::to_string);
    let ctype = resp.content_type().to_ascii_lowercase();
    if ctype.contains("text/event-stream") {
        return Ok(Posted {
            session,
            body: Body::Stream(resp.into_reader()),
        });
    }
    let text = resp.into_string().unwrap_or_default();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok(Posted {
            session,
            body: Body::Empty,
        });
    }
    let msg = serde_json::from_str(trimmed).map_err(|err| err.to_string())?;
    Ok(Posted {
        session,
        body: Body::Json(msg),
    })
}

/// User headers from the server entry, then the session id.
/// `Authorization` is whatever the entry already stored. See the oauth TODO above.
fn apply_headers(
    mut req: ureq::Request,
    headers: &BTreeMap<String, String>,
    session: Option<&str>,
) -> ureq::Request {
    req = req.set("User-Agent", USER_AGENT);
    for (key, value) in headers {
        req = req.set(key, value);
    }
    if let Some(sid) = session {
        req = req.set("Mcp-Session-Id", sid);
    }
    req
}

fn http_err(code: u16, resp: ureq::Response) -> String {
    let body = resp.into_string().unwrap_or_default();
    let body = body.trim();
    if body.is_empty() {
        format!("HTTP {code}")
    } else {
        format!("HTTP {code}: {body}")
    }
}

fn take_result(msg: &Value) -> Result<Value, String> {
    if let Some(err) = rpc::rpc_error(msg) {
        return Err(err);
    }
    Ok(msg.get("result").cloned().unwrap_or(Value::Null))
}

fn stopped(server: &str, err: &str) -> String {
    let err = err.trim();
    if err.is_empty() {
        format!("MCP server `{server}` stopped")
    } else if err.to_ascii_lowercase().contains("timed out")
        || err.to_ascii_lowercase().contains("timeout")
    {
        format!("MCP server `{server}` timed out")
    } else {
        format!("MCP server `{server}` stopped: {err}")
    }
}

fn thread_reader(
    reader: impl Read + Send + 'static,
    tx: mpsc::Sender<IoMsg>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    std::thread::spawn(move || pump_sse(BufReader::new(reader), tx, stop))
}

fn pump_sse(mut reader: impl BufRead, tx: mpsc::Sender<IoMsg>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        match read_sse(&mut reader) {
            Ok(Some((event, data))) => {
                if event == "endpoint"
                    || (!data.trim_start().starts_with('{') && !data.trim_start().starts_with('['))
                {
                    if tx.send(IoMsg::Endpoint(data)).is_err() {
                        break;
                    }
                    continue;
                }
                match serde_json::from_str::<Value>(&data) {
                    Ok(msg) => {
                        if tx.send(IoMsg::Msg(msg)).is_err() {
                            break;
                        }
                    }
                    Err(err) => {
                        let _ = tx.send(IoMsg::Eof(err.to_string()));
                        break;
                    }
                }
            }
            Ok(None) => {
                let _ = tx.send(IoMsg::Eof(String::new()));
                break;
            }
            Err(err) => {
                let _ = tx.send(IoMsg::Eof(err));
                break;
            }
        }
    }
}

fn read_sse(reader: &mut impl BufRead) -> Result<Option<(String, String)>, String> {
    let mut event = String::new();
    let mut data = String::new();
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).map_err(|err| err.to_string())?;
        if n == 0 {
            if data.is_empty() && event.is_empty() {
                return Ok(None);
            }
            break;
        }
        if line.len() > 8 * 1024 * 1024 {
            return Err("message too large".into());
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            if data.is_empty() && event.is_empty() {
                continue;
            }
            break;
        }
        if let Some(rest) = line.strip_prefix(':') {
            let _ = rest;
            continue;
        }
        if let Some(rest) = line.strip_prefix("event:") {
            event = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(rest.trim_start());
        }
    }
    Ok(Some((event, data)))
}

pub(crate) fn resolve_endpoint(base: &str, endpoint: &str) -> Result<String, String> {
    let endpoint = endpoint.trim();
    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        return Ok(endpoint.to_string());
    }
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err("MCP url must be http or https".into());
    }
    let Some(scheme) = base.find("://") else {
        return Err("MCP url must be http or https".into());
    };
    let rest = &base[scheme + 3..];
    let host_end = rest.find('/').unwrap_or(rest.len());
    let origin = &base[..scheme + 3 + host_end];
    if endpoint.starts_with('/') {
        return Ok(format!("{origin}{endpoint}"));
    }
    let path = &base[scheme + 3 + host_end..];
    let path = path.split('?').next().unwrap_or(path);
    let dir = match path.rfind('/') {
        Some(i) => &path[..=i],
        None => "/",
    };
    Ok(format!("{origin}{dir}{endpoint}"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::harness::{self as hx, DataClass};
    use std::io::Write;
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::sync::atomic::AtomicUsize;

    /// `*.test` names reach the loopback server on the same port.
    pub(crate) fn resolve_test_host(netloc: &str) -> std::io::Result<Vec<SocketAddr>> {
        use std::net::ToSocketAddrs;
        let (host, port) = netloc.rsplit_once(':').unwrap_or((netloc, "80"));
        if host.ends_with(".test") {
            let port: u16 = port.parse().unwrap_or(80);
            return Ok(vec![SocketAddr::from(([127, 0, 0, 1], port))]);
        }
        netloc.to_socket_addrs().map(|a| a.collect())
    }

    /// A Streamable HTTP MCP server that counts every request and each `tools/call`.
    struct Counting {
        port: u16,
        requests: Arc<AtomicUsize>,
        calls: Arc<AtomicUsize>,
        stop: Arc<AtomicBool>,
    }

    impl Counting {
        fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let port = listener.local_addr().unwrap().port();
            let requests = Arc::new(AtomicUsize::new(0));
            let calls = Arc::new(AtomicUsize::new(0));
            let stop = Arc::new(AtomicBool::new(false));
            let (r, c, s) = (requests.clone(), calls.clone(), stop.clone());
            std::thread::spawn(move || {
                while !s.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((sock, _)) => {
                            r.fetch_add(1, Ordering::SeqCst);
                            answer(sock, &c);
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(5)),
                    }
                }
            });
            Self { port, requests, calls, stop }
        }

        fn url(&self) -> String {
            format!("http://mcp.example.test:{}/mcp", self.port)
        }
    }

    impl Drop for Counting {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
        }
    }

    fn answer(mut sock: TcpStream, calls: &AtomicUsize) {
        let _ = sock.set_nonblocking(false);
        let _ = sock.set_read_timeout(Some(Duration::from_secs(3)));
        let mut reader = BufReader::new(sock.try_clone().unwrap());
        let mut len = 0usize;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                len = v.trim().parse().unwrap_or(0);
            }
        }
        let mut body = vec![0u8; len];
        let _ = reader.read_exact(&mut body);
        let msg: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let result = match msg["method"].as_str().unwrap_or("") {
            "initialize" => json!({"protocolVersion": rpc::PROTOCOL, "capabilities": {}, "serverInfo": {"name": "count", "version": "0"}}),
            "tools/list" => json!({"tools": [{"name": "echo", "description": "Echo", "inputSchema": {"type": "object"}}]}),
            "tools/call" => {
                calls.fetch_add(1, Ordering::SeqCst);
                json!({"content": [{"type": "text", "text": "counted"}]})
            }
            _ => json!({}),
        };
        let out = if id.is_null() {
            String::new()
        } else {
            json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string()
        };
        let status = if out.is_empty() { "202 Accepted" } else { "200 OK" };
        let _ = write!(
            sock,
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nMcp-Session-Id: s-1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
            out.len()
        );
    }

    const MEMORY: &str = "the harbor ferry leaves at nine from pier four";

    fn conn(server: &Counting) -> HttpConn {
        let (conn, tools) = connect("box", &server.url(), false, &BTreeMap::new(), Duration::from_secs(5), Duration::from_secs(5)).unwrap();
        assert_eq!(tools.len(), 1);
        conn
    }

    #[test]
    fn mcp_memory_to_an_ungranted_server_parks_and_sends_nothing_until_approved() {
        let dir = hx::test_dir("mcp-egress");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let _recall = hx::RecallScope::enter(vec![MEMORY.to_string()]);
        let server = Counting::start();
        let mut c = conn(&server);
        let handshake = server.requests.load(Ordering::SeqCst);
        assert_eq!(handshake, 3, "initialize, initialized, tools/list");
        let log = hx::read_egress(&dir);
        assert_eq!(log.len(), 1, "one line for the handshake: {log:?}");
        assert_eq!((log[0].dest.as_str(), log[0].basis.as_str()), ("mcp.example.test", "public"));
        assert!(log[0].data_classes.is_empty());

        let args = json!({"note": format!("remember: {MEMORY}")});
        let deny = hx::answer_next_park(dir.clone(), false);
        let err = c.call("echo", &args, Duration::from_secs(5), &|| false).unwrap_err();
        let card = deny.join().unwrap().expect("a hard card was posted");
        assert_eq!((card.path.as_str(), card.class.as_str()), ("E", "send"));
        assert_eq!(card.action, "mcp:box → mcp.example.test (chats, memory)");
        assert!(err.contains("Denied: nothing was sent"), "{err}");
        assert_eq!(server.calls.load(Ordering::SeqCst), 0, "a denied card sends nothing");
        assert_eq!(server.requests.load(Ordering::SeqCst), handshake);
        assert_eq!(hx::read_egress(&dir).len(), 1, "no line for a denied send");

        let approve = hx::answer_next_park(dir.clone(), true);
        let (text, failed) = c.call("echo", &args, Duration::from_secs(5), &|| false).unwrap();
        assert!(approve.join().unwrap().is_some());
        assert_eq!((text.as_str(), failed), ("counted", false));
        assert_eq!(server.calls.load(Ordering::SeqCst), 1, "approve once sends exactly once");
        let log = hx::read_egress(&dir);
        assert_eq!(log.len(), 2, "{log:?}");
        assert_eq!(log[1].dest, "mcp.example.test");
        assert_eq!(log[1].basis, "approved_once");
        assert_eq!(log[1].data_classes, vec![DataClass::Chat, DataClass::Personal]);
        let spans = hx::read_spans(&dir, "egress").unwrap();
        assert_eq!(spans.iter().filter(|s| s.decision == "park" && s.path == "E").count(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stop_while_an_mcp_send_waits_on_its_card_gives_up_and_sends_nothing() {
        let dir = hx::test_dir("mcp-egress-stop");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let _recall = hx::RecallScope::enter(vec![MEMORY.to_string()]);
        let server = Counting::start();
        let mut c = conn(&server);
        let args = json!({"note": format!("remember: {MEMORY}")});
        // Nobody answers the card; Stop is already pressed.
        let started = std::time::Instant::now();
        let err = c.call("echo", &args, Duration::from_secs(5), &|| true).unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(10), "it did not wait out the card");
        assert!(!err.is_empty());
        assert_eq!(server.calls.load(Ordering::SeqCst), 0, "nothing was sent");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn mcp_chat_args_go_with_one_line_and_no_content() {
        let dir = hx::test_dir("mcp-chat");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let server = Counting::start();
        let mut c = conn(&server);
        let args = json!({"q": "weather sk-abcdefghijklmnopqrstuv"});
        let (text, _) = c.call("echo", &args, Duration::from_secs(5), &|| false).unwrap();
        assert_eq!(text, "counted");
        assert_eq!(server.calls.load(Ordering::SeqCst), 1);
        let log = hx::read_egress(&dir);
        assert_eq!(log.len(), 2, "{log:?}");
        assert_eq!((log[1].dest.as_str(), log[1].basis.as_str()), ("mcp.example.test", "chat"));
        assert_eq!(log[1].data_classes, vec![DataClass::Chat]);
        let raw = std::fs::read_to_string(hx::egress_path(&dir)).unwrap();
        let opened = format!("{log:?}");
        for text in [&raw, &opened] {
            assert!(!text.contains("weather") && !text.contains("sk-abc") && !text.contains("/mcp"), "{text}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
