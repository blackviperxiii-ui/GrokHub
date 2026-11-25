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

use crate::client::{
    ClientError, ContentPart, InputItem, ModelClient, ResponsesRequest, TextFormat,
};
use crate::drive::Drive;
use crate::post_check::Ran;
use crate::session_tools::{todos_for, TodoItem};
use crate::{CancelToken, Usage};

/// Routed like the other low-effort background judges.
pub const CLOSE_CLASS: &str = crate::route::CLASS_JUDGE;

const STEP_CAP: usize = 3;
const STEP_CHARS: usize = 200;
/// Files the judge sees by name before "and N more".
const WORK_FILES: usize = 20;
/// The newest commands the judge sees.
const WORK_COMMANDS: usize = 8;

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
    /// This run's tool calls and the files it wrote: the judge reads the
    /// work beside the reply.
    pub ran: &'a [Ran],
    pub written: &'a [String],
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
        inp.ran,
        inp.written,
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
    ran: &[Ran],
    written: &[String],
) -> ResponsesRequest {
    ResponsesRequest {
        text_format: Some(CLOSE_FORMAT),
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
                    ran,
                    written,
                ))],
            },
        ],
        conversation_id: conversation_id.to_string(),
        tools: Vec::new(),
        hosted_search: false,
        call_timeout: Some(Duration::from_secs(30)),
    }
}

/// The close-check reply schema: the keys [`CLOSE_SYSTEM`] lists.
pub(crate) const CLOSE_FORMAT: TextFormat = TextFormat {
    name: "close_check_report",
    schema: r#"{"type":"object","properties":{"need_met":{"type":"boolean"},"open":{"type":"array","items":{"type":"string"},"maxItems":20},"next_steps":{"type":"array","items":{"type":"string"},"maxItems":3},"status_updates":{"type":"array","items":{"type":"object","properties":{"id":{"type":"string"},"status":{"type":"string","enum":["open","done","waiting"]}},"required":["id","status"],"additionalProperties":false}}},"required":["need_met","open","next_steps","status_updates"],"additionalProperties":false}"#,
};

pub(crate) const CLOSE_SYSTEM: &str = "\
You check whether a multi-step chat is finished. \
Reply with one JSON object and no other text. \
Keys: need_met (bool), open (array of ids still open), \
next_steps (1 to 3 short grounded steps, or an empty array), \
status_updates (array of objects with id and status). \
status is open, done, or waiting. Change status only. \
Judge the work list, not the reply's words: a change the reply \
claims needs a file written or a command that made it, and a \
passing check needs its command with exit 0; if they are missing, \
need_met is false. \
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

fn judge_prompt(
    intentions: &Intentions,
    todos: &[TodoItem],
    last_assistant: &str,
    ran: &[Ran],
    written: &[String],
) -> String {
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
    lines.extend(work_lines(ran, written));
    lines.push("Last assistant reply:".into());
    lines.push(last);
    lines.join("\n")
}

/// What this run did, from its own tool calls: the files it wrote, its newest
/// commands with how each ended, and its last test run.
fn work_lines(ran: &[Ran], written: &[String]) -> Vec<String> {
    let mut lines = vec!["Work this run (from the tool calls, not the reply):".to_string()];
    let files: Vec<String> = written
        .iter()
        .take(WORK_FILES)
        .map(|path| clip_chars(path, INTENTION_CLIP))
        .collect();
    lines.push(match written.len() {
        0 => "Files written: (none)".into(),
        n if n > WORK_FILES => format!(
            "Files written: {} and {} more",
            files.join(", "),
            n - WORK_FILES
        ),
        _ => format!("Files written: {}", files.join(", ")),
    });
    let command = |r: &Ran| {
        let (text, _) = grokhub_core::redact_recall(&r.command);
        let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
        format!(
            "`{}` → {}",
            clip_chars(&flat, INTENTION_CLIP).replace('`', "'"),
            r.ended()
        )
    };
    let shell: Vec<&Ran> = ran
        .iter()
        .filter(|r| r.name == "run_terminal_command")
        .collect();
    lines.push("Commands, newest last:".into());
    if shell.is_empty() {
        lines.push("(none)".into());
    }
    for r in &shell[shell.len().saturating_sub(WORK_COMMANDS)..] {
        lines.push(format!("- {}", command(r)));
    }
    lines.push(
        match shell
            .iter()
            .rev()
            .find(|r| crate::post_check::is_test_command(&r.command))
        {
            Some(r) => format!("Last test run: {}", command(r)),
            None => "Last test run: (none)".into(),
        },
    );
    lines
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

use crate::skills::clip_chars;

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::{Intention, IntentionStatus, Intentions};

    #[test]
    fn the_close_request_asks_for_the_strict_report_and_its_reply_parses() {
        let req = close_request(
            "grok-4.3",
            "c",
            &Intentions::default(),
            &[],
            "done",
            &[],
            &[],
        );
        assert_eq!(req.text_format, Some(CLOSE_FORMAT));
        let report = parse_report(
            r#"{"need_met":false,"open":["t1"],"next_steps":["run the tests"],"status_updates":[{"id":"t1","status":"open"}]}"#,
        )
        .unwrap();
        assert!(!report.need_met);
        assert_eq!(report.open, vec!["t1".to_string()]);
        assert_eq!(report.next_steps, vec!["run the tests".to_string()]);
    }

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
        let req = close_request(
            "grok-4.7",
            "chat-1",
            &open_list(),
            &[],
            "All done.",
            &[],
            &[],
        );
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

    fn user_text(req: &ResponsesRequest) -> String {
        match &req.input[1] {
            InputItem::Message { content, .. } => match &content[0] {
                ContentPart::InputText(text) => text.clone(),
                _ => String::new(),
            },
            _ => String::new(),
        }
    }

    fn shell(command: &str, ok: bool, output: &str) -> Ran {
        Ran::new(
            "run_terminal_command",
            &serde_json::json!({ "command": command }).to_string(),
            ok,
            output,
        )
    }

    #[test]
    fn the_judge_reads_the_runs_work_not_only_its_reply() {
        let ran = [
            Ran::new(
                "search_replace",
                r#"{"file_path":"src/handler.rs","old_string":"a","new_string":"b"}"#,
                true,
                "ok",
            ),
            shell("git status", true, "exit 0\nnothing to commit"),
            shell(
                "cargo test -p app",
                false,
                "exit 101\ntest result: FAILED. 3 passed; 1 failed",
            ),
        ];
        let written = ["src/handler.rs".to_string()];
        let req = close_request(
            "judge",
            "chat-1",
            &open_list(),
            &[],
            "Updated the handler.",
            &ran,
            &written,
        );
        assert_eq!(
            user_text(&req),
            "Intentions:\n- [open] ship: ship the notes\nTodos:\n(none)\n\
             Work this run (from the tool calls, not the reply):\n\
             Files written: src/handler.rs\n\
             Commands, newest last:\n\
             - `git status` → exit 0\n\
             - `cargo test -p app` → exit 101\n\
             Last test run: `cargo test -p app` → exit 101\n\
             Last assistant reply:\nUpdated the handler."
        );
        // No work: the block says so.
        let none = close_request(
            "judge",
            "chat-1",
            &open_list(),
            &[],
            "Updated the handler.",
            &[],
            &[],
        );
        assert_eq!(
            user_text(&none),
            "Intentions:\n- [open] ship: ship the notes\nTodos:\n(none)\n\
             Work this run (from the tool calls, not the reply):\n\
             Files written: (none)\n\
             Commands, newest last:\n\
             (none)\n\
             Last test run: (none)\n\
             Last assistant reply:\nUpdated the handler."
        );
        // A fenced result still says how it ended; a refused command never ran;
        // a backquote can't end the command's span early.
        let fenced = "<<<UNTRUSTED source=shell:network nonce=n1>>>\nexit 0\n200 OK\n<<<END_UNTRUSTED nonce=n1>>>";
        let ran = [
            shell("curl -s https://example.com/health", true, fenced),
            shell(
                "rm -rf build",
                false,
                &crate::gate::user_rejected("run_terminal_command"),
            ),
            shell("echo `date`", true, "exit 0\nnow"),
        ];
        let text = user_text(&close_request(
            "judge",
            "chat-1",
            &open_list(),
            &[],
            "Done.",
            &ran,
            &[],
        ));
        assert!(
            text.contains(
                "Commands, newest last:\n\
                 - `curl -s https://example.com/health` → exit 0\n\
                 - `rm -rf build` → not run\n\
                 - `echo 'date'` → exit 0\n\
                 Last test run: (none)\n"
            ),
            "{text}"
        );
        // Secrets never reach the judge.
        let ran = [shell(
            r#"curl -H "Authorization: Bearer sk-abcdefghijklmnopqrstuv" x"#,
            true,
            "exit 0",
        )];
        let text = user_text(&close_request(
            "judge",
            "chat-1",
            &open_list(),
            &[],
            "Done.",
            &ran,
            &[],
        ));
        assert!(!text.contains("sk-abcdefghijklmnopqrstuv"), "{text}");
        assert!(text.contains("- `curl -H "), "{text}");
        // Long runs: the newest 8 commands and the first 20 files by name.
        let ran: Vec<Ran> = (1..=9)
            .map(|n| shell(&format!("echo {n}"), true, "exit 0"))
            .collect();
        let written: Vec<String> = (1..=22).map(|n| format!("f{n}.txt")).collect();
        let text = user_text(&close_request(
            "judge",
            "chat-1",
            &open_list(),
            &[],
            "Done.",
            &ran,
            &written,
        ));
        let files: Vec<String> = (1..=20).map(|n| format!("f{n}.txt")).collect();
        assert!(
            text.contains(&format!("Files written: {} and 2 more\n", files.join(", "))),
            "{text}"
        );
        assert!(!text.contains("`echo 1`"), "{text}");
        assert!(
            text.contains("- `echo 2` → exit 0\n")
                && text.contains("- `echo 9` → exit 0\nLast test run: (none)"),
            "{text}"
        );
    }
}
