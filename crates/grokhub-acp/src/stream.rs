//! Headless `grok -p --output-format streaming-json` events.

use serde_json::Value;
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::client::SingleTurn;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrokPEvent {
    Thought(String),
    Text(String),
    Tool {
        id: String,
        title: String,
        status: String,
    },
    End(SingleTurn),
    Err(String),
}

/// One NDJSON line from `--output-format streaming-json`.
pub fn parse_stream_line(line: &str) -> Option<GrokPEvent> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    match v.get("type").and_then(|x| x.as_str()).unwrap_or("") {
        "thought" => {
            let d = v.get("data").and_then(|x| x.as_str()).unwrap_or("");
            if d.is_empty() {
                None
            } else {
                Some(GrokPEvent::Thought(d.to_string()))
            }
        }
        "text" => {
            let d = v.get("data").and_then(|x| x.as_str()).unwrap_or("");
            if d.is_empty() {
                None
            } else {
                Some(GrokPEvent::Text(d.to_string()))
            }
        }
        "tool_call" | "tool_call_update" => {
            let id = v
                .get("toolCallId")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let title = v
                .get("title")
                .or_else(|| v.get("toolName"))
                .and_then(|x| x.as_str())
                .unwrap_or("tool")
                .to_string();
            let status = v
                .get("status")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            Some(GrokPEvent::Tool { id, title, status })
        }
        "error" => {
            let msg = v
                .get("message")
                .and_then(|x| x.as_str())
                .unwrap_or("grok -p error")
                .to_string();
            Some(GrokPEvent::Err(msg))
        }
        "end" => {
            let session_id = v
                .get("sessionId")
                .or_else(|| v.get("session_id"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let text = v
                .get("text")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            Some(GrokPEvent::End(SingleTurn {
                session_id,
                text,
                thought: String::new(),
            }))
        }
        _ => None,
    }
}

/// Fold a full streaming-json stdout into one turn.
pub fn fold_stream(stdout: &str) -> Result<SingleTurn, String> {
    let mut text = String::new();
    let mut thought = String::new();
    let mut session_id = String::new();
    let mut err: Option<String> = None;
    for line in stdout.lines() {
        match parse_stream_line(line) {
            Some(GrokPEvent::Text(d)) => text.push_str(&d),
            Some(GrokPEvent::Thought(d)) => thought.push_str(&d),
            Some(GrokPEvent::End(t)) => {
                if !t.session_id.is_empty() {
                    session_id = t.session_id;
                }
                if !t.text.is_empty() && text.is_empty() {
                    text = t.text;
                }
            }
            Some(GrokPEvent::Err(e)) => err = Some(e),
            _ => {}
        }
    }
    if let Some(e) = err {
        if session_id.is_empty() && text.is_empty() {
            return Err(e);
        }
    }
    if session_id.is_empty() {
        if let Ok(t) = crate::client::parse_single_turn(stdout) {
            return Ok(t);
        }
        return Err("grok -p missing sessionId".into());
    }
    if text.trim().is_empty() && thought.trim().is_empty() {
        return Err("grok -p empty reply".into());
    }
    Ok(SingleTurn {
        session_id,
        text: text.trim().to_string(),
        thought: thought.trim().to_string(),
    })
}

/// `--prompt-json` content blocks for text plus an optional data-URL still.
pub fn prompt_json(text: &str, image_data_url: Option<&str>) -> String {
    let mut blocks = vec![serde_json::json!({ "type": "text", "text": text })];
    if let Some(url) = image_data_url.filter(|s| s.starts_with("data:image")) {
        if let Some((meta, b64)) = url.split_once(',') {
            if !b64.is_empty() {
                let media = meta
                    .strip_prefix("data:")
                    .and_then(|s| s.split(';').next())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("image/png");
                blocks.push(serde_json::json!({
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": media,
                        "data": b64
                    }
                }));
            }
        }
    }
    serde_json::to_string(&blocks).unwrap_or_else(|_| format!(r#"[{{"type":"text","text":{}}}]"#, serde_json::to_string(text).unwrap_or_default()))
}

pub fn kill_pid(pid: u32) {
    let _ = Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status();
    thread::sleep(Duration::from_millis(80));
    let _ = Command::new("kill")
        .args(["-KILL", &pid.to_string()])
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_json_folds_thought_text_and_end() {
        let raw = r#"
{"type":"thought","data":"The"}
{"type":"thought","data":" user"}
{"type":"text","data":"pong"}
{"type":"end","stopReason":"end_turn","sessionId":"01a0400f-2bbc-7501-ba65-578617720d19"}
"#;
        let t = fold_stream(raw).expect("fold");
        assert_eq!(t.session_id, "01a0400f-2bbc-7501-ba65-578617720d19");
        assert_eq!(t.text, "pong");
        assert_eq!(t.thought, "The user");
        assert!(matches!(
            parse_stream_line(r#"{"type":"error","message":"404 Not Found"}"#),
            Some(GrokPEvent::Err(e)) if e.contains("404")
        ));
        let tool = parse_stream_line(
            r#"{"type":"tool_call","toolCallId":"c1","title":"Read","toolName":"read_file","status":"in_progress"}"#,
        );
        assert!(
            matches!(tool, Some(GrokPEvent::Tool { id, title, .. }) if id == "c1" && title == "Read")
        );
    }

    #[test]
    fn prompt_json_sends_text_and_base64_still() {
        let j = prompt_json(
            "look",
            Some("data:image/png;base64,AAA"),
        );
        assert!(j.contains(r#""type":"text""#), "{j}");
        assert!(j.contains(r#""media_type":"image/png""#), "{j}");
        assert!(j.contains("AAA"), "{j}");
        assert!(!prompt_json("hi", None).contains("image"));
    }
}
