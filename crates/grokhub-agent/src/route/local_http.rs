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
    /// The `reasoning_effort` levels the server's `/props` listed.
    effort_levels: Vec<String>,
    /// Keeps the global sidecar's request count up while this runtime lives.
    busy: Option<Arc<super::local_sidecar::Busy>>,
}

impl std::fmt::Debug for HttpLocalRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpLocalRuntime")
            .field("base_url", &self.base_url)
            .field("token", &"[redacted]")
            .field("model", &self.model)
            .field("effort_levels", &self.effort_levels)
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
            effort_levels: Vec::new(),
            busy: None,
        }
    }

    /// The levels `/props` listed (see [`super::local_effort`]).
    pub fn with_effort_levels(mut self, levels: Vec<String>) -> Self {
        self.effort_levels = levels;
        self
    }

    /// The level to send for the turn's effort, if the model renders one.
    fn effort(&self, want: Option<&str>) -> Option<String> {
        super::local_effort::nearest(want?, &self.effort_levels)
    }

    /// Reject non-loopback bases so a mis-set URL never leaves the device.
    pub fn loopback_ok(&self) -> bool {
        loopback_base(&self.base_url)
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    fn agent() -> ureq::Agent {
        ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(5))
            .timeout_read(std::time::Duration::from_secs(120))
            .build()
    }

    fn post_chat(
        &self,
        messages: &[ChatMessage],
        stream: bool,
        effort: Option<&str>,
    ) -> Result<ureq::Response, String> {
        if !self.loopback_ok() {
            return Err("local model URL must be loopback (127.0.0.1 / localhost)".into());
        }
        super::local_sidecar::touch_global();
        let body = chat_body(
            &self.model,
            messages,
            stream,
            self.effort(effort).as_deref(),
        );
        Self::agent()
            .post(&format!("{}/v1/chat/completions", self.base_url))
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| format!("local model request failed: {e}"))
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
        let text = content_from_completion(&v)
            .ok_or_else(|| "local model returned no content".to_string())?;
        if !text.is_empty() {
            on_delta(&text);
        }
        Ok(text)
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
        // Non-streaming: tool_calls are unreliable on SSE for llama-server.
        let mut body = openai_body(&self.model, None, req);
        body["stream"] = json!(false);
        if let Some(e) = self.effort(req.effort.as_deref()) {
            body["reasoning_effort"] = json!(e);
        }
        let resp = Self::agent()
            .post(&format!("{}/v1/chat/completions", self.base_url))
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(|e| format!("local model request failed: {e}"))?;
        if resp.status() >= 400 {
            let status = resp.status();
            let body = resp.into_string().unwrap_or_default();
            return Err(format!("local model HTTP {status}: {body}"));
        }
        let text = resp
            .into_string()
            .map_err(|e| format!("local model read: {e}"))?;
        parse_openai_reply(&text).map_err(|e| e.to_string())
    }
}

/// A `/v1/chat/completions` body. `effort` is a level the model's template
/// renders (llama-server's `reasoning_effort`); `None` leaves it out.
fn chat_body(model: &str, messages: &[ChatMessage], stream: bool, effort: Option<&str>) -> Value {
    let mut body = json!({
        "model": model,
        "stream": stream,
        "messages": messages.iter().map(|m| json!({"role": m.role, "content": m.content})).collect::<Vec<_>>(),
    });
    if let Some(e) = effort {
        body["reasoning_effort"] = json!(e);
    }
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

fn read_sse(resp: ureq::Response, on_delta: &mut dyn FnMut(&str)) -> Result<String, String> {
    let reader = BufReader::new(resp.into_reader());
    let mut full = String::new();
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
    if full.is_empty() {
        return Err("local model stream ended with no content".into());
    }
    Ok(full)
}

/// Build a runtime from a running sidecar endpoint, if any.
pub fn from_global_sidecar(busy: super::local_sidecar::Busy) -> Option<Arc<dyn LocalRuntime>> {
    let end = super::local_sidecar::global_endpoint()?;
    let mut rt =
        HttpLocalRuntime::new(end.base_url, end.token).with_effort_levels(end.effort_levels);
    rt.busy = Some(Arc::new(busy));
    Some(Arc::new(rt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::local_sidecar::fake_server::FakeServer;

    #[test]
    fn http_runtime_reads_json_helpers_from_fake() {
        let fake = FakeServer::start(None);
        let rt = HttpLocalRuntime::new(fake.base_url(), fake.token.clone());
        let d = rt.difficulty("hard bug").expect("d");
        assert_eq!(d["d"], 0.5);
    }

    #[test]
    fn the_turns_effort_goes_out_only_as_a_level_the_model_renders() {
        let user = [ChatMessage {
            role: "user".into(),
            content: "hi".into(),
        }];
        let rt = HttpLocalRuntime::new("http://127.0.0.1:9", "t")
            .with_effort_levels(vec!["low".into(), "high".into()]);
        let sent = |want| chat_body("local", &user, true, rt.effort(want).as_deref());
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
        assert_eq!(plain.effort(Some("high")), None);
    }

    #[test]
    fn effort_levels_are_read_from_props_with_the_api_key() {
        let fake = FakeServer::start(None);
        assert_eq!(
            super::super::local_effort::fetch_levels(
                &fake.host,
                fake.port,
                &fake.token,
                super::super::local_effort::PROPS_TIMEOUT
            ),
            ["low", "high"]
        );
        assert_eq!(
            super::super::local_effort::fetch_levels(
                &fake.host,
                fake.port,
                "sk-abcdefghijklmnopqrstuv",
                super::super::local_effort::PROPS_TIMEOUT
            ),
            Vec::<String>::new()
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
