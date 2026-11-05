//! Round 3 item 32: what survives a compaction is kept in code, not left to
//! the model's summary.
//!
//! - The ledger: files written, commands with how they ended, open todos. The
//!   harness builds it from the tool calls themselves and from the ledger the
//!   last compaction left, so it carries across any number of compactions and
//!   turns. It goes after the pinned rules on both paths (xAI's server
//!   compaction and the summary), so xAI, OpenRouter and the local model all
//!   get it.
//! - The summary's sections are merged a piece at a time: the sections that
//!   only grow (the asks, errors and fixes, the person's messages) keep any
//!   earlier line the new summary dropped; the sections about the current state
//!   take the new text.

use super::{message_text, open_todos, path_from_write_call, OpenTodo};
use crate::client::{ContentPart, InputItem};

pub(super) const LEDGER_OPEN: &str = "<harness_ledger>";
const LEDGER_CLOSE: &str = "</harness_ledger>";
const LEDGER_HEAD: &str =
    "Kept by the app from the tool calls themselves, not written by the model.";
const FILES_HEAD: &str = "Files written:";
const COMMANDS_HEAD: &str = "Commands run, newest last:";
const TODOS_HEAD: &str = "Open todos:";
/// Files listed; the most recently written win.
pub(super) const LEDGER_FILES: usize = 40;
/// Commands listed; the newest win.
pub(super) const LEDGER_COMMANDS: usize = 20;
/// Characters kept from one command line.
const COMMAND_CHARS: usize = 200;

#[derive(Debug, Default, PartialEq, Eq)]
struct Ledger {
    files: Vec<String>,
    commands: Vec<String>,
    todos: Vec<String>,
}

/// One line, no tags that could close the ledger early.
fn one_line(text: &str, max: usize) -> String {
    let flat = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(LEDGER_CLOSE, "")
        .replace(LEDGER_OPEN, "");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut out: String = flat.chars().take(max).collect();
    out.push('…');
    out
}

/// The ledger the last compaction left in `items`, if any.
fn prior_ledger(items: &[InputItem]) -> Ledger {
    let Some(text) = items.iter().rev().find_map(|item| {
        let text = message_text(item);
        text.trim_start().starts_with(LEDGER_OPEN).then_some(text)
    }) else {
        return Ledger::default();
    };
    let mut ledger = Ledger::default();
    let mut list: Option<&mut Vec<String>> = None;
    for line in text.lines() {
        let line = line.trim();
        match line {
            FILES_HEAD => list = Some(&mut ledger.files),
            COMMANDS_HEAD => list = Some(&mut ledger.commands),
            TODOS_HEAD => list = Some(&mut ledger.todos),
            _ => {
                if let (Some(rest), Some(list)) = (line.strip_prefix("- "), list.as_deref_mut()) {
                    list.push(rest.to_string());
                }
            }
        }
    }
    ledger
}

/// How a shell call ended, read from the first lines of its result.
fn shell_status(output: Option<&str>, background: bool) -> String {
    if background {
        return "started in the background".into();
    }
    let Some(output) = output else {
        return "no result".into();
    };
    for line in output.lines().take(3) {
        let line = line.trim();
        if let Some(code) = line.strip_prefix("exit ") {
            if code.parse::<i64>().is_ok() {
                return format!("exit {code}");
            }
        }
        if line.starts_with("timed out after") || line == "killed" {
            return line.to_string();
        }
    }
    "result not kept".into()
}

fn is_background(arguments: &serde_json::Value) -> bool {
    arguments
        .get("is_background")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn push_newest(list: &mut Vec<String>, line: String, cap: usize) {
    list.retain(|l| l != &line);
    list.push(line);
    if list.len() > cap {
        let extra = list.len() - cap;
        list.drain(..extra);
    }
}

fn todo_line(todo: &OpenTodo) -> String {
    one_line(
        &format!("[{}] {}: {}", todo.status, todo.id, todo.content),
        COMMAND_CHARS,
    )
}

fn build(conversation_id: &str, folded: &[InputItem], modified_paths: &[String]) -> Ledger {
    let mut ledger = prior_ledger(folded);
    let outputs: std::collections::HashMap<&str, &str> = folded
        .iter()
        .filter_map(|item| match item {
            InputItem::FunctionCallOutput { call_id, output } => {
                Some((call_id.as_str(), output.as_str()))
            }
            _ => None,
        })
        .collect();
    for item in folded {
        let InputItem::FunctionCall {
            call_id,
            name,
            arguments,
        } = item
        else {
            continue;
        };
        if let Some(path) = path_from_write_call(name, arguments) {
            push_newest(
                &mut ledger.files,
                one_line(&path, COMMAND_CHARS),
                LEDGER_FILES,
            );
            continue;
        }
        if name != "run_terminal_command" {
            continue;
        }
        let args: serde_json::Value = serde_json::from_str(arguments).unwrap_or_default();
        let command = args
            .get("command")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim();
        if command.is_empty() {
            continue;
        }
        let status = shell_status(outputs.get(call_id.as_str()).copied(), is_background(&args));
        // A rerun replaces the older line, so the ledger shows how it ended last.
        let shown = format!("`{}`", one_line(command, COMMAND_CHARS).replace('`', "'"));
        ledger
            .commands
            .retain(|l| !l.starts_with(&format!("{shown} →")));
        push_newest(
            &mut ledger.commands,
            format!("{shown} → {status}"),
            LEDGER_COMMANDS,
        );
    }
    for path in modified_paths {
        let path = path.trim();
        if !path.is_empty() {
            push_newest(
                &mut ledger.files,
                one_line(path, COMMAND_CHARS),
                LEDGER_FILES,
            );
        }
    }
    // The session's own list is the truth. A helper has none, so its todo
    // calls stand in, and with none of those the last ledger's list stays.
    let session: Vec<OpenTodo> = crate::session_tools::todos_for(conversation_id)
        .into_iter()
        .map(|t| OpenTodo {
            id: t.id,
            content: t.content,
            status: t.status,
        })
        .collect();
    let has_todo_calls = folded
        .iter()
        .any(|item| matches!(item, InputItem::FunctionCall { name, .. } if name == "todo_write"));
    if !session.is_empty() {
        ledger.todos = session
            .iter()
            .filter(|t| {
                let s = t.status.to_ascii_lowercase();
                s == "pending" || s == "in_progress"
            })
            .map(todo_line)
            .collect();
    } else if has_todo_calls {
        ledger.todos = open_todos(folded).iter().map(todo_line).collect();
    }
    ledger
}

/// The ledger message, or `None` when there is nothing to keep.
pub(super) fn ledger_message(
    conversation_id: &str,
    folded: &[InputItem],
    modified_paths: &[String],
) -> Option<InputItem> {
    let ledger = build(conversation_id, folded, modified_paths);
    if ledger == Ledger::default() {
        return None;
    }
    let mut body = format!("{LEDGER_OPEN}\n{LEDGER_HEAD}\n");
    for (head, list) in [
        (FILES_HEAD, &ledger.files),
        (COMMANDS_HEAD, &ledger.commands),
        (TODOS_HEAD, &ledger.todos),
    ] {
        if list.is_empty() {
            continue;
        }
        body.push('\n');
        body.push_str(head);
        body.push('\n');
        for line in list {
            body.push_str("- ");
            body.push_str(line);
            body.push('\n');
        }
    }
    body.push_str(LEDGER_CLOSE);
    Some(InputItem::Message {
        role: "user".into(),
        content: vec![ContentPart::InputText(body)],
    })
}

/// Sections that only grow: an earlier line the new summary dropped is kept.
const CUMULATIVE: [&str; 3] = [
    "primary request and intent",
    "errors and fixes",
    "all user messages",
];
/// Characters of carried lines kept per section; the newest win.
pub(super) const CARRY_CHARS: usize = 3_000;
const CARRIED_HEAD: &str = "Carried from earlier summaries:";

/// The section titles the summary prompt asks for, lowercased.
const TITLES: [&str; 9] = [
    "primary request and intent",
    "key technical concepts",
    "files and code sections",
    "errors and fixes",
    "problem solving",
    "all user messages",
    "pending tasks",
    "current work",
    "work in progress",
];

/// The title when `line` opens one of the prompt's numbered sections, as in
/// `1. Primary Request and Intent: …` (headings and bold allowed). A numbered
/// line inside a section ("1. Wire auth: …") is not a header.
fn section_title(line: &str) -> Option<String> {
    let bare = line.trim().trim_start_matches(['#', '*', ' ']);
    let mut chars = bare.chars();
    let digit = chars.next()?;
    if !digit.is_ascii_digit() {
        return None;
    }
    let rest = chars.as_str().strip_prefix('.')?.trim_start();
    let title = rest.split(':').next()?.trim().trim_end_matches('*').trim();
    let title = title.to_ascii_lowercase();
    (rest.contains(':') && TITLES.contains(&title.as_str())).then_some(title)
}

/// Split into the text before the first section, then each section's title
/// and lines (header line first).
fn sections(text: &str) -> (Vec<String>, Vec<(String, Vec<String>)>) {
    let mut head = Vec::new();
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for line in text.lines() {
        if let Some(title) = section_title(line) {
            out.push((title, vec![line.to_string()]));
        } else if let Some((_, lines)) = out.last_mut() {
            lines.push(line.to_string());
        } else {
            head.push(line.to_string());
        }
    }
    (head, out)
}

fn norm(text: &str) -> String {
    text.trim()
        .trim_start_matches(['-', '*', '•', ' '])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The body of a section: what follows the header's colon, then its lines.
fn body_lines(lines: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    if let Some((_, after)) = lines[0].split_once(':') {
        let after = after.trim().trim_start_matches('*').trim();
        if !after.is_empty() {
            out.push(after.to_string());
        }
    }
    out.extend(lines[1..].iter().cloned());
    out
}

/// Merge the new summary with the prior one, a section at a time. With no
/// prior summary, or one with no numbered sections, the new text is kept.
pub(super) fn merge_summaries(prior: Option<&str>, new: &str) -> String {
    let Some(prior) = prior else {
        return new.to_string();
    };
    let (_, old_sections) = sections(prior);
    let (head, mut new_sections) = sections(new);
    if old_sections.is_empty() || new_sections.is_empty() {
        return new.to_string();
    }
    for (title, lines) in &mut new_sections {
        if !CUMULATIVE.iter().any(|c| title.starts_with(c)) {
            continue;
        }
        let Some((_, old_lines)) = old_sections.iter().find(|(t, _)| t == title) else {
            continue;
        };
        let new_text = norm(&lines.join("\n"));
        let mut carried: Vec<String> = Vec::new();
        for line in body_lines(old_lines) {
            let n = norm(&line);
            if n.is_empty()
                || n == "none"
                || n == norm(CARRIED_HEAD)
                || new_text.contains(&n)
                || carried.iter().any(|c| norm(c) == n)
            {
                continue;
            }
            carried.push(line.trim_end().to_string());
        }
        // The newest carried lines win the budget.
        let mut used = 0usize;
        let mut keep_from = carried.len();
        for (i, line) in carried.iter().enumerate().rev() {
            let cost = line.chars().count() + 1;
            if used + cost > CARRY_CHARS {
                break;
            }
            used += cost;
            keep_from = i;
        }
        let carried = &carried[keep_from..];
        if carried.is_empty() {
            continue;
        }
        let only_none = body_lines(lines).iter().all(|l| {
            let n = norm(l);
            n.is_empty() || n == "none"
        });
        if only_none {
            // "None" from the new summary, but earlier summaries had lines.
            let header = lines[0]
                .split_once(':')
                .map(|(h, _)| format!("{h}:"))
                .unwrap_or_else(|| lines[0].clone());
            *lines = vec![header];
        }
        lines.push(CARRIED_HEAD.to_string());
        lines.extend(carried.iter().cloned());
    }
    let mut out = head;
    for (_, lines) in new_sections {
        out.extend(lines);
    }
    out.join("\n")
}

#[cfg(test)]
#[path = "compact_ledger_tests.rs"]
mod tests;
