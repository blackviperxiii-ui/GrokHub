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
    ) -> Result<(String, bool), String> {
        let result = self.roundtrip(
            "tools/call",
            json!({"name": name, "arguments": args}),
            timeout,
        )?;
        Ok(rpc::tool_output(&result))
    }

    pub(crate) fn ping(&mut self, timeout: Duration) -> Result<(), String> {
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
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(3600).max(read))
        .timeout_write(Duration::from_secs(30))
        .redirects(0)
        .build()
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
