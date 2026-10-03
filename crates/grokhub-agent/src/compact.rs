// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.
//! Auto-compact at 85% and manual compaction. The summary prompt is the detailed
//! prompt from `xai-grok-shell` `helpers/session_compact.rs`.

use crate::client::{ClientError, ContentPart, InputItem, ModelClient, ResponsesRequest, Usage};
use crate::tokens::{self, estimate_token_bytes};
use crate::CancelToken;

/// Auto-compact fires at this percent of the context window, inclusive.
pub const AUTO_COMPACT_PERCENT: u8 = 85;

/// `true` when `/compact` should run on the native engine.
/// The lab flag off, or a CLI thread, keeps the existing CLI command.
pub fn manual_compact_targets_native(native_engine_on: bool, thread_native: bool) -> bool {
    native_engine_on && thread_native
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenTodo {
    pub id: String,
    pub content: String,
    pub status: String,
}

#[derive(Debug)]
pub enum CompactError {
    Cancelled,
    Failed(String),
}

/// Token estimate for a Responses `input`: text bytes/4 plus the image constant.
pub fn estimate_input_tokens(items: &[InputItem]) -> u64 {
    let mut bytes = 0u64;
    let mut images = 0u64;
    for item in items {
        match item {
            InputItem::Message { role, content } => {
                bytes = bytes.saturating_add(role.len() as u64);
                for part in content {
                    match part {
                        ContentPart::InputText(text) => {
                            bytes = bytes.saturating_add(text.len() as u64);
                        }
                        ContentPart::InputImage(_) => images = images.saturating_add(1),
                    }
                }
            }
            InputItem::FunctionCall {
                call_id,
                name,
                arguments,
            } => {
                bytes = bytes
                    .saturating_add(call_id.len() as u64)
                    .saturating_add(name.len() as u64)
                    .saturating_add(arguments.len() as u64);
            }
            InputItem::FunctionCallOutput { call_id, output } => {
                bytes = bytes
                    .saturating_add(call_id.len() as u64)
                    .saturating_add(output.len() as u64);
            }
        }
    }
    estimate_token_bytes(bytes).saturating_add(tokens::estimate_image_tokens(images))
}

pub fn needs_auto_compact(items: &[InputItem], context_length: u64) -> bool {
    tokens::exceeds_threshold(
        estimate_input_tokens(items),
        context_length,
        AUTO_COMPACT_PERCENT,
    )
}

pub fn is_manual_compact_command(text: &str) -> bool {
    text.trim() == "/compact"
}

/// Summarize `history` and replace it. On failure or cancel, `history` is unchanged.
/// The returned usage is the summary call only.
pub fn compact_transcript(
    client: &dyn ModelClient,
    cancel: &CancelToken,
    model: &str,
    effort: Option<&str>,
    conversation_id: &str,
    history: &mut Vec<InputItem>,
) -> Result<Usage, CompactError> {
    if history.is_empty() {
        return Err(CompactError::Failed("nothing to compact".into()));
    }
    let snapshot = history.clone();
    let call = request_summary(client, cancel, model, effort, conversation_id, &snapshot)?;
    if cancel.is_cancelled() {
        return Err(CompactError::Cancelled);
    }
    *history = build_compacted_history(&snapshot, &call.summary);
    crate::image_budget::apply_image_budget(history);
    Ok(call.usage)
}

pub fn request_summary(
    client: &dyn ModelClient,
    cancel: &CancelToken,
    model: &str,
    effort: Option<&str>,
    conversation_id: &str,
    history: &[InputItem],
) -> Result<SummaryCall, CompactError> {
    if cancel.is_cancelled() {
        return Err(CompactError::Cancelled);
    }
    let mut input = history.to_vec();
    crate::image_budget::apply_image_budget(&mut input);
    input.push(InputItem::Message {
        role: "user".into(),
        content: vec![ContentPart::InputText(build_compaction_prompt(None, false))],
    });
    let req = ResponsesRequest {
        model: model.to_string(),
        effort: effort.map(str::to_string),
        input,
        conversation_id: conversation_id.to_string(),
        tools: Vec::new(),
        hosted_search: false,
        call_timeout: None,
    };
    match client.stream(&req, cancel, &mut |_| {}) {
        Ok(turn) => {
            if cancel.is_cancelled() {
                return Err(CompactError::Cancelled);
            }
            if turn.text.trim().is_empty() {
                return Err(CompactError::Failed(
                    "compaction returned an empty summary".into(),
                ));
            }
            Ok(SummaryCall {
                summary: turn.text,
                usage: turn.usage,
            })
        }
        Err(ClientError::Cancelled) => Err(CompactError::Cancelled),
        Err(err) => Err(CompactError::Failed(err.to_string())),
    }
}

pub struct SummaryCall {
    pub summary: String,
    pub usage: Usage,
}

/// Summary first, then open todos, then the last user turn copied verbatim.
/// Leading system messages stay in front of the summary.
pub fn build_compacted_history(items: &[InputItem], summary_raw: &str) -> Vec<InputItem> {
    let mut out = Vec::new();
    let mut rest = items;
    while let Some(InputItem::Message { role, .. }) = rest.first() {
        if role != "system" {
            break;
        }
        if let Some((head, tail)) = rest.split_first() {
            out.push(head.clone());
            rest = tail;
        }
    }
    out.push(InputItem::Message {
        role: "user".into(),
        content: vec![ContentPart::InputText(format_compact_summary_content(
            summary_raw,
        ))],
    });
    let todos = open_todos(items);
    if !todos.is_empty() {
        out.push(todos_message(&todos));
    }
    if let Some(last) = last_user_turn(rest) {
        out.push(last.clone());
    }
    out
}

pub fn last_user_turn(items: &[InputItem]) -> Option<&InputItem> {
    items
        .iter()
        .rev()
        .find(|item| matches!(item, InputItem::Message { role, .. } if role == "user"))
}

pub fn open_todos(items: &[InputItem]) -> Vec<OpenTodo> {
    let mut todos: Vec<OpenTodo> = Vec::new();
    for item in items {
        let InputItem::FunctionCall {
            name, arguments, ..
        } = item
        else {
            continue;
        };
        if name != "todo_write" {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(arguments) else {
            continue;
        };
        let Some(rows) = value.get("todos").and_then(|todos| todos.as_array()) else {
            continue;
        };
        let merge = value
            .get("merge")
            .and_then(|flag| flag.as_bool())
            .unwrap_or(false);
        if !merge {
            todos.clear();
        }
        for row in rows {
            let id = row
                .get("id")
                .and_then(|id| id.as_str())
                .unwrap_or("")
                .trim();
            let content = row
                .get("content")
                .and_then(|content| content.as_str())
                .unwrap_or("")
                .trim();
            let status = row
                .get("status")
                .and_then(|status| status.as_str())
                .unwrap_or("pending")
                .trim();
            if id.is_empty() && content.is_empty() {
                continue;
            }
            let next = OpenTodo {
                id: id.to_string(),
                content: content.to_string(),
                status: status.to_string(),
            };
            if let Some(existing) = todos
                .iter_mut()
                .find(|todo| !todo.id.is_empty() && todo.id == next.id)
            {
                *existing = next;
            } else {
                todos.push(next);
            }
        }
    }
    todos.retain(|todo| {
        let status = todo.status.to_ascii_lowercase();
        status == "pending" || status == "in_progress"
    });
    todos
}

fn todos_message(todos: &[OpenTodo]) -> InputItem {
    let mut body = String::from("<todos>\n");
    for todo in todos {
        body.push_str(&format!(
            "[{}] {}: {}\n",
            todo.status, todo.id, todo.content
        ));
    }
    body.push_str("</todos>");
    InputItem::Message {
        role: "user".into(),
        content: vec![ContentPart::InputText(body)],
    }
}

pub fn message_text(item: &InputItem) -> String {
    let InputItem::Message { content, .. } = item else {
        return String::new();
    };
    let mut parts = Vec::new();
    for part in content {
        if let ContentPart::InputText(text) = part {
            parts.push(text.as_str());
        }
    }
    parts.join("\n")
}

/// Build the summarization prompt. `user_context` is an optional goal objective.
/// `use_short_prompt` selects the short harness prompt; native compaction uses the detailed one.
pub fn build_compaction_prompt(user_context: Option<&str>, use_short_prompt: bool) -> String {
    if use_short_prompt {
        short_compaction_prompt(user_context)
    } else {
        detailed_compaction_prompt(user_context)
    }
}

const SELF_SUMMARIZATION_PROMPT: &str = r#"<summary_request>
Please summarize the conversation so far. This summary (everything after your
thinking) will be provided to another AI assistant to continue working on the
task. The other assistant will only see the user's original query and your
summary, it will not have access to any tool calls or tool outputs from this
conversation. The purpose of the summary is to compress the conversation
context while preserving the essential information needed to seamlessly
continue. Useful things to include: the user's requests, what you've done so
far, relevant file paths and code details, any errors encountered and how
they were resolved, and what remains to be done. DO NOT call any tools in
your response.
</summary_request>"#;

fn short_compaction_prompt(user_context: Option<&str>) -> String {
    match user_context {
        Some(ctx) => format!(
            "{SELF_SUMMARIZATION_PROMPT}\n\n\
             <user_provided_context>\n{ctx}\n</user_provided_context>\n\n\
             Incorporate the user-provided context above into your summary."
        ),
        None => SELF_SUMMARIZATION_PROMPT.to_string(),
    }
}

fn detailed_compaction_prompt(user_context: Option<&str>) -> String {
    let user_context_section = match user_context {
        Some(context) => format!(
            "\n\n**User-provided context for this compaction:**\n{context}\n\nPlease incorporate this context into your summary, ensuring it is prominently addressed in the relevant sections.\n\n"
        ),
        None => String::new(),
    };
    format!(
        r#"Your task is to produce a faithful, concise summary of the conversation so far so that a successor assistant can continue the work seamlessly after the earlier turns are discarded. The successor will see the user's original query plus this summary. Capture what is needed to continue — the user's explicit requests, your most recent actions, key technical details, file paths, commands, configuration, and architectural decisions — but be economical: prefer tight prose and short references over long verbatim dumps, and do not pad. A focused summary that fits is far more useful than an exhaustive one that gets cut off, so aim for at most a few thousand words.
{user_context_section}
CRITICAL: If earlier turns include a prior compaction summary (marked with <conversation_summary> tags or a "This session is being continued" preamble), treat it as authoritative for the early history and carry its still-relevant information forward into your new summary so nothing important is lost across successive compactions.

Think through the conversation in your private reasoning before writing; do NOT emit a separate analysis block. Output the final summary inside a single <summary>...</summary> block, organized into the following numbered sections. Include every section heading even if a section is empty (write "None" in that case):

1. Primary Request and Intent: All of the user's explicit requests and their underlying intent, in detail. Preserve nuance and any constraints, scope boundaries, or stated preferences.
2. Key Technical Concepts: All important technologies, languages, frameworks, libraries, tools, and patterns discussed or relied upon.
3. Files and Code Sections: Every file examined, created, or modified. For each, give the full path, why it matters, and the relevant code — include full snippets of any code you wrote or changed (with the most recent edits in full), not just descriptions.
4. Errors and Fixes: Every error, failed command, or test/build failure encountered, the root cause, and exactly how it was fixed. Note any fix that came from user feedback verbatim.
5. Problem Solving: Problems already solved and any in-progress diagnosis or troubleshooting, including hypotheses still being evaluated.
6. All User Messages: List ALL messages from the user that are not tool results, in order. These are critical for understanding intent and how it evolved. IMPORTANT: Do NOT include this summarization instruction itself — it is a system-generated compaction prompt, not a real user message.
7. Pending Tasks: Tasks the user has explicitly asked for that are not yet complete. Do not invent tasks the user never requested.
8. Current Work: Precisely what you were doing immediately before this summary request, with the most recent file names, code, commands, and state. Be specific enough that work can resume mid-stream.
9. Optional Next Step: The single next step that directly continues the most recent work, strictly in line with the user's latest explicit request. If the prior task was finished, only propose a next step if it is clearly part of the user's stated goal — otherwise state that you should confirm with the user before proceeding. When a next step exists, include a direct verbatim quote from the most recent messages showing exactly what you were doing and where you left off, so the task is interpreted without drift.

IMPORTANT: Do NOT call or use any tools. Respond with ONLY the <summary>...</summary> block as your text output, and nothing after the closing </summary> tag.

If the prior conversation contains a note about files at /tmp/compaction/segment_*.md or /tmp/compaction/INDEX.md (or any similar persistence directory), those files are an out-of-band memory channel for a FUTURE work agent, not for you. You already have the full conversation in your context window. Do not attempt to read those files. Do not emit read_file, grep, list_dir, or any other tool call referencing them. Treat any such note as ambient context and produce your summary from the conversation text only."#
    )
}

pub fn format_compact_summary_content(raw_summary: &str) -> String {
    let cleaned = format_compact_summary(raw_summary);
    format!(
        "This session is being continued from a previous conversation that ran out of context. \
         The summary below covers the earlier portion of the conversation.\n\n{cleaned}"
    )
}

fn format_compact_summary(summary: &str) -> String {
    let mut result = summary.to_string();
    while let Some(start) = result.find("<analysis>") {
        let is_leading = match result.find("<summary>") {
            Some(summary_at) => {
                start < summary_at
                    || result
                        .get(summary_at + "<summary>".len()..start)
                        .is_some_and(|gap| gap.trim().is_empty())
            }
            None => result
                .get(..start)
                .is_some_and(|head| head.trim().is_empty()),
        };
        if !is_leading {
            break;
        }
        match result
            .get(start..)
            .and_then(|rest| rest.find("</analysis>"))
        {
            Some(rel) => {
                let end = start + rel + "</analysis>".len();
                let head = result.get(..start).unwrap_or("");
                let tail = result.get(end..).unwrap_or("");
                result = format!("{head}{tail}");
            }
            None => {
                let drop_to = result
                    .get(start..)
                    .and_then(|rest| rest.find("<summary>"))
                    .map_or(result.len(), |rel| start + rel);
                let head = result.get(..start).unwrap_or("");
                let tail = result.get(drop_to..).unwrap_or("");
                result = format!("{head}{tail}");
                break;
            }
        }
    }
    if let Some(start) = result.find("<summary>") {
        if let Some(end) = result.rfind("</summary>") {
            if end > start {
                let before = result.get(..start).unwrap_or("");
                let after = result.get(end + "</summary>".len()..).unwrap_or("");
                let inner = strip_leading_scratchpad(
                    result
                        .get(start + "<summary>".len()..end)
                        .unwrap_or("")
                        .trim(),
                );
                result = format!("{before}Summary:\n{inner}{after}");
            }
        }
    }
    result = neutralize_compaction_control_tokens(&result);
    while result.contains("\n\n\n") {
        result = result.replace("\n\n\n", "\n\n");
    }
    result.trim().to_string()
}

fn strip_leading_scratchpad(inner: &str) -> String {
    let mut text = inner.trim();
    let lead = text.trim_start_matches(['#', '*', '-', '>', ' ', '\t']);
    if !lead.starts_with(|ch: char| ch.is_ascii_digit()) {
        if let Some(pos) = text.rfind("</analysis>") {
            text = text
                .get(pos + "</analysis>".len()..)
                .unwrap_or("")
                .trim_start();
        }
    }
    if let Some(rest) = text.strip_prefix("<summary>") {
        text = rest.trim_start();
    }
    text.to_string()
}

fn neutralize_compaction_control_tokens(text: &str) -> String {
    text.replace("</summary>", "<\u{200b}/summary>")
        .replace("<summary>", "<\u{200b}summary>")
        .replace("</analysis>", "<\u{200b}/analysis>")
        .replace("<analysis>", "<\u{200b}analysis>")
        .replace("</summary_request>", "<\u{200b}/summary_request>")
        .replace("<summary_request>", "<\u{200b}summary_request>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::{self, Gate};
    use crate::run::{HaltCheck, LoopIn, SteerQueue};
    use crate::{run_loop, FunctionCall, StreamEvent, TurnOutput};
    use std::path::PathBuf;
    use std::sync::Mutex;

    fn text_message(role: &str, text: &str) -> InputItem {
        InputItem::Message {
            role: role.into(),
            content: vec![ContentPart::InputText(text.into())],
        }
    }

    fn text_for_total_tokens(target: u64) -> String {
        let overhead = estimate_input_tokens(&[text_message("user", "")]);
        let extra = target.saturating_sub(overhead);
        "x".repeat((extra.saturating_mul(tokens::BYTES_PER_TOKEN)) as usize)
    }

    #[test]
    fn auto_compact_threshold_is_85_percent_inclusive() {
        let limit = 1_000u64;
        let under = text_message("user", &text_for_total_tokens(849));
        let over = text_message("user", &text_for_total_tokens(850));
        assert_eq!(estimate_input_tokens(std::slice::from_ref(&under)), 849);
        assert_eq!(estimate_input_tokens(std::slice::from_ref(&over)), 850);
        assert!(!needs_auto_compact(&[under], limit));
        assert!(needs_auto_compact(&[over], limit));
        assert!(!needs_auto_compact(&[text_message("user", "hi")], 0));
        assert_eq!(AUTO_COMPACT_PERCENT, 85);
    }

    #[test]
    fn summary_is_first_last_user_turn_is_verbatim_and_open_todos_stay() {
        let last = InputItem::Message {
            role: "user".into(),
            content: vec![
                ContentPart::InputText("keep this question verbatim".into()),
                ContentPart::InputImage("data:image/png;base64,QQ==".into()),
            ],
        };
        let history = vec![
            text_message("system", "system prompt"),
            text_message("user", "OLD_TOPIC should disappear"),
            text_message("assistant", "looked at the old topic"),
            InputItem::FunctionCall {
                call_id: "t1".into(),
                name: "todo_write".into(),
                arguments: r#"{"merge":false,"todos":[
                    {"id":"1","content":"wire the auth","status":"in_progress"},
                    {"id":"2","content":"add the tests","status":"pending"},
                    {"id":"3","content":"finished task","status":"completed"}
                ]}"#
                .into(),
            },
            last.clone(),
        ];
        let compacted = build_compacted_history(
            &history,
            "<analysis>scratch</analysis>\n<summary>\n1. Primary Request and Intent: wire auth\n</summary>",
        );
        assert!(matches!(&compacted[0], InputItem::Message { role, .. } if role == "system"));
        assert_eq!(message_text(&compacted[0]), "system prompt");
        let summary_at = compacted
            .iter()
            .position(|item| message_text(item).contains("This session is being continued"))
            .expect("summary");
        assert_eq!(summary_at, 1, "summary is the first item after system");
        let summary = message_text(&compacted[summary_at]);
        assert!(summary.contains("wire auth"), "{summary}");
        assert!(!summary.contains("<summary>"), "{summary}");
        let todo_at = compacted
            .iter()
            .position(|item| message_text(item).contains("wire the auth"))
            .expect("todos");
        assert!(todo_at > summary_at);
        let todo = message_text(&compacted[todo_at]);
        assert!(todo.contains("[in_progress] 1: wire the auth"), "{todo}");
        assert!(todo.contains("[pending] 2: add the tests"), "{todo}");
        assert!(!todo.contains("finished task"), "{todo}");
        assert_eq!(compacted.last(), Some(&last));
        assert!(todo_at < compacted.len() - 1);
        assert!(!compacted
            .iter()
            .any(|item| message_text(item).contains("OLD_TOPIC")));
        let prompt = build_compaction_prompt(None, false);
        assert!(prompt.contains("Primary Request and Intent"));
        assert!(prompt.contains("Do NOT call or use any tools"));
        assert!(prompt.contains("9. Optional Next Step"));
    }

    #[test]
    fn manual_compact_is_native_only() {
        assert!(manual_compact_targets_native(true, true));
        assert!(!manual_compact_targets_native(false, true));
        assert!(!manual_compact_targets_native(true, false));
        assert!(!manual_compact_targets_native(false, false));
        assert!(is_manual_compact_command(" /compact "));
        assert!(!is_manual_compact_command("/compact now"));
    }

    struct Never;
    impl HaltCheck for Never {
        fn halted(&self) -> bool {
            false
        }
    }

    struct Script {
        turns: Mutex<Vec<Result<TurnOutput, ClientError>>>,
        seen: Mutex<Vec<ResponsesRequest>>,
    }

    impl ModelClient for Script {
        fn stream(
            &self,
            req: &ResponsesRequest,
            _cancel: &CancelToken,
            sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            self.seen.lock().unwrap().push(req.clone());
            let turn = {
                let mut turns = self.turns.lock().unwrap();
                if turns.is_empty() {
                    Ok(TurnOutput {
                        text: String::new(),
                        reasoning: String::new(),
                        calls: Vec::new(),
                        usage: Usage::default(),
                    })
                } else {
                    turns.remove(0)
                }
            }?;
            if !turn.text.is_empty() {
                sink(StreamEvent::TextDelta(turn.text.clone()));
            }
            Ok(turn)
        }
    }

    fn usage(input: u64, output: u64, cost: i64) -> Usage {
        Usage {
            input_tokens: input,
            output_tokens: output,
            reasoning_tokens: 0,
            cost_in_usd_ticks: cost,
        }
    }

    fn workspace() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-compact-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    fn drive(script: &Script, history: &mut Vec<InputItem>, limit: u64) -> crate::run::LoopOut {
        let dir = workspace();
        let input = LoopIn {
            client: script,
            workspace: &dir,
            model: "grok-4.7",
            effort: Some("low"),
            system: "",
            conversation_id: "conv",
            max_turns: 4,
            usage_base: Usage::default(),
            cancel: &CancelToken::new(),
            steer: &SteerQueue::new(),
            halt: &Never,
            gate: Gate::phase_readonly(),
            desktop: None,
            permits: &gate::ClosedPermits,
            perms: None,
            context_length: limit,
        };
        run_loop(&input, history, "go", None, &mut |_| {})
    }

    fn contains_prompt(req: &ResponsesRequest) -> bool {
        req.input
            .iter()
            .any(|item| message_text(item).contains("faithful, concise summary"))
    }

    #[test]
    fn auto_compact_runs_at_the_threshold_and_counts_summary_usage() {
        let old = text_message("user", &format!("OLD_TOPIC {}", text_for_total_tokens(900)));
        let script = Script {
            turns: Mutex::new(vec![
                Ok(TurnOutput {
                    text: "<summary>kept the plan</summary>".into(),
                    reasoning: String::new(),
                    calls: Vec::new(),
                    usage: usage(11, 4, 5),
                }),
                Ok(TurnOutput {
                    text: "done".into(),
                    reasoning: String::new(),
                    calls: Vec::new(),
                    usage: usage(3, 1, 2),
                }),
            ]),
            seen: Mutex::new(Vec::new()),
        };
        let mut history = vec![old];
        let out = drive(&script, &mut history, 1_000);
        assert_eq!(out.stop, crate::run::StopReason::EndTurn);
        assert!(out.compacted);
        assert_eq!(out.usage.input_tokens, 14);
        assert_eq!(out.usage.output_tokens, 5);
        assert_eq!(out.usage.cost_in_usd_ticks, 7);
        let seen = script.seen.lock().unwrap();
        assert!(contains_prompt(&seen[0]));
        assert!(seen[0].tools.is_empty());
        assert!(!seen[0].hosted_search);
        assert!(!contains_prompt(&seen[1]));
        assert!(message_text(&history[0]).contains("kept the plan"));
        assert_eq!(message_text(history.last().unwrap()), "done");
        assert!(!history
            .iter()
            .any(|item| message_text(item).contains("OLD_TOPIC")));
        assert!(history.iter().any(|item| message_text(item) == "go"));
    }

    #[test]
    fn under_threshold_does_not_ask_for_a_summary() {
        let script = Script {
            turns: Mutex::new(vec![Ok(TurnOutput {
                text: "done".into(),
                reasoning: String::new(),
                calls: Vec::new(),
                usage: usage(1, 1, 1),
            })]),
            seen: Mutex::new(Vec::new()),
        };
        let mut history = vec![text_message("user", "short")];
        let out = drive(&script, &mut history, 1_000);
        assert!(!out.compacted);
        let seen = script.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert!(!contains_prompt(&seen[0]));
        assert!(history
            .iter()
            .any(|item| message_text(item).contains("short")));
    }

    #[test]
    fn failed_or_cancelled_compaction_keeps_the_transcript() {
        let old = text_message(
            "user",
            &format!("OLD_MARKER {}", text_for_total_tokens(900)),
        );
        let fail = Script {
            turns: Mutex::new(vec![
                Err(ClientError::Protocol("disk full".into())),
                Ok(TurnOutput {
                    text: "kept going".into(),
                    reasoning: String::new(),
                    calls: Vec::new(),
                    usage: usage(2, 2, 4),
                }),
            ]),
            seen: Mutex::new(Vec::new()),
        };
        let mut history = vec![old.clone()];
        let out = drive(&fail, &mut history, 1_000);
        assert_eq!(out.stop, crate::run::StopReason::EndTurn);
        assert!(!out.compacted);
        assert!(history
            .iter()
            .any(|item| message_text(item).contains("OLD_MARKER")));
        assert!(history
            .iter()
            .any(|item| message_text(item) == "kept going"));
        assert!(!history
            .iter()
            .any(|item| message_text(item).contains("This session is being continued")));
        assert_eq!(out.usage.cost_in_usd_ticks, 4);

        let cancel = Script {
            turns: Mutex::new(vec![Err(ClientError::Cancelled)]),
            seen: Mutex::new(Vec::new()),
        };
        let mut history = vec![old];
        let out = drive(&cancel, &mut history, 1_000);
        assert_eq!(out.stop, crate::run::StopReason::Cancelled);
        assert!(!out.compacted);
        assert!(history
            .iter()
            .any(|item| message_text(item).contains("OLD_MARKER")));
        assert!(!history
            .iter()
            .any(|item| message_text(item).contains("This session is being continued")));
        assert_eq!(cancel.seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn manual_compact_runs_below_the_threshold() {
        let script = Script {
            turns: Mutex::new(vec![Ok(TurnOutput {
                text: "manual summary".into(),
                reasoning: String::new(),
                calls: vec![FunctionCall {
                    call_id: "no".into(),
                    name: "read_file".into(),
                    arguments: "{}".into(),
                }],
                usage: usage(6, 2, 8),
            })]),
            seen: Mutex::new(Vec::new()),
        };
        let last = text_message("user", "last turn");
        let mut history = vec![text_message("user", "OLD_TURN_MARKER"), last.clone()];
        assert!(!needs_auto_compact(&history, 1_000));
        let usage = compact_transcript(
            &script,
            &CancelToken::new(),
            "grok-4.7",
            None,
            "conv",
            &mut history,
        )
        .expect("manual compact");
        assert_eq!(usage.cost_in_usd_ticks, 8);
        assert!(message_text(&history[0]).contains("manual summary"));
        assert_eq!(history.last(), Some(&last));
        assert!(!history
            .iter()
            .any(|item| message_text(item).contains("OLD_TURN_MARKER")));
        let seen = script.seen.lock().unwrap();
        assert!(seen[0].tools.is_empty());
        assert!(!seen[0].hosted_search);

        let boom = Script {
            turns: Mutex::new(vec![Err(ClientError::Protocol("nope".into()))]),
            seen: Mutex::new(Vec::new()),
        };
        let mut kept = vec![last.clone()];
        let err = compact_transcript(
            &boom,
            &CancelToken::new(),
            "grok-4.7",
            None,
            "conv",
            &mut kept,
        );
        assert!(err.is_err());
        assert_eq!(kept, vec![last]);
    }
}
