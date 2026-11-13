//! Incremental parser for the xAI `/v1/responses` server-sent event stream.

use std::collections::HashSet;

use serde_json::Value;

use crate::client::HostedUsage;
use crate::{ClientError, FunctionCall, Usage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseEvent {
    TextDelta(String),
    ReasoningDelta(String),
    FunctionCall(FunctionCall),
    /// An xAI `reasoning` output item with `encrypted_content`, or a hosted
    /// search call ([`crate::client::is_hosted_call`]), as JSON text.
    ReasoningItem(String),
    /// Pages a reply message cites (`url_citation` annotations), in order.
    Sources(Vec<String>),
    /// What hosted search ran, from the final usage. Sent only when it ran.
    Hosted(HostedUsage),
    /// Stream finished. `incomplete` is set for `response.incomplete` or a
    /// final response with `status: "incomplete"` (output-token cut-off).
    Completed {
        usage: Usage,
        incomplete: bool,
        incomplete_reason: Option<String>,
    },
    Error(String),
}

pub struct SseParser {
    buf: String,
    completed: bool,
    errored: bool,
    seen: HashSet<String>,
    /// Reasoning text so far, per stream kind. A provider that sends the same thought as
    /// both raw reasoning and a summary would otherwise show it twice, back to back.
    reasoning: Vec<(String, String)>,
}

impl SseParser {
    pub fn new() -> Self {
        Self {
            buf: String::new(),
            completed: false,
            errored: false,
            seen: HashSet::new(),
            reasoning: Vec::new(),
        }
    }

    pub fn push(&mut self, chunk: &str) -> Vec<SseEvent> {
        self.buf.push_str(chunk);
        let mut out = Vec::new();
        while let Some((frame, rest)) = split_frame(&self.buf) {
            self.buf = rest;
            out.extend(self.parse_frame(&frame));
        }
        out
    }

    /// Whether a reasoning delta is new, not a repeat of what another reasoning kind
    /// already sent.
    fn new_reasoning(&mut self, kind: &str, delta: &str) -> bool {
        let at = match self.reasoning.iter().position(|(k, _)| k == kind) {
            Some(at) => at,
            None => {
                self.reasoning.push((kind.to_string(), String::new()));
                self.reasoning.len() - 1
            }
        };
        self.reasoning[at].1.push_str(delta);
        let mine = &self.reasoning[at].1;
        !self
            .reasoning
            .iter()
            .enumerate()
            .any(|(i, (_, text))| i != at && text.starts_with(mine.as_str()))
    }

    /// A stream that ends without `response.completed` or an error event is a disconnect.
    pub fn finish(&mut self) -> Result<Vec<SseEvent>, ClientError> {
        let mut out = Vec::new();
        if !self.buf.trim().is_empty() {
            let rest = std::mem::take(&mut self.buf);
            out.extend(self.parse_frame(&rest));
        }
        if self.completed || self.errored {
            Ok(out)
        } else {
            Err(ClientError::Disconnect)
        }
    }

    fn parse_frame(&mut self, frame: &str) -> Vec<SseEvent> {
        let mut data = String::new();
        for line in frame.split(['\n', '\r']) {
            let line = line.trim_end();
            if let Some(rest) = line.strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(rest.trim_start());
            }
        }
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            return Vec::new();
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else {
            return Vec::new();
        };
        let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if kind == "response.output_text.delta" {
            return text_delta(&v)
                .into_iter()
                .map(SseEvent::TextDelta)
                .collect();
        }
        if is_reasoning_delta(kind) {
            return text_delta(&v)
                .into_iter()
                .filter(|delta| self.new_reasoning(kind, delta))
                .map(SseEvent::ReasoningDelta)
                .collect();
        }
        if kind == "response.output_item.done" {
            return self
                .take_item(v.get("item").unwrap_or(&v))
                .into_iter()
                .collect();
        }
        if kind == "response.completed" || kind == "response.incomplete" {
            self.completed = true;
            let mut out = Vec::new();
            if let Some(output) = v
                .get("response")
                .and_then(|r| r.get("output"))
                .or(v.get("output"))
            {
                if let Some(items) = output.as_array() {
                    for item in items {
                        if let Some(ev) = self.take_item(item) {
                            out.push(ev);
                        }
                    }
                }
            }
            note_served_model(&v);
            let hosted = parse_hosted_usage(&v);
            if !hosted.is_empty() {
                out.push(SseEvent::Hosted(hosted));
            }
            let (incomplete, incomplete_reason) = incomplete_of(kind, &v);
            out.push(SseEvent::Completed {
                usage: parse_usage_value(&v),
                incomplete,
                incomplete_reason,
            });
            return out;
        }
        if kind == "error" || kind == "response.failed" || kind == "response.error" {
            self.errored = true;
            return vec![SseEvent::Error(error_message(&v))];
        }
        Vec::new()
    }

    /// A function call, a sealed reasoning item or a hosted search call, each
    /// once: `response.completed` repeats what `response.output_item.done`
    /// already sent. A message gives the pages it cites.
    fn take_item(&mut self, item: &Value) -> Option<SseEvent> {
        let typ = item.get("type").and_then(Value::as_str);
        if typ == Some("message") {
            let urls = cited_urls(item);
            return (!urls.is_empty()).then_some(SseEvent::Sources(urls));
        }
        if crate::client::is_hosted_call(item) {
            let raw = serde_json::to_string(item).ok()?;
            let id = item
                .get("id")
                .or(item.get("call_id"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            return self
                .seen
                .insert(format!("hosted:{id}"))
                .then_some(SseEvent::ReasoningItem(raw));
        }
        if typ == Some("reasoning") {
            let raw = serde_json::to_string(item).ok()?;
            crate::client::InputItem::reasoning(&raw, "")?;
            let key = item
                .get("id")
                .and_then(Value::as_str)
                .map(|id| format!("reasoning:{id}"))
                .unwrap_or_else(|| raw.clone());
            return self
                .seen
                .insert(key)
                .then_some(SseEvent::ReasoningItem(raw));
        }
        self.take_call(item)
    }

    fn take_call(&mut self, item: &Value) -> Option<SseEvent> {
        let call = parse_call(item)?;
        if !call.call_id.is_empty() && !self.seen.insert(call.call_id.clone()) {
            return None;
        }
        Some(SseEvent::FunctionCall(call))
    }
}

impl Default for SseParser {
    fn default() -> Self {
        Self::new()
    }
}

fn split_frame(buf: &str) -> Option<(String, String)> {
    if let Some(i) = buf.find("\r\n\r\n") {
        return Some((buf[..i].to_string(), buf[i + 4..].to_string()));
    }
    if let Some(i) = buf.find("\n\n") {
        return Some((buf[..i].to_string(), buf[i + 2..].to_string()));
    }
    None
}

fn text_delta(v: &Value) -> Option<String> {
    v.get("delta")
        .and_then(|d| d.as_str())
        .or_else(|| v.get("text").and_then(|d| d.as_str()))
        .map(str::to_string)
}

fn is_reasoning_delta(kind: &str) -> bool {
    matches!(
        kind,
        "response.reasoning_summary_text.delta"
            | "response.reasoning.delta"
            | "response.reasoning_text.delta"
            | "response.reasoning_summary.delta"
    )
}

fn parse_call(item: &Value) -> Option<FunctionCall> {
    let typ = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
    if typ != "function_call" {
        return None;
    }
    let name = item
        .get("name")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    if name.is_empty() {
        return None;
    }
    let call_id = item
        .get("call_id")
        .or(item.get("id"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    let arguments = match item.get("arguments") {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => "{}".into(),
    };
    Some(FunctionCall {
        call_id,
        name,
        arguments,
    })
}

thread_local! {
    static SERVED_MODEL: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Router R0 passive health: the `response.model` of the last completed stream
/// on this thread, so the shadow log can spot a redirect. Ids only.
fn note_served_model(v: &Value) {
    let model = v
        .get("response")
        .and_then(|r| r.get("model"))
        .and_then(|m| m.as_str())
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty());
    SERVED_MODEL.with(|s| *s.borrow_mut() = model);
}

/// Take (and clear) the model the last completed stream on this thread said it was.
pub fn take_served_model() -> Option<String> {
    SERVED_MODEL.with(|s| s.borrow_mut().take())
}

/// The `url_citation` URLs on a message's `output_text` parts, once each.
fn cited_urls(item: &Value) -> Vec<String> {
    let mut urls: Vec<String> = Vec::new();
    let parts = item.get("content").and_then(Value::as_array);
    for part in parts.into_iter().flatten() {
        let notes = part.get("annotations").and_then(Value::as_array);
        for note in notes.into_iter().flatten() {
            if note.get("type").and_then(Value::as_str) != Some("url_citation") {
                continue;
            }
            let Some(url) = note.get("url").and_then(Value::as_str).map(str::trim) else {
                continue;
            };
            if !url.is_empty() && !urls.iter().any(|u| u == url) {
                urls.push(url.to_string());
            }
        }
    }
    urls
}

/// `usage.server_side_tool_usage_details`: successful hosted search, as billed.
pub(crate) fn parse_hosted_usage(v: &Value) -> HostedUsage {
    let details = v
        .get("response")
        .and_then(|r| r.get("usage"))
        .or(v.get("usage"))
        .and_then(|u| u.get("server_side_tool_usage_details"));
    let n = |key: &str| {
        details
            .and_then(|d| d.get(key))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    HostedUsage {
        web_search_calls: n("web_search_calls"),
        x_posts_fetched: n("x_posts_fetched"),
        x_users_fetched: n("x_users_fetched"),
    }
}

pub(crate) fn parse_usage_value(v: &Value) -> Usage {
    let body = v
        .get("response")
        .and_then(|r| r.get("usage"))
        .or(v.get("usage"))
        .unwrap_or(v);
    let reasoning = body
        .get("output_tokens_details")
        .and_then(|d| d.get("reasoning_tokens"))
        .and_then(|n| n.as_u64())
        .or_else(|| body.get("reasoning_tokens").and_then(|n| n.as_u64()))
        .unwrap_or(0);
    let cost = i64_field(body, "cost_in_usd_ticks")
        .or_else(|| {
            v.get("metadata")
                .and_then(|m| i64_field(m, "cost_in_usd_ticks"))
        })
        .or_else(|| i64_field(v, "cost_in_usd_ticks"))
        .unwrap_or(0);
    Usage {
        input_tokens: body
            .get("input_tokens")
            .and_then(|n| n.as_u64())
            .unwrap_or(0),
        output_tokens: body
            .get("output_tokens")
            .and_then(|n| n.as_u64())
            .unwrap_or(0),
        reasoning_tokens: reasoning,
        cost_in_usd_ticks: cost,
        cached_tokens: body
            .get("input_tokens_details")
            .and_then(|d| d.get("cached_tokens"))
            .and_then(|n| n.as_u64())
            .unwrap_or(0),
    }
}

fn i64_field(v: &Value, key: &str) -> Option<i64> {
    let n = v.get(key)?;
    n.as_i64()
        .or_else(|| n.as_u64().map(|u| u as i64))
        .or_else(|| n.as_f64().map(|f| f as i64))
}

fn error_message(v: &Value) -> String {
    let nested = v
        .get("error")
        .or_else(|| v.get("response").and_then(|r| r.get("error")));
    nested
        .and_then(|e| e.get("message").and_then(|m| m.as_str()))
        .or_else(|| v.get("message").and_then(|m| m.as_str()))
        .unwrap_or("model stream error")
        .to_string()
}

/// `response.incomplete` always counts; `response.completed` only when the
/// body says `status: "incomplete"`. Reason comes from `incomplete_details`.
fn incomplete_of(kind: &str, v: &Value) -> (bool, Option<String>) {
    let response = v.get("response").unwrap_or(v);
    let status_incomplete = response
        .get("status")
        .and_then(|s| s.as_str())
        .is_some_and(|s| s == "incomplete");
    let incomplete = kind == "response.incomplete" || status_incomplete;
    if !incomplete {
        return (false, None);
    }
    let reason = response
        .get("incomplete_details")
        .and_then(|d| d.get("reason"))
        .and_then(|r| r.as_str())
        .map(str::to_string)
        .filter(|r| !r.is_empty());
    (true, reason)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(json: &str) -> String {
        format!("data: {json}\n\n")
    }

    #[test]
    fn a_thought_sent_as_both_reasoning_and_summary_shows_once() {
        let mut p = SseParser::new();
        let line = "The findings card is posted.";
        let mut got = p.push(&frame(&format!(
            r#"{{"type":"response.reasoning_text.delta","delta":"{line}"}}"#
        )));
        got.extend(p.push(&frame(&format!(
            r#"{{"type":"response.reasoning_summary_text.delta","delta":"{line}"}}"#
        ))));
        got.extend(p.push(&frame(
            r#"{"type":"response.reasoning_text.delta","delta":" Next."}"#,
        )));
        assert_eq!(
            got,
            vec![
                SseEvent::ReasoningDelta(line.into()),
                SseEvent::ReasoningDelta(" Next.".into()),
            ]
        );
    }

    #[test]
    fn sse_text_reasoning_tool_usage_error_and_disconnect() {
        let mut p = SseParser::new();
        let text = p.push(&frame(
            r#"{"type":"response.output_text.delta","delta":"Hello"}"#,
        ));
        assert_eq!(text, vec![SseEvent::TextDelta("Hello".into())]);

        let thought = p.push(&frame(
            r#"{"type":"response.reasoning_summary_text.delta","delta":"hmm"}"#,
        ));
        assert_eq!(thought, vec![SseEvent::ReasoningDelta("hmm".into())]);
        let thought2 = p.push(&frame(
            r#"{"type":"response.reasoning.delta","delta":" more"}"#,
        ));
        assert_eq!(thought2, vec![SseEvent::ReasoningDelta(" more".into())]);

        let call = p.push(&frame(
            r#"{"type":"response.output_item.done","item":{"type":"function_call","call_id":"c1","name":"read_file","arguments":"{\"target_file\":\"a.rs\"}"}}"#,
        ));
        assert_eq!(
            call,
            vec![SseEvent::FunctionCall(FunctionCall {
                call_id: "c1".into(),
                name: "read_file".into(),
                arguments: "{\"target_file\":\"a.rs\"}".into(),
            })]
        );

        let done = p.push(&frame(
            r#"{"type":"response.completed","response":{"usage":{"input_tokens":10,"output_tokens":4,"output_tokens_details":{"reasoning_tokens":2},"cost_in_usd_ticks":100},"output":[{"type":"function_call","call_id":"c1","name":"read_file","arguments":"{}"}]}}"#,
        ));
        assert!(matches!(
            &done[0],
            SseEvent::Completed { usage: u, incomplete: false, incomplete_reason: None }
                if u.input_tokens == 10 && u.output_tokens == 4 && u.reasoning_tokens == 2 && u.cost_in_usd_ticks == 100
        ));
        p.finish().unwrap();

        let mut err_p = SseParser::new();
        let err = err_p.push(&frame(r#"{"type":"error","error":{"message":"boom"}}"#));
        assert_eq!(err, vec![SseEvent::Error("boom".into())]);
        err_p.finish().unwrap();

        let mut drop_p = SseParser::new();
        let _ = drop_p.push("data: {\"type\":\"response.output_text.delta\",\"delta\":\"x\"}\n\n");
        assert!(matches!(drop_p.finish(), Err(ClientError::Disconnect)));
    }

    #[test]
    fn sse_incomplete_marks_truncated_and_keeps_calls() {
        let mut p = SseParser::new();
        let ev = p.push(&frame(
            r#"{"type":"response.incomplete","response":{"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"usage":{"input_tokens":3,"output_tokens":9},"output":[{"type":"function_call","call_id":"w1","name":"write","arguments":"{\"path\":\"a.rs\",\"contents\":\"partial\"}"}]}}"#,
        ));
        assert_eq!(
            ev[0],
            SseEvent::FunctionCall(FunctionCall {
                call_id: "w1".into(),
                name: "write".into(),
                arguments: "{\"path\":\"a.rs\",\"contents\":\"partial\"}".into(),
            })
        );
        assert_eq!(
            ev[1],
            SseEvent::Completed {
                usage: Usage {
                    input_tokens: 3,
                    output_tokens: 9,
                    ..Usage::default()
                },
                incomplete: true,
                incomplete_reason: Some("max_output_tokens".into()),
            }
        );
        p.finish().unwrap();
    }

    #[test]
    fn sse_completed_with_incomplete_status_is_truncated() {
        let mut p = SseParser::new();
        let ev = p.push(&frame(
            r#"{"type":"response.completed","response":{"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"usage":{"input_tokens":1,"output_tokens":2},"output":[]}}"#,
        ));
        assert_eq!(
            ev,
            vec![SseEvent::Completed {
                usage: Usage {
                    input_tokens: 1,
                    output_tokens: 2,
                    ..Usage::default()
                },
                incomplete: true,
                incomplete_reason: Some("max_output_tokens".into()),
            }]
        );
        p.finish().unwrap();
    }

    #[test]
    fn sse_frames_split_across_chunks() {
        let mut p = SseParser::new();
        assert!(p
            .push("data: {\"type\":\"response.output_text.delta\",\"del")
            .is_empty());
        let ev = p.push("ta\":\"ab\"}\n\n");
        assert_eq!(ev, vec![SseEvent::TextDelta("ab".into())]);
    }

    /// grok-4.x sends its sealed reasoning as an output item; it comes back once,
    /// whole, even when `response.completed` repeats it.
    #[test]
    fn a_sealed_reasoning_item_is_kept_once() {
        let mut p = SseParser::new();
        let item = r#"{"id":"rs_1","summary":[],"type":"reasoning","status":"completed","encrypted_content":"gAAAA=="}"#;
        let ev = p.push(&frame(&format!(
            r#"{{"type":"response.output_item.done","item":{item}}}"#
        )));
        assert_eq!(
            ev,
            vec![SseEvent::ReasoningItem(
                r#"{"encrypted_content":"gAAAA==","id":"rs_1","status":"completed","summary":[],"type":"reasoning"}"#
                    .into()
            )]
        );
        let done = p.push(&frame(&format!(
            r#"{{"type":"response.completed","response":{{"usage":{{"input_tokens":1,"output_tokens":1}},"output":[{item}]}}}}"#
        )));
        assert_eq!(done.len(), 1, "{done:?}");
        assert!(matches!(done[0], SseEvent::Completed { .. }));
        p.finish().unwrap();
    }

    #[test]
    fn hosted_search_calls_sources_and_billed_counts_come_through_once() {
        let mut p = SseParser::new();
        let web = r#"{"id":"ws_1","type":"web_search_call","status":"completed","action":{"type":"search","query":"grok"}}"#;
        let x = r#"{"call_id":"xs_9","type":"custom_tool_call","name":"x_keyword_search","input":"{}","status":"completed"}"#;
        let msg = r#"{"id":"msg_1","type":"message","role":"assistant","content":[{"type":"output_text","text":"See [[1]](https://x.ai/a) and [[2]](https://x.ai/b) [[1]](https://x.ai/a)","annotations":[{"type":"url_citation","url":"https://x.ai/a","start_index":4,"end_index":24,"title":"1"},{"type":"url_citation","url":"https://x.ai/b","start_index":29,"end_index":49,"title":"2"},{"type":"url_citation","url":"https://x.ai/a","start_index":50,"end_index":70,"title":"1"},{"type":"file_citation","file_id":"f"}]}]}"#;
        let mut events = Vec::new();
        for item in [web, x, msg] {
            events.extend(p.push(&frame(&format!(
                r#"{{"type":"response.output_item.done","item":{item}}}"#
            ))));
        }
        let raw =
            |s: &str| serde_json::to_string(&serde_json::from_str::<Value>(s).unwrap()).unwrap();
        assert_eq!(
            events,
            vec![
                SseEvent::ReasoningItem(raw(web)),
                SseEvent::ReasoningItem(raw(x)),
                SseEvent::Sources(vec!["https://x.ai/a".into(), "https://x.ai/b".into()]),
            ]
        );
        // The final reply repeats the items: the calls aren't sent twice.
        let done = p.push(&frame(&format!(
            r#"{{"type":"response.completed","response":{{"usage":{{"input_tokens":1,"output_tokens":1,"server_side_tool_usage_details":{{"web_search_calls":2,"x_posts_fetched":7,"x_users_fetched":1}}}},"output":[{web},{x}]}}}}"#
        )));
        assert_eq!(
            done[0],
            SseEvent::Hosted(HostedUsage {
                web_search_calls: 2,
                x_posts_fetched: 7,
                x_users_fetched: 1,
            })
        );
        assert!(matches!(done[1], SseEvent::Completed { .. }));
        assert_eq!(done.len(), 2, "{done:?}");
        p.finish().unwrap();
    }

    #[test]
    fn a_reply_with_no_hosted_search_sends_no_hosted_event() {
        let mut p = SseParser::new();
        let done = p.push(&frame(
            r#"{"type":"response.completed","response":{"usage":{"input_tokens":1,"server_side_tool_usage_details":{"web_search_calls":0}},"output":[]}}"#,
        ));
        assert_eq!(done.len(), 1, "{done:?}");
        assert!(matches!(done[0], SseEvent::Completed { .. }));
    }
}
