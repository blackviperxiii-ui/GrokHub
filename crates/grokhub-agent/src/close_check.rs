//! One cheap check before a multi-step turn ends.
//!
//! A chat with no open intentions and no pending todos never calls the model.
//! Depth above 0 skips the check. Drive already owns an untouched checklist
//! and a stalled run, so those stops stay as they are.

use std::time::Duration;

use grokhub_core::findings::{Finding, Findings, Fix, Severity};
use grokhub_core::{
    apply_status_updates, automation_done_card, intentions_block, load_intentions, save_intentions,
    suggestion_card, IntentionStatus, Intentions,
};
use serde_json::Value;

use crate::client::{ClientError, ContentPart, InputItem, ModelClient, ResponsesRequest};
use crate::drive::Drive;
use crate::session_tools::{todos_for, TodoItem};
use crate::{CancelToken, Usage};

/// Routed like the other low-effort background judges.
pub const CLOSE_CLASS: &str = crate::route::CLASS_JUDGE;

const STEP_CAP: usize = 3;
const STEP_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseReport {
    pub need_met: bool,
    pub open: Vec<String>,
    pub next_steps: Vec<String>,
    pub updates: Vec<Value>,
}

pub struct CloseIn<'a> {
    pub depth: u32,
    pub blocked: bool,
    pub conversation_id: &'a str,
    pub model: &'a str,
    pub client: &'a dyn ModelClient,
    pub cancel: &'a CancelToken,
    pub drive: Option<&'a Drive>,
    pub last_assistant: &'a str,
    pub usage: &'a mut Usage,
}

pub enum CloseGate {
    /// End the turn. `Some` is a findings body for a completion notice.
    Proceed(Option<String>),
    Continue {
        note: String,
        steps: Vec<String>,
    },
    Cancelled,
}

pub fn todos_pending(todos: &[TodoItem]) -> bool {
    todos
        .iter()
        .any(|item| matches!(item.status.as_str(), "pending" | "in_progress"))
}

pub fn intentions_open(list: &Intentions) -> bool {
    list.items.iter().any(|item| {
        matches!(
            item.status,
            IntentionStatus::Open | IntentionStatus::Waiting
        )
    })
}

/// The spec gate: pending todos or an open/waiting intention. Depth and Drive
/// sit in [`should_run`].
pub fn warranted(todos: &[TodoItem], intentions: &Intentions) -> bool {
    todos_pending(todos) || intentions_open(intentions)
}

/// Whether this stop should spend a model call.
///
/// `blocked` is Drive's stall stop. An untouched checklist leaves `drive_open`
/// false so a later ask is not taken over. With no Drive (plan mode), any
/// pending todo counts.
pub fn should_run(
    depth: u32,
    blocked: bool,
    drive_open: bool,
    intentions_are_open: bool,
    todos_are_pending: bool,
    drive_present: bool,
) -> bool {
    if depth != 0 || blocked {
        return false;
    }
    if intentions_are_open {
        return true;
    }
    if drive_present {
        drive_open
    } else {
        todos_are_pending
    }
}

pub fn prompt_block(conversation_id: &str) -> String {
    if conversation_id.is_empty() {
        return String::new();
    }
    intentions_block(&load_intentions(
        &crate::perm::config_dir(),
        conversation_id,
    ))
}

pub fn before_end(inp: &mut CloseIn<'_>) -> CloseGate {
    if inp.depth != 0 || inp.blocked {
        return CloseGate::Proceed(None);
    }
    if inp.cancel.is_cancelled() {
        return CloseGate::Cancelled;
    }
    let dir = crate::perm::config_dir();
    let intentions = load_intentions(&dir, inp.conversation_id);
    let todos = todos_for(inp.conversation_id);
    let drive_open = inp
        .drive
        .map(|drive| !drive.open_ids(&todos).is_empty())
        .unwrap_or(false);
    if !warranted(&todos, &intentions)
        || !should_run(
            inp.depth,
            inp.blocked,
            drive_open,
            intentions_open(&intentions),
            todos_pending(&todos),
            inp.drive.is_some(),
        )
    {
        return CloseGate::Proceed(None);
    }
    let req = close_request(
        inp.model,
        inp.conversation_id,
        &intentions,
        &todos,
        inp.last_assistant,
    );
    let turn = match crate::route::live::stream_routed(
        inp.client,
        &req,
        inp.cancel,
        &mut |_| {},
        CLOSE_CLASS,
    ) {
        Ok(turn) => turn,
        Err(ClientError::Cancelled) => return CloseGate::Cancelled,
        // A failed check must not hold the turn open.
        Err(_) => return CloseGate::Proceed(None),
    };
    inp.usage.add(&turn.usage);
    let Some(report) = parse_report(&turn.text) else {
        return CloseGate::Proceed(None);
    };
    let before = intentions.clone();
    let mut intentions = intentions;
    let _ = apply_status_updates(&mut intentions, &report.updates);
    if intentions != before {
        let _ = save_intentions(&dir, inp.conversation_id, &intentions);
    }
    let todos_open = if inp.drive.is_some() {
        drive_open
    } else {
        todos_pending(&todos)
    };
    if unfinished(&report, &intentions, todos_open) {
        CloseGate::Continue {
            note: continue_note(&report, &intentions),
            steps: report.next_steps,
        }
    } else {
        CloseGate::Proceed(completion_body(&newly_done(&before, &intentions)))
    }
}

pub fn close_request(
    model: &str,
    conversation_id: &str,
    intentions: &Intentions,
    todos: &[TodoItem],
    last_assistant: &str,
) -> ResponsesRequest {
    ResponsesRequest {
        model: model.to_string(),
        effort: Some("low".into()),
        input: vec![
            InputItem::Message {
                role: "system".into(),
                content: vec![ContentPart::InputText(CLOSE_SYSTEM.into())],
            },
            InputItem::Message {
                role: "user".into(),
                content: vec![ContentPart::InputText(judge_prompt(
                    intentions,
                    todos,
                    last_assistant,
                ))],
            },
        ],
        conversation_id: conversation_id.to_string(),
        tools: Vec::new(),
        hosted_search: false,
        call_timeout: Some(Duration::from_secs(30)),
    }
}

pub(crate) const CLOSE_SYSTEM: &str = "\
You check whether a multi-step chat is finished. \
Reply with one JSON object and no other text. \
Keys: need_met (bool), open (array of ids still open), \
next_steps (1 to 3 short grounded steps, or an empty array), \
status_updates (array of objects with id and status). \
status is open, done, or waiting. Change status only. \
Before need_met is true, require a verification step \
(create-verification-skill, maintain-verification-skill, \
or the project's verify skill). Before a ship or release \
step, require qa-walkthrough (and av-qa-walkthrough when \
audio, video, capture, or streaming changed). Multi-step \
work must keep follow-through.\
";

pub fn parse_report(text: &str) -> Option<CloseReport> {
    let value = json_object(text)?;
    let need_met = value.get("need_met")?.as_bool()?;
    let open = string_list(value.get("open"), 20);
    let next_steps = string_list(value.get("next_steps"), STEP_CAP);
    let updates = value
        .get("status_updates")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Some(CloseReport {
        need_met,
        open,
        next_steps,
        updates,
    })
}

pub fn unfinished(report: &CloseReport, intentions: &Intentions, todos_open: bool) -> bool {
    !report.need_met || !report.open.is_empty() || intentions_open(intentions) || todos_open
}

pub fn continue_note(report: &CloseReport, intentions: &Intentions) -> String {
    let mut parts = Vec::new();
    if !report.need_met {
        parts.push("Not done yet.".to_string());
    }
    let open: Vec<String> = intentions
        .items
        .iter()
        .filter(|item| item.status != IntentionStatus::Done)
        .map(|item| format!("\"{}\"", item.text))
        .collect();
    if !open.is_empty() {
        parts.push(format!("Still open: {}.", open.join(", ")));
    } else if !report.open.is_empty() {
        parts.push(format!("Still open: {}.", report.open.join(", ")));
    }
    if !report.next_steps.is_empty() {
        parts.push(format!("Next: {}.", report.next_steps.join("; ")));
    }
    parts.push("Continue with the next open item.".to_string());
    parts.join(" ")
}

/// Findings body for steps still open, built with the existing suggestion card.
pub fn steps_card(steps: &[String]) -> Option<String> {
    let steps: Vec<String> = steps
        .iter()
        .map(|step| clip_chars(step.trim(), STEP_CHARS))
        .filter(|step| !step.is_empty())
        .take(STEP_CAP)
        .collect();
    if steps.is_empty() {
        return None;
    }
    let card = suggestion_card(
        "close-check",
        "Still open",
        &steps.join(" "),
        grokhub_core::now_ms(),
    );
    let mut items = vec![Finding {
        severity: Severity::Medium,
        text: card.title,
    }];
    if let Some(body) = card.body {
        items.push(Finding {
            severity: Severity::Info,
            text: body,
        });
    }
    let fixes = steps
        .iter()
        .filter_map(|step| {
            let label = step_label(step);
            if label.is_empty() || grokhub_core::findings::is_impossible_one_tap(&label, step) {
                return None;
            }
            Some(Fix {
                label,
                goal: step.clone(),
            })
        })
        .collect();
    Some(Findings { items, fixes }.to_body())
}

pub fn lists_still_open(conversation_id: &str) -> bool {
    let intentions = load_intentions(&crate::perm::config_dir(), conversation_id);
    let todos = todos_for(conversation_id);
    intentions_open(&intentions) || todos_pending(&todos)
}

fn completion_body(done: &[String]) -> Option<String> {
    if done.is_empty() {
        return None;
    }
    let card = automation_done_card(
        "close-check",
        "Finished",
        &done.join(", "),
        grokhub_core::now_ms(),
    );
    let text = match card.body {
        Some(body) => format!("{}. {body}", card.title),
        None => card.title,
    };
    Some(
        Findings {
            items: vec![Finding {
                severity: Severity::Info,
                text,
            }],
            fixes: Vec::new(),
        }
        .to_body(),
    )
}

fn newly_done(before: &Intentions, after: &Intentions) -> Vec<String> {
    after
        .items
        .iter()
        .filter(|item| item.status == IntentionStatus::Done)
        .filter(|item| {
            before
                .items
                .iter()
                .any(|old| old.id == item.id && old.status != IntentionStatus::Done)
        })
        .map(|item| item.text.clone())
        .collect()
}

fn judge_prompt(intentions: &Intentions, todos: &[TodoItem], last_assistant: &str) -> String {
    let (last, _) = grokhub_core::redact_recall(last_assistant);
    let last = clip_chars(last.trim(), 1500);
    let mut lines = vec!["Intentions:".to_string()];
    if intentions.items.is_empty() {
        lines.push("(none)".into());
    }
    for item in &intentions.items {
        lines.push(format!(
            "- [{}] {}: {}",
            item.status.as_str(),
            item.id,
            clip_chars(&item.text, INTENTION_CLIP)
        ));
    }
    lines.push("Todos:".into());
    if todos.is_empty() {
        lines.push("(none)".into());
    }
    for todo in todos {
        lines.push(format!(
            "- [{}] {}: {}",
            todo.status,
            todo.id,
            clip_chars(&todo.content, INTENTION_CLIP)
        ));
    }
    lines.push("Last assistant reply:".into());
    lines.push(last);
    lines.join("\n")
}

const INTENTION_CLIP: usize = 160;

fn json_object(text: &str) -> Option<Value> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end < start {
        return None;
    }
    serde_json::from_str(&text[start..=end]).ok()
}

fn string_list(value: Option<&Value>, cap: usize) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.as_str())
                .map(str::trim)
                .filter(|row| !row.is_empty())
                .take(cap)
                .map(|row| clip_chars(row, STEP_CHARS))
                .collect()
        })
        .unwrap_or_default()
}

fn step_label(step: &str) -> String {
    let label: String = step
        .chars()
        .filter(|c| *c != ':')
        .take(grokhub_core::findings::LABEL_MAX)
        .collect();
    label.trim().to_string()
}

fn clip_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::{Intention, IntentionStatus, Intentions};

    fn item(id: &str, status: &str) -> TodoItem {
        TodoItem {
            id: id.into(),
            content: format!("check {id}"),
            status: status.into(),
        }
    }

    fn open_list() -> Intentions {
        Intentions {
            items: vec![Intention {
                id: "ship".into(),
                text: "ship the notes".into(),
                status: IntentionStatus::Open,
            }],
        }
    }

    #[test]
    fn a_plain_chat_skips_and_open_work_runs() {
        let empty = Intentions::default();
        assert!(!warranted(&[], &empty));
        assert!(!should_run(0, false, false, false, false, true));
        assert!(warranted(&[item("a", "pending")], &empty));
        assert!(warranted(&[item("a", "in_progress")], &empty));
        assert!(!warranted(&[item("a", "completed")], &empty));
        assert!(warranted(&[], &open_list()));
        assert!(should_run(0, false, false, false, true, false));
        assert!(!should_run(0, false, false, false, true, true));
        assert!(should_run(0, false, true, false, true, true));
        assert!(should_run(0, false, false, true, false, true));
        assert!(!should_run(1, false, true, true, true, false));
        assert!(!should_run(0, true, true, false, true, true));
    }

    #[test]
    fn status_updates_apply_and_text_rewrites_are_ignored() {
        let mut list = open_list();
        let raw = serde_json::json!({
            "id": "ship",
            "status": "done",
            "text": "stolen text"
        });
        let report = parse_report(&format!(
            "note\n{json}\n",
            json = serde_json::json!({
                "need_met": false,
                "open": ["ship", ""],
                "next_steps": ["run the tests", "write the notes", "push the branch", "extra"],
                "status_updates": [raw]
            })
        ))
        .expect("report");
        assert!(!report.need_met);
        assert_eq!(report.open, vec!["ship".to_string()]);
        assert_eq!(
            report.next_steps,
            vec![
                "run the tests".to_string(),
                "write the notes".to_string(),
                "push the branch".to_string()
            ]
        );
        assert_eq!(apply_status_updates(&mut list, &report.updates), 1);
        assert_eq!(list.items[0].id, "ship");
        assert_eq!(list.items[0].text, "ship the notes");
        assert_eq!(list.items[0].status, IntentionStatus::Done);
        assert!(!unfinished(
            &CloseReport {
                need_met: true,
                open: Vec::new(),
                next_steps: Vec::new(),
                updates: Vec::new(),
            },
            &list,
            false
        ));
        let card = steps_card(&["run the tests".into()]).expect("card");
        assert!(card.contains("run the tests"), "{card}");
        assert!(card.starts_with("Findings from this run"), "{card}");
    }

    #[test]
    fn the_check_request_is_a_low_effort_judge_with_no_tools() {
        let req = close_request("grok-4.7", "chat-1", &open_list(), &[], "All done.");
        assert_eq!(req.effort.as_deref(), Some("low"));
        assert!(req.tools.is_empty());
        assert!(!req.hosted_search);
        assert_eq!(req.call_timeout, Some(Duration::from_secs(30)));
        let text = match &req.input[1] {
            InputItem::Message { content, .. } => match &content[0] {
                ContentPart::InputText(text) => text.clone(),
                _ => String::new(),
            },
            _ => String::new(),
        };
        assert!(text.contains("ship the notes"), "{text}");
        assert!(text.contains("All done."), "{text}");
    }
}
