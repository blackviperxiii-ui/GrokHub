//! OpenAI-compatible HTTP client for an on-device [`LocalRuntime`].
//!
//! Talks to llama-server (or a test fake) at a loopback base URL with a
//! bearer token. Background helpers ask for JSON; [`LocalRuntime::chat_stream`]
//! streams SSE deltas when the server sends them, else one full message.

use std::io::{BufRead, BufReader};
use std::sync::Arc;

use serde_json::{json, Value};

use super::local::{ChatMessage, LocalRuntime};
use crate::client::{CancelToken, ResponsesRequest, TurnOutput};
use crate::route::providers::{openai_body, parse_openai_reply};

/// Runtime that posts to `{base}/v1/chat/completions`.
#[derive(Clone)]
pub struct HttpLocalRuntime {
    base_url: String,
    token: String,
    model: String,
    /// How the model's template thinks, from the server's `/props`.
    thinking: super::local_effort::Thinking,
    /// Keeps the global sidecar's request count up while this runtime lives.
    busy: Option<Arc<super::local_sidecar::Busy>>,
    /// Server slots; past one, slot 0 is left to the chat on screen.
    slots: u32,
    /// Busy slots split one KV pool's context.
    unified: bool,
    /// Requests on the server this runtime talks to.
    in_flight: Option<Arc<std::sync::atomic::AtomicUsize>>,
}

impl std::fmt::Debug for HttpLocalRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpLocalRuntime")
            .field("base_url", &self.base_url)
            .field("token", &"[redacted]")
            .field("model", &self.model)
            .field("thinking", &self.thinking)
            .finish()
    }
}

impl HttpLocalRuntime {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        Self {
            base_url,
            token: token.into(),
            model: "local".into(),
            thinking: Default::default(),
            busy: None,
            slots: 1,
            unified: false,
            in_flight: None,
        }
    }

    /// The server's slots and request count, for [`Self::share_of`] and
    /// [`Self::background_slot`].
    pub fn with_slots(
        mut self,
        slots: u32,
        unified: bool,
        in_flight: Arc<std::sync::atomic::AtomicUsize>,
    ) -> Self {
        self.slots = slots.max(1);
        self.unified = unified;
        self.in_flight = Some(in_flight);
        self
    }

    /// A request guard for one call, unless this runtime already holds one
    /// for its whole life: each request counts once.
    fn call_busy(&self) -> Option<super::local_sidecar::Busy> {
        self.busy.is_none().then(super::local_sidecar::busy)
    }

    /// One busy slot's part of `n_ctx`: the whole of it, or with a unified
    /// pool an equal share among the requests running now (at most one per slot).
    fn share_of(&self, n_ctx: u64) -> u64 {
        if !self.unified || self.slots <= 1 {
            return n_ctx;
        }
        let active = self
            .in_flight
            .as_ref()
            .map_or(1, |n| n.load(std::sync::atomic::Ordering::SeqCst))
            .clamp(1, self.slots as usize);
        n_ctx / active as u64
    }

    /// The slot a background call takes: never slot 0, which stays free (and
    /// keeps its cached prefix) for the chat on screen; `None` with one slot.
    fn background_slot(&self) -> Option<u32> {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        (self.slots > 1).then(|| 1 + NEXT.fetch_add(1, Ordering::Relaxed) % (self.slots - 1))
    }

    /// What `/props` said about thinking (see [`super::local_effort`]).
    pub fn with_thinking(mut self, thinking: super::local_effort::Thinking) -> Self {
        self.thinking = thinking;
        self
    }

    /// What to send for the turn's effort: a level the model renders, and
    /// thinking on or off where its template has the switch.
    fn effort(&self, want: Option<&str>) -> super::local_effort::Sent {
        self.thinking.plan(want)
    }

    /// Reject non-loopback bases so a mis-set URL never leaves the device.
    pub fn loopback_ok(&self) -> bool {
        loopback_base(&self.base_url)
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// A throttled model (sharing the GPU with a game, or on battery) can take
    /// minutes over a long answer; Stop and the router's own call timeout still
    /// end a turn, so the socket read waits well past the old two minutes.
    pub const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

    fn agent() -> grokhub_core::http::Agent {
        grokhub_core::http::agent(grokhub_core::http::Opts {
            connect: std::time::Duration::from_secs(5),
            read: Some(Self::READ_TIMEOUT),
            ..Default::default()
        })
    }

    fn post_chat(
        &self,
        messages: &[ChatMessage],
        stream: bool,
        effort: Option<&str>,
        slot: Option<u32>,
        lean: Option<&Lean>,
    ) -> Result<grokhub_core::http::Response, String> {
        if !self.loopback_ok() {
            return Err("local model URL must be loopback (127.0.0.1 / localhost)".into());
        }
        super::local_sidecar::touch_global();
        let mut body = chat_body(&self.model, messages, stream, &self.effort(effort));
        if let Some(id) = slot {
            body["id_slot"] = json!(id);
        }
        if let Some(lean) = lean {
            lean.apply(&mut body);
        }
        Self::agent()
            .post(&format!("{}/v1/chat/completions", self.base_url))
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("Content-Type", "application/json")
            .send_json(body)
            .map_err(request_failed)
    }

    fn chat_once(
        &self,
        messages: &[ChatMessage],
        slot: Option<u32>,
        lean: Option<&Lean>,
    ) -> Result<String, String> {
        let _busy = self.call_busy();
        let resp = self.post_chat(messages, false, None, slot, lean)?;
        if resp.status() >= 400 {
            let status = resp.status();
            let body = resp.into_string().unwrap_or_default();
            return Err(format!("local model HTTP {status}: {body}"));
        }
        let v: Value = resp
            .into_json()
            .map_err(|e| format!("local model JSON: {e}"))?;
        note_timings(&v);
        content_from_completion(&v).ok_or_else(|| "local model returned no content".into())
    }

    fn ask_json(&self, system: &str, user: &str, lean: &Lean) -> Result<Value, String> {
        let messages = [
            ChatMessage {
                role: "system".into(),
                content: system.into(),
            },
            ChatMessage {
                role: "user".into(),
                content: user.into(),
            },
        ];
        let text = self.chat_once(&messages, self.background_slot(), Some(lean))?;
        parse_json_object(&text).ok_or_else(|| format!("local model did not return JSON: {text}"))
    }
}

/// What a background call asks the server for: no thinking, a short reply,
/// and JSON in one shape only (the server turns the schema into a grammar),
/// so a label costs a few tokens instead of a thought-out answer.
struct Lean {
    max_tokens: u32,
    schema: Value,
}

/// Room for a JSON label and score, or a difficulty.
const LABEL_TOKENS: u32 = 48;

impl Lean {
    fn classify(labels: &[&str]) -> Self {
        Lean {
            max_tokens: LABEL_TOKENS,
            schema: json!({
                "type": "object",
                "properties": {
                    "label": { "type": "string", "enum": labels },
                    "score": { "type": "number" },
                },
                "required": ["label", "score"],
                "additionalProperties": false,
            }),
        }
    }

    /// About two tokens a word, plus the JSON around it.
    fn summarize(max_words: u32) -> Self {
        Lean {
            max_tokens: max_words.saturating_mul(2).saturating_add(32),
            schema: json!({
                "type": "object",
                "properties": { "summary": { "type": "string" } },
                "required": ["summary"],
                "additionalProperties": false,
            }),
        }
    }

    fn difficulty() -> Self {
        Lean {
            max_tokens: LABEL_TOKENS,
            schema: json!({
                "type": "object",
                "properties": { "d": { "type": "number" } },
                "required": ["d"],
                "additionalProperties": false,
            }),
        }
    }

    fn apply(&self, body: &mut Value) {
        let Some(o) = body.as_object_mut() else {
            return;
        };
        o.remove("reasoning_effort");
        o.insert("max_tokens".into(), json!(self.max_tokens));
        o.insert(
            "response_format".into(),
            json!({
                "type": "json_schema",
                "json_schema": { "name": "reply", "strict": true, "schema": self.schema },
            }),
        );
        let kwargs = o.entry("chat_template_kwargs").or_insert_with(|| json!({}));
        if let Some(k) = kwargs.as_object_mut() {
            k.insert("enable_thinking".into(), json!(false));
        }
    }
}

/// How long a `/props` answer (or its absence) is trusted before asking again.
const PROPS_TTL: std::time::Duration = std::time::Duration::from_secs(60);

type PropsCache =
    std::sync::Mutex<std::collections::HashMap<String, (Option<u64>, std::time::Instant)>>;

fn props_cache() -> &'static PropsCache {
    static CACHE: std::sync::OnceLock<PropsCache> = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// `n_ctx` from a llama-server `/props` body: the per-slot window.
fn n_ctx_from_props(v: &Value) -> Option<u64> {
    v.pointer("/default_generation_settings/n_ctx")
        .or_else(|| v.get("n_ctx"))
        .and_then(Value::as_u64)
        .filter(|&n| n >= 256)
}

impl HttpLocalRuntime {
    fn fetch_n_ctx(&self) -> Option<u64> {
        if !self.loopback_ok() {
            return None;
        }
        let agent = grokhub_core::http::agent(grokhub_core::http::Opts {
            connect: std::time::Duration::from_secs(2),
            read: Some(std::time::Duration::from_secs(2)),
            ..Default::default()
        });
        let v: Value = agent
            .get(&format!("{}/props", self.base_url))
            .header("Authorization", &format!("Bearer {}", self.token))
            .call()
            .ok()?
            .into_json()
            .ok()?;
        n_ctx_from_props(&v)
    }
}

impl LocalRuntime for HttpLocalRuntime {
    fn classify(&self, text: &str, labels: &[&str]) -> Result<Value, String> {
        let labels_csv = labels.join(", ");
        let system = format!(
            "Reply with one JSON object only: {{\"label\": \"<one of: {labels_csv}>\", \"score\": <0.0-1.0>}}. No markdown."
        );
        self.ask_json(&system, text, &Lean::classify(labels))
    }

    fn summarize(&self, text: &str, max_words: u32) -> Result<Value, String> {
        let system = format!(
            "Reply with one JSON object only: {{\"summary\": \"...\"}} of at most {max_words} words. No markdown."
        );
        self.ask_json(&system, text, &Lean::summarize(max_words))
    }

    fn difficulty(&self, text: &str) -> Result<Value, String> {
        let system =
            "Reply with one JSON object only: {\"d\": <0.0-1.0>} for task difficulty. No markdown.";
        self.ask_json(system, text, &Lean::difficulty())
    }

    fn chat_stream(
        &self,
        messages: &[ChatMessage],
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        self.chat_stream_at(messages, None, on_delta)
    }

    fn chat_stream_at(
        &self,
        messages: &[ChatMessage],
        effort: Option<&str>,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        let _busy = self.call_busy();
        let resp = self.post_chat(messages, true, effort, None, None)?;
        if resp.status() >= 400 {
            let status = resp.status();
            let body = resp.into_string().unwrap_or_default();
            return Err(format!("local model HTTP {status}: {body}"));
        }
        let ctype = resp.content_type().to_string();
        if ctype.contains("text/event-stream") || ctype.contains("stream") {
            return read_sse(resp, on_delta);
        }
        // Non-streaming servers (and the fake): one JSON body.
        let v: Value = resp
            .into_json()
            .map_err(|e| format!("local model JSON: {e}"))?;
        note_chat_timings(&v);
        let text = content_from_completion(&v)
            .ok_or_else(|| "local model returned no content".to_string())?;
        if !text.is_empty() {
            on_delta(&text);
        }
        Ok(text)
    }

    fn context_tokens(&self) -> Option<u64> {
        let now = std::time::Instant::now();
        if let Some((n, at)) = props_cache()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&self.base_url)
        {
            if now.duration_since(*at) < PROPS_TTL {
                return n.map(|n| self.share_of(n));
            }
        }
        let n = self.fetch_n_ctx();
        props_cache()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(self.base_url.clone(), (n, now));
        n.map(|n| self.share_of(n))
    }

    fn chat_tools(
        &self,
        req: &ResponsesRequest,
        cancel: &CancelToken,
    ) -> Result<TurnOutput, String> {
        if cancel.is_cancelled() {
            return Err("cancelled".into());
        }
        if !self.loopback_ok() {
            return Err("local model URL must be loopback (127.0.0.1 / localhost)".into());
        }
        let _busy = self.call_busy();
        super::local_sidecar::touch_global();
        // Streamed, so Stop (and a game starting) can end the turn between
        // tokens: dropping the stream closes the socket and the server stops
        // generating. The deltas are put back together into one reply.
        let mut body = openai_body(&self.model, None, req);
        body["stream"] = json!(true);
        body["stream_options"] = json!({ "include_usage": true });
        self.effort(req.effort.as_deref()).apply(&mut body);
        // Stop also hangs up while the server is still reading the prompt,
        // before any token, so the GPU goes back to the game at once.
        let stop = cancel.stop_switch();
        let sent = grokhub_core::http::with_stop(&stop, || {
            Self::agent()
                .post(&format!("{}/v1/chat/completions", self.base_url))
                .header("Authorization", &format!("Bearer {}", self.token))
                .header("Content-Type", "application/json")
                .send_json(body)
        });
        if sent.is_err() && cancel.is_cancelled() {
            return Err("cancelled".into());
        }
        let resp = sent.map_err(request_failed)?;
        if resp.status() >= 400 {
            let status = resp.status();
            let body = resp.into_string().unwrap_or_default();
            return Err(format!("local model HTTP {status}: {body}"));
        }
        let text = if resp.content_type().contains("event-stream") {
            let body = grokhub_core::http::with_stop(&stop, || resp.into_reader());
            let reply = assemble_sse(BufReader::new(body), &|| cancel.is_cancelled())?;
            note_chat_timings(&reply);
            reply.to_string()
        } else {
            // A server (or the test fake) that answers in one body.
            resp.into_string()
                .map_err(|e| format!("local model read: {e}"))?
        };
        parse_openai_reply(&text).map_err(|e| e.to_string())
    }
}

/// A failed request's message; a game cutting the server says so.
fn request_failed(e: impl std::fmt::Display) -> String {
    match super::local_sidecar::cut_reason() {
        Some(why) => why.to_string(),
        None => format!("local model request failed: {e}"),
    }
}

/// One streamed tool call being put back together.
#[derive(Default)]
struct CallParts {
    id: String,
    name: String,
    arguments: String,
}

/// Put a streamed `/v1/chat/completions` reply back into the one-body shape
/// [`parse_openai_reply`] reads: content, reasoning and tool calls joined
/// from their deltas (a call's arguments arrive in pieces, keyed by
/// `index`), plus the last chunk's usage and timings. `cancelled` is asked
/// between chunks; returning stops reading, which closes the stream.
fn assemble_sse(reader: impl BufRead, cancelled: &dyn Fn() -> bool) -> Result<Value, String> {
    let mut content = String::new();
    let mut reasoning = String::new();
    let mut calls: Vec<CallParts> = Vec::new();
    let mut finish = Value::Null;
    let mut usage = Value::Null;
    let mut timings = Value::Null;
    for line in reader.lines() {
        if cancelled() {
            return Err("cancelled".into());
        }
        let line = line.map_err(request_failed)?;
        let Some(data) = line.strip_prefix("data:").map(str::trim) else {
            continue;
        };
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        if let Some(e) = v.get("error") {
            return Err(format!("local model error: {e}"));
        }
        if !v["usage"].is_null() {
            usage = v["usage"].clone();
        }
        if !v["timings"].is_null() {
            timings = v["timings"].clone();
        }
        let choice = &v["choices"][0];
        if !choice["finish_reason"].is_null() {
            finish = choice["finish_reason"].clone();
        }
        let delta = &choice["delta"];
        if let Some(t) = delta["content"].as_str() {
            content.push_str(t);
        }
        if let Some(t) = delta["reasoning_content"].as_str() {
            reasoning.push_str(t);
        }
        for c in delta["tool_calls"].as_array().into_iter().flatten() {
            let at = c["index"]
                .as_u64()
                .and_then(|i| usize::try_from(i).ok())
                .unwrap_or(calls.len());
            if calls.len() <= at {
                calls.resize_with(at + 1, CallParts::default);
            }
            let part = &mut calls[at];
            if let Some(id) = c["id"].as_str().filter(|i| !i.is_empty()) {
                part.id = id.to_string();
            }
            if let Some(n) = c["function"]["name"].as_str() {
                part.name.push_str(n);
            }
            if let Some(a) = c["function"]["arguments"].as_str() {
                part.arguments.push_str(a);
            }
        }
    }
    if cancelled() {
        return Err("cancelled".into());
    }
    let calls: Vec<Value> = calls
        .into_iter()
        .filter(|c| !c.name.is_empty())
        .map(|c| {
            json!({
                "id": c.id,
                "type": "function",
                "function": { "name": c.name, "arguments": c.arguments },
            })
        })
        .collect();
    if content.is_empty() && reasoning.is_empty() && calls.is_empty() {
        return Err("local model stream ended with no content".into());
    }
    let mut message = json!({ "role": "assistant", "content": content });
    if !reasoning.is_empty() {
        message["reasoning_content"] = json!(reasoning);
    }
    if !calls.is_empty() {
        message["tool_calls"] = json!(calls);
    }
    let mut reply = json!({
        "choices": [{ "index": 0, "message": message, "finish_reason": finish }],
        "usage": usage,
    });
    if !timings.is_null() {
        reply["timings"] = timings;
    }
    Ok(reply)
}

/// A `/v1/chat/completions` body with what [`super::local_effort::Thinking::plan`]
/// sends for the turn's effort (`reasoning_effort`, `chat_template_kwargs`).
fn chat_body(
    model: &str,
    messages: &[ChatMessage],
    stream: bool,
    effort: &super::local_effort::Sent,
) -> Value {
    let mut body = json!({
        "model": model,
        "stream": stream,
        "messages": messages.iter().map(|m| json!({"role": m.role, "content": m.content})).collect::<Vec<_>>(),
    });
    effort.apply(&mut body);
    body
}

fn loopback_base(base: &str) -> bool {
    let rest = base
        .strip_prefix("http://")
        .or_else(|| base.strip_prefix("https://"))
        .unwrap_or(base);
    let host = rest.split(['/', ':']).next().unwrap_or("");
    super::local_sidecar::is_loopback_host(host)
}

fn content_from_completion(v: &Value) -> Option<String> {
    v.get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            v.get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("delta"))
                .and_then(|d| d.get("content"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

fn parse_json_object(text: &str) -> Option<Value> {
    let t = text.trim();
    if let Ok(v) = serde_json::from_str::<Value>(t) {
        if v.is_object() {
            return Some(v);
        }
    }
    let start = t.find('{')?;
    let end = t.rfind('}')?;
    serde_json::from_str(&t[start..=end]).ok()
}

fn read_sse(
    resp: grokhub_core::http::Response,
    on_delta: &mut dyn FnMut(&str),
) -> Result<String, String> {
    let reader = BufReader::new(resp.into_reader());
    let mut full = String::new();
    // llama-server puts the reply's timings on its last chunk.
    let mut timings = None;
    for line in reader.lines() {
        let line = line.map_err(|e| format!("local model stream: {e}"))?;
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        if v.get("timings").is_some() {
            timings = Some(v.clone());
        }
        if let Some(piece) = v
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("delta"))
            .and_then(|d| d.get("content"))
            .and_then(Value::as_str)
        {
            if !piece.is_empty() {
                on_delta(piece);
                full.push_str(piece);
            }
        }
    }
    if let Some(v) = timings {
        note_chat_timings(&v);
    }
    if full.is_empty() {
        return Err("local model stream ended with no content".into());
    }
    Ok(full)
}

/// Count a finished reply toward the live draft check.
fn note_timings(v: &Value) {
    if let Some(t) = grokhub_core::local_draft::timings_of(v) {
        super::local_spec::note_reply(&t);
    }
}

/// [`note_timings`] for a chat reply, which also scores the auto-tune arm
/// the server runs (a background label is too short to say much).
fn note_chat_timings(v: &Value) {
    note_timings(v);
    if let Some((ms, prompt_n, cache_n)) = super::local_warm::take_first_reply(v) {
        let detail = super::local_warm::first_reply_detail(prompt_n, cache_n);
        super::local_health::log_ms("first_reply", Some(&detail), Some(ms));
    }
    let t = &v["timings"];
    if let Some(tok_s) = t["predicted_per_second"].as_f64() {
        super::local_tune::observe(
            tok_s,
            t["prompt_per_second"].as_f64().unwrap_or(0.0),
            t["predicted_n"].as_u64().unwrap_or(0),
        );
    }
}

/// Build a runtime from a running sidecar endpoint, if any: the server the
/// request was admitted to, so a warm relaunch never moves it mid-call.
pub fn from_global_sidecar(busy: super::local_sidecar::Busy) -> Option<Arc<dyn LocalRuntime>> {
    let end = match busy.endpoint() {
        Some(end) => end.clone(),
        None => super::local_sidecar::global_endpoint()?,
    };
    let mut rt = HttpLocalRuntime::new(end.base_url, end.token)
        .with_thinking(end.thinking)
        .with_slots(end.slots, end.unified, end.in_flight);
    rt.busy = Some(Arc::new(busy));
    Some(Arc::new(rt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::local_sidecar::fake_server::FakeServer;

    fn sse(lines: &[&str]) -> std::io::Cursor<Vec<u8>> {
        let text: String = lines.iter().map(|l| format!("data: {l}\n\n")).collect();
        std::io::Cursor::new(text.into_bytes())
    }

    #[test]
    fn a_streamed_tool_turn_is_put_back_together_from_its_deltas() {
        let stream = sse(&[
            r#"{"choices":[{"delta":{"reasoning_content":"look "}}]}"#,
            r#"{"choices":[{"delta":{"reasoning_content":"it up"}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c1","function":{"name":"read_file","arguments":"{\"pa"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"a.rs\"}"}}]}}]}"#,
            r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"id":"c2","function":{"name":"ls","arguments":"{}"}}]}}]}"#,
            r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":12,"completion_tokens":7}}"#,
            "[DONE]",
        ]);
        let reply = assemble_sse(stream, &|| false).unwrap();
        let msg = &reply["choices"][0]["message"];
        assert_eq!(msg["reasoning_content"], "look it up");
        assert_eq!(msg["tool_calls"][0]["id"], "c1");
        assert_eq!(msg["tool_calls"][0]["function"]["name"], "read_file");
        assert_eq!(
            msg["tool_calls"][0]["function"]["arguments"],
            r#"{"path":"a.rs"}"#
        );
        assert_eq!(msg["tool_calls"][1]["function"]["name"], "ls");
        assert_eq!(reply["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(reply["usage"]["prompt_tokens"], 12);
        let out = parse_openai_reply(&reply.to_string()).unwrap();
        assert_eq!(out.calls.len(), 2);
        assert_eq!(out.calls[0].arguments, r#"{"path":"a.rs"}"#);
        assert_eq!(out.reasoning, "look it up");
    }

    #[test]
    fn a_streamed_turn_stops_reading_once_cancelled_and_an_empty_one_errs() {
        let stream = sse(&[r#"{"choices":[{"delta":{"content":"hi"}}]}"#]);
        assert_eq!(
            assemble_sse(stream, &|| true).err(),
            Some("cancelled".into())
        );
        let empty = sse(&["[DONE]"]);
        assert_eq!(
            assemble_sse(empty, &|| false).err(),
            Some("local model stream ended with no content".into())
        );
        let text = sse(&[
            r#"{"choices":[{"delta":{"content":"he"}}]}"#,
            r#"{"choices":[{"delta":{"content":"llo"}}]}"#,
        ]);
        let reply = assemble_sse(text, &|| false).unwrap();
        assert_eq!(reply["choices"][0]["message"]["content"], "hello");
        assert!(reply["choices"][0]["message"].get("tool_calls").is_none());
    }

    /// Stop while the local server still reads a long prompt (no token
    /// yet): the turn ends at once and the socket closes, so the server
    /// stops and the GPU goes back to the game.
    #[test]
    fn stop_during_a_long_prompt_hangs_up_the_local_server() {
        use std::io::Read;
        use std::time::{Duration, Instant};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            c.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut buf = [0u8; 4096];
            // Read the request and keep reading: a hang-up reads as EOF.
            loop {
                match c.read(&mut buf) {
                    Ok(0) => return true,
                    Ok(_) => {}
                    Err(e) => return e.kind() == std::io::ErrorKind::ConnectionReset,
                }
            }
        });
        let rt = HttpLocalRuntime::new(&base, "t");
        let cancel = CancelToken::new();
        let pressed = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            pressed.cancel();
        });
        let req = ResponsesRequest {
            model: "local".into(),
            effort: None,
            input: Vec::new(),
            conversation_id: "c".into(),
            tools: Vec::new(),
            hosted_search: false,
            call_timeout: None,
            text_format: None,
        };
        let started = Instant::now();
        let out = rt.chat_tools(&req, &cancel);
        assert_eq!(out.err(), Some("cancelled".into()));
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        assert!(server.join().unwrap(), "the server saw the hang-up");
    }

    #[test]
    fn http_runtime_reads_json_helpers_from_fake() {
        let fake = FakeServer::start(None);
        let rt = HttpLocalRuntime::new(fake.base_url(), fake.token.clone());
        let d = rt.difficulty("hard bug").expect("d");
        assert_eq!(d["d"], 0.5);
    }

    #[test]
    fn the_device_window_comes_from_props() {
        let fake = FakeServer::start(None);
        let rt = HttpLocalRuntime::new(fake.base_url(), fake.token.clone());
        assert_eq!(rt.context_tokens(), Some(8_192));
        let wrong = HttpLocalRuntime::new(fake.base_url(), "not-the-token");
        assert_eq!(wrong.fetch_n_ctx(), None);
    }

    /// Answers `n` chat requests with a JSON object and hands back each body.
    fn capture_server(n: usize) -> (String, std::thread::JoinHandle<Vec<Value>>) {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let h = std::thread::spawn(move || {
            let mut bodies = Vec::new();
            for _ in 0..n {
                let (mut c, _) = l.accept().unwrap();
                let mut r = BufReader::new(c.try_clone().unwrap());
                let mut len = 0usize;
                loop {
                    let mut line = String::new();
                    r.read_line(&mut line).unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0u8; len];
                r.read_exact(&mut body).unwrap();
                bodies.push(serde_json::from_slice(&body).unwrap());
                let reply = r#"{"choices":[{"message":{"content":"{\"ok\":true}"}}]}"#;
                write!(
                    c,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                )
                .unwrap();
            }
            bodies
        });
        (base, h)
    }

    #[test]
    fn background_calls_never_take_the_chats_slot_and_one_slot_pins_nothing() {
        let (base, server) = capture_server(5);
        let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let rt = HttpLocalRuntime::new(&base, "t").with_slots(3, true, n.clone());
        for _ in 0..4 {
            assert_eq!(
                rt.ask_json("s", "u", &Lean::difficulty()).unwrap(),
                json!({"ok": true})
            );
        }
        let single = HttpLocalRuntime::new(&base, "t").with_slots(1, false, n);
        single.ask_json("s", "u", &Lean::difficulty()).unwrap();
        let bodies = server.join().unwrap();
        let slots: Vec<u64> = bodies[..4]
            .iter()
            .map(|b| b["id_slot"].as_u64().unwrap())
            .collect();
        assert!(slots.iter().all(|s| *s == 1 || *s == 2), "{slots:?}");
        assert!(slots.contains(&1) && slots.contains(&2), "{slots:?}");
        assert_eq!(bodies[4].get("id_slot"), None);
    }

    #[test]
    fn background_calls_think_not_answer_short_and_only_in_their_json_shape() {
        let (base, server) = capture_server(3);
        let rt = HttpLocalRuntime::new(&base, "t");
        rt.classify("hello", &["routine", "sensitive"]).unwrap();
        rt.summarize("long text", 40).unwrap();
        rt.difficulty("hard bug").unwrap();
        let bodies = server.join().unwrap();
        for b in &bodies {
            assert_eq!(b["chat_template_kwargs"]["enable_thinking"], json!(false));
            assert_eq!(b["stream"], json!(false));
            assert_eq!(b.get("reasoning_effort"), None);
            assert_eq!(b["response_format"]["type"], json!("json_schema"));
        }
        assert_eq!(bodies[0]["max_tokens"], json!(48));
        assert_eq!(
            bodies[0]["response_format"]["json_schema"]["schema"]["properties"]["label"]["enum"],
            json!(["routine", "sensitive"])
        );
        assert_eq!(bodies[1]["max_tokens"], json!(112));
        assert_eq!(
            bodies[1]["response_format"]["json_schema"]["schema"]["required"],
            json!(["summary"])
        );
        assert_eq!(bodies[2]["max_tokens"], json!(48));
        assert_eq!(
            bodies[2]["response_format"]["json_schema"]["schema"]["required"],
            json!(["d"])
        );
    }

    #[test]
    fn busy_slots_in_one_pool_split_its_context_and_separate_slots_keep_theirs() {
        let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let rt = HttpLocalRuntime::new("http://127.0.0.1:9", "t").with_slots(4, true, n.clone());
        assert_eq!(rt.share_of(32_768), 32_768, "nothing running yet");
        n.store(1, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(rt.share_of(32_768), 32_768);
        n.store(2, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(rt.share_of(32_768), 16_384);
        n.store(9, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(rt.share_of(32_768), 8_192, "at most one request per slot");
        let own = HttpLocalRuntime::new("http://127.0.0.1:9", "t").with_slots(4, false, n.clone());
        assert_eq!(own.share_of(8_192), 8_192);
        let plain = HttpLocalRuntime::new("http://127.0.0.1:9", "t");
        assert_eq!(plain.share_of(8_192), 8_192);
    }

    #[test]
    fn n_ctx_reads_both_props_shapes_and_ignores_nonsense() {
        let v = |s: &str| serde_json::from_str::<Value>(s).unwrap();
        assert_eq!(
            n_ctx_from_props(&v(r#"{"default_generation_settings":{"n_ctx":4096}}"#)),
            Some(4_096)
        );
        assert_eq!(n_ctx_from_props(&v(r#"{"n_ctx":16384}"#)), Some(16_384));
        assert_eq!(n_ctx_from_props(&v(r#"{"n_ctx":0}"#)), None);
        assert_eq!(n_ctx_from_props(&v(r#"{"model":"x"}"#)), None);
    }

    #[test]
    fn the_turns_effort_goes_out_only_as_a_level_the_model_renders() {
        let user = [ChatMessage {
            role: "user".into(),
            content: "hi".into(),
        }];
        let rt = HttpLocalRuntime::new("http://127.0.0.1:9", "t").with_thinking(
            super::super::local_effort::Thinking {
                levels: vec!["low".into(), "high".into()],
                switch: None,
            },
        );
        let sent = |want| chat_body("local", &user, true, &rt.effort(want));
        assert_eq!(
            sent(Some("xhigh")),
            json!({
                "model": "local",
                "stream": true,
                "messages": [{"role": "user", "content": "hi"}],
                "reasoning_effort": "high"
            })
        );
        // The template's own default (between low and high), and no effort.
        assert_eq!(sent(Some("medium")).get("reasoning_effort"), None);
        assert_eq!(sent(None).get("reasoning_effort"), None);
        // A model whose template renders no level never gets one.
        let plain = HttpLocalRuntime::new("http://127.0.0.1:9", "t");
        assert_eq!(plain.effort(Some("high")).effort, None);
    }

    #[test]
    fn a_fallback_turn_at_no_effort_turns_a_template_switch_off() {
        let user = [ChatMessage {
            role: "user".into(),
            content: "hi".into(),
        }];
        let rt = HttpLocalRuntime::new("http://127.0.0.1:9", "t").with_thinking(
            super::super::local_effort::Thinking {
                levels: Vec::new(),
                switch: Some("enable_thinking"),
            },
        );
        assert_eq!(
            chat_body("local", &user, false, &rt.effort(Some("none"))),
            json!({
                "model": "local",
                "stream": false,
                "messages": [{"role": "user", "content": "hi"}],
                "chat_template_kwargs": {"enable_thinking": false}
            })
        );
        assert_eq!(
            chat_body("local", &user, false, &rt.effort(Some("high")))["chat_template_kwargs"],
            json!({"enable_thinking": true})
        );
    }

    #[test]
    fn effort_levels_are_read_from_props_with_the_api_key() {
        let fake = FakeServer::start(None);
        assert_eq!(
            super::super::local_effort::fetch_thinking(
                &fake.host,
                fake.port,
                &fake.token,
                super::super::local_effort::PROPS_TIMEOUT
            )
            .levels,
            ["low", "high"]
        );
        assert_eq!(
            super::super::local_effort::fetch_thinking(
                &fake.host,
                fake.port,
                "sk-abcdefghijklmnopqrstuv",
                super::super::local_effort::PROPS_TIMEOUT
            ),
            super::super::local_effort::Thinking::default()
        );
    }

    #[test]
    fn http_runtime_debug_redacts_token() {
        let dbg = format!(
            "{:?}",
            HttpLocalRuntime::new("http://127.0.0.1:9", "super-secret-token")
        );
        assert!(dbg.contains("[redacted]"), "{dbg}");
        assert!(!dbg.contains("super-secret-token"), "{dbg}");
    }
}
