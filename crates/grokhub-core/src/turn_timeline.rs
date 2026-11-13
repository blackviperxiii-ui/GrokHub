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
        if let Some(old) = blocks
            .iter_mut()
            .rev()
            .find(|b| b.kind == LiveKind::Tool && b.tool_id == id)
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
    word == "I"
        || word.starts_with("I'")
        || word.starts_with("I\u{2019}")
        || chars.any(char::is_lowercase)
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

/// Which part of a turn a label opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LabelKind {
    Thought,
    Tool,
    Say,
}

/// Wrapping a model puts around a label: `**TURN_SAY:**`, `## Turn say`,
/// `[TURN_THOUGHT]`, `<turn_say>`, `` `TURN_SAY:` ``.
fn is_label_decoration(c: char) -> bool {
    matches!(c, '*' | '#' | '[' | ']' | '<' | '>' | '`' | '/')
}

const LABEL_KINDS: [(&str, LabelKind); 3] = [
    ("thought", LabelKind::Thought),
    ("tool", LabelKind::Tool),
    ("say", LabelKind::Say),
];

/// A turn label at the very start of `s`, in any spelling a model writes it:
/// any case, `TURN_SAY`, `turn say`, `Turn-Say`, `TURNSAY`, a plural, with a
/// colon (or a full-width one), and wrapped in `**`, `#`, `[ ]` or `< >`. With
/// no colon it counts only when nothing else follows on the line, so prose like
/// "your turn, say hi" stays prose. Returns the kind and the label's length.
fn label_at(s: &str) -> Option<(LabelKind, usize)> {
    let lead = s.len()
        - s.trim_start_matches(|c| is_label_decoration(c) || c == ' ')
            .len();
    let rest = &s[lead..];
    let head = rest.get(..4)?;
    if !head.eq_ignore_ascii_case("turn") {
        return None;
    }
    let mut i = lead + 4;
    if let Some(c) = s[i..].chars().next() {
        if matches!(c, '_' | ' ' | '-') {
            i += 1;
        }
    }
    let (kind, word) = LABEL_KINDS.iter().find_map(|(word, kind)| {
        s.get(i..i + word.len())
            .filter(|w| w.eq_ignore_ascii_case(word))
            .map(|_| (*kind, word.len()))
    })?;
    i += word;
    if s[i..].starts_with(['s', 'S']) {
        i += 1;
    }
    // A longer word ("TURN_SAYING") is not a label.
    if s[i..]
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric() || c == '_')
    {
        return None;
    }
    i += s[i..].len() - s[i..].trim_start_matches(is_label_decoration).len();
    let after = &s[i..];
    let colon = if after.starts_with(':') {
        1
    } else if after.starts_with('：') {
        '：'.len_utf8()
    } else {
        0
    };
    if colon > 0 {
        i += colon;
        // `**TURN_SAY:**` closes its bold after the colon.
        i += s[i..].len() - s[i..].trim_start_matches(is_label_decoration).len();
    } else if !after.trim().is_empty() {
        return None;
    }
    Some((kind, i))
}

/// Every label in one line: where it starts, its kind, where it ends. A label
/// starts the line or follows a space or decoration, never inside a word.
fn labels_in(line: &str) -> Vec<(usize, LabelKind, usize)> {
    let mut found = Vec::new();
    let mut from = 0;
    while from < line.len() {
        let Some(at) = line[from..]
            .char_indices()
            .map(|(i, _)| from + i)
            .find(|&i| {
                line[i..].starts_with(is_label_decoration)
                    || line
                        .get(i..i + 4)
                        .is_some_and(|w| w.eq_ignore_ascii_case("turn"))
            })
        else {
            break;
        };
        let starts_word = line[..at]
            .chars()
            .next_back()
            .is_none_or(|c| c.is_whitespace() || is_label_decoration(c));
        match label_at(&line[at..]).filter(|_| starts_word) {
            Some((kind, len)) => {
                found.push((at, kind, at + len));
                from = at + len;
            }
            None => {
                from = at + line[at..].chars().next().map_or(1, char::len_utf8);
            }
        }
    }
    found
}

/// Whether `text` holds a turn label anywhere outside fenced code.
pub fn has_turn_label(text: &str) -> bool {
    if !text
        .as_bytes()
        .windows(4)
        .any(|w| w.eq_ignore_ascii_case(b"turn"))
    {
        return false;
    }
    let mut fence = false;
    text.lines().any(|line| {
        if is_fence(line) {
            fence = !fence;
            return false;
        }
        !fence && !labels_in(line).is_empty()
    })
}

fn is_fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

/// Text with no turn labels, for a model prompt or the screen. A timeline, or a
/// reply that echoes one in any spelling, keeps only its replies: thoughts and
/// tool rows go. Text on the same line after a say label stays. A thought that
/// opens mid-line ends at the next say label or blank line. Fenced code is left
/// alone. When there is nothing but thought, the thought is the reply, so an
/// answer never vanishes. Text with no label comes back unchanged.
pub fn strip_turn_labels(text: &str) -> String {
    if !has_turn_label(text) {
        return text.to_string();
    }
    #[derive(PartialEq)]
    enum Mode {
        Say,
        Thought { ends_at_blank: bool },
        Tool,
    }
    let mut mode = Mode::Say;
    let mut says: Vec<Vec<String>> = vec![Vec::new()];
    let mut thoughts: Vec<String> = Vec::new();
    let mut saw_tool = false;
    let mut fence = false;
    let say_line = |says: &mut Vec<Vec<String>>, line: &str| {
        if let Some(last) = says.last_mut() {
            last.push(line.to_string());
        }
    };
    for line in text.lines() {
        if fence || is_fence(line) {
            if is_fence(line) {
                fence = !fence;
            }
            match mode {
                Mode::Say => say_line(&mut says, line),
                Mode::Thought { .. } => thoughts.push(line.to_string()),
                Mode::Tool => {}
            }
            continue;
        }
        if line.trim().is_empty() {
            match mode {
                Mode::Thought {
                    ends_at_blank: true,
                }
                | Mode::Tool => {
                    mode = Mode::Say;
                    says.push(Vec::new());
                }
                Mode::Thought { .. } => thoughts.push(String::new()),
                Mode::Say => say_line(&mut says, ""),
            }
            continue;
        }
        let labels = labels_in(line);
        if labels.is_empty() {
            match mode {
                Mode::Say => say_line(&mut says, line),
                Mode::Thought { .. } => thoughts.push(line.to_string()),
                Mode::Tool => {}
            }
            continue;
        }
        let mut kept = String::new();
        let mut thought = String::new();
        let mut cursor = 0;
        for (at, kind, end) in labels {
            let piece = &line[cursor..at];
            match mode {
                Mode::Say => kept.push_str(piece),
                Mode::Thought { .. } => thought.push_str(piece),
                Mode::Tool => {}
            }
            let at_line_start = line[..at].trim().is_empty();
            mode = match kind {
                LabelKind::Say => {
                    if mode != Mode::Say {
                        if !kept.trim().is_empty() {
                            say_line(&mut says, kept.trim_end());
                            kept.clear();
                        }
                        says.push(Vec::new());
                    }
                    Mode::Say
                }
                LabelKind::Thought => Mode::Thought {
                    ends_at_blank: !at_line_start,
                },
                LabelKind::Tool => {
                    saw_tool = true;
                    Mode::Tool
                }
            };
            cursor = end;
        }
        let rest = &line[cursor..];
        match mode {
            Mode::Say => kept.push_str(rest),
            Mode::Thought { .. } => thought.push_str(rest),
            // A tool row is one line; what follows it on later lines is unlabelled.
            Mode::Tool => {}
        }
        let kept = kept.split_whitespace().collect::<Vec<_>>().join(" ");
        if !kept.is_empty() {
            say_line(&mut says, &kept);
        }
        let thought = thought.trim();
        if !thought.is_empty() {
            thoughts.push(thought.to_string());
        }
    }
    let reply = says
        .iter()
        .map(|lines| lines.join("\n").trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    if reply.is_empty() && !saw_tool {
        return thoughts.join("\n").trim().to_string();
    }
    reply
}

/// How many bytes at the end of a growing reply could be the start of a turn
/// label (`TURN_TH` before `OUGHT:` arrives). The screen holds them back until
/// the next chunk shows what they are, so a label never flashes mid-stream.
pub fn label_holdback(text: &str) -> usize {
    const MAX: usize = 24;
    let floor = text.len().saturating_sub(MAX);
    for (at, _) in text.char_indices().filter(|(i, _)| *i >= floor) {
        let starts_word = text[..at]
            .chars()
            .next_back()
            .is_none_or(|c| c.is_whitespace() || is_label_decoration(c));
        if starts_word && could_become_label(&text[at..]) {
            return text.len() - at;
        }
    }
    0
}

/// A reply still streaming, as the screen shows it: labels stripped, and a
/// label still arriving at the end held back until the next chunk.
pub fn live_reply_text(text: &str) -> String {
    strip_turn_labels(&text[..text.len() - label_holdback(text)])
}

/// A thought as its card shows it: the words stay, label words go.
pub fn thought_text(text: &str) -> String {
    let text = &text[..text.len() - label_holdback(text)];
    if !has_turn_label(text) {
        return text.to_string();
    }
    let mut fence = false;
    text.lines()
        .map(|line| {
            if is_fence(line) {
                fence = !fence;
                return line.to_string();
            }
            let labels = labels_in(line);
            if fence || labels.is_empty() {
                return line.to_string();
            }
            let mut out = String::new();
            let mut cursor = 0;
            for (at, _, end) in labels {
                out.push_str(&line[cursor..at]);
                out.push(' ');
                cursor = end;
            }
            out.push_str(&line[cursor..]);
            out.split_whitespace().collect::<Vec<_>>().join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// `s` is a strict start of some label spelling (`TURN_TH`, `turn s`), so the
/// next chunk may finish it.
fn could_become_label(s: &str) -> bool {
    let body = s.trim_start_matches(is_label_decoration);
    if body.is_empty() || body.contains('\n') {
        return false;
    }
    let lower = body.to_ascii_lowercase();
    ["_", " ", "-", ""].iter().any(|sep| {
        LABEL_KINDS.iter().any(|(word, _)| {
            let form = format!("turn{sep}{word}s:");
            let single = format!("turn{sep}{word}:");
            (form.starts_with(&lower) && lower.len() < form.len())
                || (single.starts_with(&lower) && lower.len() < single.len())
        })
    })
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

pub(crate) fn clip_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    format!(
        "{}…",
        s.chars().take(max.saturating_sub(1)).collect::<String>()
    )
}

/// True when `detail` is only a status word, so a header or body must not repeat the chip.
/// One line, trimmed, lowercased, with a trailing `…` or `...` stripped.
pub fn tool_detail_is_status(detail: &str) -> bool {
    let line = detail.trim();
    if line.contains('\n') || line.contains('\r') {
        return false;
    }
    let lower = line.to_ascii_lowercase();
    let stripped = if let Some(rest) = lower.strip_suffix('…') {
        rest.trim()
    } else if let Some(rest) = lower.strip_suffix("...") {
        rest.trim()
    } else {
        lower.as_str()
    };
    matches!(
        stripped,
        "running"
            | "pending"
            | "in progress"
            | "in_progress"
            | "started"
            | "completed"
            | "complete"
            | "done"
            | "success"
            | "succeeded"
            | "finished"
            | "ok"
            | "waiting for input"
    )
}

/// One quiet header for a run of tool calls (`title, status, detail`):
/// `Read src/main.rs`, `Grep · 3 matches`, or `4 steps · Grep, Read file, Shell +1 · 1 failed`.
/// A status-word detail (`running`, `Waiting for input`, …) is ignored: the chip is the status.
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
        // A status word does not: "Click" stays "Click" when detail is "running".
        let detail = one_line(only_detail);
        if !name.contains(' ')
            && !detail.is_empty()
            && detail != name
            && !tool_detail_is_status(only_detail)
        {
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
        assert_eq!(
            chunk_seam("then write up what I hit.", "GrokHub is running.", false),
            "\n\n"
        );
        assert_eq!(
            chunk_seam("uses WORK_PIN to track tasks.", "I'm examining", false),
            "\n\n"
        );
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
            chunk_seam(
                "I'll look.\nHOST_CMD: cat <<'EOF'\necho hi.",
                "There",
                false
            ),
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
        append_tool(
            &mut b,
            "t1",
            "Read `Cargo.toml`",
            "completed",
            "[workspace]",
        );
        append_tool(
            &mut b,
            "t2",
            "run_terminal_command",
            "failed",
            "exit 1\tboom",
        );
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
        assert_eq!(
            tool_display_title("run_terminal_command"),
            "Run terminal command"
        );
        assert_eq!(tool_display_title("Read `src/main.rs`"), "Read src/main.rs");
        assert_eq!(
            tool_group_label([("click", "completed", "running")]),
            "Click"
        );
        assert_eq!(
            tool_group_label([("grep", "completed", "3 matches")]),
            "Grep · 3 matches"
        );
        assert!(tool_detail_is_status(" Running… "));
        assert!(tool_detail_is_status("waiting for input"));
        assert!(tool_detail_is_status("in progress"));
        assert!(tool_detail_is_status("in_progress"));
        assert!(tool_detail_is_status("completed..."));
        assert!(tool_detail_is_status("ok"));
        assert!(!tool_detail_is_status("3 matches"));
        assert!(!tool_detail_is_status("running fast"));
        assert!(!tool_detail_is_status("running\nextra"));
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

    #[test]
    fn turn_labels_never_reach_a_prompt_or_the_screen() {
        let echoed = "Haha, nope.\nTURN_THOUGHT:\nthey are joking about a friend\nTURN_SAY:\nI'd rather roast her aim.";
        assert_eq!(
            strip_turn_labels(echoed),
            "Haha, nope.\n\nI'd rather roast her aim."
        );
        let mut b = Vec::new();
        append_thought(&mut b, "Plan the reply.");
        append_tool(&mut b, "t1", "Read notes", "done", "");
        append_say(&mut b, "First line.");
        append_thought(&mut b, "Second thought.");
        append_say(&mut b, "Done.");
        assert_eq!(strip_turn_labels(&encode_turn(&b)), "First line.\n\nDone.");
        assert_eq!(strip_turn_labels("ok TURN_SAY: hi"), "ok hi");
        assert_eq!(
            strip_turn_labels("  TURN_SAY:\nIndented echo."),
            "Indented echo."
        );
        assert_eq!(
            strip_turn_labels("Plain reply.\n\nTwo lines."),
            "Plain reply.\n\nTwo lines."
        );
    }

    #[test]
    fn every_label_spelling_is_stripped_and_prose_is_not() {
        for label in [
            "TURN_THOUGHT:",
            "turn_thought:",
            "Turn Thought:",
            "TURN-THOUGHT:",
            "TURNTHOUGHT:",
            "TURN_THOUGHTS:",
            "**TURN_THOUGHT:**",
            "**TURN_THOUGHT**:",
            "## Turn thought",
            "[TURN_THOUGHT]",
            "<turn_thought>",
            "TURN_THOUGHT：",
            "TURN_THOUGHT",
        ] {
            let text = format!("{label}\nthey want a tune-up\nTURN_SAY:\nRunning the checks now.");
            assert_eq!(
                strip_turn_labels(&text),
                "Running the checks now.",
                "{label}"
            );
        }
        for say in ["TURN_SAY:", "**Turn say:**", "turn_says:", "[TURN_SAY]"] {
            let text = format!("TURN_THOUGHT:\nplan\n{say}\nHi.");
            assert_eq!(strip_turn_labels(&text), "Hi.", "{say}");
        }
        for prose in [
            "Your turn, say hi to the squad.",
            "Take a turn thought experiment.",
            "Return the saying.",
            "turn say what you mean",
        ] {
            assert_eq!(strip_turn_labels(prose), prose);
            assert!(!has_turn_label(prose), "{prose}");
        }
    }

    #[test]
    fn same_line_text_stays_and_thought_only_becomes_the_reply() {
        assert_eq!(strip_turn_labels("TURN_SAY: On it."), "On it.");
        assert_eq!(
            strip_turn_labels("TURN_THOUGHT: check disk first TURN_SAY: Checking your disk."),
            "Checking your disk."
        );
        assert_eq!(
            strip_turn_labels("Sure. **TURN_THOUGHT:** they seem tired\n\nWant a quick check?"),
            "Sure.\n\nWant a quick check?"
        );
        assert_eq!(
            strip_turn_labels("TURN_THOUGHT:\nI'll run the disk check."),
            "I'll run the disk check."
        );
        // A saved work hop with no reply stays empty: its tool rows say what happened.
        assert_eq!(
            strip_turn_labels("TURN_THOUGHT:\nlook\nTURN_TOOL: done\tRead\t\n"),
            ""
        );
    }

    #[test]
    fn labels_inside_fenced_code_are_left_alone() {
        let text = "Here's the format:\n```\nTURN_SAY:\nhello\n```\nThat's it.";
        assert_eq!(strip_turn_labels(text), text);
        assert!(!has_turn_label(text));
        let mixed = "TURN_THOUGHT:\nshow them\nTURN_SAY:\n```\nTURN_SAY: kept\n```";
        assert_eq!(strip_turn_labels(mixed), "```\nTURN_SAY: kept\n```");
    }

    #[test]
    fn a_label_split_across_chunks_never_shows_on_screen() {
        let full = "TURN_THOUGHT: they want hi\nTURN_SAY: hi";
        for size in 1..=7 {
            let mut streamed = String::new();
            let mut at = 0;
            while at < full.len() {
                let end = (at + size).min(full.len());
                streamed.push_str(&full[at..end]);
                at = end;
                let held = label_holdback(&streamed);
                let shown = strip_turn_labels(&streamed[..streamed.len() - held]);
                assert!(
                    "hi".starts_with(shown.trim())
                        || shown.trim() == "they want hi"
                        || "they want hi".starts_with(shown.trim()),
                    "chunk {size}: showed {shown:?}"
                );
                assert!(!shown.to_ascii_lowercase().contains("turn"), "{shown:?}");
            }
            assert_eq!(strip_turn_labels(&streamed), "hi");
        }
        assert_eq!(label_holdback("Checking now TURN_TH"), "TURN_TH".len());
        assert_eq!(label_holdback("**Turn s"), "**Turn s".len());
        assert_eq!(label_holdback("All done."), 0);
        assert_eq!(label_holdback("Return"), 0);
    }

    #[test]
    fn thought_cards_keep_their_words_without_label_words() {
        assert_eq!(
            thought_text("TURN_THOUGHT: check disk, then TURN_SAY: tell them"),
            "check disk, then tell them"
        );
        assert_eq!(thought_text("plain thought"), "plain thought");
        assert_eq!(live_reply_text("Checking now **TURN_"), "Checking now ");
    }

    #[test]
    fn a_raw_saved_reply_never_reloads_as_a_thought_card() {
        let raw = "TURN_THOUGHT:\nthey asked for a check\nTURN_SAY:\nYour disk is 92% full.";
        let saved = strip_turn_labels(raw);
        assert_eq!(saved, "Your disk is 92% full.");
        assert!(decode_turn(&saved).is_none());
        // A reply that only thought still saves its words as the reply.
        let only = strip_turn_labels("turn_thought: checking the disk now");
        assert_eq!(only, "checking the disk now");
        assert!(decode_turn(&only).is_none());
    }
}
