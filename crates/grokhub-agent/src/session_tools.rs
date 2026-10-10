// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! Todos, questions, plan mode, and the findings card.
//! `todo_write` replaces the list unless `merge` is true. Open rows survive compaction
//! because the function call is stored on the session. Plan mode stays read-only until
//! the person approves `exit_plan_mode`. An unattended run cannot wait or leave plan mode.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};

use crate::gate::Gate;
use crate::mcp::{self, ElicitView};
use crate::tools::ToolOutput;
use crate::{CancelToken, LoopEvent, Usage};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub status: String,
}

pub struct SessionCall<'a> {
    pub name: &'a str,
    pub arguments: &'a str,
    pub gate: &'a mut Gate,
    pub session: &'a str,
    pub attended: bool,
    pub cancel: &'a CancelToken,
    pub halted: &'a dyn Fn() -> bool,
    pub base_readonly: bool,
    pub plan_at_start: bool,
    pub on_event: &'a mut dyn FnMut(LoopEvent),
}

pub fn schemas() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "name": "todo_write",
            "description": "Replace the session todo list. Set merge to true to update rows by id. Status is pending, in_progress, completed, or cancelled.",
            "parameters": {
                "type": "object",
                "properties": {
                    "merge": {"type": "boolean"},
                    "todos": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": {"type": "string"},
                                "content": {"type": "string"},
                                "status": {"type": "string"}
                            },
                            "required": ["id"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["todos"],
                "additionalProperties": false
            }
        }),
        json!({
            "type": "function",
            "name": "ask_user_question",
            "description": "Ask the person a question and wait for the chosen option. Unattended runs return an error without waiting.",
            "parameters": {
                "type": "object",
                "properties": {
                    "questions": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "question": {"type": "string"},
                                "header": {"type": "string"},
                                "multi_select": {"type": "boolean"},
                                "options": {
                                    "type": "array",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "label": {"type": "string"},
                                            "description": {"type": "string"}
                                        },
                                        "required": ["label"],
                                        "additionalProperties": false
                                    }
                                }
                            },
                            "required": ["question"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["questions"],
                "additionalProperties": false
            }
        }),
        json!({
            "type": "function",
            "name": "report_findings",
            "description": "At the end of a scan, diagnosis or review, post the findings card: each finding names the specific item and its root cause, with severity high, medium, low or info. Fixes are one-tap follow-ups the person can start (label up to 40 characters with no colon, goal is what the follow-up run does), e.g. \"Fix port 53 conflict\", \"Reinstall 6 packages\", \"Explain all\".",
            "parameters": {
                "type": "object",
                "properties": {
                    "findings": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "severity": {"type": "string", "enum": ["high", "medium", "low", "info"]},
                                "text": {"type": "string"}
                            },
                            "required": ["severity", "text"],
                            "additionalProperties": false
                        }
                    },
                    "fixes": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "label": {"type": "string"},
                                "goal": {"type": "string"}
                            },
                            "required": ["label", "goal"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["findings"],
                "additionalProperties": false
            }
        }),
        json!({
            "type": "function",
            "name": "enter_plan_mode",
            "description": "Make this session read-only so the next edits wait for an approved plan.",
            "parameters": {"type": "object", "properties": {}, "additionalProperties": false}
        }),
        json!({
            "type": "function",
            "name": "exit_plan_mode",
            "description": "Show the plan and leave plan mode only after the person approves it. Unattended runs are refused.",
            "parameters": {
                "type": "object",
                "properties": {
                    "plan": {"type": "string"}
                },
                "required": ["plan"],
                "additionalProperties": false
            }
        }),
    ]
}

pub fn try_run(call: SessionCall<'_>) -> Option<(ToolOutput, Usage)> {
    let output = match call.name {
        "todo_write" => todo_write(call.session, call.arguments),
        "ask_user_question" => ask(call),
        "report_findings" => report_findings(call.arguments, call.on_event),
        "enter_plan_mode" => enter_plan(call.session, call.gate),
        "exit_plan_mode" => exit_plan(call),
        _ => return None,
    };
    Some((output, Usage::default()))
}

fn plans() -> &'static Mutex<HashMap<String, bool>> {
    static MAP: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn set_plan_session(session: &str, on: bool) {
    if session.is_empty() {
        return;
    }
    let mut map = plans().lock().unwrap_or_else(|err| err.into_inner());
    if on {
        map.insert(session.to_string(), true);
    } else {
        map.remove(session);
    }
}

pub fn plan_on(session: &str) -> bool {
    plans()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .get(session)
        .copied()
        .unwrap_or(false)
}

fn todo_cache() -> &'static Mutex<HashMap<String, Vec<TodoItem>>> {
    static MAP: OnceLock<Mutex<HashMap<String, Vec<TodoItem>>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn todos_for(session: &str) -> Vec<TodoItem> {
    if session.is_empty() {
        return Vec::new();
    }
    let mut cache = todo_cache().lock().unwrap_or_else(|err| err.into_inner());
    if let Some(rows) = cache.get(session) {
        return rows.clone();
    }
    let loaded = load_todos(session);
    cache.insert(session.to_string(), loaded.clone());
    loaded
}

#[cfg(test)]
pub(crate) fn invalidate_todos(session: &str) {
    todo_cache()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .remove(session);
}

fn load_todos(session: &str) -> Vec<TodoItem> {
    let Ok(info) = crate::session::load_session(session) else {
        return Vec::new();
    };
    replay_todos(&info.input())
}

fn replay_todos(items: &[crate::InputItem]) -> Vec<TodoItem> {
    let mut todos = Vec::new();
    for item in items {
        let crate::InputItem::FunctionCall {
            name, arguments, ..
        } = item
        else {
            continue;
        };
        if name != "todo_write" {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(arguments) else {
            continue;
        };
        let _ = apply_todos(&mut todos, &value);
    }
    todos
}

fn todo_write(session: &str, arguments: &str) -> ToolOutput {
    let value = match serde_json::from_str::<Value>(arguments) {
        Ok(value) if value.as_object().is_some() => value,
        _ => return ToolOutput::err("todo_write arguments must be a JSON object"),
    };
    let mut current = todos_for(session);
    if let Err(err) = apply_todos(&mut current, &value) {
        return ToolOutput::err(err);
    }
    todo_cache()
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .insert(session.to_string(), current.clone());
    ToolOutput::ok(render_todos(&current))
}

fn apply_todos(todos: &mut Vec<TodoItem>, value: &Value) -> Result<(), String> {
    let Some(rows) = value.get("todos").and_then(|todos| todos.as_array()) else {
        return Err("todos must be an array".into());
    };
    let merge = value
        .get("merge")
        .and_then(|flag| flag.as_bool())
        .unwrap_or(false);
    let mut next = Vec::new();
    let mut seen = Vec::new();
    for row in rows {
        let id = row
            .get("id")
            .and_then(|id| id.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if id.is_empty() {
            return Err("todo id is required".into());
        }
        if seen.iter().any(|existing: &String| existing == &id) {
            return Err(format!("duplicate todo id `{id}`"));
        }
        seen.push(id.clone());
        let raw_content = row
            .get("content")
            .and_then(|content| content.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let had_content = !raw_content.is_empty();
        let content = if had_content { raw_content } else { id.clone() };
        let status = row
            .get("status")
            .and_then(|status| status.as_str())
            .unwrap_or("pending")
            .trim()
            .to_string();
        let status = if status.is_empty() {
            "pending".to_string()
        } else {
            status
        };
        if !matches!(
            status.as_str(),
            "pending" | "in_progress" | "completed" | "cancelled"
        ) {
            return Err(format!("unknown todo status `{status}`"));
        }
        next.push((
            TodoItem {
                id,
                content,
                status,
            },
            had_content,
        ));
    }
    if !merge {
        todos.clear();
    }
    for (row, had_content) in next {
        if let Some(existing) = todos.iter_mut().find(|todo| todo.id == row.id) {
            if merge && !had_content {
                existing.status = row.status;
            } else {
                *existing = row;
            }
        } else {
            todos.push(row);
        }
    }
    Ok(())
}

fn report_findings(arguments: &str, on_event: &mut dyn FnMut(LoopEvent)) -> ToolOutput {
    let card = serde_json::from_str::<Value>(arguments)
        .map_err(|_| "report_findings arguments must be a JSON object".to_string())
        .and_then(|value| grokhub_core::findings::Findings::from_json(&value));
    match card {
        Ok(card) => {
            on_event(LoopEvent::Findings(card.to_body()));
            ToolOutput::ok(format!(
                "Findings card posted: {} findings, {} one-tap fixes. It shows after your reply.",
                card.items.len(),
                card.fixes.len()
            ))
        }
        Err(err) => ToolOutput::err(err),
    }
}

fn render_todos(todos: &[TodoItem]) -> String {
    if todos.is_empty() {
        return "todos: (empty)".into();
    }
    let mut lines = Vec::new();
    for todo in todos {
        lines.push(format!("[{}] {}: {}", todo.status, todo.id, todo.content));
    }
    lines.join("\n")
}

fn ask(call: SessionCall<'_>) -> ToolOutput {
    if !call.attended {
        return ToolOutput::err(
            "ask_user_question was not asked: unattended runs cannot wait for a person",
        );
    }
    let value = match serde_json::from_str::<Value>(call.arguments) {
        Ok(value) if value.as_object().is_some() => value,
        _ => return ToolOutput::err("ask_user_question arguments must be a JSON object"),
    };
    let message = match question_message(&value) {
        Ok(message) => message,
        Err(err) => return ToolOutput::err(err),
    };
    let id = next_id("ask");
    let view = ElicitView {
        id: id.clone(),
        server_name: "Question".into(),
        message,
        mode: "form".into(),
        url: String::new(),
        elicitation_id: id.clone(),
        field_name: Some("answer".into()),
        field_title: "Your answer".into(),
        secret: false,
    };
    (call.on_event)(LoopEvent::Elicit(view));
    match mcp::wait_elicit(call.session, &id, call.cancel, call.halted) {
        mcp::ElicitAnswer::Accept(content) => {
            let answer = content
                .get("answer")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if answer.is_empty() {
                ToolOutput::err("ask_user_question needs an answer")
            } else {
                ToolOutput::ok(format!("answer: {answer}"))
            }
        }
        mcp::ElicitAnswer::Decline => ToolOutput::err("User declined the question"),
        mcp::ElicitAnswer::Cancel => ToolOutput::err("User cancelled the question"),
    }
}

fn question_message(value: &Value) -> Result<String, String> {
    let Some(rows) = value.get("questions").and_then(|item| item.as_array()) else {
        return Err("questions are required".into());
    };
    if rows.is_empty() {
        return Err("questions are required".into());
    }
    let mut blocks = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let question = row
            .get("question")
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .trim();
        if question.is_empty() {
            return Err("question is required".into());
        }
        let header = row
            .get("header")
            .and_then(|item| item.as_str())
            .unwrap_or("")
            .trim();
        let mut block = if header.is_empty() {
            format!("{}. {question}", index + 1)
        } else {
            format!("{}. {header}: {question}", index + 1)
        };
        if let Some(options) = row.get("options").and_then(|item| item.as_array()) {
            for option in options {
                let label = option
                    .get("label")
                    .and_then(|item| item.as_str())
                    .unwrap_or("")
                    .trim();
                if label.is_empty() {
                    continue;
                }
                let description = option
                    .get("description")
                    .and_then(|item| item.as_str())
                    .unwrap_or("")
                    .trim();
                if description.is_empty() {
                    block.push_str(&format!("\n- {label}"));
                } else {
                    block.push_str(&format!("\n- {label}: {description}"));
                }
            }
        }
        blocks.push(block);
    }
    blocks.push("Reply with the option label.".into());
    Ok(blocks.join("\n\n"))
}

fn enter_plan(session: &str, gate: &mut Gate) -> ToolOutput {
    set_plan_session(session, true);
    gate.readonly_session = true;
    ToolOutput::ok("Entered plan mode. The session is read-only until the plan is approved.")
}

fn exit_plan(call: SessionCall<'_>) -> ToolOutput {
    if !plan_on(call.session) {
        return ToolOutput::err("not in plan mode");
    }
    if !call.attended {
        return ToolOutput::err(
            "exit_plan_mode was refused: unattended runs cannot leave plan mode",
        );
    }
    let value = match serde_json::from_str::<Value>(call.arguments) {
        Ok(value) if value.as_object().is_some() => value,
        _ => return ToolOutput::err("exit_plan_mode arguments must be a JSON object"),
    };
    let plan = value
        .get("plan")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let shown = if plan.is_empty() {
        "(empty plan)".to_string()
    } else {
        plan.clone()
    };
    (call.on_event)(LoopEvent::Plan(shown.clone()));
    let id = next_id("plan");
    let view = ElicitView {
        id: id.clone(),
        server_name: "Plan".into(),
        message: shown,
        mode: "form".into(),
        url: String::new(),
        elicitation_id: id.clone(),
        field_name: None,
        field_title: String::new(),
        secret: false,
    };
    (call.on_event)(LoopEvent::Elicit(view));
    match mcp::wait_elicit(call.session, &id, call.cancel, call.halted) {
        mcp::ElicitAnswer::Accept(_) => {
            set_plan_session(call.session, false);
            if call.plan_at_start || !call.base_readonly {
                call.gate.readonly_session = false;
            } else {
                call.gate.readonly_session = call.base_readonly;
            }
            ToolOutput::ok("Plan approved. Plan mode is off.")
        }
        mcp::ElicitAnswer::Decline => ToolOutput::err("User declined the plan"),
        mcp::ElicitAnswer::Cancel => ToolOutput::err("User cancelled the plan"),
    }
}

fn next_id(prefix: &str) -> String {
    static SEQ: AtomicU64 = AtomicU64::new(1);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{n}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::{ClosedPermits, Gate, PermMode, PermitWait};
    use crate::run::{HaltCheck, LoopIn, SteerQueue};
    use crate::tools::READ_ONLY_PHASE;
    use crate::{ClientError, FunctionCall, InputItem, ModelClient, ResponsesRequest, TurnOutput};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
    use std::sync::Arc;

    struct Never;

    impl HaltCheck for Never {
        fn halted(&self) -> bool {
            false
        }
    }

    struct Script {
        turns: Mutex<Vec<TurnOutput>>,
    }

    impl ModelClient for Script {
        fn stream(
            &self,
            _req: &ResponsesRequest,
            _cancel: &CancelToken,
            _sink: &mut dyn FnMut(crate::StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            let mut turns = self.turns.lock().unwrap_or_else(|err| err.into_inner());
            if turns.is_empty() {
                return Ok(TurnOutput {
                    text: "done".into(),
                    reasoning: String::new(),
                    calls: Vec::new(),
                    usage: Usage::default(),
                });
            }
            Ok(turns.remove(0))
        }
    }

    fn unique() -> u64 {
        static N: AtomicU64 = AtomicU64::new(1);
        N.fetch_add(1, AtomicOrdering::Relaxed)
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gh-sess-{tag}-{}-{}", std::process::id(), unique()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sid(tag: &str) -> String {
        format!("sess{tag}{}{}", std::process::id(), unique())
    }

    fn call(id: &str, name: &str, args: &str) -> TurnOutput {
        TurnOutput {
            text: String::new(),
            reasoning: String::new(),
            calls: vec![FunctionCall {
                call_id: id.into(),
                name: name.into(),
                arguments: args.into(),
            }],
            usage: Usage::default(),
        }
    }

    fn chat(readonly: bool, attended: bool) -> Gate {
        Gate {
            mode: PermMode::Always,
            readonly_session: readonly,
            attended,
            desktop: false,
        }
    }

    fn run(
        dir: &Path,
        session: &str,
        gate: Gate,
        turns: Vec<TurnOutput>,
        on_event: &mut dyn FnMut(LoopEvent),
    ) -> (crate::run::LoopOut, Vec<InputItem>) {
        let client = Arc::new(Script {
            turns: Mutex::new(turns),
        });
        let permits: Arc<dyn PermitWait + Send + Sync> = Arc::new(ClosedPermits);
        let shared_permits = Arc::clone(&permits);
        let halt = Never;
        let steer = SteerQueue::new();
        let cancel = CancelToken::new();
        let input = LoopIn {
            client: client.as_ref(),
            workspace: dir,
            model: "grok-4.7",
            effort: None,
            system: "",
            conversation_id: session,
            max_turns: 8,
            usage_base: Usage::default(),
            cancel: &cancel,
            steer: &steer,
            halt: &halt,
            gate,
            desktop: None,
            permits: permits.as_ref(),
            perms: None,
            context_length: 0,
            tasks: None,
            depth: 0,
            agent_id: None,
            shared_client: Some(Arc::clone(&client) as Arc<dyn ModelClient + Send + Sync>),
            shared_permits: Some(shared_permits),
            shared_desktop: None,
        };
        let mut history = Vec::new();
        let out = crate::run_loop(&input, &mut history, "go", None, on_event);
        (out, history)
    }

    fn outputs(items: &[InputItem]) -> String {
        items
            .iter()
            .filter_map(|item| match item {
                InputItem::FunctionCallOutput { output, .. } => Some(output.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn answer(
        tx: &std::sync::mpsc::Sender<crate::mcp::ElicitNote>,
        action: &str,
        content: Option<Value>,
    ) -> impl FnMut(LoopEvent) {
        let tx = tx.clone();
        let action = action.to_string();
        move |ev: LoopEvent| {
            if let LoopEvent::Elicit(view) = ev {
                let _ = tx.send(crate::mcp::ElicitNote {
                    id: view.id,
                    action: action.clone(),
                    content: content.clone(),
                });
            }
        }
    }

    #[test]
    fn todo_write_persists_and_compaction_keeps_open_todos() {
        let dir = scratch("todo");
        let cfg = scratch("todocfg");
        let _guard = crate::perm::ConfigGuard::set(&cfg);
        let session = sid("todo");
        let first = json!({
            "merge": false,
            "todos": [
                {"id": "a", "content": "open", "status": "pending"},
                {"id": "b", "content": "work", "status": "in_progress"},
                {"id": "c", "content": "done", "status": "completed"}
            ]
        })
        .to_string();
        let second = json!({
            "merge": true,
            "todos": [{"id": "b", "status": "completed"}]
        })
        .to_string();
        let (out, history) = run(
            &dir,
            &session,
            chat(false, true),
            vec![
                call("1", "todo_write", &first),
                call("2", "todo_write", &second),
            ],
            &mut |_| {},
        );
        assert_eq!(out.stop, crate::run::StopReason::EndTurn);
        let rows = todos_for(&session);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].status, "pending");
        assert_eq!(rows[1].id, "b");
        assert_eq!(
            rows[1].content, "work",
            "merge without content keeps the text"
        );
        assert_eq!(rows[1].status, "completed");
        crate::session::record_turn(
            &session,
            &dir.display().to_string(),
            "grok-4.7",
            &history,
            &out.usage,
            "API credits",
            "todos",
        )
        .expect("record");
        invalidate_todos(&session);
        let loaded = todos_for(&session);
        assert_eq!(loaded, rows);
        let open = crate::compact::open_todos(&history);
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].id, "a");
        assert_eq!(open[0].status, "pending");
        let compacted = crate::compact::build_compacted_history(&history, "summary");
        let blob = compacted
            .iter()
            .map(crate::compact::message_text)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(blob.contains("[pending] a: open"), "{blob}");
        assert!(blob.contains("summary"), "{blob}");
        assert!(!blob.contains("[completed] c:"), "{blob}");
        let mut gate = chat(false, true);
        let cancel = CancelToken::new();
        let dup = try_run(SessionCall {
            name: "todo_write",
            arguments: r#"{"todos":[{"id":"a","content":"x","status":"pending"},{"id":"a","content":"y","status":"pending"}]}"#,
            gate: &mut gate,
            session: &session,
            attended: true,
            cancel: &cancel,
            halted: &|| false,
            base_readonly: false,
            plan_at_start: false,
            on_event: &mut |_| {},
        })
        .expect("todo tool");
        assert!(dup.0.failed, "{}", dup.0.text);
        assert!(dup.0.text.contains("duplicate"));
        assert_eq!(todos_for(&session), loaded);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn ask_user_question_waits_when_attended_and_errors_when_unattended() {
        let session = sid("ask");
        let mut gate = chat(true, false);
        let cancel = CancelToken::new();
        let mut events = Vec::new();
        let questions = r#"{"questions":[{"question":"Ship it?","header":"Ship","options":[{"label":"Yes","description":"do it"},{"label":"No","description":"stop"}]}]}"#;
        let denied = try_run(SessionCall {
            name: "ask_user_question",
            arguments: questions,
            gate: &mut gate,
            session: &session,
            attended: false,
            cancel: &cancel,
            halted: &|| false,
            base_readonly: true,
            plan_at_start: false,
            on_event: &mut |ev| events.push(ev),
        })
        .expect("ask");
        assert!(denied.0.failed);
        assert!(
            denied.0.text.contains("unattended runs cannot wait"),
            "{}",
            denied.0.text
        );
        assert!(events.is_empty(), "unattended ask must not show a card");

        let dir = scratch("ask");
        let (tx, inbox) = crate::mcp::ElicitInbox::pair();
        crate::mcp::attach_elicit(&session, inbox);
        let mut seen = Vec::new();
        let (out, history) = run(
            &dir,
            &session,
            chat(true, true),
            vec![call("1", "ask_user_question", questions)],
            &mut answer(&tx, "accept", Some(json!({"answer": "Yes"}))),
        );
        assert_eq!(out.stop, crate::run::StopReason::EndTurn);
        let text = outputs(&history);
        assert!(text.contains("answer: Yes"), "{text}");
        let declined = sid("askno");
        let (tx_no, inbox_no) = crate::mcp::ElicitInbox::pair();
        crate::mcp::attach_elicit(&declined, inbox_no);
        let (_out, history) = run(
            &dir,
            &declined,
            chat(true, true),
            vec![call("1", "ask_user_question", questions)],
            &mut |ev| {
                if let LoopEvent::Elicit(view) = &ev {
                    seen.push(view.message.clone());
                }
                answer(&tx_no, "decline", None)(ev);
            },
        );
        assert!(
            seen.iter().any(|message| message.contains("Yes: do it")),
            "{seen:?}"
        );
        assert!(
            outputs(&history).contains("User declined"),
            "{}",
            outputs(&history)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plan_mode_is_read_only_until_the_user_approves() {
        let dir = scratch("plan");
        let session = sid("plan");
        let (tx, inbox) = crate::mcp::ElicitInbox::pair();
        crate::mcp::attach_elicit(&session, inbox);
        let (_out, history) = run(
            &dir,
            &session,
            chat(false, true),
            vec![
                call("1", "enter_plan_mode", "{}"),
                call("2", "write", r#"{"path":"note.txt","content":"too-soon"}"#),
                call("3", "exit_plan_mode", r#"{"plan":"ship it"}"#),
                call("4", "write", r#"{"path":"note.txt","content":"approved"}"#),
            ],
            &mut answer(&tx, "accept", None),
        );
        let text = outputs(&history);
        assert!(text.contains(READ_ONLY_PHASE), "{text}");
        assert!(text.contains("Plan approved"), "{text}");
        assert_eq!(
            std::fs::read_to_string(dir.join("note.txt")).unwrap(),
            "approved"
        );
        assert!(!plan_on(&session));

        let declined = sid("plandecl");
        let (tx, inbox) = crate::mcp::ElicitInbox::pair();
        crate::mcp::attach_elicit(&declined, inbox);
        let (_out, history) = run(
            &dir,
            &declined,
            chat(false, true),
            vec![
                call("1", "enter_plan_mode", "{}"),
                call("2", "exit_plan_mode", r#"{"plan":"wait"}"#),
                call("3", "write", r#"{"path":"nope.txt","content":"no"}"#),
            ],
            &mut answer(&tx, "decline", None),
        );
        let text = outputs(&history);
        assert!(text.contains("User declined the plan"), "{text}");
        assert!(text.contains(READ_ONLY_PHASE), "{text}");
        assert!(!dir.join("nope.txt").exists());
        assert!(plan_on(&declined));

        let away = sid("planaway");
        set_plan_session(&away, true);
        let mut gate = chat(true, false);
        let cancel = CancelToken::new();
        let refused = try_run(SessionCall {
            name: "exit_plan_mode",
            arguments: r#"{"plan":"no"}"#,
            gate: &mut gate,
            session: &away,
            attended: false,
            cancel: &cancel,
            halted: &|| false,
            base_readonly: true,
            plan_at_start: true,
            on_event: &mut |_| {},
        })
        .expect("exit");
        assert!(
            refused
                .0
                .text
                .contains("unattended runs cannot leave plan mode"),
            "{}",
            refused.0.text
        );
        assert!(plan_on(&away));
        assert!(gate.readonly_session);

        let not_plan = sid("notplan");
        let mut gate = chat(true, true);
        let missing = try_run(SessionCall {
            name: "exit_plan_mode",
            arguments: r#"{"plan":"no"}"#,
            gate: &mut gate,
            session: &not_plan,
            attended: true,
            cancel: &cancel,
            halted: &|| false,
            base_readonly: true,
            plan_at_start: false,
            on_event: &mut |_| {},
        })
        .expect("exit");
        assert!(
            missing.0.text.contains("not in plan mode"),
            "{}",
            missing.0.text
        );

        let ask_mode = sid("askmode");
        let (tx, inbox) = crate::mcp::ElicitInbox::pair();
        crate::mcp::attach_elicit(&ask_mode, inbox);
        let (_out, history) = run(
            &dir,
            &ask_mode,
            chat(true, true),
            vec![
                call("1", "enter_plan_mode", "{}"),
                call("2", "exit_plan_mode", r#"{"plan":"still ask"}"#),
                call("3", "write", r#"{"path":"ask.txt","content":"no"}"#),
            ],
            &mut answer(&tx, "accept", None),
        );
        let text = outputs(&history);
        assert!(text.contains("Plan approved"), "{text}");
        assert!(text.contains(READ_ONLY_PHASE), "{text}");
        assert!(!dir.join("ask.txt").exists());
        assert!(!plan_on(&ask_mode));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
