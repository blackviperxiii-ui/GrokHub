//! SSE token stream. Chat Completions chunks and Responses API events.

use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamTokenKind {
    Delta,
    Replace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

pub fn chat_stream_flag(body: &mut Value, stream: bool) {
    body["stream"] = Value::Bool(stream);
}

pub fn chat_include_usage(body: &mut Value) {
    body["stream_options"] = json!({ "include_usage": true });
}

fn sse_json(line: &str) -> Option<Value> {
    let data = line.strip_prefix("data:")?.trim();
    if data.is_empty() || data == "[DONE]" {
        return None;
    }
    serde_json::from_str(data).ok()
}

fn value_text(v: &Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        if s.is_empty() {
            return None;
        }
        return Some(s.to_string());
    }
    let arr = v.as_array()?;
    let mut out = String::new();
    for part in arr {
        if let Some(t) = part.get("text").and_then(|x| x.as_str()) {
            out.push_str(t);
        } else if let Some(t) = part.as_str() {
            out.push_str(t);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn sse_choice_delta(line: &str) -> Option<Value> {
    let v = sse_json(line)?;
    v.get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("delta").or_else(|| c.get("message")))
        .cloned()
}

fn event_type(v: &Value) -> &str {
    v.get("type").and_then(|t| t.as_str()).unwrap_or("")
}

fn is_thought_event(t: &str) -> bool {
    matches!(
        t,
        "response.reasoning_text.delta"
            | "response.reasoning_summary_text.delta"
            | "response.reasoning_summary.delta"
    )
}

fn is_text_event(t: &str) -> bool {
    matches!(t, "response.output_text.delta" | "response.text.delta")
}

fn event_delta_text(v: &Value) -> Option<String> {
    v.get("delta")
        .and_then(value_text)
        .or_else(|| v.get("text").and_then(value_text))
}

pub fn parse_sse_delta(line: &str) -> Option<String> {
    if let Some(v) = sse_json(line) {
        let t = event_type(&v);
        if is_thought_event(t) {
            return None;
        }
        if is_text_event(t) {
            return event_delta_text(&v);
        }
    }
    sse_choice_delta(line)?.get("content").and_then(value_text)
}

/// Grok 4.6 may stream `reasoning_content` or Responses reasoning deltas before the answer.
pub fn parse_sse_thought(line: &str) -> Option<String> {
    if let Some(v) = sse_json(line) {
        if is_thought_event(event_type(&v)) {
            return event_delta_text(&v);
        }
    }
    let d = sse_choice_delta(line)?;
    d.get("reasoning_content")
        .or_else(|| d.get("reasoning"))
        .and_then(value_text)
}

fn u32_field(v: &Value, keys: &[&str]) -> Option<u32> {
    for k in keys {
        if let Some(n) = v.get(*k).and_then(|x| x.as_u64()) {
            return Some(n as u32);
        }
    }
    None
}

pub fn parse_sse_usage(line: &str) -> Option<StreamUsage> {
    let v = sse_json(line)?;
    let usage = v
        .get("usage")
        .cloned()
        .or_else(|| v.get("response").and_then(|r| r.get("usage")).cloned())?;
    if usage.is_null() {
        return None;
    }
    let prompt_tokens = u32_field(&usage, &["prompt_tokens", "input_tokens"])?;
    let completion_tokens = u32_field(&usage, &["completion_tokens", "output_tokens"])?;
    let total_tokens =
        u32_field(&usage, &["total_tokens"]).unwrap_or(prompt_tokens.saturating_add(completion_tokens));
    Some(StreamUsage {
        prompt_tokens,
        completion_tokens,
        total_tokens,
    })
}

pub fn fold_stream_token(
    messages: &mut Vec<(String, String)>,
    role: &str,
    text: &str,
    kind: StreamTokenKind,
) {
    if text.is_empty() {
        return;
    }
    let same = messages.last().is_some_and(|(r, _)| r == role);
    match kind {
        StreamTokenKind::Delta => {
            if same {
                if let Some(last) = messages.last_mut() {
                    last.1.push_str(text);
                }
            } else {
                messages.push((role.to_string(), text.to_string()));
            }
        }
        StreamTokenKind::Replace => {
            if same {
                if let Some(last) = messages.last_mut() {
                    last.1 = text.to_string();
                }
            } else {
                messages.push((role.to_string(), text.to_string()));
            }
        }
    }
}

pub fn sse_done(line: &str) -> bool {
    line.trim() == "data: [DONE]" || line.trim() == "data:[DONE]"
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn delta_and_done() {
        let line = r#"data: {"choices":[{"delta":{"content":"flash"}}]}"#;
        assert_eq!(parse_sse_delta(line).as_deref(), Some("flash"));
        let think = r#"data: {"choices":[{"delta":{"reasoning_content":"Need a snapshot."}}]}"#;
        assert_eq!(parse_sse_thought(think).as_deref(), Some("Need a snapshot."));
        assert!(parse_sse_delta(think).is_none());
        assert!(sse_done("data: [DONE]"));
        assert!(parse_sse_delta("data: [DONE]").is_none());
        let mut body = json!({"stream": false});
        chat_stream_flag(&mut body, true);
        assert_eq!(body["stream"], true);
        chat_include_usage(&mut body);
        assert_eq!(body["stream_options"]["include_usage"], true);
    }

    #[test]
    fn responses_and_usage_tokens() {
        let text = r#"data: {"type":"response.output_text.delta","delta":"Hello"}"#;
        assert_eq!(parse_sse_delta(text).as_deref(), Some("Hello"));
        let alias = r#"data: {"type":"response.text.delta","delta":" there"}"#;
        assert_eq!(parse_sse_delta(alias).as_deref(), Some(" there"));
        let think = r#"data: {"type":"response.reasoning_summary_text.delta","delta":"Need a snapshot."}"#;
        assert_eq!(parse_sse_thought(think).as_deref(), Some("Need a snapshot."));
        assert!(parse_sse_delta(think).is_none());
        let parts = r#"data: {"choices":[{"delta":{"content":[{"type":"text","text":"flash"}]}}]}"#;
        assert_eq!(parse_sse_delta(parts).as_deref(), Some("flash"));
        let usage_line = r#"data: {"choices":[],"usage":{"prompt_tokens":12,"completion_tokens":4,"total_tokens":16}}"#;
        let usage = parse_sse_usage(usage_line).expect("usage chunk");
        assert_eq!(usage.prompt_tokens, 12);
        assert_eq!(usage.completion_tokens, 4);
        assert_eq!(usage.total_tokens, 16);
        let completed = r#"data: {"type":"response.completed","response":{"usage":{"input_tokens":8,"output_tokens":3,"total_tokens":11}}}"#;
        let usage = parse_sse_usage(completed).expect("responses usage");
        assert_eq!(usage.prompt_tokens, 8);
        assert_eq!(usage.completion_tokens, 3);
        assert_eq!(usage.total_tokens, 11);
        let mut msgs = vec![("assistant".into(), "Hel".into())];
        fold_stream_token(&mut msgs, "assistant", "lo", StreamTokenKind::Delta);
        assert_eq!(msgs, vec![("assistant".into(), "Hello".into())]);
        fold_stream_token(&mut msgs, "assistant", "Hello!", StreamTokenKind::Replace);
        assert_eq!(msgs[0].1, "Hello!");
        fold_stream_token(&mut msgs, "user", "hey", StreamTokenKind::Delta);
        assert_eq!(msgs.last().map(|(r, t)| (r.as_str(), t.as_str())), Some(("user", "hey")));
    }
}
