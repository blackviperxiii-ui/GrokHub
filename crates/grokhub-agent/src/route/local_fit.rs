//! Fit a chat request into the on-device model's context window.
//!
//! Grok gets the whole prompt: system, every offered tool and the full
//! history. A local model often runs with a 4k to 16k window, so the same
//! request would not fit. [`fit`] shrinks only what it must, in this order,
//! and stops as soon as the request fits with room left for the reply:
//!
//! 1. Tools: keep the core workspace and web tools.
//! 2. System: drop the optional guidance sections. Safety, memory, the cabin
//!    rules, the personality and AGENTS.md stay.
//! 3. Older user turns lose the per-turn cabin notes (the newest keeps them).
//! 4. AGENTS.md: at most a quarter of the window, cut at a line, saying so.
//! 5. History: keep the newest whole turns that fit, and say earlier turns
//!    were left out. If even the newest turn is too big, AGENTS.md gives way.
//!
//! The saved chat is never changed; only the request sent to the device is.

use crate::client::{ContentPart, InputItem, ResponsesRequest};
use crate::compact::estimate_input_tokens;

/// Tools an on-device turn keeps when the full list does not fit.
pub const CORE_TOOLS: &[&str] = &[
    "read_file",
    "list_dir",
    "grep",
    "glob",
    "write",
    "search_replace",
    "run_terminal_command",
    "web_search",
    "web_fetch",
    "get_command_or_subagent_output",
];

/// System sections that are guidance, not safety, dropped in this order.
const OPTIONAL_SECTIONS: &[&str] = &[
    "formatting",
    "tools",
    "skills",
    "work_policy",
    "turn_endings",
    "communication",
];

/// Said in the system message when older turns were left out.
pub const TRIMMED_NOTE: &str = "Earlier turns of this chat were left out to fit the on-device \
model. If you need something from them, ask.";

/// Room kept for the model's reply: a quarter of the window, 512 to 4096 tokens.
pub fn reply_reserve(n_ctx: u64) -> u64 {
    (n_ctx / 4).clamp(512, 4096)
}

/// Estimated prompt tokens for a request: messages plus tool schemas.
pub fn request_tokens(req: &ResponsesRequest) -> u64 {
    let tools: u64 = req
        .tools
        .iter()
        .map(|t| serde_json::to_vec(t).map(|b| b.len() as u64).unwrap_or(0))
        .sum();
    estimate_input_tokens(&req.input).saturating_add(tools.div_ceil(4))
}

/// What [`fit`] changed, for the route log and tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Fitted {
    pub tools_cut: bool,
    pub system_cut: bool,
    pub notes_cut: bool,
    pub agents_cut: bool,
    pub turns_dropped: usize,
    /// The newest message alone was longer than the window: its middle went.
    pub newest_clipped: bool,
}

/// Shrink `req` until it fits `n_ctx` with [`reply_reserve`] left over.
pub fn fit(req: &ResponsesRequest, n_ctx: u64) -> (ResponsesRequest, Fitted) {
    let budget = n_ctx.saturating_sub(reply_reserve(n_ctx));
    let mut out = req.clone();
    let mut done = Fitted::default();
    if request_tokens(&out) <= budget {
        return (out, done);
    }
    let before = out.tools.len();
    out.tools.retain(|t| {
        t["name"]
            .as_str()
            .is_some_and(|name| CORE_TOOLS.contains(&name))
    });
    done.tools_cut = out.tools.len() < before;
    if request_tokens(&out) <= budget {
        return (out, done);
    }
    for section in OPTIONAL_SECTIONS {
        if strip_system_section(&mut out.input, section) {
            done.system_cut = true;
            if request_tokens(&out) <= budget {
                return (out, done);
            }
        }
    }
    done.notes_cut = strip_older_notes(&mut out.input);
    if request_tokens(&out) <= budget {
        return (out, done);
    }
    // AGENTS.md can be many thousand tokens: give it at most a quarter first.
    done.agents_cut = cap_agents(&mut out.input, budget / 4);
    if request_tokens(&out) <= budget {
        return (out, done);
    }
    done.turns_dropped = keep_recent_turns(&mut out, budget);
    let over = request_tokens(&out).saturating_sub(budget);
    if over > 0 {
        let agents = agents_tokens(&out.input);
        done.agents_cut |= cap_agents(&mut out.input, agents.saturating_sub(over));
    }
    // One message alone longer than the window (a pasted log): keep its start
    // and end and say what was left out, instead of the device refusing it.
    let over = request_tokens(&out).saturating_sub(budget);
    if over > 0 {
        done.newest_clipped = clip_newest_user(&mut out.input, over);
    }
    (out, done)
}

/// Stands in for the middle [`clip_newest_user`] cut.
pub const CLIPPED_NOTE: &str =
    "characters of this message were left out in the middle to fit the on-device model";

/// Cut about `over_tokens` from the middle of the newest user message.
fn clip_newest_user(input: &mut [InputItem], over_tokens: u64) -> bool {
    let Some(text) = input.iter_mut().rev().find_map(|item| match item {
        InputItem::Message { role, content } if role == "user" => {
            content.iter_mut().find_map(|c| match c {
                ContentPart::InputText(t) => Some(t),
                _ => None,
            })
        }
        _ => None,
    }) else {
        return false;
    };
    // About 4 bytes a token, with room for the note.
    let cut = (over_tokens.saturating_add(64) as usize).saturating_mul(4);
    let len = text.len();
    let keep = len.saturating_sub(cut).max(len.min(512));
    let mut head = keep / 2;
    while !text.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = len - (keep - keep / 2);
    while !text.is_char_boundary(tail) {
        tail += 1;
    }
    if tail <= head {
        return false;
    }
    let left_out = text[head..tail].chars().count();
    *text = format!(
        "{}\n\n[… {left_out} {CLIPPED_NOTE} …]\n\n{}",
        &text[..head],
        &text[tail..]
    );
    true
}

const AGENTS_OPEN: &str = "<agents>\n";
const AGENTS_CLOSE: &str = "\n</agents>";
const AGENTS_CUT: &str = "\n[The rest of AGENTS.md was left out to fit the on-device model.]";

fn agents_span(text: &str) -> Option<(usize, usize)> {
    let start = text.find(AGENTS_OPEN)? + AGENTS_OPEN.len();
    let len = text[start..].find(AGENTS_CLOSE)?;
    Some((start, start + len))
}

fn agents_tokens(input: &[InputItem]) -> u64 {
    match input.first() {
        Some(InputItem::Message { role, content }) if role == "system" => match content.first() {
            Some(ContentPart::InputText(text)) => {
                agents_span(text).map_or(0, |(a, b)| ((b - a) as u64).div_ceil(4))
            }
            _ => 0,
        },
        _ => 0,
    }
}

/// Cut the system's `<agents>` block to about `max_tokens`, at a line break,
/// saying the rest was left out. True when it cut anything.
fn cap_agents(input: &mut [InputItem], max_tokens: u64) -> bool {
    let Some(InputItem::Message { role, content }) = input.first_mut() else {
        return false;
    };
    if role != "system" {
        return false;
    }
    let Some(ContentPart::InputText(text)) = content.first_mut() else {
        return false;
    };
    let Some((start, end)) = agents_span(text) else {
        return false;
    };
    let max_bytes = (max_tokens as usize).saturating_mul(4);
    if end - start <= max_bytes {
        return false;
    }
    let body = &text[start..end];
    let mut keep = max_bytes.saturating_sub(AGENTS_CUT.len()).min(body.len());
    while !body.is_char_boundary(keep) {
        keep -= 1;
    }
    let keep = body[..keep].rfind('\n').unwrap_or(0);
    let cut = format!("{}{AGENTS_CUT}", &body[..keep]);
    text.replace_range(start..end, cut.trim_start_matches('\n'));
    true
}

/// Remove `<tag>…</tag>` from the leading system messages. True when found.
fn strip_system_section(input: &mut [InputItem], tag: &str) -> bool {
    let open = format!("<{tag}>\n");
    let close = format!("</{tag}>");
    let mut found = false;
    for item in input.iter_mut() {
        let InputItem::Message { role, content } = item else {
            break;
        };
        if role != "system" {
            break;
        }
        for part in content.iter_mut() {
            let ContentPart::InputText(text) = part else {
                continue;
            };
            let Some(start) = text.find(&open) else {
                continue;
            };
            let Some(len) = text[start..].find(&close) else {
                continue;
            };
            let end = start + len + close.len();
            let head = text[..start].trim_end();
            let tail = text[end..].trim_start();
            *text = match (head.is_empty(), tail.is_empty()) {
                (true, _) => tail.to_string(),
                (_, true) => head.to_string(),
                _ => format!("{head}\n\n{tail}"),
            };
            found = true;
        }
    }
    found
}

/// Older user turns drop their `<cabin-volatile>` notes; the newest keeps its own.
fn strip_older_notes(input: &mut [InputItem]) -> bool {
    let Some(last) = input
        .iter()
        .rposition(|item| matches!(item, InputItem::Message { role, .. } if role == "user"))
    else {
        return false;
    };
    let mut changed = false;
    for item in &mut input[..last] {
        let InputItem::Message { role, content } = item else {
            continue;
        };
        if role != "user" {
            continue;
        }
        for part in content.iter_mut() {
            if let ContentPart::InputText(text) = part {
                let visible = without_notes(text);
                if visible.len() != text.len() {
                    *text = visible.to_string();
                    changed = true;
                }
            }
        }
    }
    changed
}

/// The typed words of a stored user turn, without its `<cabin-volatile>` block.
fn without_notes(text: &str) -> &str {
    use crate::prefix_cache::{WRAPPER_END, WRAPPER_START};
    text.strip_prefix(WRAPPER_START)
        .and_then(|rest| rest.split_once(WRAPPER_END))
        .map_or(text, |(_, user)| user.strip_prefix("\n\n").unwrap_or(user))
}

/// Keep the system messages and the newest whole turns that fit `budget`.
/// A turn starts at a user message, so a tool result never loses its call.
/// The newest turn always stays. Returns how many messages were left out.
fn keep_recent_turns(req: &mut ResponsesRequest, budget: u64) -> usize {
    let lead = req
        .input
        .iter()
        .take_while(|item| matches!(item, InputItem::Message { role, .. } if role == "system"))
        .count();
    let starts: Vec<usize> = (lead..req.input.len())
        .filter(|&i| matches!(&req.input[i], InputItem::Message { role, .. } if role == "user"))
        .collect();
    let Some(&newest) = starts.last() else {
        return 0;
    };
    let mut head = req.input[..lead].to_vec();
    note_trimmed(&mut head);
    let fixed = request_tokens(&ResponsesRequest {
        input: head.clone(),
        ..req.clone()
    });
    let room = budget.saturating_sub(fixed);
    let cut = starts
        .iter()
        .copied()
        .find(|&start| estimate_input_tokens(&req.input[start..]) <= room)
        .unwrap_or(newest);
    if cut == lead {
        return 0;
    }
    let dropped = cut - lead;
    head.extend(req.input[cut..].iter().cloned());
    req.input = head;
    dropped
}

fn note_trimmed(head: &mut Vec<InputItem>) {
    if let Some(InputItem::Message { content, .. }) = head.last_mut() {
        if let Some(ContentPart::InputText(text)) = content.first_mut() {
            text.push_str("\n\n");
            text.push_str(TRIMMED_NOTE);
            return;
        }
    }
    head.push(InputItem::Message {
        role: "system".into(),
        content: vec![ContentPart::InputText(TRIMMED_NOTE.into())],
    });
}

#[cfg(test)]
#[path = "local_fit_tests.rs"]
mod tests;
