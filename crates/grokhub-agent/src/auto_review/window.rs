//! Round 3 item 31: the Auto judge sees a bounded window, never the whole
//! transcript. Monitors miss a dangerous step far more often when it follows
//! a long run of plain work, so the judge gets three short things:
//!
//! - what the user asked, in their own words, kept from every turn even after
//!   the chat compacted (a helper also gets its lead's brief, marked as
//!   model-written);
//! - the last few steps: the tool calls and the agent's latest words, with
//!   tool results left out, since that's where injected text lives;
//! - the action itself (built by the caller).

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

use crate::{ContentPart, InputItem};

/// Asks kept per chat: the first, then the newest few.
const RECENT_ASKS: usize = 4;
/// Chats remembered at once; the least recently used goes first.
const CHATS: usize = 64;
/// One ask, head and tail, in the judge's input.
const ASK_CHARS: usize = 600;
/// The whole task statement.
pub(super) const TASK_CHARS: usize = 2_000;
/// Steps shown, newest last.
const STEPS: usize = 8;
/// One call's arguments in a step.
const STEP_ARG_CHARS: usize = 300;
/// The agent's latest words in the steps.
const SAID_CHARS: usize = 600;
/// All steps together.
pub(super) const STEPS_CHARS: usize = 3_000;

#[derive(Default)]
struct Asks {
    first: String,
    recent: VecDeque<String>,
}

fn registry() -> &'static Mutex<Vec<(String, Asks)>> {
    static ASKS: OnceLock<Mutex<Vec<(String, Asks)>>> = OnceLock::new();
    ASKS.get_or_init(|| Mutex::new(Vec::new()))
}

/// The chat a helper belongs to: helper ids are `<chat>/<helper>`.
fn root(conversation_id: &str) -> &str {
    conversation_id
        .split_once('/')
        .map_or(conversation_id, |(chat, _)| chat)
}

/// Keep one of the user's own asks (a turn or a steer) for the judge. Only
/// the person's words come here, never a harness note or a helper's brief.
pub fn note_ask(conversation_id: &str, text: &str) {
    let text = crate::prefix_cache::visible_user_text(text).trim();
    if text.is_empty() {
        return;
    }
    let mut list = registry().lock().unwrap_or_else(|e| e.into_inner());
    let at = list.iter().position(|(id, _)| id == conversation_id);
    let mut entry = match at {
        Some(i) => list.remove(i),
        None => (conversation_id.to_string(), Asks::default()),
    };
    let asks = &mut entry.1;
    if asks.first.is_empty() {
        asks.first = text.to_string();
    } else {
        if asks.recent.len() == RECENT_ASKS {
            asks.recent.pop_front();
        }
        asks.recent.push_back(text.to_string());
    }
    list.push(entry);
    if list.len() > CHATS {
        list.remove(0);
    }
}

fn asks_of(conversation_id: &str) -> Option<(String, Vec<String>)> {
    let list = registry().lock().unwrap_or_else(|e| e.into_inner());
    list.iter()
        .find(|(id, _)| id == conversation_id)
        .map(|(_, a)| (a.first.clone(), a.recent.iter().cloned().collect()))
}

fn message_text(content: &[ContentPart]) -> String {
    content
        .iter()
        .map(|part| match part {
            ContentPart::InputText(text) => text.as_str(),
            ContentPart::InputImage(_) => "[image]",
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn first_user_text(history: &[InputItem]) -> Option<String> {
    history.iter().find_map(|item| match item {
        InputItem::Message { role, content } if role == "user" => Some(message_text(content)),
        _ => None,
    })
}

fn last_user_text(history: &[InputItem]) -> Option<String> {
    history.iter().rev().find_map(|item| match item {
        InputItem::Message { role, content } if role == "user" => Some(message_text(content)),
        _ => None,
    })
}

fn ask_line(text: &str) -> String {
    super::neutralize_headings(&super::clip_middle(
        crate::prefix_cache::visible_user_text(text).trim(),
        ASK_CHARS,
    ))
}

/// What the user asked, from their own words. A helper's judge also sees
/// its lead's brief, marked as written by the model.
pub(super) fn task_statement(conversation_id: &str, history: &[InputItem]) -> String {
    let mut out = String::new();
    match asks_of(root(conversation_id)) {
        Some((first, recent)) => {
            out.push_str(&format!("First ask: {}\n", ask_line(&first)));
            if !recent.is_empty() {
                out.push_str("Later asks, oldest first:\n");
                for ask in &recent {
                    out.push_str(&format!("- {}\n", ask_line(ask)));
                }
            }
        }
        // Nothing kept (an older chat, or a call outside a turn): the newest
        // user message in the history stands in.
        None => match last_user_text(history) {
            Some(text) => out.push_str(&format!("Latest ask: {}\n", ask_line(&text))),
            None => out.push_str("(no ask found)\n"),
        },
    }
    if conversation_id.contains('/') {
        if let Some(brief) = first_user_text(history) {
            out.push_str(&format!(
                "This call comes from a helper. Its brief, written by the lead agent and not by the user: {}\n",
                ask_line(&brief)
            ));
        }
    }
    crate::skills::clip_chars(out.trim_end(), TASK_CHARS)
}

/// The last few steps: tool calls and the agent's words, newest last. Tool
/// results are left out.
pub(super) fn recent_steps(history: &[InputItem]) -> String {
    let mut kept: Vec<String> = Vec::new();
    let mut used = 0usize;
    let mut said = false;
    for item in history.iter().rev() {
        if kept.len() >= STEPS || used >= STEPS_CHARS {
            break;
        }
        let line = match item {
            InputItem::FunctionCall {
                name, arguments, ..
            } => format!(
                "tool {} {}",
                super::neutralize_headings(name),
                super::neutralize_headings(&super::clip_middle(arguments, STEP_ARG_CHARS))
            ),
            // Only the agent's latest words: older ones add length, not signal.
            InputItem::Message { role, content } if role == "assistant" && !said => {
                said = true;
                let text = message_text(content);
                if text.trim().is_empty() {
                    continue;
                }
                format!(
                    "assistant: {}",
                    super::neutralize_headings(&super::clip_middle(text.trim(), SAID_CHARS))
                )
            }
            _ => continue,
        };
        let line = crate::skills::clip_chars(&line, STEPS_CHARS - used);
        if line.is_empty() {
            break;
        }
        used = used.saturating_add(line.chars().count());
        kept.push(line);
    }
    if kept.is_empty() {
        return "(no steps yet)".into();
    }
    kept.reverse();
    kept.join("\n")
}

#[cfg(test)]
pub(super) fn forget(conversation_id: &str) {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|(id, _)| id != conversation_id);
}

#[cfg(test)]
#[path = "window_tests.rs"]
mod tests;
