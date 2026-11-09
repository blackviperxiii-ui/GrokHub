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
        }
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
    ) -> Result<grokhub_core::http::Response, String> {
        if !self.loopback_ok() {
            return Err("local model URL must be loopback (127.0.0.1 / localhost)".into());
        }
        super::local_sidecar::touch_global();
        let body = chat_body(&self.model, messages, stream, &self.effort(effort));
        Self::agent()
            .post(&format!("{}/v1/chat/completions", self.base_url))
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("Content-Type", "application/json")
            .send_json(body)
            .map_err(request_failed)
    }

    fn chat_once(&self, messages: &[ChatMessage]) -> Result<String, String> {
        let _busy = super::local_sidecar::busy();
        let resp = self.post_chat(messages, false, None)?;
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

    fn ask_json(&self, system: &str, user: &str) -> Result<Value, String> {
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
        let text = self.chat_once(&messages)?;
        parse_json_object(&text).ok_or_else(|| format!("local model did not return JSON: {text}"))
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
        self.ask_json(&system, text)
    }

    fn summarize(&self, text: &str, max_words: u32) -> Result<Value, String> {
        let system = format!(
            "Reply with one JSON object only: {{\"summary\": \"...\"}} of at most {max_words} words. No markdown."
        );
        self.ask_json(&system, text)
    }

    fn difficulty(&self, text: &str) -> Result<Value, String> {
        let system =
            "Reply with one JSON object only: {\"d\": <0.0-1.0>} for task difficulty. No markdown.";
        self.ask_json(system, text)
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
        let _busy = super::local_sidecar::busy();
        let resp = self.post_chat(messages, true, effort)?;
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
        note_timings(&v);
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
                return *n;
            }
        }
        let n = self.fetch_n_ctx();
        props_cache()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(self.base_url.clone(), (n, now));
        n
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
        let _busy = super::local_sidecar::busy();
        super::local_sidecar::touch_global();
        // Streamed, so Stop (and a game starting) can end the turn between
        // tokens: dropping the stream closes the socket and the server stops
        // generating. The deltas are put back together into one reply.
        let mut body = openai_body(&self.model, None, req);
        body["stream"] = json!(true);
        body["stream_options"] = json!({ "include_usage": true });
        self.effort(req.effort.as_deref()).apply(&mut body);
        let resp = Self::agent()
            .post(&format!("{}/v1/chat/completions", self.base_url))
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("Content-Type", "application/json")
            .send_json(body)
            .map_err(request_failed)?;
        if resp.status() >= 400 {
            let status = resp.status();
            let body = resp.into_string().unwrap_or_default();
            return Err(format!("local model HTTP {status}: {body}"));
        }
        let text = if resp.content_type().contains("event-stream") {
            let reply = assemble_sse(BufReader::new(resp.into_reader()), &|| {
                cancel.is_cancelled()
            })?;
            note_timings(&reply);
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
        note_timings(&v);
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

/// Build a runtime from a running sidecar endpoint, if any: the server the
/// request was admitted to, so a warm relaunch never moves it mid-call.
pub fn from_global_sidecar(busy: super::local_sidecar::Busy) -> Option<Arc<dyn LocalRuntime>> {
    let end = match busy.endpoint() {
        Some(end) => end.clone(),
        None => super::local_sidecar::global_endpoint()?,
    };
    let mut rt = HttpLocalRuntime::new(end.base_url, end.token).with_thinking(end.thinking);
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
