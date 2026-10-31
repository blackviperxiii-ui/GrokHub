// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use crate::harness::{guard_egress, DataClass, EgressReq, GateOutcome, XAI_MODEL_ENDPOINTS};
use crate::retry::{decide_retry, DEFAULT_MAX_RETRIES};
use crate::sse::{SseEvent, SseParser};

pub const DEFAULT_MODEL: &str = "grok-4.7";
pub const RESPONSES_URL: &str = "https://api.x.ai/v1/responses";
/// xAI server-side context compaction. One pass per call; the conversation must
/// already fit the window.
pub const RESPONSES_COMPACT_URL: &str = "https://api.x.ai/v1/responses/compact";
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
        self.cost_in_usd_ticks = self
            .cost_in_usd_ticks
            .saturating_add(other.cost_in_usd_ticks);
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
    Message {
        role: String,
        content: Vec<ContentPart>,
    },
    FunctionCall {
        call_id: String,
        name: String,
        arguments: String,
    },
    FunctionCallOutput {
        call_id: String,
        output: String,
    },
    /// One `output` item of an xAI `/v1/responses/compact` reply, kept as the exact
    /// JSON text it arrived as. Opaque: it is never parsed for meaning, edited,
    /// merged or reordered, and goes back to xAI as received. Build it with
    /// [`InputItem::server_compaction`].
    ServerCompaction {
        raw: String,
    },
}

impl InputItem {
    /// A [`InputItem::ServerCompaction`] from one item's JSON text, or `None`
    /// when it is not a JSON object of type `compaction`.
    pub fn server_compaction(raw: &str) -> Option<InputItem> {
        let value: Value = serde_json::from_str(raw).ok()?;
        (value.get("type").and_then(Value::as_str) == Some("compaction")).then(|| {
            InputItem::ServerCompaction {
                raw: raw.to_string(),
            }
        })
    }
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

/// One call to xAI's server-side compaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactRequest {
    pub model: String,
    /// The part of the conversation to fold. It may start with earlier compaction items.
    pub input: Vec<InputItem>,
    pub conversation_id: String,
    /// Hard cap on the whole call, retries and backoff included. When it runs
    /// out the call fails with [`ClientError::IdleTimeout`].
    pub timeout: Duration,
}

/// What `/v1/responses/compact` sent back.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CompactOutput {
    /// The reply's `output` items, in order, each an [`InputItem::ServerCompaction`].
    pub items: Vec<InputItem>,
    pub usage: Usage,
    /// Input messages the server folded into the compaction item.
    pub dropped_message_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    TextDelta(String),
    ReasoningDelta(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TurnOutput {
    pub text: String,
    pub reasoning: String,
    pub calls: Vec<FunctionCall>,
    pub usage: Usage,
    /// True when the stream ended as `response.incomplete` (or status
    /// incomplete). Tool calls in this turn must not run.
    pub truncated: bool,
    /// Provider reason for truncation, when present (e.g. `max_output_tokens`).
    pub truncate_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientError {
    KeyOffer {
        status: u16,
        message: String,
    },
    Auth {
        status: u16,
        message: String,
    },
    RateLimited {
        retry_after_secs: Option<u64>,
        message: String,
    },
    Server {
        status: u16,
        message: String,
        retry_after_secs: Option<u64>,
    },
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
            ClientError::Auth { status, message } => {
                write!(f, "API key rejected (HTTP {status}): {message}")
            }
            ClientError::RateLimited { message, .. } => write!(f, "rate limited: {message}"),
            ClientError::Server {
                status, message, ..
            } => write!(f, "HTTP {status}: {message}"),
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
        ) && !self.is_context_length()
            && !matches!(self, ClientError::Transport(m) if grokhub_core::offline::is_refusal(m))
    }

    /// True when the provider refused the request because the prompt is too long.
    /// Not retryable as a transient error; the run loop compacts and retries once.
    pub fn is_context_length(&self) -> bool {
        let msg = match self {
            ClientError::Protocol(message)
            | ClientError::Server { message, .. }
            | ClientError::Transport(message) => message.as_str(),
            _ => return false,
        };
        context_length_message(msg)
    }

    pub fn retry_after_secs(&self) -> Option<u64> {
        match self {
            ClientError::RateLimited {
                retry_after_secs, ..
            }
            | ClientError::Server {
                retry_after_secs, ..
            } => *retry_after_secs,
            _ => None,
        }
    }
}

fn context_length_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("context_length")
        || lower.contains("context length")
        || lower.contains("maximum context")
        || lower.contains("context window")
        || lower.contains("too many tokens")
        || lower.contains("prompt is too long")
        || lower.contains("prompt too long")
        || lower.contains("token limit")
        || lower.contains("max_tokens") && lower.contains("exceed")
        || lower.contains("exceeds the context")
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
        429 => ClientError::RateLimited {
            retry_after_secs,
            message,
        },
        500..=599 => ClientError::Server {
            status,
            message,
            retry_after_secs,
        },
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

    /// True when [`ModelClient::compact`] can reach xAI's compaction endpoint.
    fn compacts_server_side(&self) -> bool {
        false
    }

    /// xAI server-side compaction. `None` when this client has no such endpoint
    /// (local runtimes, providers, test fakes): the caller summarizes instead.
    fn compact(
        &self,
        _req: &CompactRequest,
        _cancel: &CancelToken,
    ) -> Option<Result<CompactOutput, ClientError>> {
        None
    }
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
            InputItem::FunctionCall {
                call_id,
                name,
                arguments,
            } => {
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
            // Built only from valid JSON ([`InputItem::server_compaction`]).
            InputItem::ServerCompaction { raw } => {
                if let Ok(item) = serde_json::from_str::<Value>(raw) {
                    input.push(item);
                }
            }
        }
    }
    (input, has_image)
}

/// The `/v1/responses/compact` body: the model and the input, nothing else.
pub fn compact_body(req: &CompactRequest) -> Value {
    let model = if req.model.trim().is_empty() {
        DEFAULT_MODEL
    } else {
        req.model.trim()
    };
    json!({"model": model, "input": input_wire_value(&req.input)})
}

/// Largest compaction reply read (the opaque item grows with the conversation).
const COMPACT_REPLY_CAP: u64 = 64 * 1024 * 1024;

/// A `response.compaction` reply: its `output` items as the exact JSON text
/// they came in, `usage`, and `dropped_message_count`.
pub fn parse_compact_reply(body: &str) -> Result<CompactOutput, ClientError> {
    use serde_json::value::RawValue;
    let bad = |why: &str| ClientError::Protocol(format!("compaction reply: {why}"));
    let fields: std::collections::HashMap<String, Box<RawValue>> =
        serde_json::from_str(body).map_err(|_| bad("not a JSON object"))?;
    let output = fields.get("output").ok_or_else(|| bad("no output"))?;
    let rows: Vec<Box<RawValue>> =
        serde_json::from_str(output.get()).map_err(|_| bad("output is not a list"))?;
    let mut items = Vec::new();
    for row in &rows {
        let item = InputItem::server_compaction(row.get())
            .ok_or_else(|| bad("an output item is not a compaction"))?;
        let value: Value = serde_json::from_str(row.get()).map_err(|_| bad("bad item"))?;
        if value
            .get("encrypted_content")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            return Err(bad("a compaction item has no encrypted_content"));
        }
        items.push(item);
    }
    if items.is_empty() {
        return Err(bad("no compaction item"));
    }
    let usage_value: Value = fields
        .get("usage")
        .and_then(|u| serde_json::from_str(u.get()).ok())
        .unwrap_or(Value::Null);
    let dropped_message_count = usage_value
        .get("dropped_message_count")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Ok(CompactOutput {
        items,
        usage: crate::sse::parse_usage_value(&json!({"usage": usage_value})),
        dropped_message_count,
    })
}

pub fn responses_body(req: &ResponsesRequest) -> Value {
    let (input, has_image) = input_wire(&req.input);
    let mut tools = req.tools.clone();
    if req.hosted_search {
        // xAI hosted search: drop the cabin function tool of the same name so
        // OpenRouter / local keep `web_search` as a function while Grok does not
        // see both hosted and cabin search at once.
        tools.retain(|t| !crate::tools::web_search::is_function_web_search(t));
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
    // Only a model that takes an effort gets one: the rest answer HTTP 400 (2.10.97).
    // A non-grok id isn't ours to judge.
    let takes_effort = grokhub_core::accepts_reasoning_effort(model)
        || !model.to_ascii_lowercase().starts_with("grok-");
    if let Some(effort) = effort.filter(|s| takes_effort && !s.is_empty() && *s != "none") {
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
    let host = authority
        .rsplit_once(':')
        .map(|(h, _)| h)
        .unwrap_or(authority);
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

    /// The compaction endpoint next to this client's responses URL, if it has one.
    fn compact_url(&self) -> Option<String> {
        if self.url == RESPONSES_URL {
            return Some(RESPONSES_COMPACT_URL.to_string());
        }
        (loopback_http(&self.url) && self.url.ends_with("/v1/responses"))
            .then(|| format!("{}/compact", self.url))
    }

    /// Every request this client makes: the EgressGuard first, then one POST with
    /// the bearer, user agent and conv-id header. An HTTP error comes back mapped.
    fn post_guarded(
        &self,
        url: &str,
        conversation_id: &str,
        accept: &str,
        call_timeout: Option<Duration>,
        body: Value,
    ) -> Result<ureq::Response, ClientError> {
        // Only the listed xAI endpoints, or a loopback test server.
        if !loopback_http(url) && !XAI_MODEL_ENDPOINTS.contains(&url) {
            return Err(ClientError::Protocol(format!(
                "not an xAI model endpoint: {url}"
            )));
        }
        // EgressGuard (Spike-4a): api.x.ai is a default model host, so this
        // logs one `egress.jsonl` line and goes. Loopback test servers are local.
        let data = [DataClass::Chat, DataClass::Personal];
        let egress = guard_egress(&crate::perm::config_dir(), &EgressReq::new(url, &data));
        if let GateOutcome::Park { reason, .. } | GateOutcome::Refuse { reason } = egress {
            return Err(ClientError::Protocol(reason));
        }
        let mut call = self
            .agent
            .post(url)
            .set("Authorization", &{
                let bearer = &self.bearer;
                format!("Bearer {bearer}")
            })
            .set("User-Agent", USER_AGENT)
            .set("x-grok-conv-id", conversation_id)
            .set("Accept", accept);
        if let Some(limit) = call_timeout {
            call = call.timeout(limit);
        }
        let sent = crate::timing::lap("client:to_headers");
        let response = call.send_json(body);
        drop(sent);
        match response {
            Ok(resp) => {
                grokhub_core::connectivity::note_global_transport_success(grokhub_core::now_ms());
                Ok(resp)
            }
            Err(ureq::Error::Status(status, resp)) => {
                grokhub_core::connectivity::note_global_transport_hint(
                    grokhub_core::connectivity::classify_http_status(status),
                    grokhub_core::now_ms(),
                );
                let retry_after = retry_after_header(&resp);
                let message = read_error_body(resp);
                Err(map_http_status(
                    status,
                    &message,
                    retry_after,
                    self.auth_kind,
                ))
            }
            Err(ureq::Error::Transport(err)) => {
                let client_err = classify_transport(&err);
                crate::route::connectivity::note_ureq_transport(&ureq::Error::Transport(err));
                Err(client_err)
            }
        }
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
        let body = crate::timing::time("client:body", || responses_body(req));
        let response = self.post_guarded(
            &self.url,
            &req.conversation_id,
            "text/event-stream",
            req.call_timeout,
            body,
        )?;
        read_sse(response.into_reader(), cancel, sink)
    }

    fn compact_once(
        &self,
        url: &str,
        req: &CompactRequest,
        cancel: &CancelToken,
        limit: Option<Duration>,
    ) -> Result<CompactOutput, ClientError> {
        if cancel.is_cancelled() {
            return Err(ClientError::Cancelled);
        }
        let response = self.post_guarded(
            url,
            &req.conversation_id,
            "application/json",
            limit,
            compact_body(req),
        )?;
        let mut text = String::new();
        response
            .into_reader()
            .take(COMPACT_REPLY_CAP)
            .read_to_string(&mut text)
            .map_err(|e| {
                let msg = e.to_string();
                if msg.to_ascii_lowercase().contains("timed out") {
                    ClientError::IdleTimeout
                } else {
                    ClientError::Transport(msg)
                }
            })?;
        if cancel.is_cancelled() {
            return Err(ClientError::Cancelled);
        }
        parse_compact_reply(&text)
    }

    /// One call with the client's retry policy: transient errors back off and retry.
    /// With a `deadline`, each attempt gets the time left as its own timeout and no
    /// backoff runs past it, so the whole call ends by then.
    fn with_retries<T>(
        cancel: &CancelToken,
        deadline: Option<std::time::Instant>,
        mut call: impl FnMut(Option<Duration>) -> Result<T, ClientError>,
    ) -> Result<T, ClientError> {
        let left = || deadline.map(|d| d.saturating_duration_since(std::time::Instant::now()));
        let mut retry_count = 0u32;
        loop {
            if cancel.is_cancelled() {
                return Err(ClientError::Cancelled);
            }
            if left().is_some_and(|l| l.is_zero()) {
                return Err(ClientError::IdleTimeout);
            }
            match call(left()) {
                Ok(out) => return Ok(out),
                Err(err) if err.is_retryable() => {
                    match decide_retry(&err, retry_count, DEFAULT_MAX_RETRIES) {
                        Some(wait) if left().is_some_and(|l| wait >= l) => return Err(err),
                        Some(wait) => {
                            retry_count = retry_count.saturating_add(1);
                            if !sleep_cancellable(wait, cancel) {
                                return Err(ClientError::Cancelled);
                            }
                        }
                        None => return Err(err),
                    }
                }
                Err(err) => return Err(err),
            }
        }
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
        Self::with_retries(cancel, None, |_| self.once(req, cancel, sink))
    }

    fn compacts_server_side(&self) -> bool {
        self.compact_url().is_some()
    }

    fn compact(
        &self,
        req: &CompactRequest,
        cancel: &CancelToken,
    ) -> Option<Result<CompactOutput, ClientError>> {
        let url = self.compact_url()?;
        let deadline = std::time::Instant::now() + req.timeout;
        Some(Self::with_retries(cancel, Some(deadline), |limit| {
            self.compact_once(&url, req, cancel, limit)
        }))
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
    resp.header("retry-after")
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|n| *n > 0)
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
    let mut truncated = false;
    let mut truncate_reason = None;
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
        fold_events(
            parser.push(&chunk),
            sink,
            &mut text,
            &mut reasoning,
            &mut calls,
            &mut usage,
            &mut truncated,
            &mut truncate_reason,
        )?;
    }
    fold_events(
        parser.finish()?,
        sink,
        &mut text,
        &mut reasoning,
        &mut calls,
        &mut usage,
        &mut truncated,
        &mut truncate_reason,
    )?;
    Ok(TurnOutput {
        text,
        reasoning,
        calls,
        usage,
        truncated,
        truncate_reason,
    })
}

fn fold_events(
    events: Vec<SseEvent>,
    sink: &mut dyn FnMut(StreamEvent),
    text: &mut String,
    reasoning: &mut String,
    calls: &mut Vec<FunctionCall>,
    usage: &mut Usage,
    truncated: &mut bool,
    truncate_reason: &mut Option<String>,
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
            SseEvent::Completed {
                usage: next,
                incomplete,
                incomplete_reason,
            } => {
                *usage = next;
                if incomplete {
                    *truncated = true;
                    if let Some(reason) = incomplete_reason {
                        *truncate_reason = Some(reason);
                    }
                }
            }
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
        let ok = mk()
            .with_loopback_url("http://127.0.0.1:4711/v1/responses")
            .unwrap();
        assert_eq!(ok.url, "http://127.0.0.1:4711/v1/responses");
        let ok = mk()
            .with_loopback_url("http://localhost:9/v1/responses")
            .unwrap();
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
            ClientError::RateLimited {
                retry_after_secs: Some(9),
                ..
            }
        ));
        assert!(matches!(
            map_http_status(503, "down", None, AuthKind::OAuth),
            ClientError::Server { status: 503, .. }
        ));
        assert!(!map_http_status(401, "no", None, AuthKind::OAuth).is_retryable());
        assert!(map_http_status(500, "x", None, AuthKind::ApiKey).is_retryable());
        assert!(!ClientError::IdleTimeout.is_retryable());
        assert!(ClientError::Disconnect.is_retryable());
        let overflow = ClientError::Protocol(
            "HTTP 400: This model's maximum context length was exceeded".into(),
        );
        assert!(overflow.is_context_length());
        assert!(!overflow.is_retryable());
        assert!(!ClientError::Protocol("disk full".into()).is_context_length());
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
        assert_eq!(
            responses_body(&req("minimal"))["reasoning"]["effort"],
            "low"
        );
        assert_eq!(responses_body(&req("mini"))["reasoning"]["effort"], "low");
        assert_eq!(responses_body(&req("max"))["reasoning"]["effort"], "xhigh");
        assert_eq!(
            responses_body(&req("medium"))["reasoning"]["effort"],
            "medium"
        );
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
        assert!(tools
            .iter()
            .any(|t| t["type"] == "function" && t["name"] == "read_file"));
        assert!(tools.iter().any(|t| t["type"] == "web_search"));
        assert!(tools.iter().any(|t| t["type"] == "x_search"));
        assert!(
            !tools
                .iter()
                .any(|t| t["type"] == "function" && t["name"] == "web_search"),
            "hosted search must drop cabin function web_search: {tools:?}"
        );
        assert_eq!(body["input"][0]["content"][0]["type"], "input_text");

        let with_cabin = ResponsesRequest {
            model: "grok-4.7".into(),
            effort: Some("high".into()),
            input: vec![InputItem::Message {
                role: "user".into(),
                content: vec![ContentPart::InputText("hi".into())],
            }],
            conversation_id: "c".into(),
            tools: vec![json!({
                "type": "function",
                "name": "web_search",
                "description": "cabin",
                "parameters": {"type": "object", "properties": {}}
            })],
            hosted_search: true,
            call_timeout: None,
        };
        let body = responses_body(&with_cabin);
        let tools = body["tools"].as_array().unwrap();
        assert!(tools.iter().any(|t| t["type"] == "web_search"));
        assert!(
            !tools
                .iter()
                .any(|t| t["type"] == "function" && t["name"] == "web_search"),
            "{tools:?}"
        );
        let openrouter_style = ResponsesRequest {
            hosted_search: false,
            ..with_cabin.clone()
        };
        let body = responses_body(&openrouter_style);
        let tools = body["tools"].as_array().unwrap();
        assert!(
            tools
                .iter()
                .any(|t| t["type"] == "function" && t["name"] == "web_search"),
            "{tools:?}"
        );
        assert!(!tools
            .iter()
            .any(|t| t["type"] == "web_search" && t.get("name").is_none()));

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
        assert!(headers
            .iter()
            .any(|(k, v)| k == "Authorization" && v == "Bearer test-token"));
        assert!(headers
            .iter()
            .any(|(k, v)| k == "User-Agent" && v == USER_AGENT));
        assert!(headers
            .iter()
            .any(|(k, v)| k == "x-grok-conv-id" && v == "conv-1"));
        assert_eq!(DEFAULT_MODEL, grokhub_core::CABIN_FAST_MODEL);
    }

    #[test]
    fn xai_client_debug_redacts_the_bearer() {
        let client = XaiClient::new(
            "super-secret-value",
            AuthKind::ApiKey,
            Duration::from_secs(1),
        );
        let rendered = format!("{client:?}");
        assert!(!rendered.contains("super-secret-value"), "{rendered}");
        assert!(rendered.contains("redacted"), "{rendered}");
    }
    #[test]
    fn compact_reply_keeps_each_item_as_sent_and_reads_usage() {
        let body = r#"{"id":"cmp_1","object":"response.compaction","output":[ {"id":"cmp_1", "type":"compaction","encrypted_content":"Zm9v"} ],"usage":{"input_tokens":50,"output_tokens":7,"output_tokens_details":{"reasoning_tokens":2},"dropped_message_count":4,"cost_in_usd_ticks":11}}"#;
        assert_eq!(
            parse_compact_reply(body).unwrap(),
            CompactOutput {
                items: vec![InputItem::ServerCompaction {
                    raw: r#"{"id":"cmp_1", "type":"compaction","encrypted_content":"Zm9v"}"#.into(),
                }],
                usage: Usage {
                    input_tokens: 50,
                    output_tokens: 7,
                    reasoning_tokens: 2,
                    cost_in_usd_ticks: 11,
                    cached_tokens: 0,
                },
                dropped_message_count: 4,
            }
        );
        for (bad, why) in [
            ("not json", "compaction reply: not a JSON object"),
            (r#"{"usage":{}}"#, "compaction reply: no output"),
            (r#"{"output":[]}"#, "compaction reply: no compaction item"),
            (
                r#"{"output":[{"type":"message","content":[]}]}"#,
                "compaction reply: an output item is not a compaction",
            ),
            (
                r#"{"output":[{"type":"compaction","encrypted_content":""}]}"#,
                "compaction reply: a compaction item has no encrypted_content",
            ),
        ] {
            assert_eq!(
                parse_compact_reply(bad).unwrap_err(),
                ClientError::Protocol(why.into()),
                "{bad}"
            );
        }
        assert_eq!(InputItem::server_compaction(r#"{"type":"message"}"#), None);
        assert_eq!(InputItem::server_compaction("[1]"), None);
    }

    #[test]
    fn compaction_goes_only_next_to_the_xai_or_loopback_responses_url() {
        let mk = || XaiClient::new("t", AuthKind::ApiKey, Duration::from_secs(1));
        assert_eq!(
            mk().compact_url().as_deref(),
            Some("https://api.x.ai/v1/responses/compact")
        );
        assert!(mk().compacts_server_side());
        let local = mk()
            .with_loopback_url("http://127.0.0.1:4711/v1/responses")
            .unwrap();
        assert_eq!(
            local.compact_url().as_deref(),
            Some("http://127.0.0.1:4711/v1/responses/compact")
        );
        let other = mk()
            .with_loopback_url("http://127.0.0.1:4711/v1/chat/completions")
            .unwrap();
        assert_eq!(other.compact_url(), None);
        assert!(!other.compacts_server_side());
        let req = CompactRequest {
            model: String::new(),
            input: vec![InputItem::Message {
                role: "user".into(),
                content: vec![ContentPart::InputText("hi".into())],
            }],
            conversation_id: "c".into(),
            timeout: Duration::from_secs(1),
        };
        assert!(other.compact(&req, &CancelToken::new()).is_none());
        assert_eq!(
            compact_body(&req),
            json!({"model": DEFAULT_MODEL, "input": [
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "hi"}]}
            ]})
        );
    }
}
