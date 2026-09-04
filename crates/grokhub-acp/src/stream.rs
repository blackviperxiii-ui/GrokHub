//! Headless `grok -p --output-format streaming-json` events.

use serde_json::Value;
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::client::SingleTurn;
use crate::protocol::{parse_tool_card, ToolCard};

/// Server-reported spend and context. Grok Build 1.0.12+ includes reasoning.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GrokUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub total_tokens: u64,
    pub num_turns: u32,
    pub context_tokens_used: u64,
    pub context_window_tokens: u64,
    pub stop_reason: String,
}

impl GrokUsage {
    pub fn is_empty(&self) -> bool {
        self.input_tokens == 0
            && self.output_tokens == 0
            && self.reasoning_tokens == 0
            && self.total_tokens == 0
            && self.context_tokens_used == 0
            && self.context_window_tokens == 0
    }

    pub fn context_used(&self) -> u64 {
        if self.context_tokens_used > 0 {
            self.context_tokens_used
        } else {
            self.context_total()
        }
    }

    pub fn context_window(&self) -> u64 {
        if self.context_window_tokens > 0 {
            self.context_window_tokens
        } else {
            500_000
        }
    }

    pub fn context_total(&self) -> u64 {
        if self.total_tokens > 0 {
            self.total_tokens
        } else {
            self.input_tokens
                + self.cache_read_input_tokens
                + self.cache_creation_input_tokens
                + self.output_tokens
        }
    }

    pub fn merge(&mut self, other: &GrokUsage) {
        if other.input_tokens > 0 {
            self.input_tokens = other.input_tokens;
        }
        if other.output_tokens > 0 {
            self.output_tokens = other.output_tokens;
        }
        if other.reasoning_tokens > 0 {
            self.reasoning_tokens = other.reasoning_tokens;
        }
        if other.cache_read_input_tokens > 0 {
            self.cache_read_input_tokens = other.cache_read_input_tokens;
        }
        if other.cache_creation_input_tokens > 0 {
            self.cache_creation_input_tokens = other.cache_creation_input_tokens;
        }
        if other.total_tokens > 0 {
            self.total_tokens = other.total_tokens;
        }
        if other.num_turns > 0 {
            self.num_turns = other.num_turns;
        }
        if other.context_tokens_used > 0 {
            self.context_tokens_used = other.context_tokens_used;
        }
        if other.context_window_tokens > 0 {
            self.context_window_tokens = other.context_window_tokens;
        }
        if !other.stop_reason.is_empty() {
            self.stop_reason = other.stop_reason.clone();
        }
    }
}

pub fn grok_context_line(u: &GrokUsage) -> String {
    if u.is_empty() {
        return String::new();
    }
    let used = u.context_used();
    let window = u.context_window();
    let pct = (used.min(window) * 100).checked_div(window).unwrap_or(100) as u32;
    let mut s = format!("{pct}% · {}/{}", compact_k(used), compact_k(window));
    if u.reasoning_tokens > 0 {
        s.push_str(&format!(" · {} think", compact_k(u.reasoning_tokens)));
    }
    s
}

pub fn grok_usage_line(u: &GrokUsage) -> String {
    if u.is_empty() {
        return String::new();
    }
    let mut s = format!(
        "grok {} in / {} out",
        compact_k(u.input_tokens),
        compact_k(u.output_tokens)
    );
    if u.reasoning_tokens > 0 {
        s.push_str(&format!(" / {} think", compact_k(u.reasoning_tokens)));
    }
    if u.cache_read_input_tokens > 0 {
        s.push_str(&format!(" / {} cache", compact_k(u.cache_read_input_tokens)));
    }
    s
}

pub fn turn_footer(stop_reason: &str, usage: &GrokUsage) -> String {
    let reason = stop_reason.trim();
    let ctx = grok_context_line(usage);
    let head = match reason {
        "" | "end_turn" => {
            if ctx.is_empty() {
                return String::new();
            }
            "Done"
        }
        "cancelled" | "canceled" => "Cancelled",
        "max_tokens" => "Truncated — Grok is continuing",
        "max_turn_requests" | "max_turns_reached" => "Max turns",
        "refusal" => "Refused",
        other => other,
    };
    if ctx.is_empty() {
        head.to_string()
    } else {
        format!("{head} · {ctx}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamErrorKind {
    Fatal,
    Transient,
    TruncationContinue,
    CreditLimit,
}

pub fn classify_stream_error(msg: &str) -> StreamErrorKind {
    let l = msg.to_ascii_lowercase();
    if l.contains("credit")
        || l.contains("quota")
        || l.contains("usage limit")
        || l.contains("upgrade tier")
        || (l.contains("limit") && (l.contains("upsell") || l.contains("out of")))
    {
        StreamErrorKind::CreditLimit
    } else if l.contains("shorter answer")
        || l.contains("max_output")
        || l.contains("max_tokens")
        || (l.contains("truncat") && (l.contains("output") || l.contains("response") || l.contains("token")))
    {
        StreamErrorKind::TruncationContinue
    } else if ["500", "502", "503", "504"].iter().any(|c| {
        l.split(|ch: char| !ch.is_ascii_digit()).any(|w| w == *c)
    }) || l.contains("5xx")
        || l.contains("stall")
        || l.contains("dropped")
        || l.contains("timed out")
        || l.contains("timeout")
        || l.contains("unavailable")
        || l.contains("connection reset")
        || l.contains("econnreset")
        || l.contains("temporarily")
        || l.contains("try again later")
        || l.contains("unreachable")
        || l.contains("coordinator")
    {
        StreamErrorKind::Transient
    } else {
        StreamErrorKind::Fatal
    }
}

pub fn retry_status_line(msg: &str) -> String {
    let t = msg.trim();
    if t.is_empty() {
        return "Retrying…".into();
    }
    let l = t.to_ascii_lowercase();
    if l.starts_with("retry") {
        t.to_string()
    } else {
        format!("Retry: {t}")
    }
}

pub fn rewrite_truncation_error(msg: &str) -> String {
    match classify_stream_error(msg) {
        StreamErrorKind::TruncationContinue => {
            "Output hit the token limit. Grok is continuing automatically.".into()
        }
        StreamErrorKind::CreditLimit => {
            "Credit limit reached. Try Again retries the last prompt.".into()
        }
        StreamErrorKind::Transient => {
            let l = msg.to_ascii_lowercase();
            if l.contains("unreachable") || l.contains("coordinator") {
                "Subagent coordinator busy — retrying.".into()
            } else {
                "Grok hit a transient inference error and is retrying.".into()
            }
        }
        StreamErrorKind::Fatal => msg.to_string(),
    }
}

fn compact_k(n: u64) -> String {
    if n >= 1000 {
        format!("{}k", (n + 500) / 1000)
    } else {
        n.to_string()
    }
}

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

pub fn parse_usage(v: &Value) -> GrokUsage {
    let body = v.get("usage").unwrap_or(v);
    let mut u = GrokUsage {
        input_tokens: json_u64(body, &["input_tokens", "inputTokens"]),
        output_tokens: json_u64(body, &["output_tokens", "outputTokens"]),
        reasoning_tokens: json_u64(body, &["reasoning_tokens", "reasoningTokens"]),
        cache_read_input_tokens: json_u64(
            body,
            &["cache_read_input_tokens", "cacheReadInputTokens", "cachedReadTokens"],
        ),
        cache_creation_input_tokens: json_u64(
            body,
            &[
                "cache_creation_input_tokens",
                "cacheCreationInputTokens",
                "cacheCreationTokens",
            ],
        ),
        total_tokens: json_u64(body, &["total_tokens", "totalTokens"]),
        num_turns: json_u64(v, &["num_turns", "numTurns"]).min(u32::MAX as u64) as u32,
        context_tokens_used: json_u64(
            v,
            &["context_tokens_used", "contextTokensUsed", "tokens_used", "tokensUsed"],
        ),
        context_window_tokens: json_u64(
            v,
            &[
                "context_window_tokens",
                "contextWindowTokens",
                "context_window",
                "contextWindow",
            ],
        ),
        stop_reason: json_str(v, &["stopReason", "stop_reason"]),
    };
    if u.num_turns == 0 {
        u.num_turns = json_u64(body, &["num_turns", "numTurns", "modelCalls"]).min(u32::MAX as u64) as u32;
    }
    if u.context_tokens_used == 0 {
        u.context_tokens_used = json_u64(body, &["context_tokens_used", "contextTokensUsed"]);
    }
    if u.context_window_tokens == 0 {
        u.context_window_tokens = json_u64(body, &["context_window", "contextWindow", "contextWindowTokens"]);
    }
    if u.stop_reason.is_empty() {
        u.stop_reason = json_str(body, &["stopReason", "stop_reason"]);
    }
    if u.total_tokens == 0 {
        u.total_tokens = u.context_total();
    }
    u
}

pub fn parse_signals_json(raw: &str) -> Option<GrokUsage> {
    let v: Value = serde_json::from_str(raw).ok()?;
    let u = GrokUsage {
        context_tokens_used: json_u64(&v, &["contextTokensUsed", "context_tokens_used"]),
        context_window_tokens: json_u64(&v, &["contextWindowTokens", "context_window_tokens"]),
        num_turns: json_u64(&v, &["turnCount", "turn_count"]).min(u32::MAX as u64) as u32,
        ..GrokUsage::default()
    };
    if u.context_tokens_used == 0 && u.context_window_tokens == 0 {
        None
    } else {
        Some(u)
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

fn json_u64(v: &Value, keys: &[&str]) -> u64 {
    for k in keys {
        let Some(x) = v.get(*k) else { continue };
        if let Some(n) = x.as_u64() {
            return n;
        }
        if let Some(n) = x.as_i64() {
            return n.max(0) as u64;
        }
        if let Some(n) = x.as_f64() {
            return n.max(0.0) as u64;
        }
    }
    0
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
