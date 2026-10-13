// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.
//! Auto-compact at 85% and manual compaction. The summary prompt is the detailed
//! prompt from `xai-grok-shell` `helpers/session_compact.rs`.

use std::path::{Path, PathBuf};

use crate::client::{
    ClientError, ContentPart, InputItem, ModelClient, ResponsesRequest, StreamEvent, TurnOutput,
    Usage,
};
use crate::tokens::{self, estimate_token_bytes};
use crate::CancelToken;

/// Auto-compact fires at this percent of the context window, inclusive.
pub const AUTO_COMPACT_PERCENT: u8 = 85;

/// Token budget for the verbatim recent tail kept outside the summary (last user
/// turn plus trailing tool call/output pairs).
pub const RECENT_TAIL_TOKEN_BUDGET: u64 = 4_000;

/// Stop reason when a second context-length error follows a reactive compact.
pub const CONTEXT_STILL_TOO_LARGE: &str = "context still too large after compaction";

/// Config file under [`crate::perm::config_dir`] (off by default).
pub const TOOL_OUTPUT_COMPACT_CONFIG: &str = "tool_output_compact.toml";

/// Default minimum UTF-8 size before a tool result is summarized and spooled.
pub const TOOL_OUTPUT_COMPACT_MIN_BYTES: usize = 12_000;

/// Workspace-relative directory for spooled full tool outputs (re-read with `read_file`).
pub const TOOL_OUTPUT_SPOOL_DIR: &str = ".grokhub/tool-output-compact";

const TOOL_OUTPUT_SUMMARY_PROMPT: &str = r#"<tool_output_summary_request>
Summarize this tool result so another assistant can keep working. Keep exact error lines,
counts, file paths, command names, and any fact that would change the next step. Be concise;
do not invent details. Respond with plain text only (no tools, no markdown fences).
</tool_output_summary_request>"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolOutputCompactSettings {
    pub enabled: bool,
    pub min_bytes: usize,
}

impl Default for ToolOutputCompactSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            min_bytes: TOOL_OUTPUT_COMPACT_MIN_BYTES,
        }
    }
}

static TOOL_OUTPUT_COMPACT_TEST: std::sync::Mutex<Option<ToolOutputCompactSettings>> =
    std::sync::Mutex::new(None);

/// Test hook: `Some(settings)` overrides the on-disk file; `None` reads the file again.
pub fn set_tool_output_compact_for_tests(settings: Option<ToolOutputCompactSettings>) {
    *TOOL_OUTPUT_COMPACT_TEST
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = settings;
}

pub fn tool_output_compact_settings(config_dir: &Path) -> ToolOutputCompactSettings {
    if let Some(settings) = *TOOL_OUTPUT_COMPACT_TEST
        .lock()
        .unwrap_or_else(|e| e.into_inner())
    {
        return settings;
    }
    let path = config_dir.join(TOOL_OUTPUT_COMPACT_CONFIG);
    let Ok(text) = std::fs::read_to_string(path) else {
        return ToolOutputCompactSettings::default();
    };
    parse_tool_output_compact_file(&text)
}

fn parse_tool_output_compact_file(text: &str) -> ToolOutputCompactSettings {
    let mut settings = ToolOutputCompactSettings::default();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key.eq_ignore_ascii_case("enabled") && value.eq_ignore_ascii_case("true") {
            settings.enabled = true;
        } else if key.eq_ignore_ascii_case("min_bytes") {
            if let Ok(n) = value.parse::<usize>() {
                settings.min_bytes = n.max(1);
            }
        }
    }
    settings
}

/// Outcome of [`maybe_compact_tool_output`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolOutputCompactOutcome {
    Unchanged,
    Replaced { text: String, usage: Usage },
}

/// When settings allow, spool a large tool body under the workspace and replace it with a
/// short summary plus a stable handle and `read_file` path.
pub fn maybe_compact_tool_output(
    config_dir: &Path,
    workspace: &Path,
    conversation_id: &str,
    client: &dyn ModelClient,
    cancel: &CancelToken,
    model: &str,
    effort: Option<&str>,
    call_id: &str,
    tool_name: &str,
    output: &str,
) -> Result<ToolOutputCompactOutcome, CompactError> {
    let settings = tool_output_compact_settings(config_dir);
    if !settings.enabled || output.len() < settings.min_bytes {
        return Ok(ToolOutputCompactOutcome::Unchanged);
    }
    if cancel.is_cancelled() {
        return Err(CompactError::Cancelled);
    }
    let (handle, rel_path) = spool_tool_output(workspace, conversation_id, call_id, output)?;
    let (summary, usage) = match request_tool_output_summary(
        client,
        cancel,
        model,
        effort,
        conversation_id,
        tool_name,
        call_id,
        &rel_path,
        output,
    ) {
        Ok(call) => (call.summary, call.usage),
        Err(CompactError::Cancelled) => return Err(CompactError::Cancelled),
        Err(_) => (extractive_tool_output_summary(output), Usage::default()),
    };
    let body =
        format_tool_output_compact_message(tool_name, &handle, &rel_path, &summary, output.len());
    Ok(ToolOutputCompactOutcome::Replaced { text: body, usage })
}

/// Load a spooled tool body by handle (same conversation id used when it was written).
pub fn read_spooled_tool_output(
    workspace: &Path,
    conversation_id: &str,
    handle: &str,
) -> Option<String> {
    let path = spool_file_path(workspace, conversation_id, handle);
    std::fs::read_to_string(path).ok()
}

fn spool_tool_output(
    workspace: &Path,
    conversation_id: &str,
    call_id: &str,
    body: &str,
) -> Result<(String, String), CompactError> {
    let handle = spool_handle(call_id);
    let path = spool_file_path(workspace, conversation_id, &handle);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CompactError::Failed(e.to_string()))?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, body).map_err(|e| CompactError::Failed(e.to_string()))?;
    std::fs::rename(&tmp, &path).map_err(|e| CompactError::Failed(e.to_string()))?;
    let rel = format!(
        "{}/{}",
        TOOL_OUTPUT_SPOOL_DIR,
        session_dir_name(conversation_id)
    );
    let rel_path = format!("{}/{}.txt", rel, handle);
    Ok((handle, rel_path))
}

fn spool_handle(call_id: &str) -> String {
    let mut slug: String = call_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if slug.is_empty() {
        slug = "out".into();
    }
    if slug.len() > 48 {
        slug.truncate(48);
    }
    format!("to-{slug}")
}

fn session_dir_name(conversation_id: &str) -> String {
    let mut slug: String = conversation_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if slug.is_empty() {
        slug = "session".into();
    }
    if slug.len() > 64 {
        slug.truncate(64);
    }
    slug
}

fn spool_file_path(workspace: &Path, conversation_id: &str, handle: &str) -> PathBuf {
    workspace
        .join(TOOL_OUTPUT_SPOOL_DIR)
        .join(session_dir_name(conversation_id))
        .join(format!("{handle}.txt"))
}

fn format_tool_output_compact_message(
    tool_name: &str,
    handle: &str,
    rel_path: &str,
    summary: &str,
    full_bytes: usize,
) -> String {
    format!(
        "<tool_output_compact tool=\"{tool_name}\" handle=\"{handle}\" bytes=\"{full_bytes}\">\n\
         {summary}\n\
         </tool_output_compact>\n\
         Full output ({full_bytes} bytes) is spooled at `{rel_path}`. \
         Re-read it with read_file (or grep that path) when you need exact text."
    )
}

fn extractive_tool_output_summary(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    let line_count = lines.len();
    let mut errors: Vec<String> = Vec::new();
    for line in lines.iter().take(400) {
        let lower = line.to_ascii_lowercase();
        if lower.contains("error")
            || lower.contains("failed")
            || lower.contains("panic")
            || lower.contains("fatal")
        {
            let trimmed = line.trim();
            if !trimmed.is_empty() && errors.len() < 8 {
                errors.push(trimmed.to_string());
            }
        }
    }
    let head: String = lines
        .iter()
        .take(12)
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .take(8)
        .map(|l| l.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let tail: String = lines
        .iter()
        .rev()
        .take(6)
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .take(4)
        .map(|l| l.to_string())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    let mut out = format!("Lines: {line_count}. Bytes: {}.", output.len());
    if !errors.is_empty() {
        out.push_str("\nNotable errors:\n");
        for e in errors {
            out.push_str("- ");
            out.push_str(&e);
            out.push('\n');
        }
    }
    if !head.is_empty() {
        out.push_str("\nHead:\n");
        out.push_str(&head);
        out.push('\n');
    }
    if !tail.is_empty() && tail != head {
        out.push_str("\nTail:\n");
        out.push_str(&tail);
        out.push('\n');
    }
    out.trim().to_string()
}

struct ToolOutputSummaryCall {
    summary: String,
    usage: Usage,
}

fn request_tool_output_summary(
    client: &dyn ModelClient,
    cancel: &CancelToken,
    model: &str,
    effort: Option<&str>,
    conversation_id: &str,
    tool_name: &str,
    call_id: &str,
    rel_path: &str,
    output: &str,
) -> Result<ToolOutputSummaryCall, CompactError> {
    if cancel.is_cancelled() {
        return Err(CompactError::Cancelled);
    }
    let preview_cap = 48_000usize;
    let (preview, clipped) = if output.len() > preview_cap {
        (output.get(..preview_cap).unwrap_or(output), true)
    } else {
        (output, false)
    };
    let clip_note = if clipped {
        "\n(note: body truncated for the summary call; the full text is spooled on disk)\n"
    } else {
        ""
    };
    let user = format!(
        "{TOOL_OUTPUT_SUMMARY_PROMPT}\n\n\
         Tool: {tool_name}\nCall id: {call_id}\nSpool path: {rel_path}\n{clip_note}\n\
         <tool_output_body>\n{preview}\n</tool_output_body>"
    );
    let input = vec![InputItem::Message {
        role: "user".into(),
        content: vec![ContentPart::InputText(user)],
    }];
    let req = ResponsesRequest {
        model: model.to_string(),
        effort: effort.map(str::to_string),
        input,
        conversation_id: conversation_id.to_string(),
        tools: Vec::new(),
        hosted_search: false,
        call_timeout: None,
    };
    match crate::route::live::stream_routed(
        client,
        &req,
        cancel,
        &mut |_| {},
        crate::route::CLASS_COMPACT,
    ) {
        Ok(turn) => {
            if cancel.is_cancelled() {
                return Err(CompactError::Cancelled);
            }
            let summary = turn.text.trim();
            if summary.is_empty() {
                return Err(CompactError::Failed("tool output summary was empty".into()));
            }
            Ok(ToolOutputSummaryCall {
                summary: summary.to_string(),
                usage: turn.usage,
            })
        }
        Err(ClientError::Cancelled) => Err(CompactError::Cancelled),
        Err(err) => Err(CompactError::Failed(err.to_string())),
    }
}

/// Eval harness (`src/eval.rs`): settings on, large output spooled and summarized.
pub fn eval_tool_output_compact_native() -> (&'static str, u32) {
    let dir = std::env::temp_dir().join(format!(
        "gh-tool-out-compact-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    if std::fs::create_dir_all(&dir).is_err() {
        return ("failed: scratch", 0);
    }
    let _guard = crate::perm::ConfigGuard::set(&dir);
    set_tool_output_compact_for_tests(Some(ToolOutputCompactSettings {
        enabled: true,
        min_bytes: 100,
    }));
    let big = "LINE\n".repeat(80);
    let script = EvalToolOutputScript {
        summary: "errors: none; lines: 80".into(),
        seen: std::sync::Mutex::new(0),
    };
    let cancel = CancelToken::new();
    let outcome = maybe_compact_tool_output(
        &dir,
        &dir,
        "eval-tool-out",
        &script,
        &cancel,
        "grok-4.7",
        None,
        "c1",
        "run_terminal_command",
        &big,
    );
    set_tool_output_compact_for_tests(None);
    let turns = script.turns();
    let ok = match outcome {
        Ok(ToolOutputCompactOutcome::Replaced { text, .. }) => {
            text.contains("<tool_output_compact")
                && text.contains("handle=\"to-c1\"")
                && text.contains(".grokhub/tool-output-compact")
                && read_spooled_tool_output(&dir, "eval-tool-out", "to-c1").as_deref() == Some(&big)
        }
        _ => false,
    };
    let _ = std::fs::remove_dir_all(&dir);
    if ok {
        ("spooled", turns)
    } else {
        ("failed: tool-output-compact", turns)
    }
}

struct EvalToolOutputScript {
    summary: String,
    seen: std::sync::Mutex<usize>,
}

impl EvalToolOutputScript {
    fn turns(&self) -> u32 {
        *self.seen.lock().unwrap_or_else(|e| e.into_inner()) as u32
    }
}

impl ModelClient for EvalToolOutputScript {
    fn stream(
        &self,
        req: &ResponsesRequest,
        _cancel: &CancelToken,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        *self.seen.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        let prompt = req
            .input
            .iter()
            .map(message_text)
            .find(|t| t.contains("tool_output_summary_request"))
            .unwrap_or_default();
        if !prompt.contains("tool_output_summary_request") {
            return Err(ClientError::Protocol("unexpected eval prompt".into()));
        }
        sink(StreamEvent::TextDelta(self.summary.clone()));
        Ok(TurnOutput {
            text: self.summary.clone(),
            reasoning: String::new(),
            calls: Vec::new(),
            usage: Usage {
                input_tokens: 3,
                output_tokens: 2,
                reasoning_tokens: 0,
                cost_in_usd_ticks: 1,
                cached_tokens: 0,
            },
            ..Default::default()
        })
    }
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

/// Flush pending memory, then summarize. A flush error still compacts.
/// The summary call sees the memory file already written.
#[allow(clippy::too_many_arguments)]
pub fn compact_after_flush(
    workspace: &Path,
    client: &dyn ModelClient,
    cancel: &CancelToken,
    model: &str,
    effort: Option<&str>,
    conversation_id: &str,
    history: &mut Vec<InputItem>,
    modified_paths: &[String],
) -> Result<Usage, CompactError> {
    let _ = crate::memory::flush_pending(workspace, history);
    compact_transcript(
        client,
        cancel,
        model,
        effort,
        conversation_id,
        history,
        modified_paths,
    )
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
    modified_paths: &[String],
) -> Result<Usage, CompactError> {
    if history.is_empty() {
        return Err(CompactError::Failed("nothing to compact".into()));
    }
    let snapshot = history.clone();
    let call = request_summary(client, cancel, model, effort, conversation_id, &snapshot)?;
    if cancel.is_cancelled() {
        return Err(CompactError::Cancelled);
    }
    *history = build_compacted_history(&snapshot, &call.summary, modified_paths);
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
    let prior = extract_prior_summary(history);
    let prompt = match prior.as_deref() {
        Some(anchor) => build_compaction_prompt(
            Some(&format!(
                "<prior_conversation_summary>\n{anchor}\n</prior_conversation_summary>\n\
                 Merge this prior summary with everything that happened after it; keep its \
                 still-relevant facts and carry forward any unique markers."
            )),
            false,
        ),
        None => build_compaction_prompt(None, false),
    };
    input.push(InputItem::Message {
        role: "user".into(),
        content: vec![ContentPart::InputText(prompt)],
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
    match crate::route::live::stream_routed(
        client,
        &req,
        cancel,
        &mut |_| {},
        crate::route::CLASS_COMPACT,
    ) {
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

/// Summary first, then open todos, modified file paths, then the recent verbatim
/// tail (last user turn plus trailing tool pairs within [`RECENT_TAIL_TOKEN_BUDGET`]).
/// Leading system messages stay in front of the summary.
pub fn build_compacted_history(
    items: &[InputItem],
    summary_raw: &str,
    modified_paths: &[String],
) -> Vec<InputItem> {
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
    if let Some(files) = modified_files_message(modified_paths) {
        out.push(files);
    }
    out.extend(recent_tail(rest));
    out
}

/// Prior compaction body for merge-anchored re-summaries, if any.
pub fn extract_prior_summary(items: &[InputItem]) -> Option<String> {
    for item in items {
        let text = message_text(item);
        if let Some(start) = text.find("<conversation_summary>") {
            let after = text.get(start + "<conversation_summary>".len()..)?;
            if let Some(end) = after.find("</conversation_summary>") {
                let body = after.get(..end)?.trim();
                if !body.is_empty() {
                    return Some(body.to_string());
                }
            }
        }
        if text.contains("This session is being continued") {
            let body = text.trim();
            if !body.is_empty() {
                return Some(body.to_string());
            }
        }
    }
    None
}

/// Path from a successful `write` / `search_replace` call, if present.
pub fn path_from_write_call(name: &str, arguments: &str) -> Option<String> {
    if name != "write" && name != "search_replace" {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(arguments).ok()?;
    for key in ["path", "file_path", "target_file"] {
        if let Some(path) = value
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            return Some(path.to_string());
        }
    }
    None
}

fn modified_files_message(paths: &[String]) -> Option<InputItem> {
    let mut listed = Vec::new();
    for path in paths {
        let trimmed = path.trim();
        if trimmed.is_empty() || listed.iter().any(|p: &String| p == trimmed) {
            continue;
        }
        listed.push(trimmed.to_string());
    }
    if listed.is_empty() {
        return None;
    }
    let mut body = String::from("<modified_files>\n");
    for path in &listed {
        body.push_str(path);
        body.push('\n');
    }
    body.push_str("</modified_files>");
    Some(InputItem::Message {
        role: "user".into(),
        content: vec![ContentPart::InputText(body)],
    })
}

/// Last user turn plus trailing tool call/output pairs within the token budget.
pub fn recent_tail(items: &[InputItem]) -> Vec<InputItem> {
    let Some(user) = last_user_turn(items) else {
        return take_tool_suffix(items, RECENT_TAIL_TOKEN_BUDGET);
    };
    let user_at = items
        .iter()
        .rposition(|item| matches!(item, InputItem::Message { role, .. } if role == "user"))
        .expect("last_user_turn index");
    let user_cost = estimate_input_tokens(std::slice::from_ref(user));
    if user_cost > RECENT_TAIL_TOKEN_BUDGET {
        return Vec::new();
    }
    let mut out = vec![user.clone()];
    let remain = RECENT_TAIL_TOKEN_BUDGET.saturating_sub(user_cost);
    out.extend(take_tool_suffix(&items[user_at + 1..], remain));
    out
}

fn take_tool_suffix(items: &[InputItem], budget: u64) -> Vec<InputItem> {
    if budget == 0 || items.is_empty() {
        return Vec::new();
    }
    let mut acc = Vec::new();
    let mut used = 0u64;
    let mut i = items.len();
    while i > 0 {
        i -= 1;
        match &items[i] {
            InputItem::FunctionCallOutput { call_id, .. } => {
                let end = i;
                let start = match i.checked_sub(1) {
                    Some(prev) => match &items[prev] {
                        InputItem::FunctionCall { call_id: cid, .. } if cid == call_id => prev,
                        _ => continue,
                    },
                    None => continue,
                };
                let slice = &items[start..=end];
                let cost = estimate_input_tokens(slice);
                if used.saturating_add(cost) > budget {
                    break;
                }
                used = used.saturating_add(cost);
                // Accumulate reverse so the final acc.reverse() restores chronology.
                acc.extend(slice.iter().rev().cloned());
                i = start;
            }
            InputItem::FunctionCall { call_id, .. } => {
                // Paired call+output is handled when we see the output.
                if i + 1 < items.len()
                    && matches!(
                        &items[i + 1],
                        InputItem::FunctionCallOutput {
                            call_id: oid, ..
                        } if oid == call_id
                    )
                {
                    continue;
                }
                let cost = estimate_input_tokens(std::slice::from_ref(&items[i]));
                if used.saturating_add(cost) > budget {
                    break;
                }
                used = used.saturating_add(cost);
                acc.push(items[i].clone());
            }
            InputItem::Message { .. } => break,
        }
    }
    acc.reverse();
    acc
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
         The summary below covers the earlier portion of the conversation.\n\n\
         <conversation_summary>\n{cleaned}\n</conversation_summary>"
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
            &[],
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
    fn manual_compact_is_the_bare_command() {
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
                        ..Default::default()
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
            cached_tokens: 0,
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
            tasks: None,
            depth: 0,
            agent_id: None,
            shared_client: None,
            shared_permits: None,
            shared_desktop: None,
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
                    ..Default::default()
                }),
                Ok(TurnOutput {
                    text: "done".into(),
                    reasoning: String::new(),
                    calls: Vec::new(),
                    usage: usage(3, 1, 2),
                    ..Default::default()
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
                ..Default::default()
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
                    ..Default::default()
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
                ..Default::default()
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
            &[],
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
            &[],
        );
        assert!(err.is_err());
        assert_eq!(kept, vec![last]);
    }

    #[test]
    fn flush_runs_before_the_summary_model_call() {
        let dir = workspace();
        let _guard = crate::perm::ConfigGuard::set(&dir);
        struct Watch {
            dir: PathBuf,
        }
        impl ModelClient for Watch {
            fn stream(
                &self,
                req: &ResponsesRequest,
                _cancel: &CancelToken,
                _sink: &mut dyn FnMut(StreamEvent),
            ) -> Result<TurnOutput, ClientError> {
                let mem = std::fs::read_to_string(crate::memory::flush_memory_path(&self.dir))
                    .unwrap_or_default();
                assert!(
                    mem.contains("keep the quay lantern"),
                    "flush must land before the summary call: {mem}"
                );
                assert!(req
                    .input
                    .iter()
                    .any(|item| message_text(item).contains("faithful, concise summary")));
                Ok(TurnOutput {
                    text: "kept the quay".into(),
                    reasoning: String::new(),
                    calls: Vec::new(),
                    usage: Usage::default(),
                    ..Default::default()
                })
            }
        }
        let mut history = vec![text_message("user", "keep the quay lantern")];
        compact_after_flush(
            &dir,
            &Watch { dir: dir.clone() },
            &CancelToken::new(),
            "grok-4.7",
            Some("low"),
            "conv",
            &mut history,
            &[],
        )
        .expect("compact after flush");
        assert!(message_text(&history[0]).contains("kept the quay"));
        drop(_guard);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn take_tool_suffix_keeps_call_and_output_as_one_pair() {
        let call = InputItem::FunctionCall {
            call_id: "c1".into(),
            name: "read_file".into(),
            arguments: r#"{"path":"a.rs"}"#.into(),
        };
        let output = InputItem::FunctionCallOutput {
            call_id: "c1".into(),
            output: "ok".into(),
        };
        let pair_cost = estimate_input_tokens(&[call.clone(), output.clone()]);
        let items = vec![call.clone(), output.clone()];
        let kept = take_tool_suffix(&items, pair_cost);
        assert_eq!(kept.len(), 2, "pair fits as a unit");
        assert!(matches!(&kept[0], InputItem::FunctionCall { call_id, .. } if call_id == "c1"));
        assert!(
            matches!(&kept[1], InputItem::FunctionCallOutput { call_id, .. } if call_id == "c1")
        );
        let half = pair_cost.saturating_sub(1).max(1);
        if half < pair_cost {
            assert!(
                take_tool_suffix(&items, half).is_empty(),
                "budget below the pair keeps neither half"
            );
        }
    }

    #[test]
    fn recent_tail_keeps_last_user_and_tool_results_within_budget() {
        let last = text_message("user", "keep this question verbatim");
        let call = InputItem::FunctionCall {
            call_id: "c1".into(),
            name: "read_file".into(),
            arguments: r#"{"path":"src/a.rs"}"#.into(),
        };
        let output = InputItem::FunctionCallOutput {
            call_id: "c1".into(),
            output: "fn main() {}".into(),
        };
        let history = vec![
            text_message("user", "OLD_TOPIC"),
            text_message("assistant", "working"),
            last.clone(),
            call.clone(),
            output.clone(),
        ];
        let compacted = build_compacted_history(&history, "summary body", &[]);
        assert_eq!(compacted.iter().filter(|i| matches!(i, InputItem::Message { role, .. } if role == "user" && message_text(i).contains("keep this"))).count(), 1);
        assert!(compacted
            .iter()
            .any(|i| matches!(i, InputItem::FunctionCall { call_id, .. } if call_id == "c1")));
        assert!(compacted.iter().any(
            |i| matches!(i, InputItem::FunctionCallOutput { call_id, .. } if call_id == "c1")
        ));
        assert!(!compacted
            .iter()
            .any(|item| message_text(item).contains("OLD_TOPIC")));
    }

    #[test]
    fn modified_file_paths_are_reinjected_after_compaction() {
        let history = vec![
            text_message("user", "edit files"),
            text_message("user", "last ask"),
        ];
        let compacted =
            build_compacted_history(&history, "summary", &["src/a.rs".into(), "src/b.rs".into()]);
        let files = compacted
            .iter()
            .find(|item| message_text(item).contains("<modified_files>"))
            .map(message_text)
            .expect("modified files");
        assert!(files.contains("src/a.rs"), "{files}");
        assert!(files.contains("src/b.rs"), "{files}");
        assert_eq!(
            path_from_write_call("write", r#"{"path":"src/a.rs","contents":"x"}"#).as_deref(),
            Some("src/a.rs")
        );
        assert!(path_from_write_call("read_file", r#"{"path":"src/a.rs"}"#).is_none());
    }

    #[test]
    fn prior_summary_is_passed_as_merge_anchor_on_second_compact() {
        let first = format_compact_summary_content("MARKER_ALPHA unique early work");
        assert!(first.contains("<conversation_summary>"));
        assert!(extract_prior_summary(&[text_message("user", &first)])
            .unwrap_or_default()
            .contains("MARKER_ALPHA"));
        let history = vec![
            text_message("user", &first),
            text_message("user", "more work after the first compact"),
        ];
        let script = Script {
            turns: Mutex::new(vec![Ok(TurnOutput {
                text: "<summary>MARKER_ALPHA and the later edit</summary>".into(),
                reasoning: String::new(),
                calls: Vec::new(),
                usage: usage(2, 2, 1),
                ..Default::default()
            })]),
            seen: Mutex::new(Vec::new()),
        };
        let mut hist = history;
        compact_transcript(
            &script,
            &CancelToken::new(),
            "grok-4.7",
            None,
            "conv",
            &mut hist,
            &[],
        )
        .expect("second compact");
        let seen = script.seen.lock().unwrap();
        let prompt = seen[0]
            .input
            .iter()
            .map(message_text)
            .find(|t| t.contains("prior_conversation_summary"))
            .expect("merge anchor in prompt");
        assert!(prompt.contains("MARKER_ALPHA"), "{prompt}");
        assert!(message_text(&hist[0]).contains("MARKER_ALPHA"));
    }

    #[test]
    fn auto_compact_rearms_after_new_turns_and_compacts_three_times() {
        // Tool turns keep the loop alive; huge call arguments re-cross the 85%
        // threshold so compaction re-arms after each new turn.
        let inflate = text_for_total_tokens(900);
        let pad_call = |id: &str| FunctionCall {
            call_id: id.into(),
            name: "read_file".into(),
            arguments: format!(r#"{{"path":"pad-{id}.rs","blob":"{inflate}"}}"#),
        };
        let script = Script {
            turns: Mutex::new(vec![
                Ok(TurnOutput {
                    text: "<summary>pass one MARKER_ONE</summary>".into(),
                    reasoning: String::new(),
                    calls: Vec::new(),
                    usage: usage(1, 1, 1),
                    ..Default::default()
                }),
                Ok(TurnOutput {
                    text: String::new(),
                    reasoning: String::new(),
                    calls: vec![pad_call("1")],
                    usage: usage(1, 1, 1),
                    ..Default::default()
                }),
                Ok(TurnOutput {
                    text: "<summary>pass two MARKER_ONE MARKER_TWO</summary>".into(),
                    reasoning: String::new(),
                    calls: Vec::new(),
                    usage: usage(1, 1, 1),
                    ..Default::default()
                }),
                Ok(TurnOutput {
                    text: String::new(),
                    reasoning: String::new(),
                    calls: vec![pad_call("2")],
                    usage: usage(1, 1, 1),
                    ..Default::default()
                }),
                Ok(TurnOutput {
                    text: "<summary>pass three MARKER_ONE MARKER_TWO MARKER_THREE</summary>".into(),
                    reasoning: String::new(),
                    calls: Vec::new(),
                    usage: usage(1, 1, 1),
                    ..Default::default()
                }),
                Ok(TurnOutput {
                    text: "done".into(),
                    reasoning: String::new(),
                    calls: Vec::new(),
                    usage: usage(1, 1, 1),
                    ..Default::default()
                }),
            ]),
            seen: Mutex::new(Vec::new()),
        };
        let mut history = vec![text_message("user", &format!("OLD {inflate}"))];
        let dir = workspace();
        let input = LoopIn {
            client: &script,
            workspace: &dir,
            model: "grok-4.7",
            effort: Some("low"),
            system: "",
            conversation_id: "conv-rearm",
            max_turns: 8,
            usage_base: Usage::default(),
            cancel: &CancelToken::new(),
            steer: &SteerQueue::new(),
            halt: &Never,
            gate: Gate::phase_readonly(),
            desktop: None,
            permits: &gate::ClosedPermits,
            perms: None,
            context_length: 1_000,
            tasks: None,
            depth: 0,
            agent_id: None,
            shared_client: None,
            shared_permits: None,
            shared_desktop: None,
        };
        let out = run_loop(&input, &mut history, "go", None, &mut |_| {});
        assert_eq!(out.stop, crate::run::StopReason::EndTurn);
        assert!(out.compacted);
        let prompts = script
            .seen
            .lock()
            .unwrap()
            .iter()
            .filter(|req| contains_prompt(req))
            .count();
        assert_eq!(prompts, 3, "expected three compactions");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn context_length_error_compacts_once_and_retries_the_request() {
        let script = Script {
            turns: Mutex::new(vec![
                Err(ClientError::Protocol(
                    "HTTP 400: This model's maximum context length was exceeded".into(),
                )),
                Ok(TurnOutput {
                    text: "<summary>trimmed MARKER_OVERFLOW</summary>".into(),
                    reasoning: String::new(),
                    calls: Vec::new(),
                    usage: usage(2, 2, 1),
                    ..Default::default()
                }),
                Ok(TurnOutput {
                    text: "recovered".into(),
                    reasoning: String::new(),
                    calls: Vec::new(),
                    usage: usage(1, 1, 1),
                    ..Default::default()
                }),
            ]),
            seen: Mutex::new(Vec::new()),
        };
        let mut history = vec![text_message("user", "short enough")];
        let out = drive(&script, &mut history, 0);
        assert_eq!(out.stop, crate::run::StopReason::EndTurn);
        assert!(out.compacted);
        assert!(history.iter().any(|i| message_text(i) == "recovered"));
        assert!(history
            .iter()
            .any(|i| message_text(i).contains("MARKER_OVERFLOW")));
        let seen = script.seen.lock().unwrap();
        assert!(contains_prompt(&seen[1]));
        assert!(!contains_prompt(&seen[2]));
    }

    #[test]
    fn tool_output_compact_off_by_default_and_spools_when_enabled() {
        let dir = workspace();
        let _guard = crate::perm::ConfigGuard::set(&dir);
        set_tool_output_compact_for_tests(None);
        assert!(!tool_output_compact_settings(&dir).enabled);
        let big = "x".repeat(200);
        let script = Script {
            turns: Mutex::new(vec![]),
            seen: Mutex::new(Vec::new()),
        };
        let unchanged = maybe_compact_tool_output(
            &dir,
            &dir,
            "sess",
            &script,
            &CancelToken::new(),
            "grok-4.7",
            None,
            "c1",
            "grep",
            &big,
        )
        .expect("unchanged");
        assert_eq!(unchanged, ToolOutputCompactOutcome::Unchanged);

        set_tool_output_compact_for_tests(Some(ToolOutputCompactSettings {
            enabled: true,
            min_bytes: 50,
        }));
        let script = Script {
            turns: Mutex::new(vec![Ok(TurnOutput {
                text: "found 3 matches in src/main.rs".into(),
                reasoning: String::new(),
                calls: Vec::new(),
                usage: usage(2, 1, 1),
                ..Default::default()
            })]),
            seen: Mutex::new(Vec::new()),
        };
        let replaced = maybe_compact_tool_output(
            &dir,
            &dir,
            "sess",
            &script,
            &CancelToken::new(),
            "grok-4.7",
            None,
            "call-9",
            "grep",
            &big,
        )
        .expect("replaced");
        let ToolOutputCompactOutcome::Replaced { text, usage } = replaced else {
            panic!("expected replacement");
        };
        assert!(text.contains("handle=\"to-call-9\""), "{text}");
        assert!(text.contains("read_file"), "{text}");
        assert_eq!(usage.cost_in_usd_ticks, 1);
        let full = read_spooled_tool_output(&dir, "sess", "to-call-9").expect("spool");
        assert_eq!(full, big);
        set_tool_output_compact_for_tests(None);
        drop(_guard);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tool_output_compact_parse_settings_from_toml() {
        let dir = workspace();
        let cfg = dir.join(TOOL_OUTPUT_COMPACT_CONFIG);
        std::fs::write(&cfg, "# comment\nenabled = true\nmin_bytes = 999\n").expect("write cfg");
        let _guard = crate::perm::ConfigGuard::set(&dir);
        set_tool_output_compact_for_tests(None);
        let parsed = tool_output_compact_settings(&dir);
        assert!(parsed.enabled);
        assert_eq!(parsed.min_bytes, 999);
        drop(_guard);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn second_context_length_error_after_compaction_ends_with_named_reason() {
        let script = Script {
            turns: Mutex::new(vec![
                Err(ClientError::Protocol(
                    "HTTP 400: prompt is too long for the context window".into(),
                )),
                Ok(TurnOutput {
                    text: "<summary>still huge</summary>".into(),
                    reasoning: String::new(),
                    calls: Vec::new(),
                    usage: usage(1, 1, 1),
                    ..Default::default()
                }),
                Err(ClientError::Protocol(
                    "HTTP 400: prompt is too long for the context window".into(),
                )),
            ]),
            seen: Mutex::new(Vec::new()),
        };
        let mut history = vec![text_message("user", "go")];
        let out = drive(&script, &mut history, 0);
        assert_eq!(
            out.stop,
            crate::run::StopReason::Error(CONTEXT_STILL_TOO_LARGE.into())
        );
        assert!(out.compacted);
    }
}
