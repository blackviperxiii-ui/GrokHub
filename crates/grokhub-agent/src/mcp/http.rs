//! Streamable HTTP and legacy SSE. Synchronous, on the `grokhub_core::http` facade.
//!
//! Requests send the `Authorization` header the caller hands in: the one on
//! the server entry (`headers.Authorization`, `bearerToken` / `bearer_token`,
//! a sealed `tokenRef`), else the server's browser sign-in (`oauth`). This
//! client does not read credential files or invent tokens. A 401 comes back
//! as an `HTTP 401` error ([`is_unauthorized`]) so the caller can refresh or
//! ask for a sign-in.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use grokhub_core::http;

use super::headers::{self, ParamHeader};
use super::rpc::{self, Era, RawTool};
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

/// A POST that got no 2xx: the status and body, or a transport failure.
enum PostErr {
    Status(u16, String),
    Transport(String),
}

impl PostErr {
    fn text(&self) -> String {
        match self {
            Self::Status(code, body) if body.is_empty() => format!("HTTP {code}"),
            Self::Status(code, body) => format!("HTTP {code}: {body}"),
            Self::Transport(err) => err.clone(),
        }
    }
}

pub(crate) struct HttpConn {
    server: String,
    agent: http::Agent,
    /// Message URL. Legacy SSE replaces this with the `endpoint` event.
    post_url: String,
    headers: BTreeMap<String, String>,
    session_id: Option<String>,
    /// The negotiated revision, sent as `MCP-Protocol-Version` on every request
    /// after `initialize`. `None` until then. In the stateless era it is set
    /// from the probe and also rides in every request's `_meta`.
    protocol: Option<&'static str>,
    /// Stateless era: no `Mcp-Session-Id`, `Mcp-Method` / `Mcp-Name` on each POST.
    stateless: bool,
    /// Stateless era: each tool's `x-mcp-header` annotations from `tools/list`.
    param_headers: HashMap<String, Vec<ParamHeader>>,
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
        protocol: None,
        stateless: false,
        param_headers: HashMap::new(),
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
    // The deprecated HTTP+SSE transport is session era only.
    let era = if sse {
        Era::Session
    } else {
        conn.probe(startup)?
    };
    match era {
        Era::Stateless(version) => {
            conn.stateless = true;
            conn.protocol = Some(version);
        }
        Era::Session => {
            let init = conn.roundtrip("initialize", rpc::initialize_params(), startup)?;
            conn.protocol = Some(rpc::negotiate(server, &init)?);
            conn.notify("notifications/initialized", json!({}))?;
        }
    }
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
        if self.stateless {
            let server = self.server.clone();
            return rpc::call_tool(&server, name, args, &mut |params| {
                self.roundtrip("tools/call", params, timeout)
            });
        }
        let result = self.roundtrip(
            "tools/call",
            json!({"name": name, "arguments": args}),
            timeout,
        )?;
        Ok(rpc::tool_output(&result))
    }

    /// `ping` in the session era; the stateless era dropped it, so `server/discover`.
    pub(crate) fn ping(&mut self, timeout: Duration) -> Result<(), String> {
        guard_handshake(&self.post_url)?;
        let method = if self.stateless {
            "server/discover"
        } else {
            "ping"
        };
        self.roundtrip(method, json!({}), timeout).map(|_| ())
    }

    /// POST `server/discover` as a stateless-era request and read the era
    /// from the answer (Streamable HTTP "Backward Compatibility", MCP
    /// 2026-07-28). A 4xx other than 401 / 403 is read by its body: a reserved
    /// modern error keeps the stateless era, anything else falls back to
    /// `initialize`. 401, 403, 5xx and transport errors end the connect, so a
    /// sign-in still starts. A session id the probe got is not kept.
    fn probe(&mut self, timeout: Duration) -> Result<Era, String> {
        self.next_id = self.next_id.saturating_add(1);
        let id = self.next_id;
        let body = rpc::request(
            id,
            "server/discover",
            rpc::with_meta(json!({}), rpc::STATELESS),
        );
        let mut extra = vec![(
            "MCP-Protocol-Version".to_string(),
            rpc::STATELESS.to_string(),
        )];
        extra.extend(headers::standard(&body));
        let posted = post_value(
            &self.agent,
            &self.post_url,
            &self.headers,
            &extra,
            &body,
            timeout,
        );
        let reply = match posted {
            Ok(Posted { body, .. }) => match body {
                Body::Json(msg) => Some(msg),
                Body::Empty => None,
                Body::Stream(reader) => {
                    let (tx, rx) = mpsc::channel();
                    let _reader = thread_reader(reader, tx, self.stop.clone());
                    Some(self.wait_rx(id, &rx, timeout)?)
                }
            },
            Err(PostErr::Status(code, body))
                if (400..500).contains(&code) && code != 401 && code != 403 =>
            {
                serde_json::from_str::<Value>(body.trim()).ok()
            }
            Err(err) => return Err(stopped(&self.server, &err.text())),
        };
        let reply = reply
            .filter(|msg| rpc::id_matches(msg, id) || msg.get("id").is_none_or(Value::is_null));
        rpc::pick_era(&self.server, reply.as_ref())
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
        let params = match (self.stateless, self.protocol) {
            (true, Some(version)) => rpc::with_meta(params, version),
            _ => params,
        };
        let posted = self.exchange(&rpc::request(id, method, params), timeout)?;
        let msg = match posted {
            Body::Json(msg) if rpc::id_matches(&msg, id) => msg,
            Body::Json(msg) => {
                if let Some(reply) = super::elicit::answer_elicitation(&self.server, &msg) {
                    self.exchange(&reply, timeout)?;
                }
                return Err(format!("MCP server `{}` stopped", self.server));
            }
            Body::Empty => {
                if self.legacy {
                    self.wait_legacy(id, timeout)?
                } else {
                    return Err(format!("MCP server `{}` stopped", self.server));
                }
            }
            Body::Stream(reader) => {
                let (tx, rx) = mpsc::channel();
                let stop = self.stop.clone();
                let _reader = thread_reader(reader, tx, stop);
                self.wait_rx(id, &rx, timeout)?
            }
        };
        take_result(&msg)
    }

    /// The reply message for `id` from the legacy SSE stream.
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
                        return Ok(msg);
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

    /// The reply message for `id` from one POST's SSE stream.
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
                        return Ok(msg);
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
        let extra = self.protocol_headers(body);
        let posted = post_value(
            &self.agent,
            &self.post_url,
            &self.headers,
            &extra,
            body,
            timeout,
        )
        .map_err(|err| stopped(&self.server, &err.text()))?;
        if let Some(sid) = posted.session {
            if self.session_id.is_none() && !self.stateless {
                self.session_id = Some(sid);
            }
        }
        Ok(posted.body)
    }

    /// Session era: `Mcp-Session-Id` (once the server gave one) and the
    /// negotiated `MCP-Protocol-Version`. Stateless era: the version, then
    /// `Mcp-Method`, `Mcp-Name` and a tool's `Mcp-Param-*` headers; never a
    /// session id.
    fn protocol_headers(&self, body: &Value) -> Vec<(String, String)> {
        let mut out = Vec::new();
        if let (Some(sid), false) = (&self.session_id, self.stateless) {
            out.push(("Mcp-Session-Id".to_string(), sid.clone()));
        }
        if let Some(version) = self.protocol {
            out.push(("MCP-Protocol-Version".to_string(), version.to_string()));
        }
        if self.stateless {
            out.extend(headers::standard(body));
            if body.get("method").and_then(Value::as_str) == Some("tools/call") {
                let tool = body.pointer("/params/name").and_then(Value::as_str);
                if let Some(wanted) = tool.and_then(|name| self.param_headers.get(name)) {
                    let args = body.pointer("/params/arguments").unwrap_or(&Value::Null);
                    out.extend(headers::param_values(wanted, args));
                }
            }
        }
        out
    }

    fn open_legacy(&mut self, timeout: Duration) -> Result<(), String> {
        let base = self.post_url.clone();
        let resp = open_get(&self.agent, &base, &self.headers)
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

pub(super) fn guard_handshake(url: &str) -> Result<(), String> {
    crate::harness::guard_quiet(
        &crate::perm::config_dir(),
        &crate::harness::EgressReq::new(url, &[]),
    )
}

fn list_tools(conn: &mut HttpConn, timeout: Duration) -> Result<Vec<RawTool>, String> {
    let mut all = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..100 {
        let mut params = json!({});
        if let Some(token) = &cursor {
            params["cursor"] = json!(token);
        }
        let mut result = conn.roundtrip("tools/list", params, timeout)?;
        if conn.stateless {
            result = rpc::complete(&conn.server, result)?;
        }
        let (page, next) = rpc::parse_tools(&result);
        for tool in page {
            if conn.stateless {
                // A tool with a bad `x-mcp-header` is left out; the rest still list.
                match headers::param_headers(&tool.input_schema) {
                    Ok(found) if found.is_empty() => {}
                    Ok(found) => {
                        conn.param_headers.insert(tool.name.clone(), found);
                    }
                    Err(_) => continue,
                }
            }
            all.push(tool);
        }
        match next {
            Some(token) => cursor = Some(token),
            None => return Ok(all),
        }
    }
    Ok(all)
}

/// Tests reach a loopback server under a `.test` name, so the guard sees a
/// real (non-loopback) destination. Production resolves as usual.
#[cfg(test)]
const RESOLVE: Option<http::Resolve> = Some(tests::resolve_test_host);
#[cfg(not(test))]
const RESOLVE: Option<http::Resolve> = None;

pub(super) fn agent(read: Duration) -> http::Agent {
    let read = if read.is_zero() {
        Duration::from_secs(60)
    } else {
        read
    };
    http::agent(http::Opts {
        connect: Duration::from_secs(10),
        read: Some(Duration::from_secs(3600).max(read)),
        write: Some(Duration::from_secs(30)),
        redirects: 0,
        resolve: RESOLVE,
        ..Default::default()
    })
}

/// The legacy SSE stream opens before `initialize`: no session or version yet.
fn open_get(
    agent: &http::Agent,
    url: &str,
    headers: &BTreeMap<String, String>,
) -> Result<http::Response, String> {
    let mut req = agent.get(url).any_status();
    req = apply_headers(req, headers, &[]);
    req = req.header("Accept", "text/event-stream");
    match req.call() {
        Ok(resp) if resp.status() >= 400 => Err(http_err(resp)),
        Ok(resp) => Ok(resp),
        Err(err) => Err(err.to_string()),
    }
}

fn post_value(
    agent: &http::Agent,
    url: &str,
    headers: &BTreeMap<String, String>,
    extra: &[(String, String)],
    body: &Value,
    timeout: Duration,
) -> Result<Posted, PostErr> {
    let text = serde_json::to_string(body).map_err(|err| PostErr::Transport(err.to_string()))?;
    let mut req = agent.post(url).any_status();
    req = apply_headers(req, headers, extra);
    req = req
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .timeout(timeout.max(Duration::from_secs(1)));
    let resp = match req.send_string(&text) {
        Ok(resp) if resp.status() >= 400 => {
            let code = resp.status();
            let body = resp.into_string().unwrap_or_default();
            return Err(PostErr::Status(code, body.trim().to_string()));
        }
        Ok(resp) => resp,
        Err(err) => return Err(PostErr::Transport(err.to_string())),
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
    let msg = serde_json::from_str(trimmed).map_err(|err| PostErr::Transport(err.to_string()))?;
    Ok(Posted {
        session,
        body: Body::Json(msg),
    })
}

/// User headers from the server entry (with the sign-in's bearer, if any),
/// then the protocol's own ([`HttpConn::protocol_headers`]), which win over a
/// same-named user header. Before `initialize` answers there is no version
/// yet, so none is sent.
fn apply_headers(
    mut req: http::Request,
    headers: &BTreeMap<String, String>,
    extra: &[(String, String)],
) -> http::Request {
    req = req.header("User-Agent", USER_AGENT);
    for (key, value) in headers {
        req = req.header(key, value);
    }
    for (key, value) in extra {
        req = req.header(key, value);
    }
    req
}

/// The server refused the credentials (or had none): the error [`http_err`] gives a 401.
pub(super) fn is_unauthorized(err: &str) -> bool {
    err.contains("HTTP 401")
}

/// A 4xx/5xx answer (sent with `any_status`) as `HTTP {code}[: body]`.
fn http_err(resp: http::Response) -> String {
    let code = resp.status();
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

/// Scheme + host[:port] for an http(s) URL. Userinfo is dropped; host is lowercased.
fn http_origin(url: &str) -> Option<String> {
    let url = url.trim();
    let (scheme, rest) = url
        .strip_prefix("https://")
        .map(|rest| ("https", rest))
        .or_else(|| url.strip_prefix("http://").map(|rest| ("http", rest)))?;
    let authority = rest.split(['/', '?', '#']).next()?.trim();
    if authority.is_empty() {
        return None;
    }
    let hostport = authority.rsplit('@').next()?.trim();
    if hostport.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{}", hostport.to_ascii_lowercase()))
}

pub(crate) fn resolve_endpoint(base: &str, endpoint: &str) -> Result<String, String> {
    let endpoint = endpoint.trim();
    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        // Absolute SSE `endpoint` events must stay on the connect host. A remote
        // MCP that pivots to link-local / LAN / loopback would otherwise POST
        // JSON-RPC (and later chat tool args) there after an empty-data handshake.
        if http_origin(base) != http_origin(endpoint) {
            return Err("MCP endpoint must stay on the same host".into());
        }
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

    /// One request the mock saw: its JSON-RPC body and the MCP headers.
    #[derive(Debug, Clone, Default)]
    struct Req {
        method: String,
        protocol: Option<String>,
        session: Option<String>,
        mcp_method: Option<String>,
        mcp_name: Option<String>,
        /// `Mcp-Param-*` headers, names lowercased.
        params: Vec<(String, String)>,
        body: Value,
    }

    type Seen = Arc<std::sync::Mutex<Vec<Req>>>;

    /// How the mock answers.
    #[derive(Clone)]
    enum Mode {
        /// Session era: `initialize` answers this version; a request before it
        /// gets the plain-text 400 older SDKs send.
        Session(&'static str),
        /// MCP 2026-07-28 only: `server/discover`, `_meta` on every request.
        Stateless,
        /// Every request gets this status and body.
        Refuse(u16, &'static str),
        /// Stateless, but `tools/list` adds tools with these schemas.
        StatelessTools(Value),
    }

    /// A Streamable HTTP MCP server that counts every request and each
    /// `tools/call`, and records what it saw.
    struct Counting {
        port: u16,
        requests: Arc<AtomicUsize>,
        calls: Arc<AtomicUsize>,
        seen: Seen,
        stop: Arc<AtomicBool>,
    }

    impl Counting {
        fn start() -> Self {
            Self::answering(rpc::LATEST)
        }

        fn answering(version: &'static str) -> Self {
            Self::with(Mode::Session(version))
        }

        fn with(mode: Mode) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let port = listener.local_addr().unwrap().port();
            let requests = Arc::new(AtomicUsize::new(0));
            let calls = Arc::new(AtomicUsize::new(0));
            let seen = Seen::default();
            let stop = Arc::new(AtomicBool::new(false));
            let (r, c, w, s) = (requests.clone(), calls.clone(), seen.clone(), stop.clone());
            std::thread::spawn(move || {
                while !s.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((sock, _)) => {
                            r.fetch_add(1, Ordering::SeqCst);
                            answer(sock, &c, &mode, &w);
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(5)),
                    }
                }
            });
            Self {
                port,
                requests,
                calls,
                seen,
                stop,
            }
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

    fn read_req(sock: &TcpStream) -> Option<Req> {
        let mut reader = BufReader::new(sock.try_clone().ok()?);
        let mut len = 0usize;
        let mut req = Req::default();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return None;
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            let (name, value) = (name.trim().to_ascii_lowercase(), value.trim().to_string());
            match name.as_str() {
                "content-length" => len = value.parse().unwrap_or(0),
                "mcp-protocol-version" => req.protocol = Some(value),
                "mcp-session-id" => req.session = Some(value),
                "mcp-method" => req.mcp_method = Some(value),
                "mcp-name" => req.mcp_name = Some(value),
                _ if name.starts_with("mcp-param-") => req.params.push((name, value)),
                _ => {}
            }
        }
        let mut body = vec![0u8; len];
        let _ = reader.read_exact(&mut body);
        req.body = serde_json::from_slice(&body).unwrap_or(Value::Null);
        req.method = req.body["method"].as_str().unwrap_or("").to_string();
        Some(req)
    }

    fn reply(sock: &mut TcpStream, status: &str, ctype: &str, out: &str) {
        let _ = write!(
            sock,
            "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nMcp-Session-Id: s-1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
            out.len()
        );
    }

    fn answer(mut sock: TcpStream, calls: &AtomicUsize, mode: &Mode, seen: &Seen) {
        let _ = sock.set_nonblocking(false);
        let _ = sock.set_read_timeout(Some(Duration::from_secs(3)));
        let Some(req) = read_req(&sock) else {
            return;
        };
        seen.lock().unwrap().push(req.clone());
        let id = req.body.get("id").cloned().unwrap_or(Value::Null);
        let method = req.method.as_str();
        let extra = match mode {
            Mode::Refuse(code, body) => {
                return reply(&mut sock, &format!("{code} Refused"), "text/plain", body);
            }
            Mode::Session(_) if method == "server/discover" => {
                return reply(
                    &mut sock,
                    "400 Bad Request",
                    "text/plain",
                    "Bad Request: No valid session ID provided",
                );
            }
            Mode::Session(_) => Value::Null,
            Mode::Stateless => json!([]),
            Mode::StatelessTools(tools) => tools.clone(),
        };
        let stateless = !extra.is_null();
        let result = match method {
            "initialize" if stateless => {
                let err = json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "Method not found"}});
                return reply(
                    &mut sock,
                    "404 Not Found",
                    "application/json",
                    &err.to_string(),
                );
            }
            "initialize" => {
                let Mode::Session(version) = mode else {
                    return;
                };
                json!({"protocolVersion": version, "capabilities": {}, "serverInfo": {"name": "count", "version": "0"}})
            }
            "server/discover" => {
                json!({"resultType": "complete", "supportedVersions": ["2026-07-28"], "capabilities": {"tools": {}}})
            }
            "tools/list" => {
                let mut tools = vec![
                    json!({"name": "echo", "description": "Echo", "inputSchema": {"type": "object"}}),
                ];
                tools.extend(extra.as_array().cloned().unwrap_or_default());
                json!({"tools": tools})
            }
            "tools/call" => {
                calls.fetch_add(1, Ordering::SeqCst);
                let asked = req.body.pointer("/params/arguments/ask").is_some();
                let answered = req.body.pointer("/params/inputResponses").is_some();
                if asked && !answered {
                    json!({
                        "resultType": "input_required",
                        "inputRequests": {"note": {"method": "elicitation/create", "params": {"mode": "form", "message": "Need a value"}}},
                        "requestState": "rs-1"
                    })
                } else if req.body.pointer("/params/arguments/odd").is_some() {
                    json!({"resultType": "partial", "content": []})
                } else {
                    json!({"content": [{"type": "text", "text": "counted"}]})
                }
            }
            _ => json!({}),
        };
        let out = if id.is_null() {
            String::new()
        } else {
            json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string()
        };
        let status = if out.is_empty() {
            "202 Accepted"
        } else {
            "200 OK"
        };
        reply(&mut sock, status, "application/json", &out);
    }

    const MEMORY: &str = "the harbor ferry leaves at nine from pier four";

    fn conn(server: &Counting) -> HttpConn {
        let (conn, tools) = connect(
            "box",
            &server.url(),
            false,
            &BTreeMap::new(),
            Duration::from_secs(5),
            Duration::from_secs(5),
        )
        .unwrap();
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
        assert_eq!(
            handshake, 4,
            "server/discover, initialize, initialized, tools/list"
        );
        let log = hx::read_egress(&dir);
        assert_eq!(log.len(), 1, "one line for the handshake: {log:?}");
        assert_eq!(
            (log[0].dest.as_str(), log[0].basis.as_str()),
            ("mcp.example.test", "public")
        );
        assert!(log[0].data_classes.is_empty());

        let args = json!({"note": format!("remember: {MEMORY}")});
        let deny = hx::answer_next_park(dir.clone(), false);
        let err = c
            .call("echo", &args, Duration::from_secs(5), &|| false)
            .unwrap_err();
        let card = deny.join().unwrap().expect("a hard card was posted");
        assert_eq!((card.path.as_str(), card.class.as_str()), ("E", "send"));
        assert_eq!(card.action, "mcp:box → mcp.example.test (chats, memory)");
        assert!(err.contains("Denied: nothing was sent"), "{err}");
        assert_eq!(
            server.calls.load(Ordering::SeqCst),
            0,
            "a denied card sends nothing"
        );
        assert_eq!(server.requests.load(Ordering::SeqCst), handshake);
        assert_eq!(hx::read_egress(&dir).len(), 1, "no line for a denied send");

        let approve = hx::answer_next_park(dir.clone(), true);
        let (text, failed) = c
            .call("echo", &args, Duration::from_secs(5), &|| false)
            .unwrap();
        assert!(approve.join().unwrap().is_some());
        assert_eq!((text.as_str(), failed), ("counted", false));
        assert_eq!(
            server.calls.load(Ordering::SeqCst),
            1,
            "approve once sends exactly once"
        );
        let log = hx::read_egress(&dir);
        assert_eq!(log.len(), 2, "{log:?}");
        assert_eq!(log[1].dest, "mcp.example.test");
        assert_eq!(log[1].basis, "approved_once");
        assert_eq!(
            log[1].data_classes,
            vec![DataClass::Chat, DataClass::Personal]
        );
        let spans = hx::read_spans(&dir, "egress").unwrap();
        assert_eq!(
            spans
                .iter()
                .filter(|s| s.decision == "park" && s.path == "E")
                .count(),
            2
        );
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
        let err = c
            .call("echo", &args, Duration::from_secs(5), &|| true)
            .unwrap_err();
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "it did not wait out the card"
        );
        assert!(!err.is_empty());
        assert_eq!(server.calls.load(Ordering::SeqCst), 0, "nothing was sent");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn absolute_sse_endpoint_must_stay_on_the_connect_host() {
        assert_eq!(
            resolve_endpoint("http://127.0.0.1:9/sse", "http://127.0.0.1:9/message").unwrap(),
            "http://127.0.0.1:9/message"
        );
        assert_eq!(
            resolve_endpoint("https://mcp.example.test/sse", "/rpc").unwrap(),
            "https://mcp.example.test/rpc"
        );
        assert_eq!(
            resolve_endpoint(
                "https://mcp.example.test/v1/sse",
                "https://mcp.example.test/v1/message"
            )
            .unwrap(),
            "https://mcp.example.test/v1/message"
        );
        assert_eq!(
            resolve_endpoint(
                "https://mcp.example.test/sse",
                "http://169.254.169.254/latest/meta-data/"
            )
            .unwrap_err(),
            "MCP endpoint must stay on the same host"
        );
        assert_eq!(
            resolve_endpoint("https://mcp.example.test/sse", "https://10.0.0.1/internal")
                .unwrap_err(),
            "MCP endpoint must stay on the same host"
        );
        assert_eq!(
            resolve_endpoint("https://mcp.example.test/sse", "http://127.0.0.1:9/x").unwrap_err(),
            "MCP endpoint must stay on the same host"
        );
    }

    #[test]
    fn mcp_chat_args_go_with_one_line_and_no_content() {
        let dir = hx::test_dir("mcp-chat");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let server = Counting::start();
        let mut c = conn(&server);
        let args = json!({"q": "weather sk-abcdefghijklmnopqrstuv"});
        let (text, _) = c
            .call("echo", &args, Duration::from_secs(5), &|| false)
            .unwrap();
        assert_eq!(text, "counted");
        assert_eq!(server.calls.load(Ordering::SeqCst), 1);
        let log = hx::read_egress(&dir);
        assert_eq!(log.len(), 2, "{log:?}");
        assert_eq!(
            (log[1].dest.as_str(), log[1].basis.as_str()),
            ("mcp.example.test", "chat")
        );
        assert_eq!(log[1].data_classes, vec![DataClass::Chat]);
        let raw = std::fs::read_to_string(hx::egress_path(&dir)).unwrap();
        let opened = format!("{log:?}");
        for text in [&raw, &opened] {
            assert!(
                !text.contains("weather") && !text.contains("sk-abc") && !text.contains("/mcp"),
                "{text}"
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    fn connect_to(server: &Counting) -> Result<(HttpConn, Vec<RawTool>), String> {
        connect(
            "box",
            &server.url(),
            false,
            &BTreeMap::new(),
            Duration::from_secs(5),
            Duration::from_secs(5),
        )
    }

    /// Each request's JSON-RPC method and its `MCP-Protocol-Version` header.
    fn seen(server: &Counting) -> Vec<(String, Option<String>)> {
        server
            .seen
            .lock()
            .unwrap()
            .iter()
            .map(|r| (r.method.clone(), r.protocol.clone()))
            .collect()
    }

    fn reqs(server: &Counting) -> Vec<Req> {
        server.seen.lock().unwrap().clone()
    }

    fn modern() -> Option<String> {
        Some("2026-07-28".to_string())
    }

    #[test]
    fn every_request_after_initialize_names_the_negotiated_version() {
        let dir = hx::test_dir("mcp-version-new");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let server = Counting::answering("2025-11-25");
        let (mut c, _) = connect_to(&server).unwrap();
        assert_eq!(c.protocol, Some("2025-11-25"));
        c.call("echo", &json!({}), Duration::from_secs(5), &|| false)
            .unwrap();
        c.ping(Duration::from_secs(5)).unwrap();
        let v = Some("2025-11-25".to_string());
        assert_eq!(
            seen(&server),
            vec![
                ("server/discover".to_string(), modern()),
                ("initialize".to_string(), None),
                ("notifications/initialized".to_string(), v.clone()),
                ("tools/list".to_string(), v.clone()),
                ("tools/call".to_string(), v.clone()),
                ("ping".to_string(), v),
            ]
        );
        // The probe's answer carried a session id too; only initialize's is kept.
        let sessions: Vec<Option<String>> = reqs(&server).into_iter().map(|r| r.session).collect();
        let s1 = Some("s-1".to_string());
        assert_eq!(
            sessions,
            vec![None, None, s1.clone(), s1.clone(), s1.clone(), s1]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_older_server_gets_its_own_version_in_the_header() {
        let dir = hx::test_dir("mcp-version-old");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let server = Counting::answering("2025-03-26");
        let (mut c, tools) = connect_to(&server).unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(c.protocol, Some("2025-03-26"));
        c.call("echo", &json!({}), Duration::from_secs(5), &|| false)
            .unwrap();
        let headers: Vec<Option<String>> = seen(&server).into_iter().map(|(_, v)| v).collect();
        let old = Some("2025-03-26".to_string());
        assert_eq!(headers, vec![modern(), None, old.clone(), old.clone(), old]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_unknown_version_ends_the_connection_before_tools_list() {
        let dir = hx::test_dir("mcp-version-unknown");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let server = Counting::answering("1999-01-01");
        let err = connect_to(&server).err().unwrap();
        assert_eq!(
            err,
            "MCP server `box` answered with MCP version 1999-01-01; this client speaks 2025-11-25, 2025-06-18, 2025-03-26, 2024-11-05"
        );
        assert_eq!(
            seen(&server),
            vec![
                ("server/discover".to_string(), modern()),
                ("initialize".to_string(), None)
            ]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    fn meta_version(req: &Req) -> Option<&str> {
        req.body
            .pointer("/params/_meta/io.modelcontextprotocol~1protocolVersion")
            .and_then(Value::as_str)
    }

    #[test]
    fn a_stateless_server_gets_no_initialize_and_no_session_id() {
        let dir = hx::test_dir("mcp-stateless");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let server = Counting::with(Mode::Stateless);
        let (mut c, tools) = connect_to(&server).unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!((c.protocol, c.stateless), (Some("2026-07-28"), true));
        let (text, failed) = c
            .call("echo", &json!({"q": 1}), Duration::from_secs(5), &|| false)
            .unwrap();
        assert_eq!((text.as_str(), failed), ("counted", false));
        c.ping(Duration::from_secs(5)).unwrap();
        let seen = reqs(&server);
        // (method, MCP-Protocol-Version, Mcp-Method, Mcp-Name, Mcp-Session-Id, _meta version)
        type Row<'a> = (
            &'a str,
            Option<&'a str>,
            Option<&'a str>,
            Option<&'a str>,
            Option<&'a str>,
            Option<&'a str>,
        );
        let rows: Vec<Row> = seen
            .iter()
            .map(|r| {
                (
                    r.method.as_str(),
                    r.protocol.as_deref(),
                    r.mcp_method.as_deref(),
                    r.mcp_name.as_deref(),
                    r.session.as_deref(),
                    meta_version(r),
                )
            })
            .collect();
        let v = Some("2026-07-28");
        assert_eq!(
            rows,
            vec![
                ("server/discover", v, Some("server/discover"), None, None, v),
                ("tools/list", v, Some("tools/list"), None, None, v),
                ("tools/call", v, Some("tools/call"), Some("echo"), None, v),
                ("server/discover", v, Some("server/discover"), None, None, v),
            ]
        );
        let meta = &seen[2].body["params"]["_meta"];
        assert_eq!(
            meta["io.modelcontextprotocol/clientCapabilities"],
            json!({"elicitation": {"form": {}, "url": {}}})
        );
        assert_eq!(
            meta["io.modelcontextprotocol/clientInfo"]["name"],
            "GrokHub"
        );
        assert_eq!(seen[2].body["params"]["arguments"], json!({"q": 1}));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn x_mcp_header_values_ride_as_param_headers_and_a_bad_tool_is_left_out() {
        let dir = hx::test_dir("mcp-param-headers");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let server = Counting::with(Mode::StatelessTools(json!([
            {"name": "execute_sql", "inputSchema": {"type": "object", "properties": {
                "region": {"type": "string", "x-mcp-header": "Region"},
                "query": {"type": "string"}
            }}},
            {"name": "bad", "inputSchema": {"type": "object", "properties": {
                "tags": {"type": "array", "items": {"type": "string", "x-mcp-header": "Tag"}}
            }}}
        ])));
        let (mut c, tools) = connect_to(&server).unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["echo", "execute_sql"]);
        c.call(
            "execute_sql",
            &json!({"region": "us-west1", "query": "SELECT * FROM users"}),
            Duration::from_secs(5),
            &|| false,
        )
        .unwrap();
        c.call(
            "execute_sql",
            &json!({"region": "Zürich"}),
            Duration::from_secs(5),
            &|| false,
        )
        .unwrap();
        c.call(
            "echo",
            &json!({"region": "x"}),
            Duration::from_secs(5),
            &|| false,
        )
        .unwrap();
        let params: Vec<Vec<(String, String)>> = reqs(&server)
            .into_iter()
            .filter(|r| r.method == "tools/call")
            .map(|r| r.params)
            .collect();
        assert_eq!(
            params,
            vec![
                vec![("mcp-param-region".to_string(), "us-west1".to_string())],
                vec![(
                    "mcp-param-region".to_string(),
                    "=?base64?WsO8cmljaA==?=".to_string()
                )],
                vec![],
            ]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_input_required_result_is_answered_and_sent_again_with_the_request_state() {
        let dir = hx::test_dir("mcp-mrtr");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let server = Counting::with(Mode::Stateless);
        let (mut c, _) = connect_to(&server).unwrap();
        // No elicit card on this thread: the question is declined, as unattended.
        let (text, _) = c
            .call(
                "echo",
                &json!({"ask": true}),
                Duration::from_secs(5),
                &|| false,
            )
            .unwrap();
        assert_eq!(text, "counted");
        let calls: Vec<Value> = reqs(&server)
            .into_iter()
            .filter(|r| r.method == "tools/call")
            .map(|r| r.body)
            .collect();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["params"].get("inputResponses"), None);
        assert_eq!(
            calls[1]["params"]["inputResponses"],
            json!({"note": {"action": "decline"}})
        );
        assert_eq!(calls[1]["params"]["requestState"], "rs-1");
        assert_eq!(calls[1]["params"]["arguments"], json!({"ask": true}));
        assert_ne!(calls[0]["id"], calls[1]["id"], "a retry is a new request");
        let err = c
            .call("echo", &json!({"odd": 1}), Duration::from_secs(5), &|| {
                false
            })
            .unwrap_err();
        assert_eq!(err, "MCP server `box` sent a result of type partial");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_server_that_answers_neither_fails_after_one_initialize() {
        let dir = hx::test_dir("mcp-neither");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let server = Counting::with(Mode::Refuse(404, "not here"));
        let err = connect_to(&server).err().unwrap();
        assert_eq!(err, "MCP server `box` stopped: HTTP 404: not here");
        let methods: Vec<String> = seen(&server).into_iter().map(|(m, _)| m).collect();
        assert_eq!(methods, vec!["server/discover", "initialize"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_401_on_the_probe_asks_for_sign_in_instead_of_falling_back() {
        let dir = hx::test_dir("mcp-probe-401");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let server = Counting::with(Mode::Refuse(401, ""));
        let err = connect_to(&server).err().unwrap();
        assert_eq!(err, "MCP server `box` stopped: HTTP 401");
        assert!(is_unauthorized(&err));
        let methods: Vec<String> = seen(&server).into_iter().map(|(m, _)| m).collect();
        assert_eq!(methods, vec!["server/discover"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_modern_error_on_the_probe_never_falls_back_to_initialize() {
        let dir = hx::test_dir("mcp-probe-modern");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let unsupported = Counting::with(Mode::Refuse(
            400,
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32022,"message":"Unsupported protocol version","data":{"supported":["2027-03-01"],"requested":"2026-07-28"}}}"#,
        ));
        assert_eq!(
            connect_to(&unsupported).err().unwrap(),
            "MCP server `box` speaks MCP versions 2027-03-01; this client speaks 2026-07-28, 2025-11-25, 2025-06-18, 2025-03-26, 2024-11-05"
        );
        let mismatch = Counting::with(Mode::Refuse(
            400,
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32020,"message":"Header mismatch"}}"#,
        ));
        assert_eq!(
            connect_to(&mismatch).err().unwrap(),
            "MCP server `box` refused the request: Header mismatch (-32020)"
        );
        for server in [&unsupported, &mismatch] {
            let methods: Vec<String> = seen(server).into_iter().map(|(m, _)| m).collect();
            assert_eq!(methods, vec!["server/discover"]);
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
