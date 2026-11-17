//! Claude Code's `--output-format stream-json` lines, turned into the chat's
//! events. Pure: one line in, events out; the engine owns the process.
//! Shapes: code.claude.com/docs/en/headless and /agent-sdk/typescript.
//!
//! With `--include-partial-messages`, the reply's text and thinking arrive
//! twice: as `stream_event` deltas while Claude writes, then whole in an
//! `assistant` line. Claude Code sends each content block as its own
//! `assistant` line, right after that block's deltas, so a block's whole text
//! is shown only when no delta came since the last `assistant` line. Subagent messages (a `parent_tool_use_id`) show
//! their tool rows but not their text, which isn't the reply.

use std::collections::HashMap;

use grokhub_core::wire::{AcpEvent, GrokUsage, ToolCard};
use serde_json::Value;

/// A tool row's input summary and output stay this short, like native rows.
const SUMMARY_CHARS: usize = 200;
const OUTPUT_CHARS: usize = 2_000;

/// What a turn ended with, once its `result` line came.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Still running: no `result` yet.
    Running,
    Done,
    Failed(String),
}

/// How much of a limit window the plan has used, from a `rate_limit_event`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Headroom {
    /// Claude Code's window name, such as `five_hour` or `seven_day`.
    pub window: String,
    /// Whole percent used, 0-100, when Claude Code said.
    pub used_pct: Option<u8>,
    /// Unix seconds when the window resets, when Claude Code said.
    pub resets_at: Option<u64>,
    /// The plan refused the turn: its limit is reached.
    pub limited: bool,
}

/// One turn's stream.
#[derive(Debug)]
pub struct Stream {
    session_id: Option<String>,
    /// Tool names by `tool_use` id, so a result row keeps its name.
    tools: HashMap<String, String>,
    /// A delta came for the message being written, so its whole text is a repeat.
    text_streamed: bool,
    thought_streamed: bool,
    outcome: Outcome,
    headroom: Option<Headroom>,
}

impl Default for Stream {
    fn default() -> Self {
        Self::new()
    }
}

impl Stream {
    pub fn new() -> Self {
        Self {
            session_id: None,
            tools: HashMap::new(),
            text_streamed: false,
            thought_streamed: false,
            outcome: Outcome::Running,
            headroom: None,
        }
    }

    /// The session Claude Code named in its `init` line, for `--resume`.
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn outcome(&self) -> &Outcome {
        &self.outcome
    }

    /// The plan's newest limit reading this turn, if Claude Code sent one.
    pub fn headroom(&self) -> Option<&Headroom> {
        self.headroom.as_ref()
    }

    /// The chat events for one stdout line. A line that isn't JSON, or a kind
    /// this doesn't show, gives none.
    pub fn line(&mut self, line: &str) -> Vec<AcpEvent> {
        let Ok(msg) = serde_json::from_str::<Value>(line.trim()) else {
            return Vec::new();
        };
        match str_at(&msg, "type") {
            "system" => self.system(&msg),
            "stream_event" => self.partial(&msg),
            "assistant" => self.assistant(&msg),
            "user" => self.tool_results(&msg),
            "result" => self.result(&msg),
            "rate_limit_event" => {
                if let Some(h) = headroom(&msg["rate_limit_info"]) {
                    self.headroom = Some(h);
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// The turn's end, after stdout closed: the error to show when no
    /// `result` line came, or the one the `result` line gave.
    pub fn finish(&mut self) -> Result<(), String> {
        match &self.outcome {
            Outcome::Done => Ok(()),
            Outcome::Failed(why) => Err(why.clone()),
            Outcome::Running => {
                let why = "Claude Code stopped before it finished the turn.".to_string();
                self.outcome = Outcome::Failed(why.clone());
                Err(why)
            }
        }
    }

    fn system(&mut self, msg: &Value) -> Vec<AcpEvent> {
        if str_at(msg, "subtype") != "init" {
            return Vec::new();
        }
        let Some(id) = msg
            .get("session_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            return Vec::new();
        };
        self.session_id = Some(id.to_string());
        vec![AcpEvent::Ready {
            session_id: id.to_string(),
        }]
    }

    fn partial(&mut self, msg: &Value) -> Vec<AcpEvent> {
        if !msg["parent_tool_use_id"].is_null() {
            return Vec::new();
        }
        let event = &msg["event"];
        match str_at(event, "type") {
            "message_start" => {
                self.text_streamed = false;
                self.thought_streamed = false;
                Vec::new()
            }
            "content_block_delta" => {
                let delta = &event["delta"];
                match str_at(delta, "type") {
                    "text_delta" => {
                        self.text_streamed = true;
                        non_empty(str_at(delta, "text"))
                            .map(AcpEvent::Text)
                            .into_iter()
                            .collect()
                    }
                    "thinking_delta" => {
                        self.thought_streamed = true;
                        non_empty(str_at(delta, "thinking"))
                            .map(AcpEvent::Thought)
                            .into_iter()
                            .collect()
                    }
                    _ => Vec::new(),
                }
            }
            _ => Vec::new(),
        }
    }

    fn assistant(&mut self, msg: &Value) -> Vec<AcpEvent> {
        let main = msg["parent_tool_use_id"].is_null();
        let mut out = Vec::new();
        // An error message's text is Claude Code's own wording of the error;
        // the plain line below replaces it.
        let error = msg.get("error").and_then(Value::as_str).filter(|_| main);
        if let Some(code) = error {
            out.push(AcpEvent::Err(error_line(code)));
        }
        for block in blocks(&msg["message"]) {
            match str_at(block, "type") {
                "text" if main && error.is_none() && !self.text_streamed => {
                    out.extend(non_empty(str_at(block, "text")).map(AcpEvent::Text));
                }
                "thinking" if main && !self.thought_streamed => {
                    out.extend(non_empty(str_at(block, "thinking")).map(AcpEvent::Thought));
                }
                "tool_use" => {
                    let id = str_at(block, "id").to_string();
                    let name = str_at(block, "name").to_string();
                    if id.is_empty() || name.is_empty() {
                        continue;
                    }
                    self.tools.insert(id.clone(), name.clone());
                    out.push(tool(&id, &name, "in_progress", summary(&block["input"])));
                }
                _ => {}
            }
        }
        if main {
            // The next block starts fresh; its whole text is a repeat only if
            // deltas came for it.
            self.text_streamed = false;
            self.thought_streamed = false;
        }
        out
    }

    fn tool_results(&mut self, msg: &Value) -> Vec<AcpEvent> {
        blocks(&msg["message"])
            .filter(|block| str_at(block, "type") == "tool_result")
            .filter_map(|block| {
                let id = str_at(block, "tool_use_id");
                let name = self.tools.get(id)?.clone();
                let failed = block["is_error"].as_bool().unwrap_or(false);
                let status = if failed { "failed" } else { "completed" };
                Some(tool(
                    id,
                    &name,
                    status,
                    clip(&result_text(&block["content"]), OUTPUT_CHARS),
                ))
            })
            .collect()
    }

    fn result(&mut self, msg: &Value) -> Vec<AcpEvent> {
        let mut out: Vec<AcpEvent> = msg["permission_denials"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|denial| {
                let id = str_at(denial, "tool_use_id");
                let name = self
                    .tools
                    .get(id)
                    .cloned()
                    .or_else(|| non_empty(str_at(denial, "tool_name")))?;
                Some(tool(
                    id,
                    &name,
                    "failed",
                    "Not allowed in this chat.".into(),
                ))
            })
            .collect();
        out.push(AcpEvent::Usage(usage(msg)));
        let failed =
            msg["is_error"].as_bool().unwrap_or(false) || str_at(msg, "subtype") != "success";
        self.outcome = if failed {
            Outcome::Failed(result_error(msg))
        } else {
            out.push(AcpEvent::Done {
                stop_reason: "end_turn".into(),
            });
            Outcome::Done
        };
        out
    }
}

fn str_at<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn non_empty(s: &str) -> Option<String> {
    (!s.is_empty()).then(|| s.to_string())
}

fn blocks(message: &Value) -> impl Iterator<Item = &Value> {
    message["content"].as_array().into_iter().flatten()
}

fn tool(id: &str, name: &str, status: &str, detail: String) -> AcpEvent {
    AcpEvent::Tool(ToolCard {
        id: id.to_string(),
        title: name.to_string(),
        kind: name.to_string(),
        status: status.to_string(),
        detail,
        diff: String::new(),
        image_data_url: None,
        raw_input: String::new(),
    })
}

/// The part of a tool's input a person reads first: the command, the file,
/// the pattern or the link, else the input itself, short.
fn summary(input: &Value) -> String {
    for key in [
        "command",
        "file_path",
        "path",
        "pattern",
        "url",
        "query",
        "description",
    ] {
        if let Some(s) = input
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            return clip(s, SUMMARY_CHARS);
        }
    }
    match input {
        Value::Null => String::new(),
        Value::Object(map) if map.is_empty() => String::new(),
        other => clip(&other.to_string(), SUMMARY_CHARS),
    }
}

/// A `tool_result`'s content: a string, or text blocks.
fn result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter(|p| str_at(p, "type") == "text")
            .map(|p| str_at(p, "text"))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn clip(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}

/// The `result` line's tokens. `modelUsage` covers every model the turn used
/// (subagents too), so it's preferred; `usage` is the main loop only.
fn usage(msg: &Value) -> GrokUsage {
    let n = |v: &Value, key: &str| v.get(key).and_then(Value::as_u64).unwrap_or(0);
    let mut u = GrokUsage {
        num_turns: msg["num_turns"]
            .as_u64()
            .unwrap_or(0)
            .min(u64::from(u32::MAX)) as u32,
        stop_reason: str_at(msg, "stop_reason").to_string(),
        meter: super::engine::METER.into(),
        ..GrokUsage::default()
    };
    match msg["modelUsage"].as_object().filter(|m| !m.is_empty()) {
        Some(models) => {
            for m in models.values() {
                u.input_tokens += n(m, "inputTokens");
                u.output_tokens += n(m, "outputTokens");
                u.cache_read_input_tokens += n(m, "cacheReadInputTokens");
                u.cache_creation_input_tokens += n(m, "cacheCreationInputTokens");
                u.context_window_tokens = u.context_window_tokens.max(n(m, "contextWindow"));
            }
        }
        None => {
            let main = &msg["usage"];
            u.input_tokens = n(main, "input_tokens");
            u.output_tokens = n(main, "output_tokens");
            u.cache_read_input_tokens = n(main, "cache_read_input_tokens");
            u.cache_creation_input_tokens = n(main, "cache_creation_input_tokens");
        }
    }
    u.total_tokens = u.input_tokens
        + u.output_tokens
        + u.cache_read_input_tokens
        + u.cache_creation_input_tokens;
    u
}

/// The window nearest its limit. `unifiedWindows` lists each window the plan
/// has; without it, the line's own `rateLimitType` is the one.
fn headroom(info: &Value) -> Option<Headroom> {
    let reading = |name: &str, w: &Value| Headroom {
        window: name.to_string(),
        used_pct: w["utilization"]
            .as_f64()
            .filter(|u| u.is_finite())
            .map(|u| (u.clamp(0.0, 1.0) * 100.0).round() as u8),
        resets_at: w["resetsAt"].as_u64(),
        limited: str_at(w, "status") == "rejected",
    };
    let windows = info["unifiedWindows"].as_object().filter(|m| !m.is_empty());
    match windows {
        Some(windows) => windows
            .iter()
            .map(|(name, w)| reading(name, w))
            .max_by_key(|h| (h.limited, h.used_pct.unwrap_or(0))),
        None => non_empty(str_at(info, "rateLimitType")).map(|name| reading(&name, info)),
    }
}

/// The words for a failed `result`: its `errors`, else its `result` text,
/// else what kind of stop it was.
fn result_error(msg: &Value) -> String {
    let errors: Vec<&str> = msg["errors"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|s| !s.is_empty())
        .collect();
    if !errors.is_empty() {
        return clip(&errors.join(" "), OUTPUT_CHARS);
    }
    if let Some(text) = non_empty(str_at(msg, "result")) {
        return clip(&text, OUTPUT_CHARS);
    }
    match str_at(msg, "subtype") {
        "error_max_turns" => "Claude Code reached this chat's step limit.".into(),
        "error_max_budget_usd" => "Claude Code reached this chat's spend limit.".into(),
        _ => "Claude Code couldn't finish the turn.".into(),
    }
}

/// An `assistant.error` code in plain words.
fn error_line(code: &str) -> String {
    match code {
        "authentication_failed" => {
            "Claude Code isn't signed in. In a terminal, run claude auth login, then try again."
                .into()
        }
        "rate_limit" => "Your Claude plan's limit was reached. Try again after it resets.".into(),
        "overloaded" | "server_error" => "Claude is busy right now. Try again in a moment.".into(),
        "billing_error" | "account_on_hold" => {
            "Claude says there's a problem with your account's billing. Check it on claude.ai."
                .into()
        }
        "model_not_found" => "That Claude model isn't available on your plan.".into(),
        other => format!("Claude Code reported an error ({other})."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(lines: &[&str]) -> (Vec<AcpEvent>, Stream) {
        let mut s = Stream::new();
        let events = lines.iter().flat_map(|l| s.line(l)).collect();
        (events, s)
    }

    fn row(id: &str, name: &str, status: &str, detail: &str) -> AcpEvent {
        tool(id, name, status, detail.into())
    }

    const INIT: &str = r#"{"type":"system","subtype":"init","session_id":"5d2c6b1e-0000-4000-8000-000000000001","model":"claude-opus","tools":["Read","Grep"],"apiKeySource":"none"}"#;
    const RESULT_OK: &str = r#"{"type":"result","subtype":"success","is_error":false,"num_turns":2,"result":"Two files.","stop_reason":"end_turn","session_id":"5d2c6b1e-0000-4000-8000-000000000001","usage":{"input_tokens":9,"output_tokens":9},"modelUsage":{"claude-opus":{"inputTokens":1200,"outputTokens":80,"cacheReadInputTokens":3000,"cacheCreationInputTokens":500,"contextWindow":200000},"claude-haiku":{"inputTokens":100,"outputTokens":20,"cacheReadInputTokens":0,"cacheCreationInputTokens":0,"contextWindow":200000}},"permission_denials":[]}"#;

    fn done_usage() -> GrokUsage {
        GrokUsage {
            input_tokens: 1300,
            output_tokens: 100,
            cache_read_input_tokens: 3000,
            cache_creation_input_tokens: 500,
            total_tokens: 4900,
            num_turns: 2,
            context_window_tokens: 200_000,
            stop_reason: "end_turn".into(),
            meter: "Claude plan".into(),
            ..GrokUsage::default()
        }
    }

    #[test]
    fn a_streamed_turn_with_a_tool_shows_each_delta_once_then_the_row_then_done() {
        let (events, mut s) = run(&[
            INIT,
            r#"{"type":"stream_event","parent_tool_use_id":null,"event":{"type":"message_start"}}"#,
            r#"{"type":"stream_event","parent_tool_use_id":null,"event":{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"Look first."}}}"#,
            r#"{"type":"stream_event","parent_tool_use_id":null,"event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Let me "}}}"#,
            r#"{"type":"stream_event","parent_tool_use_id":null,"event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"check."}}}"#,
            r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"thinking","thinking":"Look first."},{"type":"text","text":"Let me check."},{"type":"tool_use","id":"toolu_1","name":"Glob","input":{"pattern":"src/*.rs"}}]}}"#,
            r#"{"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"src/main.rs\nsrc/lib.rs","is_error":false}]}}"#,
            r#"{"type":"stream_event","parent_tool_use_id":null,"event":{"type":"message_start"}}"#,
            r#"{"type":"stream_event","parent_tool_use_id":null,"event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Two files."}}}"#,
            r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"text","text":"Two files."}]}}"#,
            RESULT_OK,
        ]);
        assert_eq!(
            events,
            vec![
                AcpEvent::Ready {
                    session_id: "5d2c6b1e-0000-4000-8000-000000000001".into()
                },
                AcpEvent::Thought("Look first.".into()),
                AcpEvent::Text("Let me ".into()),
                AcpEvent::Text("check.".into()),
                row("toolu_1", "Glob", "in_progress", "src/*.rs"),
                row("toolu_1", "Glob", "completed", "src/main.rs\nsrc/lib.rs"),
                AcpEvent::Text("Two files.".into()),
                AcpEvent::Usage(done_usage()),
                AcpEvent::Done {
                    stop_reason: "end_turn".into()
                },
            ]
        );
        assert_eq!(s.session_id(), Some("5d2c6b1e-0000-4000-8000-000000000001"));
        assert_eq!(s.finish(), Ok(()));
    }

    #[test]
    fn each_block_arrives_whole_after_its_own_deltas_and_shows_once() {
        // The order a real `claude -p` 2.1.325 turn gave: an empty (hidden)
        // thinking block, then the text block, each followed by its own line.
        let (events, _) = run(&[
            r#"{"type":"stream_event","parent_tool_use_id":null,"event":{"type":"message_start"}}"#,
            r#"{"type":"stream_event","parent_tool_use_id":null,"event":{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":""}}}"#,
            r#"{"type":"assistant","parent_tool_use_id":null,"message":{"id":"msg_1","content":[{"type":"thinking","thinking":""}]}}"#,
            r#"{"type":"stream_event","parent_tool_use_id":null,"event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"2"}}}"#,
            r#"{"type":"assistant","parent_tool_use_id":null,"message":{"id":"msg_1","content":[{"type":"text","text":"2"}]}}"#,
        ]);
        assert_eq!(events, vec![AcpEvent::Text("2".into())]);
    }

    #[test]
    fn without_partial_messages_the_whole_message_is_the_reply() {
        let (events, _) = run(&[
            r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"text","text":"Hello."}]}}"#,
            r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"text","text":"Still here."}]}}"#,
        ]);
        assert_eq!(
            events,
            vec![
                AcpEvent::Text("Hello.".into()),
                AcpEvent::Text("Still here.".into())
            ]
        );
    }

    #[test]
    fn a_subagent_shows_its_tool_rows_but_not_its_words() {
        let (events, _) = run(&[
            r#"{"type":"assistant","parent_tool_use_id":"toolu_task","message":{"content":[{"type":"text","text":"Searching the repo."},{"type":"tool_use","id":"toolu_2","name":"Bash","input":{"command":"git log -1","description":"Last commit"}}]}}"#,
            r#"{"type":"user","parent_tool_use_id":"toolu_task","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_2","content":[{"type":"text","text":"abc123 Fix"}],"is_error":false}]}}"#,
            r#"{"type":"stream_event","parent_tool_use_id":"toolu_task","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"inner"}}}"#,
        ]);
        assert_eq!(
            events,
            vec![
                row("toolu_2", "Bash", "in_progress", "git log -1"),
                row("toolu_2", "Bash", "completed", "abc123 Fix"),
            ]
        );
    }

    #[test]
    fn a_refused_tool_fails_its_row_and_the_turn_still_ends() {
        let (events, mut s) = run(&[
            r#"{"type":"assistant","parent_tool_use_id":null,"message":{"content":[{"type":"tool_use","id":"toolu_3","name":"Bash","input":{"command":"rm -rf build"}}]}}"#,
            r#"{"type":"user","parent_tool_use_id":null,"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_3","content":"Claude requested permissions to use Bash, but you haven't granted it yet.","is_error":true}]}}"#,
            r#"{"type":"result","subtype":"success","is_error":false,"num_turns":1,"result":"I couldn't run it.","stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":0,"cache_creation_input_tokens":0},"permission_denials":[{"tool_name":"Bash","tool_use_id":"toolu_3","tool_input":{"command":"rm -rf build"}}]}"#,
        ]);
        assert_eq!(
            events,
            vec![
                row("toolu_3", "Bash", "in_progress", "rm -rf build"),
                row(
                    "toolu_3",
                    "Bash",
                    "failed",
                    "Claude requested permissions to use Bash, but you haven't granted it yet."
                ),
                row("toolu_3", "Bash", "failed", "Not allowed in this chat."),
                AcpEvent::Usage(GrokUsage {
                    input_tokens: 10,
                    output_tokens: 5,
                    total_tokens: 15,
                    num_turns: 1,
                    stop_reason: "end_turn".into(),
                    meter: "Claude plan".into(),
                    ..GrokUsage::default()
                }),
                AcpEvent::Done {
                    stop_reason: "end_turn".into()
                },
            ]
        );
        assert_eq!(s.finish(), Ok(()));
    }

    #[test]
    fn a_signed_out_cli_says_how_to_sign_in_and_the_turn_fails() {
        let (events, mut s) = run(&[
            r#"{"type":"assistant","parent_tool_use_id":null,"error":"authentication_failed","message":{"content":[{"type":"text","text":"Invalid API key · Please run /login"}]}}"#,
            r#"{"type":"result","subtype":"success","is_error":true,"num_turns":1,"result":"Invalid API key · Please run /login","usage":{}}"#,
        ]);
        assert_eq!(
            events[0],
            AcpEvent::Err(
                "Claude Code isn't signed in. In a terminal, run claude auth login, then try again."
                    .into()
            )
        );
        assert!(!events
            .iter()
            .any(|e| matches!(e, AcpEvent::Done { .. } | AcpEvent::Text(_))));
        assert_eq!(
            s.finish(),
            Err("Invalid API key · Please run /login".into())
        );
    }

    #[test]
    fn a_failed_result_gives_its_errors_or_names_the_limit() {
        let (_, mut s) = run(&[
            r#"{"type":"result","subtype":"error_during_execution","is_error":true,"errors":["Tool crashed.","Stopped."]}"#,
        ]);
        assert_eq!(s.finish(), Err("Tool crashed. Stopped.".into()));
        let (_, mut s) = run(&[r#"{"type":"result","subtype":"error_max_turns","is_error":true}"#]);
        assert_eq!(
            s.finish(),
            Err("Claude Code reached this chat's step limit.".into())
        );
    }

    #[test]
    fn a_rate_limit_line_keeps_the_window_nearest_its_limit() {
        // Shaped like Claude Code 2.1's line on a signed-in plan.
        let (events, s) = run(&[
            r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1795341600,"rateLimitType":"seven_day","overageStatus":"rejected","isUsingOverage":false,"unifiedWindows":{"five_hour":{"utilization":0.12,"resetsAt":1794900000,"status":"allowed"},"seven_day":{"utilization":0.43,"resetsAt":1795341600,"status":"allowed"}}}}"#,
        ]);
        assert!(events.is_empty(), "{events:?}");
        assert_eq!(
            s.headroom(),
            Some(&Headroom {
                window: "seven_day".into(),
                used_pct: Some(43),
                resets_at: Some(1_795_341_600),
                limited: false,
            })
        );
    }

    #[test]
    fn a_refused_window_wins_and_a_later_line_replaces_an_earlier_one() {
        let (_, s) = run(&[
            r#"{"type":"rate_limit_event","rate_limit_info":{"unifiedWindows":{"seven_day":{"utilization":0.5}}}}"#,
            r#"{"type":"rate_limit_event","rate_limit_info":{"unifiedWindows":{"five_hour":{"utilization":1.0,"resetsAt":1794900000,"status":"rejected"},"seven_day":{"utilization":1.7,"status":"allowed"}}}}"#,
        ]);
        assert_eq!(
            s.headroom(),
            Some(&Headroom {
                window: "five_hour".into(),
                used_pct: Some(100),
                resets_at: Some(1_794_900_000),
                limited: true,
            })
        );
    }

    #[test]
    fn a_rate_limit_line_without_windows_reads_its_own_fields() {
        let (_, s) = run(&[
            r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed_warning","rateLimitType":"five_hour","resetsAt":1794900000}}"#,
            r#"{"type":"rate_limit_event","rate_limit_info":{}}"#,
            r#"{"type":"rate_limit_event"}"#,
        ]);
        assert_eq!(
            s.headroom(),
            Some(&Headroom {
                window: "five_hour".into(),
                used_pct: None,
                resets_at: Some(1_794_900_000),
                limited: false,
            })
        );
    }

    #[test]
    fn a_stream_that_stops_without_a_result_fails_the_turn() {
        let (events, mut s) = run(&[INIT, "not json", "", r#"{"type":"rate_limit_event"}"#]);
        assert_eq!(events.len(), 1);
        assert_eq!(s.outcome(), &Outcome::Running);
        assert_eq!(
            s.finish(),
            Err("Claude Code stopped before it finished the turn.".into())
        );
    }

    #[test]
    fn long_inputs_and_outputs_are_clipped_and_unknown_results_are_skipped() {
        let long = "x".repeat(5_000);
        let input = format!(
            r#"{{"type":"assistant","parent_tool_use_id":null,"message":{{"content":[{{"type":"tool_use","id":"t","name":"Write","input":{{"file_path":"{long}"}}}}]}}}}"#
        );
        let output = format!(
            r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","tool_use_id":"t","content":"{long}"}},{{"type":"tool_result","tool_use_id":"nobody","content":"x"}}]}}}}"#
        );
        let (events, _) = run(&[&input, &output]);
        let AcpEvent::Tool(started) = &events[0] else {
            panic!("{events:?}")
        };
        assert_eq!(started.detail.chars().count(), SUMMARY_CHARS + 1);
        let AcpEvent::Tool(ended) = &events[1] else {
            panic!("{events:?}")
        };
        assert_eq!(ended.detail.chars().count(), OUTPUT_CHARS + 1);
        assert_eq!(events.len(), 2);
    }
}
