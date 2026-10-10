//! Headless `grok -p --output-format streaming-json` events.

use serde_json::Value;

use crate::client::SingleTurn;
use crate::protocol::{parse_tool_card, ToolCard};
pub use grokhub_core::proc_util::kill_pid;
pub use grokhub_core::wire::{
    classify_stream_error, grok_context_line, grok_usage_line, parse_signals_json, parse_usage,
    retry_status_line, rewrite_truncation_error, turn_footer, GrokUsage, StreamErrorKind,
};

#[derive(Debug, Clone, PartialEq)]
pub enum GrokPEvent {
    Thought(String),
    Text(String),
    Tool(ToolCard),
    Usage(GrokUsage),
    Plan(String),
    Compact {
        started: bool,
        usage: GrokUsage,
        error: Option<String>,
    },
    Commands(Vec<String>),
    Task { id: String, title: String, done: bool },
    Recovering(String),
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
        "tool_call" | "tool_call_update" => Some(GrokPEvent::Tool(parse_tool_card(&v))),
        "plan" => {
            let t = plan_from_value(&v);
            if t.is_empty() {
                None
            } else {
                Some(GrokPEvent::Plan(t))
            }
        }
        "usage" => {
            let u = parse_usage(&v);
            if u.is_empty() {
                None
            } else {
                Some(GrokPEvent::Usage(u))
            }
        }
        "auto_compact_started" => Some(GrokPEvent::Compact {
            started: true,
            usage: parse_usage(&v),
            error: None,
        }),
        "auto_compact_completed" => Some(GrokPEvent::Compact {
            started: false,
            usage: parse_usage(&v),
            error: None,
        }),
        "auto_compact_failed" => {
            let msg = json_str(&v, &["message", "error", "reason"]);
            Some(GrokPEvent::Compact {
                started: false,
                usage: parse_usage(&v),
                error: Some(if msg.is_empty() {
                    "Compact failed".into()
                } else {
                    msg
                }),
            })
        }
        "available_commands" => {
            let cmds = v
                .get("commands")
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if cmds.is_empty() {
                None
            } else {
                Some(GrokPEvent::Commands(cmds))
            }
        }
        "task_backgrounded" | "task_completed" | "task_failed" | "todo_failed" => {
            let kind = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
            let done = kind == "task_completed";
            let failed = kind == "task_failed" || kind == "todo_failed";
            let id = json_str(&v, &["task_id", "tool_call_id", "toolCallId"]);
            let mut title: String = json_str(&v, &["command", "title", "error", "message"])
                .chars()
                .take(80)
                .collect();
            if title.is_empty() {
                title = "task".into();
            }
            if failed && !title.to_ascii_lowercase().starts_with("failed") {
                title = format!("Failed · {title}");
            }
            Some(GrokPEvent::Task { id, title, done })
        }
        "max_turns_reached" => Some(GrokPEvent::Err("Max turns reached".into())),
        "error" => {
            let raw = v
                .get("message")
                .and_then(|x| x.as_str())
                .unwrap_or("grok -p error")
                .to_string();
            let msg = rewrite_truncation_error(&raw);
            match classify_stream_error(&raw) {
                StreamErrorKind::Transient | StreamErrorKind::TruncationContinue => {
                    Some(GrokPEvent::Recovering(msg))
                }
                StreamErrorKind::CreditLimit | StreamErrorKind::Fatal => Some(GrokPEvent::Err(msg)),
            }
        }
        "end" => Some(GrokPEvent::End(end_turn_from_value(&v))),
        _ => None,
    }
}

fn end_turn_from_value(v: &Value) -> SingleTurn {
    let session_id = json_str(v, &["sessionId", "session_id"]);
    let text = v
        .get("text")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let thought = v
        .get("thought")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let mut usage = parse_usage(v);
    if usage.stop_reason.is_empty() {
        usage.stop_reason = json_str(v, &["stopReason", "stop_reason"]);
    }
    SingleTurn {
        session_id,
        text,
        thought,
        usage,
        stop_reason: json_str(v, &["stopReason", "stop_reason"]),
    }
}

fn plan_from_value(v: &Value) -> String {
    if let Some(entries) = v.get("entries").and_then(|e| e.as_array()) {
        let lines: Vec<String> = entries
            .iter()
            .filter_map(|e| {
                let c = e.get("content").and_then(|x| x.as_str())?.trim();
                if c.is_empty() {
                    return None;
                }
                let st = e.get("status").and_then(|x| x.as_str()).unwrap_or("").trim();
                Some(if st.is_empty() {
                    c.to_string()
                } else {
                    format!("{c} ({st})")
                })
            })
            .collect();
        if !lines.is_empty() {
            return lines.join(" · ");
        }
    }
    json_str(v, &["title", "text", "data"])
}

fn json_str(v: &Value, keys: &[&str]) -> String {
    for k in keys {
        if let Some(s) = v.get(*k).and_then(|x| x.as_str()).map(str::trim).filter(|s| !s.is_empty()) {
            return s.to_string();
        }
    }
    String::new()
}

/// Fold a full streaming-json stdout into one turn.
pub fn fold_stream(stdout: &str) -> Result<SingleTurn, String> {
    let mut text = String::new();
    let mut thought = String::new();
    let mut session_id = String::new();
    let mut usage = GrokUsage::default();
    let mut stop_reason = String::new();
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
                usage.merge(&t.usage);
                if !t.stop_reason.is_empty() {
                    stop_reason = t.stop_reason;
                }
            }
            Some(GrokPEvent::Usage(u)) => usage.merge(&u),
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
        usage,
        stop_reason,
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
            matches!(tool, Some(GrokPEvent::Tool(card)) if card.id == "c1" && card.title == "Read")
        );
    }

    #[test]
    fn streaming_json_1_0_12_usage_compact_and_plan() {
        let usage = parse_stream_line(
            r#"{"type":"usage","usage":{"input_tokens":18007,"output_tokens":45,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"reasoning_tokens":40},"signature":"sig"}"#,
        );
        match usage {
            Some(GrokPEvent::Usage(u)) => {
                assert_eq!(u.input_tokens, 18007);
                assert_eq!(u.output_tokens, 45);
                assert_eq!(u.reasoning_tokens, 40);
                assert!(grok_context_line(&u).contains("think"), "{}", grok_context_line(&u));
            }
            other => panic!("{other:?}"),
        }
        let end = parse_stream_line(
            r#"{"type":"end","stopReason":"end_turn","sessionId":"01a04535-7671-75f0-9635-8d6c68bb2537","usage":{"input_tokens":18007,"output_tokens":45,"reasoning_tokens":40,"total_tokens":18052},"num_turns":1}"#,
        );
        match end {
            Some(GrokPEvent::End(t)) => {
                assert_eq!(t.session_id, "01a04535-7671-75f0-9635-8d6c68bb2537");
                assert_eq!(t.usage.reasoning_tokens, 40);
                assert_eq!(t.usage.total_tokens, 18052);
                assert_eq!(t.stop_reason, "end_turn");
            }
            other => panic!("{other:?}"),
        }
        let compact = parse_stream_line(
            r#"{"type":"auto_compact_started","percentage":85,"tokens_used":420000,"context_window":500000}"#,
        );
        match compact {
            Some(GrokPEvent::Compact { started, usage, error }) => {
                assert!(started);
                assert!(error.is_none());
                assert_eq!(usage.context_tokens_used, 420000);
                assert_eq!(usage.context_window_tokens, 500000);
                assert!(grok_context_line(&usage).starts_with("84%") || grok_context_line(&usage).starts_with("85%"), "{}", grok_context_line(&usage));
            }
            other => panic!("{other:?}"),
        }
        match parse_stream_line(r#"{"type":"auto_compact_failed","message":"disk full while compacting"}"#) {
            Some(GrokPEvent::Compact { started, error, .. }) => {
                assert!(!started);
                assert_eq!(error.as_deref(), Some("disk full while compacting"));
            }
            other => panic!("{other:?}"),
        }
        let plan = parse_stream_line(
            r#"{"type":"plan","entries":[{"content":"Read the changelog","status":"in_progress"},{"content":"Wire usage","status":"pending"}]}"#,
        );
        match plan {
            Some(GrokPEvent::Plan(t)) => assert!(t.contains("changelog") && t.contains("Wire usage"), "{t}"),
            other => panic!("{other:?}"),
        }
        assert!(
            matches!(
                parse_stream_line(r#"{"type":"error","message":"Output truncated. Try asking for a shorter answer."}"#),
                Some(GrokPEvent::Recovering(e)) if e.contains("continuing automatically")
            )
        );
        assert!(
            matches!(
                parse_stream_line(r#"{"type":"error","message":"503 Bad Gateway"}"#),
                Some(GrokPEvent::Recovering(e)) if e.to_ascii_lowercase().contains("retry")
            )
        );
        assert!(
            matches!(
                parse_stream_line(r#"{"type":"error","message":"Credit limit reached. Upgrade tier."}"#),
                Some(GrokPEvent::Err(e)) if e.contains("Try Again")
            )
        );
        assert_eq!(
            classify_stream_error("Output truncated. Try asking for a shorter answer."),
            StreamErrorKind::TruncationContinue
        );
        assert_eq!(classify_stream_error("502 Bad Gateway"), StreamErrorKind::Transient);
        assert_eq!(
            classify_stream_error("Credit limit reached. Upgrade tier."),
            StreamErrorKind::CreditLimit
        );
        assert_eq!(
            classify_stream_error("subagent coordinator unreachable"),
            StreamErrorKind::Transient
        );
        assert!(retry_status_line("Subagent coordinator busy — retrying.").starts_with("Retry"));
        match parse_stream_line(r#"{"type":"task_failed","task_id":"t1","title":"todo"}"#) {
            Some(GrokPEvent::Task { id, title, done }) => {
                assert_eq!(id, "t1");
                assert!(!done);
                assert!(title.contains("Failed"), "{title}");
            }
            other => panic!("{other:?}"),
        }
        let folded = fold_stream(
            r#"
{"type":"text","data":"pong"}
{"type":"usage","usage":{"input_tokens":18007,"output_tokens":45,"reasoning_tokens":40}}
{"type":"end","stopReason":"end_turn","sessionId":"sid","usage":{"input_tokens":18007,"output_tokens":45,"reasoning_tokens":40,"total_tokens":18052},"num_turns":1}
"#,
        )
        .expect("fold usage");
        assert_eq!(folded.usage.reasoning_tokens, 40);
        assert_eq!(folded.stop_reason, "end_turn");
        let sig = parse_signals_json(
            r#"{"turnCount":2,"contextWindowUsage":14,"contextTokensUsed":72438,"contextWindowTokens":500000}"#,
        )
        .expect("signals");
        assert_eq!(sig.context_tokens_used, 72438);
        assert_eq!(sig.context_window_tokens, 500000);
        assert!(grok_context_line(&sig).contains("14%"), "{}", grok_context_line(&sig));
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
