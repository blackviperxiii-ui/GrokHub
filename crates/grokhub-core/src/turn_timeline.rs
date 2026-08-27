//! Live turn order: freeze finished sentences above tools, continue below.

use crate::chat_view::{ChatKind, ChatView};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveKind {
    Thought,
    Say,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveBlock {
    pub kind: LiveKind,
    pub body: String,
    pub tool_id: String,
    pub tool_title: String,
    pub tool_status: String,
    pub tool_detail: String,
}

fn tool_block(id: &str, title: &str, status: &str, detail: &str) -> LiveBlock {
    LiveBlock {
        kind: LiveKind::Tool,
        body: String::new(),
        tool_id: id.to_string(),
        tool_title: title.to_string(),
        tool_status: status.to_string(),
        tool_detail: detail.to_string(),
    }
}

fn text_block(kind: LiveKind, body: String) -> LiveBlock {
    LiveBlock {
        kind,
        body,
        tool_id: String::new(),
        tool_title: String::new(),
        tool_status: String::new(),
        tool_detail: String::new(),
    }
}

/// Split on the last `.` `!` `?` that ends a sentence. Prefix is done; rest continues after a tool.
pub fn split_at_last_sentence(s: &str) -> (String, String) {
    let t = s.trim_end();
    if t.is_empty() {
        return (String::new(), String::new());
    }
    let mut last_end = None;
    for (i, ch) in t.char_indices() {
        if matches!(ch, '.' | '!' | '?') {
            let end = i + ch.len_utf8();
            let rest = &t[end..];
            if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                last_end = Some(end);
            }
        }
    }
    match last_end {
        Some(end) => (
            t[..end].trim_end().to_string(),
            t[end..].trim_start().to_string(),
        ),
        None => (String::new(), t.to_string()),
    }
}

fn append_text(blocks: &mut Vec<LiveBlock>, kind: LiveKind, delta: &str) {
    if delta.is_empty() {
        return;
    }
    if let Some(last) = blocks.last_mut() {
        if last.kind == kind {
            last.body.push_str(delta);
            return;
        }
    }
    blocks.push(text_block(kind, delta.to_string()));
}

pub fn append_thought(blocks: &mut Vec<LiveBlock>, delta: &str) {
    append_text(blocks, LiveKind::Thought, delta);
}

pub fn append_say(blocks: &mut Vec<LiveBlock>, delta: &str) {
    append_text(blocks, LiveKind::Say, delta);
}

pub fn append_tool(blocks: &mut Vec<LiveBlock>, id: &str, title: &str, status: &str, detail: &str) {
    if !id.is_empty() {
        if let Some(old) = blocks.iter_mut().rev().find(|b| b.kind == LiveKind::Tool && b.tool_id == id)
        {
            if !title.is_empty() {
                old.tool_title = title.to_string();
            }
            if !status.is_empty() {
                old.tool_status = status.to_string();
            }
            if !detail.is_empty() {
                old.tool_detail = detail.to_string();
            }
            return;
        }
    }
    let remainder = if let Some(last) = blocks.last() {
        if last.kind == LiveKind::Thought || last.kind == LiveKind::Say {
            let (done, rest) = split_at_last_sentence(&last.body);
            if !done.is_empty() && !rest.is_empty() {
                Some((last.kind, done, rest))
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };
    if let Some((kind, done, rest)) = remainder {
        if let Some(last) = blocks.last_mut() {
            last.body = done;
        }
        blocks.push(tool_block(id, title, status, detail));
        blocks.push(text_block(kind, rest));
        return;
    }
    blocks.push(tool_block(id, title, status, detail));
}

/// History through the last user turn. Live thought/tools/reply paint after that.
pub fn views_up_to_last_user(views: &[ChatView]) -> &[ChatView] {
    match views.iter().rposition(|v| v.kind == ChatKind::User) {
        Some(i) => &views[..=i],
        None => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(b: &[LiveBlock]) -> Vec<LiveKind> {
        b.iter().map(|x| x.kind).collect()
    }

    #[test]
    fn complete_thought_stays_above_the_tool() {
        let mut b = Vec::new();
        append_thought(
            &mut b,
            "I'll start by checking which desktop environment you already have.",
        );
        append_tool(&mut b, "t1", "run_terminal_command", "pending", "");
        append_thought(&mut b, "Now I'll wire window restore.");
        assert_eq!(
            kinds(&b),
            vec![LiveKind::Thought, LiveKind::Tool, LiveKind::Thought]
        );
        assert!(b[0].body.contains("desktop environment"));
        assert_eq!(b[1].tool_title, "run_terminal_command");
        assert_eq!(b[2].body, "Now I'll wire window restore.");
    }

    #[test]
    fn unfinished_sentence_continues_below_the_tool() {
        let mut b = Vec::new();
        append_thought(&mut b, "First I'll look around. Then I still need to");
        append_tool(&mut b, "t1", "run_terminal_command", "pending", "");
        assert_eq!(
            kinds(&b),
            vec![LiveKind::Thought, LiveKind::Tool, LiveKind::Thought]
        );
        assert_eq!(b[0].body, "First I'll look around.");
        assert_eq!(b[2].body, "Then I still need to");
        append_thought(&mut b, " finish the thought.");
        assert_eq!(b[2].body, "Then I still need to finish the thought.");
    }

    #[test]
    fn tool_status_updates_in_place() {
        let mut b = Vec::new();
        append_tool(&mut b, "t1", "run_terminal_command", "pending", "");
        append_tool(&mut b, "t1", "run_terminal_command", "failed", "cancelled");
        assert_eq!(kinds(&b), vec![LiveKind::Tool]);
        assert_eq!(b[0].tool_status, "failed");
        assert_eq!(b[0].tool_detail, "cancelled");
    }

    #[test]
    fn final_say_sits_after_tools() {
        let mut b = Vec::new();
        append_thought(&mut b, "Checking the session path.");
        append_tool(&mut b, "t1", "run_terminal_command", "completed", "");
        append_say(&mut b, "Restore is on. Log out once to apply it.");
        assert_eq!(
            kinds(&b),
            vec![LiveKind::Thought, LiveKind::Tool, LiveKind::Say]
        );
        assert_eq!(b.last().unwrap().kind, LiveKind::Say);
    }

    #[test]
    fn history_stops_at_the_last_user_bubble() {
        let views = vec![
            ChatView {
                kind: ChatKind::User,
                title: String::new(),
                body: "hi".into(),
            },
            ChatView {
                kind: ChatKind::Thought,
                title: "Thought".into(),
                body: "old".into(),
            },
            ChatView {
                kind: ChatKind::User,
                title: String::new(),
                body: "again".into(),
            },
            ChatView {
                kind: ChatKind::Thought,
                title: "Thought".into(),
                body: "live".into(),
            },
        ];
        let kept = views_up_to_last_user(&views);
        assert_eq!(kept.len(), 3);
        assert_eq!(kept[2].body, "again");
    }
}
