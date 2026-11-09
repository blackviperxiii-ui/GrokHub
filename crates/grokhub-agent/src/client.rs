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
/// xAI's Batch API (HX-13). Night passes on an API key go here at 20% off
/// ([`XaiClient::deferrable`]); every call under it is [`batch_url`]-checked.
pub const BATCHES_URL: &str = "https://api.x.ai/v1/batches";
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

/// The `type` of a provider's reasoning item ([`InputItem::provider_reasoning`]).
pub const PROVIDER_REASONING: &str = "reasoning_details";

/// Output items of xAI's hosted search: `web_search_call`, and `custom_tool_call`
/// for X search (`xs_` call ids) and attachment search (`as_`). They go back
/// with the next request like sealed reasoning, as xAI asks for every output
/// item, so the reasoning that refers to them still lines up.
pub const HOSTED_CALLS: [&str; 3] = ["web_search_call", "x_search_call", "custom_tool_call"];

/// A hosted search call item: one of [`HOSTED_CALLS`] with an `id` or `call_id`.
pub fn is_hosted_call(value: &Value) -> bool {
    value
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|t| HOSTED_CALLS.contains(&t))
        && ["id", "call_id"].iter().any(|k| {
            value
                .get(*k)
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty())
        })
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
    ///
    /// `digest` is a plain-text copy of what the item folded, built locally when
    /// it was made. xAI never sees it; OpenRouter and the local model, which
    /// can't read `raw`, get the digest instead so the earlier chat isn't lost
    /// when a turn routes away from xAI. Empty for items saved before it existed.
    ServerCompaction {
        raw: String,
        digest: String,
    },
    /// One xAI `reasoning` output item with its `encrypted_content`, or one hosted
    /// search call ([`HOSTED_CALLS`]), kept as the JSON text it arrived as. Opaque like [`InputItem::ServerCompaction`]: it goes
    /// back to xAI unchanged on the next request, so the model keeps its own
    /// reasoning between tool steps and the prompt cache keeps matching. Other
    /// providers and the local model never see it, and it goes only to the
    /// model that wrote it (`model`), since another model can't read it and the
    /// cache is per model anyway. Build it with [`InputItem::reasoning`].
    /// A provider's `reasoning_details` ride in the same item
    /// ([`InputItem::provider_reasoning`]) and likewise go back only to the
    /// model that wrote them, never to xAI.
    Reasoning {
        raw: String,
        model: String,
    },
}

/// The output a tool call gets when the turn ended before it ran.
pub const NOT_RUN: &str = "Not run: the turn stopped before this call ran.";

/// Give every tool call in `history` an output, so no provider sees a call
/// with no answer. A call left without one (Stop, Halt or a denied step
/// partway through a batch, or a chat saved like that) gets [`NOT_RUN`],
/// placed after the run of calls and outputs it belongs to. Returns how many
/// outputs it added.
pub fn pair_orphan_calls(history: &mut Vec<InputItem>) -> usize {
    let answered: std::collections::HashSet<String> = history
        .iter()
        .filter_map(|item| match item {
            InputItem::FunctionCallOutput { call_id, .. } => Some(call_id.clone()),
            _ => None,
        })
        .collect();
    let mut added = 0;
    let mut i = 0;
    while i < history.len() {
        if !matches!(history[i], InputItem::FunctionCall { .. }) {
            i += 1;
            continue;
        }
        // One batch: the calls and the outputs that follow them.
        let start = i;
        let mut end = i;
        while end < history.len()
            && matches!(
                history[end],
                InputItem::FunctionCall { .. } | InputItem::FunctionCallOutput { .. }
            )
        {
            end += 1;
        }
        let missing: Vec<String> = history[start..end]
            .iter()
            .filter_map(|item| match item {
                InputItem::FunctionCall { call_id, .. } if !answered.contains(call_id) => {
                    Some(call_id.clone())
                }
                _ => None,
            })
            .collect();
        let count = missing.len();
        for (k, call_id) in missing.into_iter().enumerate() {
            history.insert(
                end + k,
                InputItem::FunctionCallOutput {
                    call_id,
                    output: NOT_RUN.to_string(),
                },
            );
        }
        added += count;
        i = end + count;
    }
    added
}

impl InputItem {
    /// A [`InputItem::Reasoning`] from one item's JSON text, or `None` when it is
    /// neither a JSON object of type `reasoning` with a non-empty
    /// `encrypted_content` (xAI), a hosted search call ([`is_hosted_call`]), nor
    /// one of type [`PROVIDER_REASONING`] with a non-empty `reasoning_details`
    /// list (an OpenAI-compatible provider).
    pub fn reasoning(raw: &str, model: &str) -> Option<InputItem> {
        let value: Value = serde_json::from_str(raw).ok()?;
        let sealed = is_hosted_call(&value)
            || match value.get("type").and_then(Value::as_str) {
                Some("reasoning") => value
                    .get("encrypted_content")
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.is_empty()),
                Some(PROVIDER_REASONING) => value
                    .get("reasoning_details")
                    .and_then(Value::as_array)
                    .is_some_and(|d| !d.is_empty()),
                _ => false,
            };
        sealed.then(|| InputItem::Reasoning {
            raw: raw.to_string(),
            model: model.trim().to_string(),
        })
    }

    /// A provider's `reasoning_details` (OpenRouter) as a [`InputItem::Reasoning`]
    /// for the model that wrote it, so the next tool step sends them back.
    pub fn provider_reasoning(details: &Value, model: &str) -> Option<InputItem> {
        let raw = json!({"type": PROVIDER_REASONING, "reasoning_details": details});
        Self::reasoning(&raw.to_string(), model)
    }

    /// The `reasoning_details` this item carries for `model`, when it is a
    /// provider's and that model wrote it.
    pub fn reasoning_details(&self, model: &str) -> Option<Value> {
        let InputItem::Reasoning { raw, model: wrote } = self else {
            return None;
        };
        if !wrote.eq_ignore_ascii_case(model.trim()) {
            return None;
        }
        let mut v: Value = serde_json::from_str(raw).ok()?;
        (v.get("type").and_then(Value::as_str) == Some(PROVIDER_REASONING))
            .then(|| v["reasoning_details"].take())
    }

    /// xAI-only opaque items: server compaction and encrypted reasoning.
    pub fn is_opaque(&self) -> bool {
        matches!(
            self,
            InputItem::ServerCompaction { .. } | InputItem::Reasoning { .. }
        )
    }

    /// A [`InputItem::ServerCompaction`] from one item's JSON text, or `None`
    /// when it is not a JSON object of type `compaction`.
    pub fn server_compaction(raw: &str) -> Option<InputItem> {
        let value: Value = serde_json::from_str(raw).ok()?;
        (value.get("type").and_then(Value::as_str) == Some("compaction")).then(|| {
            InputItem::ServerCompaction {
                raw: raw.to_string(),
                digest: String::new(),
            }
        })
    }

    /// The same item with `digest` as its text copy. Other items are unchanged.
    pub fn with_digest(self, text: &str) -> InputItem {
        match self {
            InputItem::ServerCompaction { raw, .. } => InputItem::ServerCompaction {
                raw,
                digest: text.to_string(),
            },
            other => other,
        }
    }
}

/// A strict JSON schema the reply must follow (xAI structured outputs). Sent to
/// Grok models only; other providers and the local model keep the caller's
/// text parser, so the prompt still asks for the same JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextFormat {
    /// The schema's name on the wire (letters, digits, `_`).
    pub name: &'static str,
    /// The JSON Schema as JSON text. Every key listed in `required`;
    /// `additionalProperties` false.
    pub schema: &'static str,
}

impl TextFormat {
    /// The Responses API `text` field, or `None` when the schema text does not parse.
    pub fn wire(&self) -> Option<Value> {
        let schema: Value = serde_json::from_str(self.schema).ok()?;
        Some(json!({
            "format": {
                "type": "json_schema",
                "name": self.name,
                "schema": schema,
                "strict": true,
            }
        }))
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
    /// A strict JSON schema for the reply ([`TextFormat`]). `None` for free text.
    pub text_format: Option<TextFormat>,
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
    /// xAI asked to slow down: the call waits this long, then tries again.
    RateWait(Duration),
}

/// What xAI's hosted search did in one reply (`usage.server_side_tool_usage_details`).
/// Only successful calls are counted, and only those are billed: web search per
/// call, X search per post and per profile fetched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct HostedUsage {
    #[serde(default, skip_serializing_if = "is_zero")]
    pub web_search_calls: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub x_posts_fetched: u64,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub x_users_fetched: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

impl HostedUsage {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub fn add(&mut self, other: &HostedUsage) {
        self.web_search_calls = self.web_search_calls.saturating_add(other.web_search_calls);
        self.x_posts_fetched = self.x_posts_fetched.saturating_add(other.x_posts_fetched);
        self.x_users_fetched = self.x_users_fetched.saturating_add(other.x_users_fetched);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TurnOutput {
    pub text: String,
    pub reasoning: String,
    /// xAI sealed reasoning ([`InputItem::Reasoning`]), in order, tagged with the
    /// model that wrote it. It goes into the history ahead of this turn's reply.
    pub reasoning_items: Vec<InputItem>,
    pub calls: Vec<FunctionCall>,
    pub usage: Usage,
    /// True when the stream ended as `response.incomplete` (or status
    /// incomplete). Tool calls in this turn must not run.
    pub truncated: bool,
    /// Provider reason for truncation, when present (e.g. `max_output_tokens`).
    pub truncate_reason: Option<String>,
    /// Pages hosted search cited in the reply (`url_citation` annotations), once
    /// each, in order. The reply text carries them inline as well.
    pub sources: Vec<String>,
    /// Hosted search the reply ran and was billed for.
    pub hosted: HostedUsage,
}

impl TurnOutput {
    /// E14: cut off at the output-token limit with no answer text, so the
    /// reasoning used the whole budget. A cut-off with no reason counts too
    /// (an incomplete stream that names none is the output limit).
    pub fn out_of_budget(&self) -> bool {
        self.truncated
            && self.text.trim().is_empty()
            && self
                .truncate_reason
                .as_deref()
                .is_none_or(|r| r == "max_output_tokens")
    }
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
    /// HTTP 402: the account is out of credit or over its spending limit.
    /// Not retryable on this route — billing doesn't clear by waiting — but
    /// another route can take the turn, so the router fails over on it.
    PaymentRequired {
        status: u16,
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
    /// A retryable failure after part of the reply already showed in the chat.
    /// Not retried: a second attempt would show the text twice and bill again,
    /// so the turn stops with what was shown. Holds the first error's text.
    CutOff(String),
    /// The model declined to answer (Anthropic's `refusal` stop reason).
    /// Its own class, not a generic failure: the turn stops and the user
    /// hears it was a refusal. Never retried and never failed over — the
    /// refusal is about this ask, not about the route's health.
    Refused {
        message: String,
    },
    Cancelled,
    Protocol(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::KeyOffer { status, message }
                if crate::plan_room::plan_refusal(*status, message) =>
            {
                write!(f, "Grok plan refused this (HTTP {status}): {message}")
            }
            ClientError::KeyOffer { status, message } => {
                write!(f, "sign-in was rejected (HTTP {status}): {message}")
            }
            ClientError::Auth { status, message } => {
                write!(f, "API key rejected (HTTP {status}): {message}")
            }
            ClientError::RateLimited { message, .. } => write!(f, "rate limited: {message}"),
            ClientError::PaymentRequired { status, message } => {
                write!(f, "HTTP {status}: {message}")
            }
            ClientError::Server {
                status, message, ..
            } => write!(f, "HTTP {status}: {message}"),
            ClientError::Transport(msg) => write!(f, "transport: {msg}"),
            ClientError::IdleTimeout => write!(f, "model stopped responding"),
            ClientError::Disconnect => write!(f, "stream disconnected before it completed"),
            ClientError::CutOff(cause) => {
                write!(
                    f,
                    "the reply was cut off partway ({cause}); what showed is kept"
                )
            }
            ClientError::Refused { message } => {
                write!(f, "the model declined to answer: {message}")
            }
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
        402 => ClientError::PaymentRequired { status, message },
        500..=599 => ClientError::Server {
            status,
            message,
            retry_after_secs,
        },
        other if overload_message(&message) => ClientError::Server {
            status: other,
            message,
            retry_after_secs,
        },
        other => ClientError::Protocol(format!("HTTP {other}: {message}")),
    }
}

/// The provider is busy, not refusing this request: at capacity, overloaded,
/// briefly unavailable. Worth a retry and then another route. Quota and
/// billing text is never this: those don't clear by waiting.
pub fn overload_message(message: &str) -> bool {
    let l = message.to_ascii_lowercase();
    if billing_message(&l) {
        return false;
    }
    [
        "at capacity",
        "high demand",
        "overloaded",
        "over capacity",
        "temporarily unavailable",
        "service unavailable",
        "server is busy",
        "try again later",
        "try again in a few",
        "internal server error",
        "server_error",
    ]
    .iter()
    .any(|needle| l.contains(needle))
}

fn rate_limit_message(lower: &str) -> bool {
    !billing_message(lower)
        && (lower.contains("rate limit")
            || lower.contains("rate_limit")
            || lower.contains("too many requests"))
}

fn billing_message(lower: &str) -> bool {
    lower.contains("quota")
        || lower.contains("credit")
        || lower.contains("billing")
        || lower.contains("spending limit")
        || lower.contains("payment")
}

/// An error the model stream sent mid-reply (`error`, `response.failed`).
/// A busy or rate-limited provider is retried and failed over like the same
/// answer over HTTP; anything else ends the call as it says.
pub fn stream_error(message: String) -> ClientError {
    let lower = message.to_ascii_lowercase();
    if rate_limit_message(&lower) {
        ClientError::RateLimited {
            retry_after_secs: None,
            message,
        }
    } else if overload_message(&message) {
        ClientError::Server {
            status: 503,
            message,
            retry_after_secs: None,
        }
    } else {
        ClientError::Protocol(message)
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

    /// True when calls go through a batch queue that answers in minutes, so
    /// their wall time says nothing about how fast the model is.
    fn batches(&self) -> bool {
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

/// Every item as it would be sent, sealed reasoning included.
pub fn input_wire_value(items: &[InputItem]) -> Value {
    Value::Array(input_wire(items, None).0)
}

pub fn input_wire_len(items: &[InputItem]) -> usize {
    serde_json::to_vec(&input_wire_value(items))
        .map(|bytes| bytes.len())
        .unwrap_or(0)
}

/// The `input` list for `model`. A sealed reasoning item goes only to the model
/// that wrote it; `None` keeps every one (size estimates).
pub(crate) fn input_wire(items: &[InputItem], model: Option<&str>) -> (Vec<Value>, bool) {
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
            InputItem::ServerCompaction { raw, .. } => {
                if let Ok(item) = serde_json::from_str::<Value>(raw) {
                    input.push(item);
                }
            }
            // Built only from valid JSON ([`InputItem::reasoning`]).
            InputItem::Reasoning { raw, model: wrote } => {
                let same = model.is_none_or(|m| m.trim().eq_ignore_ascii_case(wrote));
                // A provider's reasoning details are never xAI's to read.
                if let Some(item) = same
                    .then(|| serde_json::from_str::<Value>(raw).ok())
                    .flatten()
                    .filter(|v| v["type"] == "reasoning" || is_hosted_call(v))
                {
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
    json!({"model": model, "input": input_wire(&req.input, Some(model)).0})
}

/// [`compact_body`] plus the chat's cache key (a fork's is its chat's), so the
/// fold reaches the server that holds the chat's cached prefix. Grok models only.
pub fn keyed_compact_body(req: &CompactRequest) -> Value {
    let mut body = compact_body(req);
    let key = crate::subagent::cache_key(req.conversation_id.trim());
    if !key.is_empty() && body["model"].as_str().is_some_and(is_grok_model) {
        body["prompt_cache_key"] = json!(key);
    }
    body
}

/// A refusal that a field we added could cause: HTTP 400 or 422.
fn refused_body(err: &ClientError) -> bool {
    matches!(err, ClientError::Protocol(m) if m.starts_with("HTTP 400") || m.starts_with("HTTP 422"))
}

/// Largest compaction reply read (the opaque item grows with the conversation).
const COMPACT_REPLY_CAP: u64 = 64 * 1024 * 1024;

/// What the compaction thread hands back: the body of a 2xx, or any other
/// answer (an error status, or none) for [`XaiClient::answered`] to map.
enum CompactReply {
    Body(std::io::Result<String>),
    Answer(Result<grokhub_core::http::Response, grokhub_core::http::Error>),
}

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

/// Cap on xAI's own search loop in one reply (`max_turns`), by what the turn
/// is for: a quick lookup for chat, more room for a task, deep for research
/// (xAI suggests 1 to 2, 3 to 5 and 10 or more). A run with no brief, such as
/// a subagent, gets the middle. Hosted search is billed per call, so this
/// bounds its spend; a client tool call starts the count again.
pub fn hosted_max_turns(conversation_id: &str) -> u32 {
    use crate::brain::Intent;
    match crate::checkpoint::turn_brief(conversation_id).map(|b| b.intent) {
        Some(Intent::Research) => 5,
        Some(Intent::Task | Intent::Repair) | None => 3,
        Some(Intent::Chat | Intent::Question | Intent::Creative | Intent::Background) => 2,
    }
}

pub fn responses_body(req: &ResponsesRequest) -> Value {
    let model = if req.model.trim().is_empty() {
        DEFAULT_MODEL
    } else {
        req.model.trim()
    };
    let (input, has_image) = input_wire(&req.input, Some(model));
    let mut tools = req.tools.clone();
    if req.hosted_search {
        // xAI hosted search: drop the cabin function tool of the same name so
        // OpenRouter / local keep `web_search` as a function while Grok does not
        // see both hosted and cabin search at once.
        tools.retain(|t| !crate::tools::web_search::is_function_web_search(t));
        tools.push(json!({"type": "web_search"}));
        tools.push(json!({"type": "x_search"}));
    }
    let hosted_turns = (req.hosted_search && is_grok_model(model))
        .then(|| hosted_max_turns(req.conversation_id.trim()));
    let mut body = json!({
        "model": model,
        "stream": true,
        "input": input,
        "tools": tools,
    });
    if let Some(turns) = hosted_turns {
        body["max_turns"] = json!(turns);
    }
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
    // `none` switches reasoning off; only Grok 4.3 takes it (Fast).
    let takes_none = grokhub_core::accepts_effort_none(model);
    if let Some(effort) = effort.filter(|s| {
        !s.is_empty()
            && if *s == "none" {
                takes_none
            } else {
                takes_effort
            }
    }) {
        body["reasoning"] = json!({"effort": effort});
    }
    if has_image {
        body["store"] = json!(false);
    }
    if is_grok_model(model) {
        // Routes this chat's requests to the same warm server (xAI's cache key
        // on the Responses API; `x-grok-conv-id` is the Chat Completions one).
        // A fork shares its chat's key, so it reaches the warm prefix.
        let key = crate::subagent::cache_key(req.conversation_id.trim());
        if !key.is_empty() {
            body["prompt_cache_key"] = json!(key);
        }
        // History is managed here, so xAI never needs to keep a copy of a
        // request or its reply, reasoning or not.
        body["store"] = json!(false);
        if body["tools"]
            .as_array()
            .is_some_and(|tools| tools.iter().any(|t| t["type"] == "function"))
        {
            // Said outright, so a batch of read-only calls in one reply never
            // hangs on a server default changing. Hosted search alone runs
            // server-side and needs no say.
            body["parallel_tool_calls"] = json!(true);
        }
        if returns_encrypted_reasoning(model) {
            // Ask for the reasoning back and pass it in on the next request
            // ([`InputItem::Reasoning`]).
            body["include"] = json!(["reasoning.encrypted_content"]);
        }
        if let Some(text) = req.text_format.as_ref().and_then(TextFormat::wire) {
            // Constrained decoding: the reply is the schema's JSON, no fences.
            body["text"] = text;
        }
    }
    body
}

/// The chat's socket carries only the chat's own agent steps. A side call
/// (auto-review judge, close check, Brief, memory, dream) sends no tools or
/// has a hard deadline: it goes over HTTP, so it never replaces the chat's
/// `previous_response_id` chain, and HTTP holds it to its deadline.
pub(crate) fn rides_chat_socket(req: &ResponsesRequest) -> bool {
    !req.tools.is_empty() && req.call_timeout.is_none()
}

/// An xAI Grok id as the Responses API takes it (not an OpenRouter slug).
pub fn is_grok_model(model: &str) -> bool {
    model.trim().to_ascii_lowercase().starts_with("grok-")
}

/// A Grok reasoning model: xAI returns its reasoning as an encrypted item that
/// must go back unchanged for the prompt cache to keep matching.
pub fn returns_encrypted_reasoning(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    if !is_grok_model(&m) || m.contains("non-reasoning") || m.contains("imagine") {
        return false;
    }
    grokhub_core::accepts_reasoning_effort(&m)
        || grokhub_core::accepts_effort_none(&m)
        || m.starts_with("grok-3-mini")
}

pub struct XaiClient {
    agent: grokhub_core::http::Agent,
    bearer: String,
    auth_kind: AuthKind,
    url: String,
    /// Set by [`XaiClient::deferrable`]: eligible asks go through the Batch API.
    batch: Option<crate::batch::BatchPolicy>,
    /// Set once the compact endpoint answered 400 or 422 to a body with
    /// `prompt_cache_key` and took the same body without it: that key is not in
    /// the endpoint's docs, so from then on this client sends only `model` and
    /// `input` there.
    compact_key_refused: AtomicBool,
    /// The base idle clock for a stream; see [`idle_for`].
    idle: Duration,
    /// WebSocket mode's sockets (`ws_mode`), for api.x.ai or a loopback test
    /// server; `None` for a plain loopback client.
    ws: Option<crate::ws_mode::WsPool>,
}

/// One Batch API call on api.x.ai: the root, or a batch id with `/requests`,
/// `/results` or `:cancel`, and an optional `?limit=` read.
pub fn batch_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix(BATCHES_URL) else {
        return false;
    };
    if rest.is_empty() {
        return true;
    }
    let Some(rest) = rest.strip_prefix('/') else {
        return false;
    };
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    if !query.is_empty()
        && !query
            .strip_prefix("limit=")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    {
        return false;
    }
    let (id, tail) = match path.split_once(['/', ':']) {
        Some((id, _)) => (id, &path[id.len()..]),
        None => (path, ""),
    };
    crate::batch::valid_batch_id(id) && matches!(tail, "" | "/requests" | "/results" | ":cancel")
}

/// Each Batch API call is a small JSON exchange.
const BATCH_CALL_TIMEOUT: Duration = Duration::from_secs(60);
/// A batch call's reply is read up to this many bytes.
const BATCH_REPLY_CAP: u64 = 8 * 1024 * 1024;

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
        let agent = grokhub_core::http::agent(grokhub_core::http::Opts {
            connect: Duration::from_secs(20),
            read: Some(socket_read_timeout(idle)),
            ..Default::default()
        });
        Self {
            idle,
            agent,
            bearer: bearer.into(),
            auth_kind,
            url: RESPONSES_URL.to_string(),
            batch: None,
            compact_key_refused: AtomicBool::new(false),
            // Unit tests never dial api.x.ai; their sockets are loopback only.
            ws: (!cfg!(test)).then(|| crate::ws_mode::WsPool::new(crate::ws_mode::WS_URL)),
        }
    }

    /// For night passes that can wait: a one-shot text ask goes through xAI's
    /// Batch API (20% off, answered within hours) and falls back to a normal
    /// call when the batch can't answer. Only on an API key (the SuperGrok
    /// pool has no batch price), and not when `app.json` has
    /// `"batch_background": false`.
    pub fn deferrable(mut self, config_dir: &std::path::Path) -> Self {
        if self.auth_kind == AuthKind::ApiKey && crate::batch::enabled_at(config_dir) {
            self.batch = Some(crate::batch::BatchPolicy::new(config_dir));
        }
        self
    }

    /// Whether [`XaiClient::deferrable`] took effect.
    pub fn defers(&self) -> bool {
        self.batch.is_some()
    }

    /// The Batch API root next to this client's responses URL, if it has one.
    fn batches_url(&self) -> Option<String> {
        if self.url == RESPONSES_URL {
            return Some(BATCHES_URL.to_string());
        }
        (loopback_http(&self.url) && self.url.ends_with("/v1/responses")).then(|| {
            let root = &self.url[..self.url.len() - "/v1/responses".len()];
            format!("{root}/v1/batches")
        })
    }

    /// Send to a local test server instead of api.x.ai. Only plain-http loopback
    /// origins are accepted, so no caller can move real traffic to another host.
    pub fn with_loopback_url(mut self, url: &str) -> Result<Self, String> {
        if !loopback_http(url) {
            return Err(format!("not a loopback URL: {url}"));
        }
        self.url = url.to_string();
        // A loopback server speaks HTTP only, unless a test adds a socket.
        self.ws = None;
        Ok(self)
    }

    /// Test servers: send turns over a WebSocket at this loopback `ws://` URL
    /// first, as the real client does to api.x.ai.
    pub fn with_loopback_ws(mut self, url: &str) -> Result<Self, String> {
        let http = url.replacen("ws://", "http://", 1);
        if !url.starts_with("ws://") || !loopback_http(&http) {
            return Err(format!("not a loopback ws URL: {url}"));
        }
        self.ws = Some(crate::ws_mode::WsPool::new(url));
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
    ) -> Result<grokhub_core::http::Response, ClientError> {
        let call = self.guarded_post(url, conversation_id, accept, call_timeout)?;
        let sent = crate::timing::lap("client:to_headers");
        let response = call.send_json(body);
        drop(sent);
        self.answered(response)
    }

    /// The POST [`Self::post_guarded`] sends, built and checked but not sent:
    /// only an xAI model endpoint, a batch call or a loopback test server, and
    /// only once the egress guard lets it go.
    fn guarded_post(
        &self,
        url: &str,
        conversation_id: &str,
        accept: &str,
        call_timeout: Option<Duration>,
    ) -> Result<grokhub_core::http::Request, ClientError> {
        // Only the listed xAI endpoints, a batch call, or a loopback test server.
        if !loopback_http(url) && !XAI_MODEL_ENDPOINTS.contains(&url) && !batch_url(url) {
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
            .header("Authorization", &{
                let bearer = &self.bearer;
                format!("Bearer {bearer}")
            })
            .header("User-Agent", USER_AGENT)
            // A fork rides its chat's conversation, as its cache key does.
            .header(
                "x-grok-conv-id",
                &crate::subagent::cache_key(conversation_id.trim()),
            )
            .header("Accept", accept)
            .any_status();
        if let Some(limit) = call_timeout {
            call = call.timeout(limit);
        }
        Ok(call)
    }

    /// The Batch API's reads (status and results): batch URLs only, then the
    /// guard, then one GET with the bearer. Nothing of the user's goes out.
    fn get_guarded(&self, url: &str) -> Result<grokhub_core::http::Response, ClientError> {
        if !loopback_http(url) && !batch_url(url) {
            return Err(ClientError::Protocol(format!(
                "not an xAI batch call: {url}"
            )));
        }
        let egress = guard_egress(&crate::perm::config_dir(), &EgressReq::new(url, &[]));
        if let GateOutcome::Park { reason, .. } | GateOutcome::Refuse { reason } = egress {
            return Err(ClientError::Protocol(reason));
        }
        let response = self
            .agent
            .get(url)
            .header("Authorization", &format!("Bearer {}", self.bearer))
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
            .timeout(BATCH_CALL_TIMEOUT)
            .any_status()
            .call();
        self.answered(response)
    }

    /// An HTTP reply as this client reports it: errors mapped, transport noted.
    fn answered(
        &self,
        response: Result<grokhub_core::http::Response, grokhub_core::http::Error>,
    ) -> Result<grokhub_core::http::Response, ClientError> {
        if let (Ok(resp), AuthKind::OAuth) = (&response, self.auth_kind) {
            crate::plan_room::note_reply(
                resp.status(),
                &|name| resp.header(name).map(str::to_string),
                grokhub_core::now_ms(),
            );
        }
        match response {
            Ok(resp) if resp.status() < 400 => {
                grokhub_core::connectivity::note_global_transport_success(grokhub_core::now_ms());
                Ok(resp)
            }
            Ok(resp) => {
                let status = resp.status();
                grokhub_core::connectivity::note_global_transport_hint(
                    grokhub_core::connectivity::classify_http_status(status),
                    grokhub_core::now_ms(),
                );
                let retry_after = retry_after_header(&resp);
                let message = read_error_body(resp);
                if self.auth_kind == AuthKind::OAuth
                    && crate::plan_room::plan_refusal(status, &message)
                {
                    crate::plan_room::note_refused(grokhub_core::now_ms());
                }
                Err(map_http_status(
                    status,
                    &message,
                    retry_after,
                    self.auth_kind,
                ))
            }
            Err(err) => {
                let client_err = classify_transport(err.to_string());
                crate::route::connectivity::note_http_transport(&err);
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
        // Round 3 item 28: kept sealed by hash, so this exact request can be replayed.
        crate::harness::replay::note_request(&crate::perm::config_dir(), &body);
        if !crate::pace::take(&req.model, cancel) {
            return Err(ClientError::Cancelled);
        }
        let idle = idle_for(req.effort.as_deref(), self.idle);
        let wrote = req.model.trim();
        let wrote = if wrote.is_empty() {
            DEFAULT_MODEL
        } else {
            wrote
        };
        if let Some(turn) = self.over_socket(req, &body, wrote, cancel, idle, sink) {
            return turn;
        }
        let response = self
            .post_guarded(
                &self.url,
                &req.conversation_id,
                "text/event-stream",
                req.call_timeout,
                body,
            )
            .inspect_err(|err| {
                if matches!(err, ClientError::RateLimited { .. }) {
                    crate::pace::note_limited(&req.model);
                }
            })?;
        crate::pace::note_ok(&req.model);
        let mut turn = read_sse(response.into_reader(), cancel, idle, sink)?;
        for item in &mut turn.reasoning_items {
            if let InputItem::Reasoning { model, .. } = item {
                *model = wrote.to_string();
            }
        }
        Ok(turn)
    }

    /// WebSocket mode first, when it's on: `None` hands the call to HTTP.
    fn over_socket(
        &self,
        req: &ResponsesRequest,
        body: &Value,
        wrote: &str,
        cancel: &CancelToken,
        idle: Duration,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Option<Result<TurnOutput, ClientError>> {
        let pool = self.ws.as_ref()?;
        if !rides_chat_socket(req) {
            return None;
        }
        let config_dir = crate::perm::config_dir();
        if !crate::ws_mode::enabled_at(&config_dir) {
            return None;
        }
        // The same EgressGuard as HTTP, for the same endpoint.
        let data = [DataClass::Chat, DataClass::Personal];
        let egress = guard_egress(&config_dir, &EgressReq::new(&self.url, &data));
        if let GateOutcome::Park { reason, .. } | GateOutcome::Refuse { reason } = egress {
            return Some(Err(ClientError::Protocol(reason)));
        }
        let turn = pool.turn(
            &self.bearer,
            self.auth_kind,
            &req.conversation_id,
            body,
            wrote,
            cancel,
            idle,
            sink,
        );
        match turn {
            crate::ws_mode::WsTurn::Done(turn) => {
                grokhub_core::connectivity::note_global_transport_success(grokhub_core::now_ms());
                crate::pace::note_ok(&req.model);
                Some(Ok(turn))
            }
            crate::ws_mode::WsTurn::Failed(err) => {
                if matches!(err, ClientError::RateLimited { .. }) {
                    crate::pace::note_limited(&req.model);
                }
                Some(Err(err))
            }
            crate::ws_mode::WsTurn::Http => None,
        }
    }

    /// One compaction call. The endpoint answers only when it is done, so the
    /// call runs on its own thread and Stop is checked every [`READ_POLL`]: a
    /// Stop returns at once instead of after up to [`crate::compact`]'s 90 s cap.
    /// The abandoned call ends on its own when its timeout runs out.
    fn compact_once(
        &self,
        url: &str,
        req: &CompactRequest,
        cancel: &CancelToken,
        limit: Option<Duration>,
        keyed: bool,
    ) -> Result<CompactOutput, ClientError> {
        use std::sync::mpsc::RecvTimeoutError;
        if cancel.is_cancelled() {
            return Err(ClientError::Cancelled);
        }
        let call = self.guarded_post(url, &req.conversation_id, "application/json", limit)?;
        let body = if keyed {
            keyed_compact_body(req)
        } else {
            compact_body(req)
        };
        crate::harness::replay::note_request(&crate::perm::config_dir(), &body);
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        let spawned = std::thread::Builder::new()
            .name("compact-read".into())
            .spawn(move || {
                let reply = match call.send_json(body) {
                    Ok(resp) if resp.status() < 400 => {
                        let mut text = String::new();
                        CompactReply::Body(
                            resp.into_reader()
                                .take(COMPACT_REPLY_CAP)
                                .read_to_string(&mut text)
                                .map(|_| text),
                        )
                    }
                    other => CompactReply::Answer(other),
                };
                let _ = tx.send(reply);
            });
        if let Err(e) = spawned {
            return Err(ClientError::Transport(e.to_string()));
        }
        let reply = loop {
            if cancel.is_cancelled() {
                return Err(ClientError::Cancelled);
            }
            match rx.recv_timeout(READ_POLL) {
                Ok(reply) => break reply,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(ClientError::Transport(
                        "the compaction call stopped without an answer".into(),
                    ))
                }
            }
        };
        let text = match reply {
            CompactReply::Body(read) => {
                grokhub_core::connectivity::note_global_transport_success(grokhub_core::now_ms());
                read.map_err(|e| {
                    let msg = e.to_string();
                    if msg.to_ascii_lowercase().contains("timed out") {
                        ClientError::IdleTimeout
                    } else {
                        ClientError::Transport(msg)
                    }
                })?
            }
            CompactReply::Answer(answer) => {
                // Only an error status or no answer lands here.
                let resp = self.answered(answer)?;
                let mut text = String::new();
                resp.into_reader()
                    .take(COMPACT_REPLY_CAP)
                    .read_to_string(&mut text)
                    .map_err(|e| ClientError::Transport(e.to_string()))?;
                text
            }
        };
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
        call: impl FnMut(Option<Duration>) -> Result<T, ClientError>,
    ) -> Result<T, ClientError> {
        Self::with_retries_noting(cancel, deadline, call, &mut |_, _| {})
    }

    /// [`Self::with_retries`], telling `on_wait` about each wait before it starts.
    fn with_retries_noting<T>(
        cancel: &CancelToken,
        deadline: Option<std::time::Instant>,
        mut call: impl FnMut(Option<Duration>) -> Result<T, ClientError>,
        on_wait: &mut dyn FnMut(&ClientError, Duration),
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
                            on_wait(&err, wait);
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

/// Runs `once` under the retry policy, but never retries once reply text has
/// reached `sink`: the chat already shows it, so a retry would show it twice
/// and bill the request again. That failure comes back as
/// [`ClientError::CutOff`] and the turn stops with what was shown.
fn stream_once_shown(
    cancel: &CancelToken,
    mut once: impl FnMut(&mut dyn FnMut(StreamEvent)) -> Result<TurnOutput, ClientError>,
    sink: &mut dyn FnMut(StreamEvent),
) -> Result<TurnOutput, ClientError> {
    let sink = std::cell::RefCell::new(sink);
    let mut shown = false;
    XaiClient::with_retries_noting(
        cancel,
        None,
        |_| {
            once(&mut |ev: StreamEvent| {
                if matches!(&ev, StreamEvent::TextDelta(t) if !t.is_empty()) {
                    shown = true;
                }
                (*sink.borrow_mut())(ev)
            })
            .map_err(|err| {
                if shown && err.is_retryable() {
                    ClientError::CutOff(err.to_string())
                } else {
                    err
                }
            })
        },
        // A 429's wait shows in the chat, so a pause isn't a hang.
        &mut |err, wait| {
            if matches!(err, ClientError::RateLimited { .. }) {
                (*sink.borrow_mut())(StreamEvent::RateWait(wait));
            }
        },
    )
}

impl std::fmt::Debug for XaiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XaiClient")
            .field("bearer", &"<redacted>")
            .field("auth_kind", &self.auth_kind)
            .field("batch", &self.batch.is_some())
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
        if let Some(policy) = self.batch.as_ref().filter(|_| crate::batch::eligible(req)) {
            match crate::batch::run(
                self,
                policy,
                req,
                cancel,
                &grokhub_core::now_ms,
                &sleep_cancellable,
            ) {
                crate::batch::Deferred::Done(turn) => {
                    if !turn.text.is_empty() {
                        sink(StreamEvent::TextDelta(turn.text.clone()));
                    }
                    return Ok(turn);
                }
                crate::batch::Deferred::Cancelled => return Err(ClientError::Cancelled),
                crate::batch::Deferred::Fallback(_) => {}
            }
        }
        stream_once_shown(cancel, |sink| self.once(req, cancel, sink), sink)
    }

    fn compacts_server_side(&self) -> bool {
        self.compact_url().is_some()
    }

    fn batches(&self) -> bool {
        self.defers()
    }

    fn compact(
        &self,
        req: &CompactRequest,
        cancel: &CancelToken,
    ) -> Option<Result<CompactOutput, ClientError>> {
        let url = self.compact_url()?;
        let deadline = std::time::Instant::now() + req.timeout;
        let call = |keyed: bool| {
            Self::with_retries(cancel, Some(deadline), |limit| {
                self.compact_once(&url, req, cancel, limit, keyed)
            })
        };
        let keyed = !self.compact_key_refused.load(Ordering::Relaxed)
            && keyed_compact_body(req).get("prompt_cache_key").is_some();
        let first = call(keyed);
        if !(keyed && first.as_ref().is_err_and(refused_body)) {
            return Some(first);
        }
        // The endpoint may not take the key: once more with the documented body.
        let again = call(false);
        if again.is_ok() {
            self.compact_key_refused.store(true, Ordering::Relaxed);
            eprintln!(
                "grokhub compact: the compact endpoint refused prompt_cache_key; sending model and input only"
            );
        }
        Some(again)
    }
}

impl XaiClient {
    fn batch_root(&self) -> Result<String, ClientError> {
        self.batches_url()
            .ok_or_else(|| ClientError::Protocol("this client has no batch endpoint".into()))
    }

    fn batch_id_url(&self, batch_id: &str, tail: &str) -> Result<String, ClientError> {
        if !crate::batch::valid_batch_id(batch_id) {
            return Err(ClientError::Protocol(format!("not a batch id: {batch_id}")));
        }
        Ok(format!("{}/{batch_id}{tail}", self.batch_root()?))
    }

    fn batch_post(&self, url: &str, body: Value) -> Result<Value, ClientError> {
        let response = self.post_guarded(
            url,
            BATCH_CONV_ID,
            "application/json",
            Some(BATCH_CALL_TIMEOUT),
            body,
        )?;
        read_json(response)
    }

    fn batch_get(&self, url: &str) -> Result<Value, ClientError> {
        read_json(self.get_guarded(url)?)
    }
}

/// The `x-grok-conv-id` on Batch API calls (no chat of the user's).
const BATCH_CONV_ID: &str = "loadout-batch";

fn read_json(response: grokhub_core::http::Response) -> Result<Value, ClientError> {
    let mut text = String::new();
    response
        .into_reader()
        .take(BATCH_REPLY_CAP)
        .read_to_string(&mut text)
        .map_err(|e| ClientError::Transport(e.to_string()))?;
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text).map_err(|e| ClientError::Protocol(format!("batch reply: {e}")))
}

impl crate::batch::BatchApi for XaiClient {
    fn create(&self, name: &str) -> Result<String, ClientError> {
        let reply = self.batch_post(&self.batch_root()?, json!({ "name": name }))?;
        reply
            .get("batch_id")
            .and_then(Value::as_str)
            .filter(|id| crate::batch::valid_batch_id(id))
            .map(str::to_string)
            .ok_or_else(|| ClientError::Protocol("batch reply had no batch id".into()))
    }

    fn add(&self, batch_id: &str, request_id: &str, body: Value) -> Result<(), ClientError> {
        let url = self.batch_id_url(batch_id, "/requests")?;
        let wire = json!({
            "batch_requests": [{
                "batch_request_id": request_id,
                "batch_request": { "responses": body },
            }]
        });
        self.batch_post(&url, wire).map(|_| ())
    }

    fn get(&self, batch_id: &str) -> Result<crate::batch::BatchState, ClientError> {
        let url = self.batch_id_url(batch_id, "")?;
        Ok(crate::batch::BatchState::of(&self.batch_get(&url)?))
    }

    fn results(&self, batch_id: &str) -> Result<Vec<Value>, ClientError> {
        let url = self.batch_id_url(batch_id, "/results?limit=10")?;
        let reply = self.batch_get(&url)?;
        Ok(reply
            .get("results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    fn cancel(&self, batch_id: &str) -> Result<crate::batch::BatchState, ClientError> {
        let url = self.batch_id_url(batch_id, ":cancel")?;
        Ok(crate::batch::BatchState::of(
            &self.batch_post(&url, json!({}))?,
        ))
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

fn retry_after_header(resp: &grokhub_core::http::Response) -> Option<u64> {
    resp.header("retry-after")
        .and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|n| *n > 0)
}

fn read_error_body(resp: grokhub_core::http::Response) -> String {
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

fn classify_transport(msg: String) -> ClientError {
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

/// The text in `pending` that is whole UTF-8, leaving the start of a
/// character a read cut in half for the next read. A byte that can never be
/// UTF-8 decodes as U+FFFD right away.
fn take_utf8(pending: &mut Vec<u8>) -> String {
    // The last character's lead byte sits at most 3 bytes from the end; keep
    // that character back when it is unfinished, whatever came before it.
    let lead = (pending.len().saturating_sub(4)..pending.len())
        .rev()
        .find(|&i| pending[i] & 0xC0 != 0x80);
    let keep = match lead.map(|i| (i, std::str::from_utf8(&pending[i..]))) {
        Some((i, Err(err))) if err.error_len().is_none() => pending.len() - i,
        _ => 0,
    };
    let rest = pending.split_off(pending.len() - keep);
    let whole = std::mem::replace(pending, rest);
    String::from_utf8(whole).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// How often a stream that is waiting on xAI checks for Stop.
const READ_POLL: Duration = Duration::from_millis(250);

/// How long a stream may go quiet before it counts as stalled. High and
/// xhigh effort can think in silence for minutes, so they get two and a half
/// times the base (120 s becomes 300 s).
fn idle_for(effort: Option<&str>, base: Duration) -> Duration {
    match effort.map(str::trim) {
        Some("high" | "xhigh") => base.saturating_mul(5) / 2,
        _ => base,
    }
}

/// The socket's own read timeout: a backstop past the longest idle clock, so
/// the clock in [`read_sse`] is what ends a quiet stream.
fn socket_read_timeout(base: Duration) -> Duration {
    idle_for(Some("xhigh"), base) + Duration::from_secs(5)
}

/// Reads the body on its own thread, so Stop and the idle clock are checked
/// every [`READ_POLL`] even while a read is blocked.
fn pump(
    mut reader: impl Read + Send + 'static,
) -> std::sync::mpsc::Receiver<std::io::Result<Vec<u8>>> {
    let (tx, rx) = std::sync::mpsc::sync_channel(16);
    let spawned = std::thread::Builder::new().name("sse-read".into()).spawn({
        let tx = tx.clone();
        move || {
            let mut buf = [0u8; 4096];
            loop {
                let sent = match reader.read(&mut buf) {
                    Ok(0) => {
                        let _ = tx.send(Ok(Vec::new()));
                        break;
                    }
                    Ok(n) => tx.send(Ok(buf[..n].to_vec())),
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        break;
                    }
                };
                // The stream was dropped (Stop, a stall, an error): stop reading.
                if sent.is_err() {
                    break;
                }
            }
        }
    });
    if let Err(e) = spawned {
        let _ = tx.send(Err(e));
    }
    rx
}

fn read_sse(
    reader: impl Read + Send + 'static,
    cancel: &CancelToken,
    idle: Duration,
    sink: &mut dyn FnMut(StreamEvent),
) -> Result<TurnOutput, ClientError> {
    use std::sync::mpsc::RecvTimeoutError;
    let chunks = pump(reader);
    let mut parser = SseParser::new();
    let mut out = TurnOutput::default();
    // Bytes of a character that a read cut in half, kept for the next read.
    let mut pending: Vec<u8> = Vec::new();
    let mut last = std::time::Instant::now();
    let mut longest = Duration::ZERO;
    let gap_done = |longest: Duration| crate::timing::record("stream:longest_gap", longest);
    loop {
        if cancel.is_cancelled() {
            return Err(ClientError::Cancelled);
        }
        let buf = match chunks.recv_timeout(READ_POLL.min(idle)) {
            Ok(Ok(buf)) if buf.is_empty() => break,
            Ok(Ok(buf)) => buf,
            Ok(Err(e)) => {
                gap_done(longest.max(last.elapsed()));
                let msg = e.to_string();
                let lower = msg.to_ascii_lowercase();
                if lower.contains("timed out") || lower.contains("timeout") {
                    return Err(ClientError::IdleTimeout);
                }
                return Err(ClientError::Transport(msg));
            }
            Err(RecvTimeoutError::Timeout) => {
                if last.elapsed() >= idle {
                    gap_done(last.elapsed());
                    return Err(ClientError::IdleTimeout);
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(ClientError::Transport("the stream reader stopped".into()));
            }
        };
        longest = longest.max(last.elapsed());
        last = std::time::Instant::now();
        let n = buf.len();
        pending.extend_from_slice(&buf[..n]);
        let chunk = take_utf8(&mut pending);
        fold_events(parser.push(&chunk), sink, &mut out)?;
    }
    if !pending.is_empty() {
        let tail = String::from_utf8_lossy(&pending).into_owned();
        fold_events(parser.push(&tail), sink, &mut out)?;
    }
    fold_events(parser.finish()?, sink, &mut out)?;
    gap_done(longest);
    Ok(out)
}

pub(crate) fn fold_events(
    events: Vec<SseEvent>,
    sink: &mut dyn FnMut(StreamEvent),
    out: &mut TurnOutput,
) -> Result<(), ClientError> {
    let TurnOutput {
        text,
        reasoning,
        reasoning_items,
        calls,
        usage,
        truncated,
        truncate_reason,
        sources,
        hosted,
    } = out;
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
            SseEvent::ReasoningItem(raw) => {
                reasoning_items.extend(InputItem::reasoning(&raw, ""));
            }
            SseEvent::Sources(urls) => {
                for url in urls {
                    if !sources.contains(&url) {
                        sources.push(url);
                    }
                }
            }
            SseEvent::Hosted(next) => *hosted = next,
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
            SseEvent::Error(message) => return Err(stream_error(message)),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fc(id: &str) -> InputItem {
        InputItem::FunctionCall {
            call_id: id.into(),
            name: "read_file".into(),
            arguments: "{}".into(),
        }
    }

    fn fco(id: &str, out: &str) -> InputItem {
        InputItem::FunctionCallOutput {
            call_id: id.into(),
            output: out.into(),
        }
    }

    fn user_text(text: &str) -> InputItem {
        InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText(text.into())],
        }
    }

    #[test]
    fn a_stopped_batch_gets_an_output_for_every_call_in_order() {
        let mut history = vec![
            user_text("go"),
            fc("a"),
            fc("b"),
            fc("c"),
            fco("a", "ran"),
            user_text("next"),
        ];
        assert_eq!(pair_orphan_calls(&mut history), 2);
        assert_eq!(
            history,
            vec![
                user_text("go"),
                fc("a"),
                fc("b"),
                fc("c"),
                fco("a", "ran"),
                fco("b", NOT_RUN),
                fco("c", NOT_RUN),
                user_text("next"),
            ]
        );
        assert_eq!(
            pair_orphan_calls(&mut history),
            0,
            "a second pass changes nothing"
        );
    }

    #[test]
    fn answered_calls_are_left_alone() {
        let mut history = vec![fc("a"), fco("a", "ok"), fc("b"), fco("b", "ok")];
        let before = history.clone();
        assert_eq!(pair_orphan_calls(&mut history), 0);
        assert_eq!(history, before);
    }

    fn format_request(model: &str, format: TextFormat) -> ResponsesRequest {
        ResponsesRequest {
            model: model.into(),
            effort: Some("low".into()),
            input: vec![InputItem::Message {
                role: "user".into(),
                content: vec![ContentPart::InputText("check".into())],
            }],
            conversation_id: "c".into(),
            tools: Vec::new(),
            hosted_search: false,
            call_timeout: None,
            text_format: Some(format),
        }
    }

    #[test]
    fn every_machine_read_schema_is_strict_and_lists_every_key() {
        for format in [
            crate::auto_review::JUDGE_FORMAT,
            crate::close_check::CLOSE_FORMAT,
            crate::episode::VERIFY_FORMAT,
        ] {
            let wire = format.wire().expect(format.name);
            assert_eq!(wire["format"]["type"], "json_schema", "{}", format.name);
            assert_eq!(wire["format"]["strict"], true, "{}", format.name);
            assert_eq!(wire["format"]["name"], format.name);
            let mut objects = vec![wire["format"]["schema"].clone()];
            while let Some(obj) = objects.pop() {
                if let Some(items) = obj.get("items") {
                    objects.push(items.clone());
                }
                let Some(props) = obj.get("properties").and_then(Value::as_object) else {
                    continue;
                };
                assert_eq!(obj["additionalProperties"], false, "{}", format.name);
                let required: Vec<&str> = obj["required"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(Value::as_str)
                    .collect();
                for (key, sub) in props {
                    assert!(required.contains(&key.as_str()), "{}: {key}", format.name);
                    objects.push(sub.clone());
                }
            }
        }
    }

    #[test]
    fn a_text_format_goes_to_grok_only_and_free_text_sends_none() {
        let body = responses_body(&format_request("grok-4.3", crate::episode::VERIFY_FORMAT));
        assert_eq!(body["text"]["format"]["type"], "json_schema");
        assert_eq!(body["text"]["format"]["name"], "verify_gate_verdict");
        assert_eq!(
            body["text"]["format"]["schema"]["properties"]["verdict"]["enum"],
            json!(["ok", "reject"])
        );
        let other = responses_body(&format_request(
            "openrouter/some-model",
            crate::episode::VERIFY_FORMAT,
        ));
        assert!(other.get("text").is_none(), "{other}");
        let mut free = format_request("grok-4.3", crate::episode::VERIFY_FORMAT);
        free.text_format = None;
        assert!(responses_body(&free).get("text").is_none());
        let openai = crate::route::providers::openai_body(
            "some-model",
            None,
            &format_request("some-model", crate::episode::VERIFY_FORMAT),
        );
        assert!(openai.get("response_format").is_none(), "{openai}");
        assert!(openai.get("text").is_none(), "{openai}");
    }

    /// A body that hands out its bytes in the given cuts, one per read.
    struct Cuts(std::collections::VecDeque<Vec<u8>>);

    impl Read for Cuts {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let Some(next) = self.0.pop_front() else {
                return Ok(0);
            };
            out[..next.len()].copy_from_slice(&next);
            Ok(next.len())
        }
    }

    #[test]
    fn a_character_split_between_reads_stays_whole() {
        let body = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"café 🦀\"}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":1,\"output_tokens\":1},\"output\":[]}}\n\n",
        )
        .as_bytes();
        let e = body.windows(2).position(|w| w == [0xC3, 0xA9]).unwrap();
        let crab = body
            .windows(4)
            .position(|w| w == [0xF0, 0x9F, 0xA6, 0x80])
            .unwrap();
        // Cut inside é (C3|A9) and inside the crab (F0 9F|A6 80).
        let cuts = [&body[..e + 1], &body[e + 1..crab + 2], &body[crab + 2..]];
        let reader = Cuts(cuts.iter().map(|c| c.to_vec()).collect());
        let mut shown = String::new();
        let out = read_sse(
            reader,
            &CancelToken::new(),
            Duration::from_secs(5),
            &mut |ev| {
                if let StreamEvent::TextDelta(t) = ev {
                    shown.push_str(&t);
                }
            },
        )
        .unwrap();
        assert_eq!(out.text, "café 🦀");
        assert_eq!(shown, "café 🦀");

        let mut pending = vec![b'o', b'k', 0xFF, b'!'];
        assert_eq!(take_utf8(&mut pending), "ok\u{FFFD}!");
        assert!(pending.is_empty());
        // A bad byte earlier in a read still lets a cut character at its end wait.
        let mut pending = b"ok\xFF caf\xC3".to_vec();
        let mut text = take_utf8(&mut pending);
        pending.extend_from_slice(b"\xA9!");
        text.push_str(&take_utf8(&mut pending));
        assert_eq!(text, "ok\u{FFFD} café!");
    }

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

    /// A body that never sends another byte, like xAI thinking in silence.
    struct Quiet;
    impl Read for Quiet {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            std::thread::sleep(Duration::from_secs(30));
            Ok(0)
        }
    }

    #[test]
    fn stop_ends_a_quiet_stream_at_once_and_the_idle_clock_follows_effort() {
        let cancel = CancelToken::new();
        let stopper = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            stopper.cancel();
        });
        let began = std::time::Instant::now();
        let out = read_sse(Quiet, &cancel, Duration::from_secs(120), &mut |_| {});
        assert!(matches!(out, Err(ClientError::Cancelled)));
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "{:?}",
            began.elapsed()
        );

        let began = std::time::Instant::now();
        let out = read_sse(
            Quiet,
            &CancelToken::new(),
            Duration::from_millis(400),
            &mut |_| {},
        );
        assert!(matches!(out, Err(ClientError::IdleTimeout)));
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "{:?}",
            began.elapsed()
        );

        let base = Duration::from_secs(120);
        assert_eq!(idle_for(None, base), base);
        assert_eq!(idle_for(Some("low"), base), base);
        assert_eq!(idle_for(Some("medium"), base), base);
        assert_eq!(idle_for(Some("high"), base), Duration::from_secs(300));
        assert_eq!(idle_for(Some("xhigh"), base), Duration::from_secs(300));
        assert!(socket_read_timeout(base) > Duration::from_secs(300));
    }

    #[test]
    fn a_reply_cut_off_after_it_showed_stops_instead_of_showing_twice() {
        let cancel = CancelToken::new();
        let mut shown = Vec::new();
        let mut calls = 0;
        let out = stream_once_shown(
            &cancel,
            |sink| {
                calls += 1;
                sink(StreamEvent::TextDelta("Here is the plan".into()));
                Err(ClientError::Disconnect)
            },
            &mut |ev| shown.push(ev),
        );
        assert_eq!(calls, 1, "a reply that showed text is never sent again");
        assert!(matches!(out, Err(ClientError::CutOff(_))));
        assert!(!out.unwrap_err().is_retryable());
        assert_eq!(
            shown,
            vec![StreamEvent::TextDelta("Here is the plan".into())]
        );

        // Thinking alone showed nothing in the reply, so the call still retries.
        let mut calls = 0;
        let out = stream_once_shown(
            &cancel,
            |sink| {
                calls += 1;
                sink(StreamEvent::ReasoningDelta("hmm".into()));
                if calls == 1 {
                    return Err(ClientError::Disconnect);
                }
                sink(StreamEvent::TextDelta("done".into()));
                Ok(TurnOutput {
                    text: "done".into(),
                    ..TurnOutput::default()
                })
            },
            &mut |_| {},
        );
        assert_eq!(calls, 2);
        assert_eq!(out.unwrap().text, "done");

        // A 429's wait shows before it starts, and the call then goes through.
        let mut seen = Vec::new();
        let mut calls = 0;
        let out = stream_once_shown(
            &cancel,
            |_| {
                calls += 1;
                if calls == 1 {
                    return Err(ClientError::RateLimited {
                        retry_after_secs: Some(1),
                        message: "slow down".into(),
                    });
                }
                Ok(TurnOutput::default())
            },
            &mut |ev| seen.push(ev),
        );
        assert!(out.is_ok());
        assert_eq!(seen, vec![StreamEvent::RateWait(Duration::from_secs(1))]);
    }

    #[test]
    fn plan_gate_403_reads_as_the_plan_not_the_sign_in() {
        let tier = "You have either run out of available resources or do not have an active Grok subscription.";
        let plan = map_http_status(403, tier, None, AuthKind::OAuth);
        assert!(matches!(plan, ClientError::KeyOffer { status: 403, .. }));
        assert!(plan
            .to_string()
            .starts_with("Grok plan refused this (HTTP 403)"));
        let dead = map_http_status(
            401,
            "The OAuth2 access token could not be validated.",
            None,
            AuthKind::OAuth,
        );
        assert!(dead
            .to_string()
            .starts_with("sign-in was rejected (HTTP 401)"));
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
        // 402 is its own class: out of credit, never retried on this route.
        let unpaid = map_http_status(402, "Insufficient credits", None, AuthKind::ApiKey);
        assert!(matches!(
            unpaid,
            ClientError::PaymentRequired { status: 402, .. }
        ));
        assert!(!unpaid.is_retryable());
        assert_eq!(unpaid.to_string(), "HTTP 402: Insufficient credits");
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

    #[test]
    fn a_busy_model_mid_stream_is_retried_and_billing_is_not() {
        let busy = "The model is currently at capacity due to high demand. Please try again in a few minutes, or use a higher service tier for priority processing";
        let err = stream_error(busy.into());
        assert!(
            matches!(err, ClientError::Server { status: 503, .. }),
            "{err:?}"
        );
        assert!(err.is_retryable());
        assert!(matches!(
            stream_error("Rate limit reached for requests".into()),
            ClientError::RateLimited {
                retry_after_secs: None,
                ..
            }
        ));
        let quota = stream_error("You exceeded your current quota, please check billing".into());
        assert!(matches!(&quota, ClientError::Protocol(m) if m.contains("quota")));
        assert!(!quota.is_retryable());
        assert!(matches!(
            stream_error("boom".into()),
            ClientError::Protocol(m) if m == "boom"
        ));
        // The same text on an odd HTTP status is still a busy provider.
        assert!(matches!(
            map_http_status(400, busy, None, AuthKind::ApiKey),
            ClientError::Server { status: 400, .. }
        ));
        assert!(matches!(
            map_http_status(400, "bad tool schema", None, AuthKind::ApiKey),
            ClientError::Protocol(m) if m == "HTTP 400: bad tool schema"
        ));
        let mut out = TurnOutput::default();
        let folded = fold_events(vec![SseEvent::Error(busy.into())], &mut |_| {}, &mut out);
        assert!(
            matches!(folded, Err(ClientError::Server { status: 503, .. })),
            "{folded:?}"
        );
    }

    /// 2.10.87: native path. A saved `minimal` goes out as `low`; `off` sends none.
    #[test]
    fn responses_body_sends_a_legacy_minimal_effort_as_low() {
        let req = |effort: &str| ResponsesRequest {
            text_format: None,
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

    /// The body carries only fields xAI's Responses API reads. Ones it ignores
    /// or rejects (server-side history, sampling knobs) would cost bytes or a
    /// 400 and never help: none goes out, for any request shape.
    #[test]
    fn responses_body_sends_only_fields_xai_reads() {
        const ALLOWED: &[&str] = &[
            "model",
            "stream",
            "input",
            "tools",
            "max_turns",
            "reasoning",
            "store",
            "prompt_cache_key",
            "include",
            "text",
            "parallel_tool_calls",
        ];
        const NEVER: &[&str] = &[
            "context_management",
            "background",
            "truncation",
            "metadata",
            "logprobs",
            "top_logprobs",
            "top_p",
            "temperature",
            "min_p",
            "top_k",
            "reasoning_effort",
            "previous_response_id",
        ];
        let format = TextFormat {
            name: "pick",
            schema: r#"{"type":"object","properties":{"a":{"type":"string"}},"required":["a"],"additionalProperties":false}"#,
        };
        let req = |model: &str, image: bool, text_format: Option<TextFormat>, hosted: bool| {
            let part = if image {
                ContentPart::InputImage("data:image/png;base64,YQ==".into())
            } else {
                ContentPart::InputText("hi".into())
            };
            ResponsesRequest {
                text_format,
                model: model.into(),
                effort: Some("high".into()),
                input: vec![InputItem::Message {
                    role: "user".into(),
                    content: vec![part],
                }],
                conversation_id: "c".into(),
                tools: crate::tool_schemas(),
                hosted_search: hosted,
                call_timeout: None,
            }
        };
        let shapes = [
            req("grok-4.3", false, None, false),
            req("grok-4.3", false, None, true),
            req("grok-4.3", true, None, false),
            req("grok-4.3", false, Some(format), false),
            req("grok-4-fast-non-reasoning", false, None, true),
            req("openai/gpt-5", false, None, true),
            req("openai/gpt-5", true, Some(format), false),
            req("", false, None, false),
        ];
        for r in &shapes {
            let body = responses_body(r);
            for key in body.as_object().unwrap().keys() {
                assert!(ALLOWED.contains(&key.as_str()), "{}: sends {key}", r.model);
                assert!(!NEVER.contains(&key.as_str()), "{}: sends {key}", r.model);
            }
        }
    }

    #[test]
    fn responses_body_tools_effort_and_store_flag() {
        let plain = ResponsesRequest {
            text_format: None,
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
        // Grok reasoning comes back sealed: not stored on xAI's side, sent back
        // whole, under a cache key that pins the chat to one cache.
        assert_eq!(body["store"], false);
        assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
        assert_eq!(body["prompt_cache_key"], "c");
        assert_eq!(body["parallel_tool_calls"], true);
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
            text_format: None,
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
            text_format: None,
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
                    digest: String::new(),
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
        // The keyed body adds only the chat's cache key, and only for Grok.
        let mut keyed = keyed_compact_body(&req);
        assert_eq!(keyed["prompt_cache_key"], "c");
        keyed.as_object_mut().unwrap().remove("prompt_cache_key");
        assert_eq!(keyed, compact_body(&req));
        let other_model = CompactRequest {
            model: "gpt-5".into(),
            ..req.clone()
        };
        assert!(keyed_compact_body(&other_model)
            .get("prompt_cache_key")
            .is_none());
    }

    fn grok_req(model: &str, effort: Option<&str>) -> ResponsesRequest {
        ResponsesRequest {
            text_format: None,
            model: model.into(),
            effort: effort.map(Into::into),
            input: vec![InputItem::Message {
                role: "user".into(),
                content: vec![ContentPart::InputText("hi".into())],
            }],
            conversation_id: "chat-9".into(),
            tools: Vec::new(),
            hosted_search: false,
            call_timeout: None,
        }
    }

    const SEALED: &str = r#"{"id":"rs_1","summary":[],"type":"reasoning","status":"completed","encrypted_content":"gAAAA=="}"#;

    #[test]
    fn grok_bodies_carry_the_cache_key_and_ask_for_sealed_reasoning() {
        let body = responses_body(&grok_req("grok-4.7", Some("high"))).to_string();
        assert_eq!(
            body,
            r#"{"include":["reasoning.encrypted_content"],"input":[{"content":[{"text":"hi","type":"input_text"}],"role":"user","type":"message"}],"model":"grok-4.7","prompt_cache_key":"chat-9","reasoning":{"effort":"high"},"store":false,"stream":true,"tools":[]}"#
        );
        // A non-reasoning Grok still pins its cache and isn't stored, and asks
        // for nothing sealed.
        let body = responses_body(&grok_req("grok-4.20-0309-non-reasoning", None)).to_string();
        assert_eq!(
            body,
            r#"{"input":[{"content":[{"text":"hi","type":"input_text"}],"role":"user","type":"message"}],"model":"grok-4.20-0309-non-reasoning","prompt_cache_key":"chat-9","store":false,"stream":true,"tools":[]}"#
        );
        // Another provider's model gets neither.
        let body = responses_body(&grok_req("vendor/model", None)).to_string();
        assert_eq!(
            body,
            r#"{"input":[{"content":[{"text":"hi","type":"input_text"}],"role":"user","type":"message"}],"model":"vendor/model","stream":true,"tools":[]}"#
        );
        // No chat id, no key.
        let mut anon = grok_req("grok-4.7", None);
        anon.conversation_id = String::new();
        assert!(responses_body(&anon).get("prompt_cache_key").is_none());
    }

    #[test]
    fn effort_none_goes_out_only_to_grok_4_3() {
        let body = responses_body(&grok_req("grok-4.3", Some("none")));
        assert_eq!(body["reasoning"], json!({"effort": "none"}));
        assert_eq!(body["store"], false);
        let body = responses_body(&grok_req("grok-4.3-latest", Some("none")));
        assert_eq!(body["reasoning"], json!({"effort": "none"}));
        assert!(responses_body(&grok_req("grok-4.7", Some("none")))
            .get("reasoning")
            .is_none());
        assert!(responses_body(&grok_req("grok-4.30", Some("none")))
            .get("reasoning")
            .is_none());
        assert!(grokhub_core::accepts_effort_none("GROK-4.3"));
        assert!(!grokhub_core::accepts_effort_none("grok-4"));
    }

    /// Grok 4.3 takes every level, so the router's pick reaches it.
    #[test]
    fn grok_4_3_gets_low_to_xhigh_too() {
        for effort in ["low", "medium", "high", "xhigh"] {
            let body = responses_body(&grok_req("grok-4.3", Some(effort)));
            assert_eq!(body["reasoning"], json!({"effort": effort}), "{effort}");
        }
        // A saved legacy name still goes out as its level.
        let body = responses_body(&grok_req("grok-4.3-latest", Some("think")));
        assert_eq!(body["reasoning"], json!({"effort": "high"}));
    }

    #[test]
    fn sealed_reasoning_goes_back_only_to_the_model_that_wrote_it() {
        let sealed = InputItem::reasoning(SEALED, "grok-4.7").unwrap();
        let mut req = grok_req("grok-4.7", Some("high"));
        req.input.push(sealed.clone());
        let body = responses_body(&req);
        assert_eq!(
            body["input"][1],
            serde_json::from_str::<Value>(SEALED).unwrap()
        );
        assert_eq!(body["input"].as_array().unwrap().len(), 2);
        // The router moved the chat to another model: the item stays home.
        req.model = "GROK-4.3".into();
        assert_eq!(responses_body(&req)["input"].as_array().unwrap().len(), 1);
        req.model = "Grok-4.7".into();
        assert_eq!(responses_body(&req)["input"].as_array().unwrap().len(), 2);
        // Size estimates count every item whatever the model.
        assert_eq!(input_wire_value(&req.input).as_array().unwrap().len(), 2);
        // Not sealed, or not reasoning: not kept.
        assert!(
            InputItem::reasoning(r#"{"type":"reasoning","encrypted_content":""}"#, "m").is_none()
        );
        assert!(
            InputItem::reasoning(r#"{"type":"message","encrypted_content":"x"}"#, "m").is_none()
        );
        assert!(InputItem::reasoning("not json", "m").is_none());
        assert!(sealed.is_opaque());
    }

    #[test]
    fn hosted_search_calls_go_back_in_order_to_the_same_model_only() {
        let web = r#"{"id":"ws_1","type":"web_search_call","status":"completed"}"#;
        let x = r#"{"call_id":"xs_2","type":"custom_tool_call","name":"x_keyword_search","input":"{}"}"#;
        let mut req = grok_req("grok-4.7", Some("high"));
        req.input
            .push(InputItem::reasoning(SEALED, "grok-4.7").unwrap());
        req.input
            .push(InputItem::reasoning(web, "grok-4.7").unwrap());
        req.input.push(InputItem::reasoning(x, "grok-4.7").unwrap());
        let body = responses_body(&req);
        let sent = body["input"].as_array().unwrap();
        assert_eq!(sent.len(), 4);
        assert_eq!(sent[2], serde_json::from_str::<Value>(web).unwrap());
        assert_eq!(sent[3], serde_json::from_str::<Value>(x).unwrap());
        req.model = "grok-4.3".into();
        assert_eq!(responses_body(&req)["input"].as_array().unwrap().len(), 1);
        // A call item needs its id to be passed back.
        assert!(InputItem::reasoning(r#"{"type":"web_search_call"}"#, "m").is_none());
        assert!(InputItem::reasoning(r#"{"type":"function_call","id":"f"}"#, "m").is_none());
    }

    #[test]
    fn hosted_search_turns_are_capped_by_what_the_turn_is_for() {
        use crate::brain::{Brief, Intent};
        let conv = format!("hosted-turns-{}", std::process::id());
        let mut req = grok_req("grok-4.7", Some("low"));
        req.conversation_id = conv.clone();
        assert!(
            responses_body(&req).get("max_turns").is_none(),
            "no hosted search"
        );
        req.hosted_search = true;
        // No brief (a subagent): the middle.
        assert_eq!(responses_body(&req)["max_turns"], 3);
        for (intent, turns) in [
            (Intent::Chat, 2),
            (Intent::Question, 2),
            (Intent::Task, 3),
            (Intent::Research, 5),
        ] {
            crate::checkpoint::set_turn_brief(
                &conv,
                Brief {
                    intent,
                    ..Default::default()
                },
            );
            assert_eq!(responses_body(&req)["max_turns"], turns, "{intent:?}");
        }
    }
}
