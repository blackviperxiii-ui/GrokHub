//! SSE token stream. Same wire as Electron `streamXaiChat`.

use serde_json::Value;

pub fn chat_stream_flag(body: &mut Value, stream: bool) {
    body["stream"] = Value::Bool(stream);
}

fn sse_delta(line: &str) -> Option<Value> {
    let data = line.strip_prefix("data:")?.trim();
    if data.is_empty() || data == "[DONE]" {
        return None;
    }
    let v: Value = serde_json::from_str(data).ok()?;
    v.get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("delta").or_else(|| c.get("message")))
        .cloned()
}

pub fn parse_sse_delta(line: &str) -> Option<String> {
    sse_delta(line)?
        .get("content")
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Grok 4.6 Chat Completions may stream `reasoning_content` before the answer.
pub fn parse_sse_thought(line: &str) -> Option<String> {
    let d = sse_delta(line)?;
    d.get("reasoning_content")
        .or_else(|| d.get("reasoning"))
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
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
    }
}
