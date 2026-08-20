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

fn is_text_done_event(t: &str) -> bool {
    matches!(t, "response.output_text.done" | "response.text.done")
}

fn event_delta_text(v: &Value) -> Option<String> {
    v.get("delta")
        .and_then(value_text)
        .or_else(|| v.get("text").and_then(value_text))
}

fn event_done_text(v: &Value) -> Option<String> {
    v.get("text")
        .and_then(value_text)
        .or_else(|| v.get("output_text").and_then(value_text))
        .or_else(|| v.get("delta").and_then(value_text))
}

/// Chat Completions deltas and Responses `output_text` delta/done events.
pub fn parse_sse_text(line: &str) -> Option<(String, StreamTokenKind)> {
    if let Some(v) = sse_json(line) {
        let t = event_type(&v);
        if is_thought_event(t) {
            return None;
        }
        if is_text_event(t) {
            return event_delta_text(&v).map(|s| (s, StreamTokenKind::Delta));
        }
        if is_text_done_event(t) {
            return event_done_text(&v).map(|s| (s, StreamTokenKind::Replace));
        }
    }
    sse_choice_delta(line)?
        .get("content")
        .and_then(value_text)
        .map(|s| (s, StreamTokenKind::Delta))
}

pub fn parse_sse_delta(line: &str) -> Option<String> {
    match parse_sse_text(line) {
        Some((s, StreamTokenKind::Delta)) => Some(s),
        Some(_) | None => None,
    }
}

/// Live UI only emits a done-event when no deltas arrived (otherwise it would duplicate).
pub fn sse_live_delta(acc_was_empty: bool, kind: StreamTokenKind) -> bool {
    match kind {
        StreamTokenKind::Delta => true,
        StreamTokenKind::Replace => acc_was_empty,
    }
}

pub fn fold_sse_acc(acc: &mut String, text: &str, kind: StreamTokenKind) {
    if text.is_empty() {
        return;
    }
    match kind {
        StreamTokenKind::Delta => acc.push_str(text),
        StreamTokenKind::Replace => {
            if should_replace_stream_acc(acc, text) {
                *acc = text.to_string();
            }
        }
    }
}

/// A later `output_text.done` for one item must not wipe a longer delta acc.
pub fn should_replace_stream_acc(acc: &str, done: &str) -> bool {
    acc.is_empty() || done.len() >= acc.len() || done.starts_with(acc)
}

/// Live deltas can be ahead of the worker acc when a short done-event replaces it.
pub fn prefer_complete_reply(streamed: &str, finished: &str) -> String {
    let s = streamed.trim_end();
    let f = finished.trim_end();
    if s.is_empty() {
        return finished.to_string();
    }
    if f.is_empty() {
        return streamed.to_string();
    }
    if f.len() >= s.len() {
        return finished.to_string();
    }
    let s_tool = s.contains("COMPUTER_CMD") || s.contains("HOST_CMD:");
    let f_tool = f.contains("COMPUTER_CMD") || f.contains("HOST_CMD:");
    if s.starts_with(f) || (s_tool && !f_tool) {
        return streamed.to_string();
    }
    finished.to_string()
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
    let push = {
        let last = messages.last_mut().map(|(r, c)| (r.as_str(), c));
        fold_stream_fields(last, role, text, kind)
    };
    if let Some(pair) = push {
        messages.push(pair);
    }
}

/// Fold a stream token onto the live pane without cloning prior turns.
/// Returns a new row to push, or `None` when the last row was mutated.
pub fn fold_stream_fields(
    last: Option<(&str, &mut String)>,
    role: &str,
    text: &str,
    kind: StreamTokenKind,
) -> Option<(String, String)> {
    if text.is_empty() {
        return None;
    }
    let same = last.as_ref().is_some_and(|(r, _)| *r == role);
    match kind {
        StreamTokenKind::Delta => {
            if same {
                if let Some((_, body)) = last {
                    body.push_str(text);
                }
                None
            } else {
                Some((role.to_string(), text.to_string()))
            }
        }
        StreamTokenKind::Replace => {
            if same {
                if let Some((_, body)) = last {
                    *body = text.to_string();
                }
                None
            } else {
                Some((role.to_string(), text.to_string()))
            }
        }
    }
}

pub fn sse_done(line: &str) -> bool {
    line.trim() == "data: [DONE]" || line.trim() == "data:[DONE]"
}

/// Chat Completions `finish_reason` or Responses incomplete status.
pub fn parse_sse_finish(line: &str) -> Option<String> {
    let v = sse_json(line)?;
    if let Some(reason) = v
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("finish_reason"))
        .and_then(|r| r.as_str())
        .map(str::trim)
        .filter(|r| !r.is_empty() && *r != "null")
    {
        return Some(reason.to_string());
    }
    let resp = v.get("response").unwrap_or(&v);
    if resp.get("status").and_then(|s| s.as_str()) == Some("incomplete") {
        if let Some(reason) = resp
            .get("incomplete_details")
            .and_then(|d| d.get("reason"))
            .and_then(|r| r.as_str())
            .map(str::trim)
            .filter(|r| !r.is_empty())
        {
            return Some(reason.to_string());
        }
        return Some("incomplete".into());
    }
    None
}

pub fn keep_sse_acc(acc: &str, truncated: bool) -> bool {
    !acc.is_empty() || truncated
}

pub fn stream_was_truncated(reason: Option<&str>) -> bool {
    let Some(r) = reason.map(|s| s.trim().to_ascii_lowercase()) else {
        return false;
    };
    matches!(
        r.as_str(),
        "length" | "max_tokens" | "max_output_tokens" | "incomplete"
    )
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
        assert_eq!(
            parse_sse_finish(r#"data: {"choices":[{"delta":{},"finish_reason":"length"}]}"#)
                .as_deref(),
            Some("length")
        );
        assert_eq!(
            parse_sse_finish(r#"data: {"choices":[{"finish_reason":"stop"}]}"#).as_deref(),
            Some("stop")
        );
        assert!(stream_was_truncated(Some("length")));
        assert!(stream_was_truncated(Some("max_tokens")));
        assert!(!stream_was_truncated(Some("stop")));
        assert!(!stream_was_truncated(None));
        assert!(keep_sse_acc("hello", false));
        assert!(
            keep_sse_acc("", true),
            "a length cutoff with no text deltas is still truncated"
        );
        assert!(!keep_sse_acc("", false));
        let incomplete = r#"data: {"type":"response.incomplete","response":{"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"}}}"#;
        assert_eq!(parse_sse_finish(incomplete).as_deref(), Some("max_output_tokens"));
        assert!(stream_was_truncated(parse_sse_finish(incomplete).as_deref()));
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
        let done = r#"data: {"type":"response.output_text.done","text":"Hello"}"#;
        assert_eq!(
            parse_sse_text(done),
            Some(("Hello".into(), StreamTokenKind::Replace))
        );
        assert!(parse_sse_delta(done).is_none());
        let mut acc = String::new();
        fold_sse_acc(&mut acc, "Hello", StreamTokenKind::Replace);
        assert_eq!(acc, "Hello");
        assert!(sse_live_delta(true, StreamTokenKind::Replace));
        assert!(!sse_live_delta(false, StreamTokenKind::Replace));
        let mut acc = String::from("Hel");
        fold_sse_acc(&mut acc, "Hello", StreamTokenKind::Replace);
        assert_eq!(acc, "Hello");
        let mut acc = String::from("Hello\nCOMPUTER_CMD: key Alt+F4");
        fold_sse_acc(&mut acc, "Hello", StreamTokenKind::Replace);
        assert!(
            acc.contains("COMPUTER_CMD: key Alt+F4"),
            "short done must not wipe a longer stream: {acc}"
        );
        assert!(!should_replace_stream_acc(
            "Hello\nCOMPUTER_CMD: key Alt+F4",
            "Hello"
        ));
        assert_eq!(
            prefer_complete_reply(
                "I'll click.\nCOMPUTER_CMD: click 10 20\n",
                "I'll click."
            ),
            "I'll click.\nCOMPUTER_CMD: click 10 20\n"
        );
        assert_eq!(prefer_complete_reply("", "final"), "final");
        assert_eq!(prefer_complete_reply("Hel", "Hello"), "Hello");
    }
}
