//! SSE token stream. Same wire as Electron `streamXaiChat`.

use serde_json::Value;

pub fn chat_stream_flag(body: &mut Value, stream: bool) {
    body["stream"] = Value::Bool(stream);
}

pub fn parse_sse_delta(line: &str) -> Option<String> {
    let data = line.strip_prefix("data:")?.trim();
    if data.is_empty() || data == "[DONE]" {
        return None;
    }
    let v: Value = serde_json::from_str(data).ok()?;
    v.get("choices")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("delta").or_else(|| c.get("message")))
        .and_then(|d| d.get("content"))
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
        assert!(sse_done("data: [DONE]"));
        assert!(parse_sse_delta("data: [DONE]").is_none());
        let mut body = json!({"stream": false});
        chat_stream_flag(&mut body, true);
        assert_eq!(body["stream"], true);
    }
}
