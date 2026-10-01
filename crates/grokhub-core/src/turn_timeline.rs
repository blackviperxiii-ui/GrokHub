//! Live turn order: freeze finished sentences above tools, continue below.

use std::sync::atomic::{AtomicU64, Ordering};

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
    /// Stable while this block lives. Hide / Minimize stay on it as the body grows.
    /// `0` is unused (tool rows). The stored transcript keys the same fold with `thought_body_key`.
    pub fold_slot: u64,
}

fn next_fold_slot() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn tool_block(id: &str, title: &str, status: &str, detail: &str) -> LiveBlock {
    LiveBlock {
        kind: LiveKind::Tool,
        body: String::new(),
        tool_id: id.to_string(),
        tool_title: title.to_string(),
        tool_status: status.to_string(),
        tool_detail: detail.to_string(),
        fold_slot: 0,
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
        fold_slot: next_fold_slot(),
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
    // A seam that opened this block belongs to the previous one.
    let delta = delta.trim_start_matches(['\n', '\r']);
    if delta.is_empty() {
        return;
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
            // Grok's `tool_call_update` carries no title, which parses as the
            // placeholder `Tool`. It must not replace the name from the call.
            if !title.trim().is_empty() && !title.trim().eq_ignore_ascii_case("tool") {
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

/// A grok work hop: `HOST_CMD`, `COMPUTER_CMD`, `CONNECTOR_CMD`, `IMAGINE:`,
/// or `IMAGINE_PROMPT:`. The stream buffer is parsed for these lines, including
/// heredocs, so a sentence seam must not rewrite it.
pub(crate) fn hop_is_work(text: &str) -> bool {
    text.lines().any(|line| {
        let t = line.trim();
        t.starts_with("HOST_CMD")
            || t.starts_with("COMPUTER_CMD")
            || t.starts_with("CONNECTOR_CMD")
            || t.starts_with("IMAGINE:")
            || t.starts_with("IMAGINE_PROMPT:")
    })
}

/// What goes between a stream buffer and its next chunk.
/// Grok sends each message after a tool call, and each thought summary, with no
/// leading space. Joined raw they read `hit.GrokHub`; this keeps them apart.
/// A buffer that already holds a work-protocol line stays exact. The seam after
/// a tool call still opens a new paragraph.
pub fn chunk_seam(prev: &str, next: &str, after_tool: bool) -> &'static str {
    let (Some(last), Some(first)) = (prev.chars().next_back(), next.chars().next()) else {
        return "";
    };
    if last.is_whitespace() || first.is_whitespace() {
        return "";
    }
    if after_tool {
        return "\n\n";
    }
    if matches!(last, '.' | '!' | '?')
        && starts_sentence(next)
        && !in_code(prev)
        && !hop_is_work(prev)
    {
        return "\n\n";
    }
    ""
}

/// `The`, `I'm`, `GrokHub`, a lone `I`. Not `OK` or `React` in `x.React`-style code.
fn starts_sentence(s: &str) -> bool {
    let word: String = s
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '\'' || *c == '\u{2019}')
        .collect();
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_uppercase() {
        return false;
    }
    word == "I" || word.starts_with("I'") || word.starts_with("I\u{2019}") || chars.any(char::is_lowercase)
}

/// Inside an open fence or inline code span on the last line.
fn in_code(s: &str) -> bool {
    if s.matches("```").count() % 2 == 1 {
        return true;
    }
    let line = s.rsplit('\n').next().unwrap_or(s);
    line.matches('`').count() % 2 == 1
}

/// One tool row as the finished transcript keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRow {
    pub status: String,
    pub title: String,
    pub detail: String,
}

/// One part of a finished agent turn, in the order it happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnPart {
    Thought(String),
    Tool(ToolRow),
    Say(String),
}

pub const TURN_THOUGHT: &str = "TURN_THOUGHT:";
pub const TURN_TOOL: &str = "TURN_TOOL:";
pub const TURN_SAY: &str = "TURN_SAY:";

/// Persisted tool rows stay short. ACP titles can be a whole command or heredoc.
const STORED_TOOL_TITLE_CHARS: usize = 160;
const STORED_TOOL_DETAIL_CHARS: usize = 240;

fn is_turn_marker(line: &str) -> bool {
    line.starts_with(TURN_THOUGHT) || line.starts_with(TURN_TOOL) || line.starts_with(TURN_SAY)
}

/// A turn is worth its timeline when it has a thought, a tool, or more than one reply.
/// A plain single reply stays plain text in the transcript.
pub fn turn_needs_timeline(blocks: &[LiveBlock]) -> bool {
    let says = blocks.iter().filter(|b| b.kind == LiveKind::Say).count();
    says > 1 || blocks.iter().any(|b| b.kind != LiveKind::Say)
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn push_turn_body(out: &mut String, body: &str) {
    for line in body.trim().lines() {
        // A body line that looks like a marker is indented so it stays body text.
        if is_turn_marker(line) {
            out.push(' ');
        }
        out.push_str(line);
        out.push('\n');
    }
}

/// Transcript form of a turn: thoughts, tools, and replies stay separate and in order.
/// Marker lines keep a multi-paragraph thought from leaking into the reply.
pub fn encode_turn(blocks: &[LiveBlock]) -> String {
    let mut out = String::new();
    for b in blocks {
        match b.kind {
            LiveKind::Thought | LiveKind::Say => {
                if b.body.trim().is_empty() {
                    continue;
                }
                out.push_str(if b.kind == LiveKind::Thought {
                    TURN_THOUGHT
                } else {
                    TURN_SAY
                });
                out.push('\n');
                push_turn_body(&mut out, &b.body);
            }
            LiveKind::Tool => {
                let title = clip_chars(&one_line(&b.tool_title), STORED_TOOL_TITLE_CHARS);
                let detail = clip_chars(&one_line(&b.tool_detail), STORED_TOOL_DETAIL_CHARS);
                out.push_str(TURN_TOOL);
                out.push(' ');
                out.push_str(&one_line(&b.tool_status));
                out.push('\t');
                out.push_str(if title.is_empty() { "Work" } else { &title });
                out.push('\t');
                out.push_str(&detail);
                out.push('\n');
            }
        }
    }
    out
}

/// `None` when `content` is not a turn timeline (older or plain transcripts).
pub fn decode_turn(content: &str) -> Option<Vec<TurnPart>> {
    let content = content.trim_start();
    if !is_turn_marker(content.lines().next()?) {
        return None;
    }
    let mut parts = Vec::new();
    let mut text: Option<(LiveKind, Vec<&str>)> = None;
    let flush = |parts: &mut Vec<TurnPart>, text: &mut Option<(LiveKind, Vec<&str>)>| {
        if let Some((kind, lines)) = text.take() {
            let body = lines.join("\n").trim().to_string();
            if !body.is_empty() {
                parts.push(if kind == LiveKind::Thought {
                    TurnPart::Thought(body)
                } else {
                    TurnPart::Say(body)
                });
            }
        }
    };
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix(TURN_TOOL) {
            flush(&mut parts, &mut text);
            let mut f = rest.trim_start().splitn(3, '\t');
            let status = f.next().unwrap_or("").trim().to_string();
            let title = f.next().unwrap_or("").trim().to_string();
            let detail = f.next().unwrap_or("").trim().to_string();
            parts.push(TurnPart::Tool(ToolRow {
                status,
                title,
                detail,
            }));
        } else if line.starts_with(TURN_THOUGHT) {
            flush(&mut parts, &mut text);
            text = Some((LiveKind::Thought, Vec::new()));
        } else if line.starts_with(TURN_SAY) {
            flush(&mut parts, &mut text);
            text = Some((LiveKind::Say, Vec::new()));
        } else if let Some((_, lines)) = text.as_mut() {
            let line = match line.strip_prefix(' ') {
                Some(l) if is_turn_marker(l) => l,
                _ => line,
            };
            lines.push(line);
        }
    }
    flush(&mut parts, &mut text);
    Some(parts)
}

/// Every reply in a timeline, joined. `None` for a plain transcript.
pub fn turn_says(content: &str) -> Option<String> {
    let parts = decode_turn(content)?;
    let says: Vec<&str> = parts
        .iter()
        .filter_map(|p| match p {
            TurnPart::Say(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    Some(says.join("\n\n"))
}

/// The newest reply segment of a live turn.
pub fn last_say(blocks: &[LiveBlock]) -> Option<&str> {
    blocks
        .iter()
        .rev()
        .find(|b| b.kind == LiveKind::Say && !b.body.trim().is_empty())
        .map(|b| b.body.as_str())
}

/// `read_file` reads as `Read file`. A title that is already words stays as is.
pub fn tool_display_title(title: &str) -> String {
    let t = title.trim().replace('`', "");
    let t = t.trim();
    if t.is_empty() {
        return "Work".into();
    }
    let ident = t
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    if !ident {
        return t.to_string();
    }
    let words = t.replace(['_', '-'], " ");
    let mut chars = words.trim().chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => "Work".into(),
    }
}

pub fn tool_status_failed(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "failed" | "error" | "errored" | "rejected" | "cancelled" | "canceled"
    )
}

pub fn tool_status_running(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "pending" | "in_progress" | "running" | "started"
    )
}

fn clip_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>())
}

/// One quiet header for a run of tool calls (`title, status, detail`):
/// `Read src/main.rs`, `Grep · 3 matches`, or `4 steps · Grep, Read file, Shell +1 · 1 failed`.
pub fn tool_group_label<'a>(rows: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>) -> String {
    let mut names: Vec<String> = Vec::new();
    let mut n = 0usize;
    let mut failed = 0usize;
    let mut only_detail = "";
    for (title, status, detail) in rows {
        n += 1;
        if tool_status_failed(status) {
            failed += 1;
        }
        let name = tool_display_title(title);
        if !names.contains(&name) {
            names.push(name);
        }
        only_detail = detail;
    }
    let mut label = if n <= 1 {
        let name = names.first().cloned().unwrap_or_else(|| "Work".into());
        // A bare verb says little; its detail says what it touched.
        let detail = one_line(only_detail);
        if !name.contains(' ') && !detail.is_empty() && detail != name {
            format!("{name} · {}", clip_chars(&detail, 60))
        } else {
            name
        }
    } else {
        let shown: Vec<&str> = names.iter().take(3).map(String::as_str).collect();
        let mut s = format!("{n} steps · {}", shown.join(", "));
        if names.len() > 3 {
            s.push_str(&format!(" +{}", names.len() - 3));
        }
        s
    };
    if failed > 0 {
        label.push_str(&format!(" · {failed} failed"));
    }
    label
}

/// Grouped tool rows live in a `ChatKind::Tool` body, one `status\ttitle\tdetail` per line.
pub fn encode_tool_rows(rows: &[ToolRow]) -> String {
    rows.iter()
        .map(|r| format!("{}\t{}\t{}", r.status, r.title, r.detail))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `None` for a single-label tool view such as a Hands step.
pub fn decode_tool_rows(body: &str) -> Option<Vec<ToolRow>> {
    let mut rows = Vec::new();
    for line in body.lines().filter(|l| !l.trim().is_empty()) {
        let mut f = line.splitn(3, '\t');
        let (Some(status), Some(title), Some(detail)) = (f.next(), f.next(), f.next()) else {
            return None;
        };
        rows.push(ToolRow {
            status: status.trim().to_string(),
            title: title.trim().to_string(),
            detail: detail.trim().to_string(),
        });
    }
    if rows.is_empty() {
        None
    } else {
        Some(rows)
    }
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
        // Grok 1.0.46 updates carry no title; the parser's placeholder keeps the name.
        append_tool(&mut b, "t1", "Tool", "completed", "32GB");
        assert_eq!(b[0].tool_title, "run_terminal_command");
        assert_eq!(b[0].tool_status, "completed");
        append_tool(&mut b, "t1", "Read `notes.md`", "", "");
        assert_eq!(
            b[0].tool_title, "Read `notes.md`",
            "a real new title still lands"
        );
    }

    #[test]
    fn title_less_tool_update_keeps_its_name_in_the_saved_turn() {
        let mut b = Vec::new();
        append_tool(&mut b, "t1", "run_terminal_command", "pending", "");
        append_tool(&mut b, "t1", "Tool", "completed", "32GB");
        let parts = decode_turn(&encode_turn(&b)).expect("timeline");
        assert_eq!(parts.len(), 1, "{parts:?}");
        let TurnPart::Tool(row) = &parts[0] else {
            panic!("tool row");
        };
        assert_eq!(row.title, "run_terminal_command");
        assert_eq!(row.status, "completed");
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
    fn chunk_seam_keeps_messages_and_summaries_apart() {
        assert_eq!(chunk_seam("then write up what I hit.", "GrokHub is running.", false), "\n\n");
        assert_eq!(chunk_seam("uses WORK_PIN to track tasks.", "I'm examining", false), "\n\n");
        assert_eq!(chunk_seam("Checking the path", "Next", true), "\n\n");
        assert_eq!(chunk_seam("Checking the ", "path", true), "");
        assert_eq!(chunk_seam("", "Hello", true), "");
        // Ordinary token deltas stay glued.
        assert_eq!(chunk_seam("hel", "lo", false), "");
        assert_eq!(chunk_seam("Done.", " Next", false), "");
        assert_eq!(chunk_seam("It is OK.", "OK", false), "");
        assert_eq!(chunk_seam("v2.", "10", false), "");
        // Code stays exact.
        assert_eq!(chunk_seam("Use `React.", "Component`", false), "");
        assert_eq!(chunk_seam("```rust\nlet x = a.", "Foo;", false), "");
    }

    #[test]
    fn chunk_seam_leaves_work_protocol_intact() {
        assert_eq!(
            chunk_seam("I'll look.\nHOST_CMD: rm -rf /tmp/foo.", "Bar", false),
            ""
        );
        assert_eq!(
            chunk_seam("HOST_CMD: cat <<'EOF'\nrm -rf /tmp/foo.", "Bar", false),
            ""
        );
        assert_eq!(
            chunk_seam("I'll look.\nHOST_CMD: cat <<'EOF'\necho hi.", "There", false),
            ""
        );
        // A tool call still opens a paragraph, even when the buffer holds a command.
        assert_eq!(
            chunk_seam("I'll look.\nHOST_CMD: rm -rf /tmp/foo.", "Bar", true),
            "\n\n"
        );
    }

    #[test]
    fn stored_tool_row_clips_long_multibyte_title_and_detail() {
        let grain = "🔧я";
        let title = grain.repeat(120);
        let detail = grain.repeat(180);
        assert!(title.chars().count() > STORED_TOOL_TITLE_CHARS);
        assert!(detail.chars().count() > STORED_TOOL_DETAIL_CHARS);
        let mut b = Vec::new();
        append_tool(&mut b, "t1", &title, "completed", &detail);
        let parts = decode_turn(&encode_turn(&b)).expect("timeline");
        let TurnPart::Tool(row) = &parts[0] else {
            panic!("tool row");
        };
        assert_eq!(row.title.chars().count(), STORED_TOOL_TITLE_CHARS);
        assert_eq!(row.detail.chars().count(), STORED_TOOL_DETAIL_CHARS);
        assert!(row.title.ends_with('…'));
        assert!(row.detail.ends_with('…'));
        let title_prefix: String = title.chars().take(STORED_TOOL_TITLE_CHARS - 1).collect();
        let detail_prefix: String = detail.chars().take(STORED_TOOL_DETAIL_CHARS - 1).collect();
        assert_eq!(row.title, format!("{title_prefix}…"));
        assert_eq!(row.detail, format!("{detail_prefix}…"));
        assert_eq!(row.status, "completed");
    }

    #[test]
    fn a_new_block_does_not_start_with_the_seam() {
        let mut b = Vec::new();
        append_say(&mut b, "I'll look.");
        append_tool(&mut b, "t1", "grep", "completed", "");
        append_say(&mut b, "\n\nFound it.");
        assert_eq!(b[2].body, "Found it.");
    }

    #[test]
    fn turn_roundtrips_in_order_with_multi_paragraph_thoughts() {
        let mut b = Vec::new();
        append_thought(&mut b, "First paragraph.\n\nSecond paragraph.");
        append_say(&mut b, "I'll find the app.");
        append_tool(&mut b, "t1", "Read `Cargo.toml`", "completed", "[workspace]");
        append_tool(&mut b, "t2", "run_terminal_command", "failed", "exit 1\tboom");
        append_say(&mut b, "Walked it.\nTURN_SAY: literal");
        assert!(turn_needs_timeline(&b));
        let enc = encode_turn(&b);
        let parts = decode_turn(&enc).expect("timeline");
        assert_eq!(
            parts,
            vec![
                TurnPart::Thought("First paragraph.\n\nSecond paragraph.".into()),
                TurnPart::Say("I'll find the app.".into()),
                TurnPart::Tool(ToolRow {
                    status: "completed".into(),
                    title: "Read `Cargo.toml`".into(),
                    detail: "[workspace]".into(),
                }),
                TurnPart::Tool(ToolRow {
                    status: "failed".into(),
                    title: "run_terminal_command".into(),
                    detail: "exit 1 boom".into(),
                }),
                TurnPart::Say("Walked it.\nTURN_SAY: literal".into()),
            ]
        );
        assert_eq!(
            turn_says(&enc).as_deref(),
            Some("I'll find the app.\n\nWalked it.\nTURN_SAY: literal")
        );
    }

    #[test]
    fn plain_reply_is_not_a_timeline() {
        let mut b = Vec::new();
        append_say(&mut b, "Hello.");
        assert!(!turn_needs_timeline(&b));
        assert!(decode_turn("Hello.\nTURN_SAY: not first").is_none());
        assert!(decode_turn("THINKING:\nx\n\ny").is_none());
    }

    #[test]
    fn tool_group_labels_read_like_steps() {
        assert_eq!(tool_display_title("run_terminal_command"), "Run terminal command");
        assert_eq!(tool_display_title("Read `src/main.rs`"), "Read src/main.rs");
        assert_eq!(tool_group_label([("grep", "completed", "3 matches")]), "Grep · 3 matches");
        assert_eq!(
            tool_group_label([("Read `a.rs`", "completed", "fn main")]),
            "Read a.rs"
        );
        assert_eq!(
            tool_group_label([
                ("grep", "completed", ""),
                ("read_file", "completed", ""),
                ("grep", "completed", ""),
                ("shell", "failed", ""),
                ("web", "completed", ""),
            ]),
            "5 steps · Grep, Read file, Shell +1 · 1 failed"
        );
        let rows = vec![ToolRow {
            status: "completed".into(),
            title: "grep".into(),
            detail: "x".into(),
        }];
        assert_eq!(decode_tool_rows(&encode_tool_rows(&rows)), Some(rows));
        assert_eq!(decode_tool_rows("Click Settings"), None);
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
