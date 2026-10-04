//! Incremental parser for the xAI `/v1/responses` server-sent event stream.

use std::collections::HashSet;

use serde_json::Value;

use crate::{ClientError, FunctionCall, Usage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseEvent {
    TextDelta(String),
    ReasoningDelta(String),
    FunctionCall(FunctionCall),
    Completed(Usage),
    Error(String),
}

pub struct SseParser {
    buf: String,
    completed: bool,
    errored: bool,
    seen: HashSet<String>,
}

impl SseParser {
    pub fn new() -> Self {
        Self {
            buf: String::new(),
            completed: false,
            errored: false,
            seen: HashSet::new(),
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
            return text_delta(&v).into_iter().map(SseEvent::TextDelta).collect();
        }
        if is_reasoning_delta(kind) {
            return text_delta(&v).into_iter().map(SseEvent::ReasoningDelta).collect();
        }
        if kind == "response.output_item.done" {
            return self.take_call(v.get("item").unwrap_or(&v)).into_iter().collect();
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
                        if let Some(ev) = self.take_call(item) {
                            out.push(ev);
                        }
                    }
                }
            }
            out.push(SseEvent::Completed(parse_usage_value(&v)));
            return out;
        }
        if kind == "error" || kind == "response.failed" || kind == "response.error" {
            self.errored = true;
            return vec![SseEvent::Error(error_message(&v))];
        }
        Vec::new()
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
    let name = item.get("name").and_then(|t| t.as_str()).unwrap_or("").to_string();
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
        input_tokens: body.get("input_tokens").and_then(|n| n.as_u64()).unwrap_or(0),
        output_tokens: body.get("output_tokens").and_then(|n| n.as_u64()).unwrap_or(0),
        reasoning_tokens: reasoning,
        cost_in_usd_ticks: cost,
    }
}

fn i64_field(v: &Value, key: &str) -> Option<i64> {
    let n = v.get(key)?;
    n.as_i64().or_else(|| n.as_u64().map(|u| u as i64)).or_else(|| n.as_f64().map(|f| f as i64))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(json: &str) -> String {
        format!("data: {json}\n\n")
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
        let thought2 = p.push(&frame(r#"{"type":"response.reasoning.delta","delta":" more"}"#));
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
            SseEvent::Completed(u) if u.input_tokens == 10 && u.output_tokens == 4 && u.reasoning_tokens == 2 && u.cost_in_usd_ticks == 100
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
    fn sse_frames_split_across_chunks() {
        let mut p = SseParser::new();
        assert!(p.push("data: {\"type\":\"response.output_text.delta\",\"del").is_empty());
        let ev = p.push("ta\":\"ab\"}\n\n");
        assert_eq!(ev, vec![SseEvent::TextDelta("ab".into())]);
    }
}
