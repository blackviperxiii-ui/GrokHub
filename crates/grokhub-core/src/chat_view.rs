//! Clean chat surface. The model still sees HOST_RESULT; the user sees thought and the answer.
//! Host, hands, and connector work stay off the pane until the final reply.

use crate::attach::TEXT_FILE_CAP;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatKind {
    User,
    Assistant,
    Thought,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatView {
    pub kind: ChatKind,
    pub title: String,
    pub body: String,
}

pub const CHAT_BLOCK_GAP: f32 = 10.0;
pub const THOUGHT_CLUSTER_GAP: f32 = 3.0;

/// Consecutive thoughts sit in one cluster; other blocks keep the chat gap.
pub fn cluster_gap(this_thought: bool, next_thought: bool) -> f32 {
    if this_thought && next_thought {
        THOUGHT_CLUSTER_GAP
    } else {
        CHAT_BLOCK_GAP
    }
}

pub fn thought_shows_label(prev_thought: bool) -> bool {
    !prev_thought
}

pub fn thought_shows_acts(next_thought: bool) -> bool {
    !next_thought
}

pub fn visible_turn_count(messages: &[(String, String)]) -> usize {
    visible_turn_count_from(messages.iter().map(|(r, c)| (r.as_str(), c.as_str())))
}

/// Same count without cloning an 8MB transcript onto the UI thread.
pub fn visible_turn_count_from<'a, I>(messages: I) -> usize
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    messages
        .into_iter()
        .filter(|(role, content)| *role == "user" && !is_workload_user(content))
        .count()
}

pub fn is_workload_user(content: &str) -> bool {
    let t = content.trim_start();
    t.starts_with("HOST_RESULT")
        || t.starts_with("HOST_DIFF")
        || t.starts_with("CONNECTOR_RESULT")
        || t.starts_with("COMPUTER_RESULT")
        || t.starts_with("FOLLOWUP:")
        || t.starts_with("[Goal step ")
        || t.starts_with("VERIFY_RESULT:")
}

pub fn merge_thinking(thought: &str, content: &str) -> String {
    merge_thinking_capped(thought, content, usize::MAX)
}

/// Same as [`merge_thinking`], but never allocates past `cap` (UTF-8 safe).
pub fn merge_thinking_capped(thought: &str, content: &str, cap: usize) -> String {
    let thought = thought.trim();
    let mut out = String::new();
    if thought.is_empty() {
        push_capped(&mut out, content, cap);
        return out;
    }
    push_capped(&mut out, "THINKING:\n", cap);
    push_capped(&mut out, thought, cap);
    push_capped(&mut out, "\n\n", cap);
    push_capped(&mut out, content, cap);
    out
}

fn push_capped(buf: &mut String, part: &str, cap: usize) {
    if buf.len() >= cap {
        return;
    }
    let room = cap.saturating_sub(buf.len());
    if part.len() <= room {
        buf.push_str(part);
        return;
    }
    let mut end = room;
    while end > 0 && !part.is_char_boundary(end) {
        end -= 1;
    }
    buf.push_str(&part[..end]);
}

pub fn strip_thinking(content: &str) -> String {
    let (thought, rest) = split_thought(content);
    if thought.is_empty() {
        rest.trim().to_string()
    } else {
        strip_thinking(&rest)
    }
}

pub fn visible_chat(messages: &[(String, String)]) -> Vec<ChatView> {
    visible_chat_refs(messages.iter().map(|(role, content)| (role.as_str(), content.as_str())))
}

pub fn visible_chat_refs<'a>(messages: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<ChatView> {
    let mut out = Vec::new();
    let mut stretch: Vec<(&str, &str)> = Vec::new();
    let mut ask = "";
    for (role, content) in messages {
        if role == "user" && !is_workload_user(content) {
            emit_stretch(&mut out, &stretch, ask);
            stretch.clear();
            ask = content;
            out.push(ChatView {
                kind: ChatKind::User,
                title: String::new(),
                body: view_text(content).to_string(),
            });
        } else {
            stretch.push((role, content));
        }
    }
    emit_stretch(&mut out, &stretch, ask);
    out
}

/// Display prefix. Transcript messages may hold `IMAGE_FILE_CAP`; views must not.
fn view_text(s: &str) -> &str {
    if s.len() <= TEXT_FILE_CAP {
        return s;
    }
    let mut end = TEXT_FILE_CAP;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Rebuild only the trailing stretch after the last real user turn.
/// Stream deltas must not clone earlier messages into a new view list.
pub fn refresh_last_stretch(views: &mut Vec<ChatView>, messages: &[(&str, &str)]) {
    let Some(user_i) = messages
        .iter()
        .rposition(|(role, content)| *role == "user" && !is_workload_user(content))
    else {
        *views = visible_chat_refs(messages.iter().copied());
        return;
    };
    let Some(view_i) = views.iter().rposition(|v| v.kind == ChatKind::User) else {
        *views = visible_chat_refs(messages.iter().copied());
        return;
    };
    let ask = messages[user_i].1;
    views.truncate(view_i + 1);
    emit_stretch(views, &messages[user_i + 1..], ask);
}

fn hop_is_work(rest: &str) -> bool {
    rest.lines().any(|line| {
        let t = line.trim();
        t.starts_with("HOST_CMD")
            || t.starts_with("COMPUTER_CMD")
            || t.starts_with("CONNECTOR_CMD")
            || t.starts_with("IMAGINE:")
            || t.starts_with("IMAGINE_PROMPT:")
    })
}

fn push_thought(out: &mut Vec<ChatView>, body: String) {
    if body.is_empty() {
        return;
    }
    out.push(ChatView {
        kind: ChatKind::Thought,
        title: "Thought".into(),
        body,
    });
}

fn emit_stretch(out: &mut Vec<ChatView>, stretch: &[(&str, &str)], ask: &str) {
    let ask = view_text(ask);
    let teach = crate::recipe::user_asks_gui_help(ask) && !crate::recipe::user_asks_guide_only(ask);
    let mut last_final: Option<String> = None;
    let mut last_was_work = false;
    for &(role, content) in stretch {
        let content = view_text(content);
        if role == "user" && teach {
            if let Some(label) = crate::recipe::hands_step_label(content) {
                out.push(ChatView {
                    kind: ChatKind::Tool,
                    title: "Hands".into(),
                    body: label,
                });
            }
            continue;
        }
        if role != "assistant" {
            continue;
        }
        let (thought, rest) = split_thought(content);
        if !thought.is_empty() {
            let thought = scrub_thought(&thought);
            if !thought.is_empty() {
                push_thought(out, thought);
            }
        }
        let prose = visible_assistant(&rest);
        if hop_is_work(&rest) {
            if !prose.is_empty() {
                push_thought(out, prose);
            }
            last_was_work = true;
        } else {
            last_was_work = false;
            if !prose.is_empty() {
                if let Some(prev) = last_final.take() {
                    push_thought(out, prev);
                }
                last_final = Some(prose);
            }
        }
    }
    if last_was_work {
        if let Some(prev) = last_final.take() {
            push_thought(out, prev);
        }
    }
    if let Some(prose) = last_final {
        out.push(ChatView {
            kind: ChatKind::Assistant,
            title: String::new(),
            body: prose,
        });
    }
}

fn is_protocol_line(line: &str) -> bool {
    let t = line.trim();
    t.starts_with("HOST_CMD")
        || t.starts_with("COMPUTER_CMD")
        || t.starts_with("CONNECTOR_CMD")
        || t.starts_with("WORK_PIN:")
        || t.starts_with("WORK_UPDATE:")
        || t.starts_with("VERIFY_OK")
        || t.starts_with("GOAL_COMPLETE")
        || t.starts_with("GOAL_BLOCKED")
        || t.starts_with("CONSULT:")
        || t.starts_with("IMAGINE_PROMPT:")
}

/// User-visible assistant prose. Thinking and HOST_CMD lines do not count.
pub fn assistant_prose(text: &str) -> String {
    visible_assistant(&strip_thinking(text))
}

fn visible_assistant(text: &str) -> String {
    let text = text
        .strip_prefix("SLASH_RESULT:\n")
        .or_else(|| text.strip_prefix("SLASH_RESULT:"))
        .unwrap_or(text);
    let mut lines: Vec<&str> = Vec::new();
    let mut skip_until: Option<String> = None;
    for line in text.lines() {
        if let Some(end) = skip_until.as_deref() {
            if line.trim() == end {
                skip_until = None;
            }
            continue;
        }
        if is_protocol_line(line) {
            skip_until = host_cmd_heredoc_delim(line);
            continue;
        }
        lines.push(line);
    }
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    lines.join("\n").trim().to_string()
}

/// `HOST_CMD: cat <<'EOF'` hides the script until `EOF`.
fn host_cmd_heredoc_delim(line: &str) -> Option<String> {
    let t = line.trim();
    let idx = t.find("<<")?;
    let rest = t[idx + 2..].trim_start();
    let rest = rest.strip_prefix('-').unwrap_or(rest).trim_start();
    let rest = rest
        .trim_start_matches('\'')
        .trim_start_matches('"')
        .trim_start_matches('\'');
    let delim: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if delim.is_empty() {
        None
    } else {
        Some(delim)
    }
}

/// Drop “an image is attached” narration. Cabin eyes / a drop already sent the frame.
pub fn scrub_thought(text: &str) -> String {
    let mut out = String::new();
    for chunk in split_thought_chunks(text) {
        if thought_chunk_is_attach_noise(chunk) || thought_chunk_is_false_no_computer(chunk) {
            continue;
        }
        out.push_str(chunk);
    }
    out.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

fn split_thought_chunks(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    for (i, ch) in text.char_indices() {
        if ch == '.' || ch == '!' || ch == '?' || ch == '\n' {
            let end = i + ch.len_utf8();
            if end <= bytes.len() {
                out.push(&text[start..end]);
                start = end;
            }
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

fn thought_chunk_is_false_no_computer(chunk: &str) -> bool {
    let t = chunk.to_ascii_lowercase();
    if t.trim().is_empty() {
        return false;
    }
    let no_access = t.contains("don't have")
        || t.contains("do not have")
        || t.contains("can't access")
        || t.contains("cannot access")
        || t.contains("no direct access")
        || t.contains("don't have direct access")
        || t.contains("do not have direct access");
    let target = t.contains("file system")
        || t.contains("filesystem")
        || t.contains("your computer")
        || t.contains("your files")
        || t.contains("move files around")
        || t.contains("on your computer");
    let cop_out = t.contains("exact commands to run on your computer")
        || (t.contains("give you") && t.contains("plan") && t.contains("command"));
    (no_access && target) || cop_out
}

fn thought_chunk_is_attach_noise(chunk: &str) -> bool {
    let t = chunk.to_ascii_lowercase();
    if t.trim().is_empty() {
        return false;
    }
    [
        "image attached",
        "attached an image",
        "attached a image",
        "an image is attached",
        "image is attached",
        "there's an image",
        "there is an image",
        "user attached",
        "you attached",
        "uploaded an image",
        "sent an image",
        "provided an image",
        "image was attached",
        "attached image",
        "image you attached",
        "image you just",
        "you just dropped",
        "you dropped",
        "you uploaded",
        "screenshot attached",
        "picture attached",
        "picture was attached",
        "photo attached",
    ]
    .iter()
    .any(|n| t.contains(n))
}

fn split_thought(content: &str) -> (String, String) {
    let t = content.trim_start();
    if let Some(rest) = t.strip_prefix("THINKING:") {
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        if let Some((thought, body)) = rest.split_once("\n\n") {
            return (thought.trim().to_string(), body.to_string());
        }
        return (rest.trim().to_string(), String::new());
    }
    if let Some(start) = t.find("<think>") {
        if let Some(rel_end) = t[start + 7..].find("</think>") {
            let end = start + 7 + rel_end;
            let thought = t[start + 7..end].trim().to_string();
            let mut body = t[..start].trim().to_string();
            let after = t[end + 8..].trim_start();
            if !body.is_empty() && !after.is_empty() {
                body.push('\n');
            }
            body.push_str(after);
            return (thought, body);
        }
    }
    (String::new(), content.to_string())
}

/// Quote a visible message into the composer so Reply can continue the thread.
pub fn quote_for_reply(body: &str) -> String {
    let body = body.trim();
    if body.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for line in body.lines() {
        out.push('>');
        if !line.is_empty() {
            out.push(' ');
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(v: &[ChatView]) -> Vec<ChatKind> {
        v.iter().map(|x| x.kind).collect()
    }

    #[test]
    fn hides_host_receipts_and_protocol_lines() {
        let msgs = vec![
            ("user".into(), "check the box".into()),
            (
                "assistant".into(),
                "THINKING:\nNeed a snapshot.\n\nI'll look.\nHOST_CMD: uname -a\nCOMPUTER_CMD: click 10 20\nVERIFY_OK\n".into(),
            ),
            (
                "user".into(),
                "HOST_RESULT (facts only):\n$ uname -a\nLinux cabin 6.12\nexit 0".into(),
            ),
            ("user".into(), "HOST_DIFF:\ndiff — /tmp/a\n- old\n+ new\n".into()),
            ("assistant".into(), "You're on Linux cabin.".into()),
        ];
        let v = visible_chat(&msgs);
        assert_eq!(
            kinds(&v),
            vec![
                ChatKind::User,
                ChatKind::Thought,
                ChatKind::Thought,
                ChatKind::Assistant
            ]
        );
        assert_eq!(v[0].body, "check the box");
        assert!(v[1].body.contains("snapshot"));
        assert_eq!(v[1].title, "Thought");
        assert_eq!(v[2].body, "I'll look.");
        assert_eq!(v[2].title, "Thought");
        assert_eq!(v[3].body, "You're on Linux cabin.");
        assert!(!v[3].body.contains("HOST_CMD"));
        assert!(!v[3].body.contains("COMPUTER_CMD"));
        assert!(!v[3].body.contains("VERIFY_OK"));
        assert!(!v.iter().any(|x| x.kind == ChatKind::Tool));
        assert!(!v.iter().any(|x| x.body.contains("uname -a")));
        assert!(!v.iter().any(|x| x.body.contains("HOST_RESULT")));
        assert!(!v.iter().any(|x| x.body.contains("HOST_DIFF")));
        assert_eq!(
            v.iter().filter(|x| x.kind == ChatKind::Assistant).count(),
            1
        );
    }

    #[test]
    fn pending_host_cmd_stays_off_the_pane() {
        let msgs = vec![
            ("user".into(), "go".into()),
            ("assistant".into(), "On it.\nHOST_CMD: ls /tmp\n".into()),
        ];
        let v = visible_chat(&msgs);
        assert_eq!(kinds(&v), vec![ChatKind::User, ChatKind::Thought]);
        assert_eq!(v[1].body, "On it.");
        assert!(!v.iter().any(|x| x.kind == ChatKind::Assistant));
        assert!(!v.iter().any(|x| x.kind == ChatKind::Tool));
        assert!(!v.iter().any(|x| x.body.contains("ls /tmp")));
    }

    #[test]
    fn refresh_last_stretch_keeps_earlier_turns() {
        let user = "first ask";
        let mid = "first answer";
        let ask = "second ask";
        let short = "Hi";
        let long = "Hi there, this grew while streaming.";
        let mut views = visible_chat(&[
            ("user".into(), user.into()),
            ("assistant".into(), mid.into()),
            ("user".into(), ask.into()),
            ("assistant".into(), short.into()),
        ]);
        let prefix = views.clone();
        let msgs = [
            ("user", user),
            ("assistant", mid),
            ("user", ask),
            ("assistant", long),
        ];
        refresh_last_stretch(&mut views, &msgs);
        assert_eq!(views[0].body, user);
        assert_eq!(views[1].body, mid);
        assert_eq!(views[2].body, ask);
        assert_eq!(views.last().map(|v| v.body.as_str()), Some(long));
        assert_eq!(prefix[0], views[0]);
        assert_eq!(prefix[1], views[1]);
        assert_eq!(prefix[2], views[2]);
    }

    #[test]
    fn chat_views_do_not_keep_an_8mb_body() {
        let huge = "word ".repeat(crate::attach::TEXT_FILE_CAP / 2);
        assert!(huge.len() > crate::attach::TEXT_FILE_CAP);
        let v = visible_chat(&[
            ("user".into(), "hi".into()),
            ("assistant".into(), huge.clone()),
        ]);
        let body = v
            .iter()
            .find(|x| x.kind == ChatKind::Assistant)
            .expect("assistant view")
            .body
            .len();
        assert!(
            body <= crate::attach::TEXT_FILE_CAP,
            "stream views must not clone an 8MB bubble: {body}"
        );
        let mut views = v;
        refresh_last_stretch(
            &mut views,
            &[("user", "hi"), ("assistant", huge.as_str())],
        );
        let refreshed = views
            .iter()
            .find(|x| x.kind == ChatKind::Assistant)
            .expect("refreshed assistant")
            .body
            .len();
        assert!(
            refreshed <= crate::attach::TEXT_FILE_CAP,
            "stretch refresh must not clone an 8MB bubble: {refreshed}"
        );
    }

    #[test]
    fn connector_work_stays_off_the_pane() {
        let msgs = vec![
            ("user".into(), "who am I".into()),
            (
                "assistant".into(),
                "CONNECTOR_CMD: github user\n".into(),
            ),
            (
                "user".into(),
                "CONNECTOR_RESULT (facts only):\ngithub user\nlogin: viper".into(),
            ),
        ];
        let v = visible_chat(&msgs);
        assert_eq!(kinds(&v), vec![ChatKind::User]);
        assert!(!v.iter().any(|x| x.kind == ChatKind::Tool));
        assert!(!v.iter().any(|x| x.body.contains("CONNECTOR_RESULT")));
        assert!(!v.iter().any(|x| x.body.contains("CONNECTOR_CMD")));
        assert!(!v.iter().any(|x| x.body.contains("github user")));
    }

    #[test]
    fn consecutive_thoughts_cluster_tighter_than_chat() {
        assert_eq!(cluster_gap(true, true), THOUGHT_CLUSTER_GAP);
        assert_eq!(cluster_gap(true, false), CHAT_BLOCK_GAP);
        assert_eq!(cluster_gap(false, true), CHAT_BLOCK_GAP);
        assert!(THOUGHT_CLUSTER_GAP < CHAT_BLOCK_GAP);
        assert!(thought_shows_label(false));
        assert!(!thought_shows_label(true));
        assert!(thought_shows_acts(false));
        assert!(!thought_shows_acts(true));
    }

    #[test]
    fn think_tags_and_merge_roundtrip() {
        let merged = merge_thinking("Need a snapshot.", "I'll look.");
        assert!(merged.starts_with("THINKING:"));
        assert!(merged.contains("I'll look."));
        assert_eq!(strip_thinking(&merged), "I'll look.");
        let thought = "t".repeat(80);
        let content = "c".repeat(80);
        let capped = merge_thinking_capped(&thought, &content, 40);
        assert!(capped.len() <= 40, "live merge must stay inside the UI cap");
        assert!(capped.starts_with("THINKING:\n"));
        assert!(capped.is_char_boundary(capped.len()));
        assert_eq!(
            assistant_prose("THINKING:\nnot found, I'll apt install\n\nHands are ready."),
            "Hands are ready."
        );
        assert!(assistant_prose("THINKING:\nlet me check PATH\n\n").is_empty());
        let tagged = "<think>plan the night</think>\nHello.";
        assert_eq!(strip_thinking(tagged), "Hello.");
        let v = visible_chat(&[("assistant".into(), tagged.into())]);
        assert_eq!(kinds(&v), vec![ChatKind::Thought, ChatKind::Assistant]);
        assert!(v[0].body.contains("plan the night"));
        assert_eq!(v[1].body, "Hello.");
    }

    #[test]
    fn workload_user_is_not_a_spoken_turn() {
        assert!(is_workload_user("HOST_RESULT (facts only):\n$ ls\n"));
        assert!(is_workload_user("CONNECTOR_RESULT (facts only):\nok"));
        assert!(is_workload_user("COMPUTER_RESULT (facts only):\nclicked 10,20"));
        assert!(is_workload_user("HOST_DIFF:\n- a"));
        assert!(is_workload_user(
            "FOLLOWUP: Finish the incomplete work from your last reply. Act now (HOST_CMD if needed). End with status."
        ));
        assert!(is_workload_user(
            "[Goal step 2/6]\nTask: flash the pi\nLast progress:\nWriting the image."
        ));
        assert!(is_workload_user("VERIFY_RESULT:\nexit 0\nchecked"));
        assert!(!is_workload_user("check the box"));
        assert_eq!(
            visible_turn_count(&[
                ("user".into(), "check the box".into()),
                ("assistant".into(), "ok".into()),
                ("user".into(), "HOST_RESULT (facts only):\n$ ls\n".into()),
                ("user".into(), "HOST_DIFF:\n- a".into()),
                ("user".into(), "VERIFY_RESULT:\nexit 0\n".into()),
            ]),
            1
        );
        let leaked = visible_chat(&[
            ("user".into(), "flash the pi".into()),
            ("assistant".into(), "Writing the image.".into()),
            (
                "user".into(),
                "[Goal step 2/6]\nTask: flash the pi\nContinue autonomously.".into(),
            ),
            (
                "user".into(),
                "VERIFY_RESULT:\nexit 0\nchecked".into(),
            ),
            ("assistant".into(), "Flashed.".into()),
        ]);
        assert_eq!(
            leaked.iter().filter(|x| x.kind == ChatKind::User).count(),
            1,
            "goal steps and verify receipts must stay off the pane"
        );
        assert!(!leaked.iter().any(|x| x.body.contains("[Goal step")));
        assert!(!leaked.iter().any(|x| x.body.contains("VERIFY_RESULT")));
    }

    #[test]
    fn pending_computer_cmd_stays_off_the_pane() {
        let msgs = vec![
            ("user".into(), "click the Save button".into()),
            (
                "assistant".into(),
                "On it.\nCOMPUTER_CMD: click 10 20\n".into(),
            ),
        ];
        let v = visible_chat(&msgs);
        assert_eq!(kinds(&v), vec![ChatKind::User, ChatKind::Thought]);
        assert_eq!(v[1].body, "On it.");
        assert!(!v.iter().any(|x| x.kind == ChatKind::Assistant));
        assert!(!v.iter().any(|x| x.kind == ChatKind::Tool));
        assert!(!v.iter().any(|x| x.body.contains("click 10 20")));
    }

    #[test]
    fn computer_result_stays_off_the_pane() {
        let msgs = vec![
            ("user".into(), "click save".into()),
            (
                "assistant".into(),
                "Clicking.\nCOMPUTER_CMD: act Save\n".into(),
            ),
            (
                "user".into(),
                "COMPUTER_RESULT (facts only):\n$ COMPUTER_CMD: act Save\nclicked 40,80\n".into(),
            ),
        ];
        let v = visible_chat(&msgs);
        assert_eq!(kinds(&v), vec![ChatKind::User, ChatKind::Thought]);
        assert_eq!(v[1].body, "Clicking.");
        assert!(!v.iter().any(|x| x.kind == ChatKind::Assistant));
        assert!(!v.iter().any(|x| x.kind == ChatKind::Tool));
        assert!(!v.iter().any(|x| x.body.contains("COMPUTER_RESULT")));
        assert!(!v.iter().any(|x| x.body.contains("clicked 40,80")));
    }

    #[test]
    fn gui_help_shows_hands_chip_and_howto() {
        let msgs = vec![
            ("user".into(), "close that firefox tab".into()),
            (
                "assistant".into(),
                "Closing it.\nCOMPUTER_CMD: tab close Firefox\n".into(),
            ),
            (
                "user".into(),
                "COMPUTER_RESULT (facts only):\n$ COMPUTER_CMD: tab close Firefox\nclosed Firefox\n".into(),
            ),
            (
                "assistant".into(),
                "Closed. Next time: click the tab, then the ×, or press Ctrl+W.".into(),
            ),
        ];
        let v = visible_chat(&msgs);
        assert_eq!(
            kinds(&v),
            vec![
                ChatKind::User,
                ChatKind::Thought,
                ChatKind::Tool,
                ChatKind::Assistant
            ]
        );
        assert_eq!(v[2].title, "Hands");
        assert_eq!(v[2].body, "Closed tab Firefox");
        assert!(v[3].body.contains("Ctrl+W"));
        assert!(!v.iter().any(|x| x.body.contains("COMPUTER_RESULT")));
        assert!(!v.iter().any(|x| x.body.contains("COMPUTER_CMD")));
    }

    #[test]
    fn guide_only_has_no_hands_chip() {
        let msgs = vec![
            (
                "user".into(),
                "just tell me don't click how to enable bluetooth".into(),
            ),
            (
                "assistant".into(),
                "I would click it.\nCOMPUTER_CMD: act Bluetooth\n".into(),
            ),
            (
                "user".into(),
                "COMPUTER_RESULT (facts only):\n$ COMPUTER_CMD: act Bluetooth\nact Bluetooth @1,2\n".into(),
            ),
            (
                "assistant".into(),
                "Settings → Bluetooth → the switch.".into(),
            ),
        ];
        let v = visible_chat(&msgs);
        assert!(!v.iter().any(|x| x.kind == ChatKind::Tool));
        assert!(!v.iter().any(|x| x.body.contains("COMPUTER_RESULT")));
        assert!(v.iter().any(|x| x.kind == ChatKind::Assistant && x.body.contains("Bluetooth")));
    }

    #[test]
    fn thought_drops_attach_narration() {
        assert_eq!(
            scrub_thought("The user attached an image. They asked about chowder."),
            "They asked about chowder."
        );
        assert_eq!(scrub_thought("There is an image attached."), "");
        assert_eq!(scrub_thought("There's a screenshot attached to this message."), "");
        assert_eq!(scrub_thought("A picture was attached."), "");
        assert_eq!(
            scrub_thought("You just dropped a black void. Need a snapshot."),
            "Need a snapshot."
        );
        assert_eq!(scrub_thought("Need a snapshot."), "Need a snapshot.");
        let kept = visible_chat(&[(
            "assistant".into(),
            "THINKING:\nThere is an image attached. Plan the reply.\n\nHello.".into(),
        )]);
        assert_eq!(kinds(&kept), vec![ChatKind::Thought, ChatKind::Assistant]);
        assert_eq!(kept[0].body, "Plan the reply.");
        assert_eq!(kept[1].body, "Hello.");
        let gone = visible_chat(&[(
            "assistant".into(),
            "THINKING:\nYou just dropped an image.\n\nHello.".into(),
        )]);
        assert_eq!(kinds(&gone), vec![ChatKind::Assistant]);
        assert_eq!(gone[0].body, "Hello.");
    }

    #[test]
    fn thought_drops_false_no_computer_claim() {
        assert_eq!(
            scrub_thought("I don't have direct access to your actual file system. I'll list Videos."),
            "I'll list Videos."
        );
        assert!(scrub_thought(
            "I need to be upfront about a limitation: I don't have the ability to move files around on your computer."
        )
        .is_empty());
        let v = visible_chat(&[(
            "assistant".into(),
            "THINKING:\nI don't have access to your computer.\n\nYour Videos folder is sorted.".into(),
        )]);
        assert_eq!(kinds(&v), vec![ChatKind::Assistant]);
        assert_eq!(v[0].body, "Your Videos folder is sorted.");
    }

    #[test]
    fn imagine_prompt_stays_off_the_pane_until_final() {
        let verb = visible_chat(&[(
            "assistant".into(),
            "IMAGINE_PROMPT: a cabin at night\n".into(),
        )]);
        assert!(
            !verb.iter().any(|x| x.body.contains("IMAGINE_PROMPT")),
            "the Imagine kick verb must not be the answer bubble"
        );
        assert!(!verb.iter().any(|x| x.kind == ChatKind::Assistant));
        let v = visible_chat(&[(
            "assistant".into(),
            "IMAGINE: a cabin at night\nHOST_CMD: true\n".into(),
        )]);
        assert_eq!(kinds(&v), vec![ChatKind::Thought]);
        assert!(v[0].body.contains("IMAGINE:"));
        assert!(!v.iter().any(|x| x.kind == ChatKind::Assistant));
        assert!(!v.iter().any(|x| x.kind == ChatKind::Tool));
        assert!(!v.iter().any(|x| x.body.contains("HOST_CMD")));
        assert!(!v.iter().any(|x| x.body == "true"));
    }

    #[test]
    fn work_dump_stays_off_the_chat_surface() {
        let msgs = vec![
            ("user".into(), "check the machine".into()),
            (
                "assistant".into(),
                "THINKING:\nNeed a snapshot.\n\nI'll look.\nHOST_CMD: cat <<'EOF'\n===== GPU =====\nlong dump\nEOF\n".into(),
            ),
            (
                "user".into(),
                "HOST_RESULT (facts only):\n$ cat script\n===== GPU =====\n===== UINPUT =====\nexit 0".into(),
            ),
            ("assistant".into(), "You're on Linux cabin.".into()),
        ];
        let v = visible_chat(&msgs);
        assert_eq!(
            kinds(&v),
            vec![
                ChatKind::User,
                ChatKind::Thought,
                ChatKind::Thought,
                ChatKind::Assistant
            ]
        );
        assert!(!v.iter().any(|x| x.kind == ChatKind::Tool));
        assert!(!v.iter().any(|x| x.body.contains("===== GPU")));
        assert!(!v.iter().any(|x| x.body.contains("HOST_CMD")));
        assert_eq!(v.last().map(|x| x.body.as_str()), Some("You're on Linux cabin."));
    }

    #[test]
    fn two_work_hops_then_closer_splits_thoughts() {
        let msgs = vec![
            ("user".into(), "fix the box".into()),
            (
                "assistant".into(),
                "THINKING:\nNeed a snapshot.\n\nI'll look.\nHOST_CMD: uname -a\n".into(),
            ),
            (
                "user".into(),
                "HOST_RESULT (facts only):\n$ uname -a\nLinux cabin 6.12\nexit 0".into(),
            ),
            (
                "assistant".into(),
                "Restarting the service.\nHOST_CMD: systemctl --user restart grokhub\n".into(),
            ),
            (
                "user".into(),
                "HOST_RESULT (facts only):\n$ systemctl --user restart grokhub\nexit 0".into(),
            ),
            ("assistant".into(), "You're on Linux cabin.".into()),
        ];
        let v = visible_chat(&msgs);
        assert_eq!(
            kinds(&v),
            vec![
                ChatKind::User,
                ChatKind::Thought,
                ChatKind::Thought,
                ChatKind::Thought,
                ChatKind::Assistant
            ]
        );
        assert_eq!(v[1].body, "Need a snapshot.");
        assert_eq!(v[2].body, "I'll look.");
        assert_eq!(v[3].body, "Restarting the service.");
        assert_eq!(v[4].body, "You're on Linux cabin.");
        assert_eq!(
            v.iter().filter(|x| x.kind == ChatKind::Assistant).count(),
            1
        );
        assert!(!v.iter().any(|x| x.body.contains("HOST_CMD")));
        assert!(!v.iter().any(|x| x.body.contains("HOST_RESULT")));
        assert!(!v.iter().any(|x| x.body.contains("uname -a")));
        assert!(!v.iter().any(|x| x.body.contains("systemctl")));
    }

    #[test]
    fn heredoc_host_cmd_is_not_assistant_prose() {
        assert_eq!(
            assistant_prose("I'll look.\nHOST_CMD: cat <<'EOF'\n===== GPU =====\nEOF\nDone."),
            "I'll look.\nDone."
        );
        assert_eq!(
            host_cmd_heredoc_delim("HOST_CMD: cat <<'EOF'"),
            Some("EOF".into())
        );
        assert_eq!(
            host_cmd_heredoc_delim("HOST_CMD: uname -a"),
            None
        );
        assert_eq!(
            host_cmd_heredoc_delim("HOST_CMD: cat <<'EOF-2'"),
            Some("EOF-2".into())
        );
        assert_eq!(
            assistant_prose("I'll look.\nHOST_CMD: cat <<'EOF-2'\nsecret dump\nEOF-2\nDone."),
            "I'll look.\nDone."
        );
    }

    #[test]
    fn status_then_work_keeps_the_status_as_thought() {
        let msgs = vec![
            ("user".into(), "check the box".into()),
            ("assistant".into(), "Checking the system.".into()),
            ("assistant".into(), "HOST_CMD: uname -a\n".into()),
        ];
        let v = visible_chat(&msgs);
        assert_eq!(kinds(&v), vec![ChatKind::User, ChatKind::Thought]);
        assert_eq!(v[1].body, "Checking the system.");
        assert!(!v.iter().any(|x| x.kind == ChatKind::Assistant));
        assert!(!v.iter().any(|x| x.body.contains("uname")));
    }

    #[test]
    fn quote_for_reply_prefixes_each_line() {
        assert_eq!(quote_for_reply("hi"), "> hi\n");
        assert_eq!(quote_for_reply("a\nb"), "> a\n> b\n");
        assert_eq!(quote_for_reply("  "), "");
        assert_eq!(
            crate::append_composer("draft", &quote_for_reply("check the box")),
            "draft\n> check the box"
        );
    }

    #[test]
    fn slash_result_marker_stays_off_the_pane() {
        let v = visible_chat(&[(
            "assistant".into(),
            "SLASH_RESULT:\n/help — this list\n/new — new chat".into(),
        )]);
        assert_eq!(kinds(&v), vec![ChatKind::Assistant]);
        assert_eq!(v[0].body, "/help — this list\n/new — new chat");
        assert!(!v[0].body.contains("SLASH_RESULT"));
        assert_eq!(
            assistant_prose("SLASH_RESULT:\nGrok 4.6 — chat"),
            "Grok 4.6 — chat"
        );
    }
}
