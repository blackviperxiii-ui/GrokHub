// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::harness::{guard_egress, DataClass, EgressReq, GateOutcome};
use crate::retry::{decide_retry, DEFAULT_MAX_RETRIES};
use crate::sse::{SseEvent, SseParser};

pub const DEFAULT_MODEL: &str = "grok-4.7";
pub const RESPONSES_URL: &str = "https://api.x.ai/v1/responses";
pub const USER_AGENT: &str = concat!("GrokHub/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    OAuth,
    ApiKey,
}

impl AuthKind {
    pub fn meter(self) -> &'static str {
        match self {
            AuthKind::OAuth => "SuperGrok pool",
            AuthKind::ApiKey => "API credits",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub cost_in_usd_ticks: i64,
    /// Input tokens served from the prompt cache (`input_tokens_details.cached_tokens`).
    pub cached_tokens: u64,
}

impl Usage {
    pub fn add(&mut self, other: &Usage) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.reasoning_tokens = self.reasoning_tokens.saturating_add(other.reasoning_tokens);
        self.cost_in_usd_ticks = self.cost_in_usd_ticks.saturating_add(other.cost_in_usd_ticks);
        self.cached_tokens = self.cached_tokens.saturating_add(other.cached_tokens);
    }

    /// Tokens and cost added since `earlier`. Underflow stays at zero.
    pub fn saturating_delta(&self, earlier: &Usage) -> Usage {
        Usage {
            input_tokens: self.input_tokens.saturating_sub(earlier.input_tokens),
            output_tokens: self.output_tokens.saturating_sub(earlier.output_tokens),
            reasoning_tokens: self
                .reasoning_tokens
                .saturating_sub(earlier.reasoning_tokens),
            cost_in_usd_ticks: self
                .cost_in_usd_ticks
                .saturating_sub(earlier.cost_in_usd_ticks),
            cached_tokens: self.cached_tokens.saturating_sub(earlier.cached_tokens),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionCall {
    pub call_id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentPart {
    InputText(String),
    InputImage(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputItem {
    Message { role: String, content: Vec<ContentPart> },
    FunctionCall { call_id: String, name: String, arguments: String },
    FunctionCallOutput { call_id: String, output: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponsesRequest {
    pub model: String,
    pub effort: Option<String>,
    pub input: Vec<InputItem>,
    pub conversation_id: String,
    /// Function tools for this turn. Hosted web_search and x_search are added in [`responses_body`]
    /// when [`ResponsesRequest::hosted_search`] is set.
    pub tools: Vec<Value>,
    /// Hosted `web_search` and `x_search`. The auto-review judge turns this off.
    pub hosted_search: bool,
    /// Overrides the agent timeout for this call. `None` keeps the client's idle timeout.
    pub call_timeout: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    TextDelta(String),
    ReasoningDelta(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnOutput {
    pub text: String,
    pub reasoning: String,
    pub calls: Vec<FunctionCall>,
    pub usage: Usage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    KeyOffer { status: u16, message: String },
    Auth { status: u16, message: String },
    RateLimited { retry_after_secs: Option<u64>, message: String },
    Server { status: u16, message: String, retry_after_secs: Option<u64> },
    Transport(String),
    IdleTimeout,
    Disconnect,
    Cancelled,
    Protocol(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::KeyOffer { status, message } => {
                write!(f, "sign-in was rejected (HTTP {status}): {message}")
            }
            ClientError::Auth { status, message } => write!(f, "API key rejected (HTTP {status}): {message}"),
            ClientError::RateLimited { message, .. } => write!(f, "rate limited: {message}"),
            ClientError::Server { status, message, .. } => write!(f, "HTTP {status}: {message}"),
            ClientError::Transport(msg) => write!(f, "transport: {msg}"),
            ClientError::IdleTimeout => write!(f, "model stopped responding"),
            ClientError::Disconnect => write!(f, "stream disconnected before it completed"),
            ClientError::Cancelled => write!(f, "cancelled"),
            ClientError::Protocol(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for ClientError {}

impl ClientError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ClientError::RateLimited { .. }
                | ClientError::Server { .. }
                | ClientError::Transport(_)
                | ClientError::Disconnect
        )
    }

    pub fn retry_after_secs(&self) -> Option<u64> {
        match self {
            ClientError::RateLimited { retry_after_secs, .. }
            | ClientError::Server { retry_after_secs, .. } => *retry_after_secs,
            _ => None,
        }
    }
}

pub fn map_http_status(
    status: u16,
    message: &str,
    retry_after_secs: Option<u64>,
    kind: AuthKind,
) -> ClientError {
    let message = if message.trim().is_empty() {
        format!("HTTP {status}")
    } else {
        message.trim().to_string()
    };
    match status {
        401 | 403 if kind == AuthKind::OAuth => ClientError::KeyOffer { status, message },
        401 | 403 => ClientError::Auth { status, message },
        429 => ClientError::RateLimited { retry_after_secs, message },
        500..=599 => ClientError::Server { status, message, retry_after_secs },
        other => ClientError::Protocol(format!("HTTP {other}: {message}")),
    }
}

#[derive(Clone, Debug)]
struct CancelLink {
    parent: CancelToken,
    snapshot: u64,
}

/// Cooperative cancel. `reset` clears only this token.
/// A child created with [`CancelToken::child_of`] stays cancelled after the parent
/// is cancelled, even if the parent is later reset for a new user turn.
#[derive(Clone, Debug)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    parent: Option<Arc<CancelLink>>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
            parent: None,
        }
    }

    /// A token that follows `parent`, including parents of parents.
    /// The snapshot is the parent's generation now. A later parent `cancel`
    /// moves that generation, so a parent `reset` does not revive the child.
    pub fn child_of(parent: &CancelToken) -> Self {
        let snapshot = parent.generation.load(Ordering::SeqCst);
        Self {
            flag: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(0)),
            parent: Some(Arc::new(CancelLink {
                parent: parent.clone(),
                snapshot,
            })),
        }
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        let _ = self.generation.fetch_add(1, Ordering::SeqCst);
    }

    pub fn reset(&self) {
        self.flag.store(false, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        if self.flag.load(Ordering::SeqCst) {
            return true;
        }
        let Some(link) = &self.parent else {
            return false;
        };
        link.snapshot != link.parent.generation.load(Ordering::SeqCst) || link.parent.is_cancelled()
    }
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

pub trait ModelClient {
    fn stream(
        &self,
        req: &ResponsesRequest,
        cancel: &CancelToken,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError>;
}

pub fn request_headers(bearer: &str, conv_id: &str) -> Vec<(String, String)> {
    vec![
        ("Authorization".into(), format!("Bearer {bearer}")),
        ("User-Agent".into(), USER_AGENT.into()),
        ("x-grok-conv-id".into(), conv_id.into()),
        ("Accept".into(), "text/event-stream".into()),
    ]
}

pub fn input_wire_value(items: &[InputItem]) -> Value {
    Value::Array(input_wire(items).0)
}

pub fn input_wire_len(items: &[InputItem]) -> usize {
    serde_json::to_vec(&input_wire_value(items))
        .map(|bytes| bytes.len())
        .unwrap_or(0)
}

fn input_wire(items: &[InputItem]) -> (Vec<Value>, bool) {
    let mut input = Vec::new();
    let mut has_image = false;
    for item in items {
        match item {
            InputItem::Message { role, content } => {
                let mut parts = Vec::new();
                for part in content {
                    match part {
                        ContentPart::InputText(text) => {
                            parts.push(json!({"type": "input_text", "text": text}));
                        }
                        ContentPart::InputImage(url) => {
                            has_image = true;
                            parts.push(json!({"type": "input_image", "image_url": url}));
                        }
                    }
                }
                input.push(json!({"type": "message", "role": role, "content": parts}));
            }
            InputItem::FunctionCall { call_id, name, arguments } => {
                input.push(json!({
                    "type": "function_call",
                    "call_id": call_id,
                    "name": name,
                    "arguments": arguments,
                }));
            }
            InputItem::FunctionCallOutput { call_id, output } => {
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": output,
                }));
            }
        }
    }
    (input, has_image)
}

pub fn responses_body(req: &ResponsesRequest) -> Value {
    let (input, has_image) = input_wire(&req.input);
    let mut tools = req.tools.clone();
    if req.hosted_search {
        tools.push(json!({"type": "web_search"}));
        tools.push(json!({"type": "x_search"}));
    }
    let model = if req.model.trim().is_empty() {
        DEFAULT_MODEL
    } else {
        req.model.trim()
    };
    let mut body = json!({
        "model": model,
        "stream": true,
        "input": input,
        "tools": tools,
    });
    // Normalize here too, so a saved legacy level (e.g. `minimal`) never reaches the wire.
    let effort = req
        .effort
        .as_deref()
        .map(str::trim)
        .map(|e| grokhub_core::parse_reasoning_effort(e).unwrap_or(e));
    if let Some(effort) = effort.filter(|s| !s.is_empty() && *s != "none") {
        body["reasoning"] = json!({"effort": effort});
    }
    if has_image {
        body["store"] = json!(false);
    }
    body
}

pub struct XaiClient {
    agent: ureq::Agent,
    bearer: String,
    auth_kind: AuthKind,
    url: String,
}

/// `http://127.0.0.1:<port>/…` or `http://localhost:<port>/…` only.
fn loopback_http(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return false;
    }
    let host = authority.rsplit_once(':').map(|(h, _)| h).unwrap_or(authority);
    matches!(host, "127.0.0.1" | "localhost")
}

impl XaiClient {
    pub fn new(bearer: impl Into<String>, auth_kind: AuthKind, idle: Duration) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(20))
            .timeout_read(idle)
            .build();
        Self {
            agent,
            bearer: bearer.into(),
            auth_kind,
            url: RESPONSES_URL.to_string(),
        }
    }

    /// Send to a local test server instead of api.x.ai. Only plain-http loopback
    /// origins are accepted, so no caller can move real traffic to another host.
    pub fn with_loopback_url(mut self, url: &str) -> Result<Self, String> {
        if !loopback_http(url) {
            return Err(format!("not a loopback URL: {url}"));
        }
        self.url = url.to_string();
        Ok(self)
    }

    fn once(
        &self,
        req: &ResponsesRequest,
        cancel: &CancelToken,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        if cancel.is_cancelled() {
            return Err(ClientError::Cancelled);
        }
        let _ = crate::sse::take_served_model();
        // EgressGuard (Spike-4a): api.x.ai is a default model host, so this
        // logs one `egress.jsonl` line and goes. Loopback test servers are local.
        let data = [DataClass::Chat, DataClass::Personal];
        let egress = guard_egress(&crate::perm::config_dir(), &EgressReq::new(&self.url, &data));
        if let GateOutcome::Park { reason, .. } | GateOutcome::Refuse { reason } = egress {
            return Err(ClientError::Protocol(reason));
        }
        let body = responses_body(req);
        let mut call = self
            .agent
            .post(&self.url)
            .set("Authorization", &{
                let bearer = &self.bearer;
                format!("Bearer {bearer}")
            })
            .set("User-Agent", USER_AGENT)
            .set("x-grok-conv-id", &req.conversation_id)
            .set("Accept", "text/event-stream");
        if let Some(limit) = req.call_timeout {
            call = call.timeout(limit);
        }
        let response = call.send_json(body);
        let response = match response {
            Ok(resp) => resp,
            Err(ureq::Error::Status(status, resp)) => {
                let retry_after = retry_after_header(&resp);
                let message = read_error_body(resp);
                return Err(map_http_status(status, &message, retry_after, self.auth_kind));
            }
            Err(ureq::Error::Transport(err)) => return Err(classify_transport(&err)),
        };
        read_sse(response.into_reader(), cancel, sink)
    }
}

impl std::fmt::Debug for XaiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XaiClient")
            .field("bearer", &"<redacted>")
            .field("auth_kind", &self.auth_kind)
            .finish()
    }
}

impl ModelClient for XaiClient {
    fn stream(
        &self,
        req: &ResponsesRequest,
        cancel: &CancelToken,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        let mut retry_count = 0u32;
        loop {
            if cancel.is_cancelled() {
                return Err(ClientError::Cancelled);
            }
            match self.once(req, cancel, sink) {
                Ok(out) => return Ok(out),
                Err(err) if err.is_retryable() => match decide_retry(&err, retry_count, DEFAULT_MAX_RETRIES) {
                    Some(wait) => {
                        retry_count = retry_count.saturating_add(1);
                        if !sleep_cancellable(wait, cancel) {
                            return Err(ClientError::Cancelled);
                        }
                    }
                    None => return Err(err),
                },
                Err(err) => return Err(err),
            }
        }
    }
}

fn sleep_cancellable(total: Duration, cancel: &CancelToken) -> bool {
    let slice = Duration::from_millis(50);
    let mut left = total;
    while !left.is_zero() {
        if cancel.is_cancelled() {
            return false;
        }
        let step = left.min(slice);
        std::thread::sleep(step);
        left = left.saturating_sub(step);
    }
    !cancel.is_cancelled()
}

fn retry_after_header(resp: &ureq::Response) -> Option<u64> {
    resp.header("retry-after").and_then(|s| s.trim().parse::<u64>().ok()).filter(|n| *n > 0)
}

fn read_error_body(resp: ureq::Response) -> String {
    let mut buf = String::new();
    let _ = resp.into_reader().take(4096).read_to_string(&mut buf);
    let trimmed = buf.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if let Some(msg) = v
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
        {
            return msg.to_string();
        }
    }
    trimmed.chars().take(500).collect()
}

fn classify_transport(err: &ureq::Transport) -> ClientError {
    let msg = err.to_string();
    let lower = msg.to_ascii_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") {
        if lower.contains("connect") || lower.contains("dns") {
            ClientError::Transport(msg)
        } else {
            ClientError::IdleTimeout
        }
    } else {
        ClientError::Transport(msg)
    }
}

fn read_sse<R: Read>(
    mut reader: R,
    cancel: &CancelToken,
    sink: &mut dyn FnMut(StreamEvent),
) -> Result<TurnOutput, ClientError> {
    let mut parser = SseParser::new();
    let mut buf = [0u8; 4096];
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut calls = Vec::new();
    let mut usage = Usage::default();
    loop {
        if cancel.is_cancelled() {
            drop(reader);
            return Err(ClientError::Cancelled);
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                let msg = e.to_string();
                let lower = msg.to_ascii_lowercase();
                if lower.contains("timed out") || lower.contains("timeout") {
                    return Err(ClientError::IdleTimeout);
                }
                return Err(ClientError::Transport(msg));
            }
        };
        let chunk = String::from_utf8_lossy(&buf[..n]);
        fold_events(parser.push(&chunk), sink, &mut text, &mut reasoning, &mut calls, &mut usage)?;
    }
    fold_events(
        parser.finish()?,
        sink,
        &mut text,
        &mut reasoning,
        &mut calls,
        &mut usage,
    )?;
    Ok(TurnOutput { text, reasoning, calls, usage })
}

fn fold_events(
    events: Vec<SseEvent>,
    sink: &mut dyn FnMut(StreamEvent),
    text: &mut String,
    reasoning: &mut String,
    calls: &mut Vec<FunctionCall>,
    usage: &mut Usage,
) -> Result<(), ClientError> {
    for ev in events {
        match ev {
            SseEvent::TextDelta(delta) => {
                text.push_str(&delta);
                sink(StreamEvent::TextDelta(delta));
            }
            SseEvent::ReasoningDelta(delta) => {
                reasoning.push_str(&delta);
                sink(StreamEvent::ReasoningDelta(delta));
            }
            SseEvent::FunctionCall(call) => calls.push(call),
            SseEvent::Completed(next) => *usage = next,
            SseEvent::Error(message) => return Err(ClientError::Protocol(message)),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xai_client_loopback_url_accepts_only_local_http() {
        let mk = || XaiClient::new("test-access-token", AuthKind::OAuth, Duration::from_secs(5));
        assert_eq!(mk().url, "https://api.x.ai/v1/responses");
        let ok = mk().with_loopback_url("http://127.0.0.1:4711/v1/responses").unwrap();
        assert_eq!(ok.url, "http://127.0.0.1:4711/v1/responses");
        let ok = mk().with_loopback_url("http://localhost:9/v1/responses").unwrap();
        assert_eq!(ok.url, "http://localhost:9/v1/responses");
        for bad in [
            "https://api.x.ai/v1/responses",
            "http://api.x.ai/v1/responses",
            "https://127.0.0.1:4711/v1/responses",
            "http://127.0.0.1.example.com/v1/responses",
            "http://evil@example.com:80/v1/responses",
            "http://127.0.0.1:80@example.com/v1/responses",
            "http://10.0.0.5:4711/v1/responses",
        ] {
            assert_eq!(
                mk().with_loopback_url(bad).unwrap_err(),
                format!("not a loopback URL: {bad}")
            );
        }
        assert!(!format!("{:?}", mk()).contains("test-access-token"));
    }

    #[test]
    fn cancel_token_child_does_not_revive_when_the_parent_resets() {
        let parent = CancelToken::new();
        let child = CancelToken::child_of(&parent);
        let grand = CancelToken::child_of(&child);
        assert!(!parent.is_cancelled());
        assert!(!child.is_cancelled());
        assert!(!grand.is_cancelled());
        parent.cancel();
        assert!(parent.is_cancelled());
        assert!(child.is_cancelled());
        assert!(grand.is_cancelled());
        parent.reset();
        assert!(!parent.is_cancelled());
        assert!(child.is_cancelled());
        assert!(grand.is_cancelled());
        let later = CancelToken::child_of(&parent);
        assert!(!later.is_cancelled());
    }

    #[test]
    fn http_status_maps_oauth_key_rate_and_server() {
        assert!(matches!(
            map_http_status(401, "no", None, AuthKind::OAuth),
            ClientError::KeyOffer { status: 401, .. }
        ));
        assert!(matches!(
            map_http_status(403, "no", None, AuthKind::ApiKey),
            ClientError::Auth { status: 403, .. }
        ));
        assert!(matches!(
            map_http_status(429, "slow", Some(9), AuthKind::ApiKey),
            ClientError::RateLimited { retry_after_secs: Some(9), .. }
        ));
        assert!(matches!(
            map_http_status(503, "down", None, AuthKind::OAuth),
            ClientError::Server { status: 503, .. }
        ));
        assert!(!map_http_status(401, "no", None, AuthKind::OAuth).is_retryable());
        assert!(map_http_status(500, "x", None, AuthKind::ApiKey).is_retryable());
        assert!(!ClientError::IdleTimeout.is_retryable());
        assert!(ClientError::Disconnect.is_retryable());
    }

    /// 2.10.87: native path. A saved `minimal` goes out as `low`; `off` sends none.
    #[test]
    fn responses_body_sends_a_legacy_minimal_effort_as_low() {
        let req = |effort: &str| ResponsesRequest {
            model: String::new(),
            effort: Some(effort.into()),
            input: vec![InputItem::Message {
                role: "user".into(),
                content: vec![ContentPart::InputText("hi".into())],
            }],
            conversation_id: "c".into(),
            tools: crate::tool_schemas(),
            hosted_search: true,
            call_timeout: None,
        };
        assert_eq!(responses_body(&req("minimal"))["reasoning"]["effort"], "low");
        assert_eq!(responses_body(&req("mini"))["reasoning"]["effort"], "low");
        assert_eq!(responses_body(&req("max"))["reasoning"]["effort"], "xhigh");
        assert_eq!(responses_body(&req("medium"))["reasoning"]["effort"], "medium");
        assert!(responses_body(&req("off")).get("reasoning").is_none());
    }

    #[test]
    fn responses_body_tools_effort_and_store_flag() {
        let plain = ResponsesRequest {
            model: String::new(),
            effort: Some("high".into()),
            input: vec![InputItem::Message {
                role: "user".into(),
                content: vec![ContentPart::InputText("hi".into())],
            }],
            conversation_id: "c".into(),
            tools: crate::tool_schemas(),
            hosted_search: true,
            call_timeout: None,
        };
        let body = responses_body(&plain);
        assert_eq!(body["model"], DEFAULT_MODEL);
        assert_eq!(body["stream"], true);
        assert_eq!(body["reasoning"]["effort"], "high");
        assert!(body.get("store").is_none());
        let tools = body["tools"].as_array().unwrap();
        assert!(tools.iter().any(|t| t["type"] == "function" && t["name"] == "read_file"));
        assert!(tools.iter().any(|t| t["type"] == "web_search"));
        assert!(tools.iter().any(|t| t["type"] == "x_search"));
        assert_eq!(body["input"][0]["content"][0]["type"], "input_text");

        let with_image = ResponsesRequest {
            model: "grok-4.7".into(),
            effort: Some("none".into()),
            input: vec![InputItem::Message {
                role: "user".into(),
                content: vec![ContentPart::InputImage("data:image/png;base64,YQ==".into())],
            }],
            conversation_id: "c".into(),
            tools: crate::tool_schemas(),
            hosted_search: true,
            call_timeout: None,
        };
        let body = responses_body(&with_image);
        assert_eq!(body["store"], false);
        assert!(body.get("reasoning").is_none());
        assert_eq!(body["input"][0]["content"][0]["type"], "input_image");

        let headers = request_headers("test-token", "conv-1");
        assert!(headers.iter().any(|(k, v)| k == "Authorization" && v == "Bearer test-token"));
        assert!(headers.iter().any(|(k, v)| k == "User-Agent" && v == USER_AGENT));
        assert!(headers.iter().any(|(k, v)| k == "x-grok-conv-id" && v == "conv-1"));
        assert_eq!(DEFAULT_MODEL, grokhub_core::CABIN_FAST_MODEL);
    }

    #[test]
    fn xai_client_debug_redacts_the_bearer() {
        let client = XaiClient::new("super-secret-value", AuthKind::ApiKey, Duration::from_secs(1));
        let rendered = format!("{client:?}");
        assert!(!rendered.contains("super-secret-value"), "{rendered}");
        assert!(rendered.contains("redacted"), "{rendered}");
    }
}
