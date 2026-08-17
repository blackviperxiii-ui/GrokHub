//! Clean chat surface. The model still sees HOST_RESULT; the user sees thought and the answer.
//! Host, hands, and connector work stay off the pane until the final reply.

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

pub fn is_workload_user(content: &str) -> bool {
    let t = content.trim_start();
    t.starts_with("HOST_RESULT")
        || t.starts_with("HOST_DIFF")
        || t.starts_with("CONNECTOR_RESULT")
        || t.starts_with("COMPUTER_RESULT")
        || t.starts_with("FOLLOWUP:")
}

pub fn merge_thinking(thought: &str, content: &str) -> String {
    let thought = thought.trim();
    if thought.is_empty() {
        content.to_string()
    } else {
        format!("THINKING:\n{thought}\n\n{content}")
    }
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
    let mut out = Vec::new();
    let mut stretch: Vec<&(String, String)> = Vec::new();
    for msg in messages {
        if msg.0 == "user" && !is_workload_user(&msg.1) {
            emit_stretch(&mut out, &stretch);
            stretch.clear();
            out.push(ChatView {
                kind: ChatKind::User,
                title: String::new(),
                body: msg.1.clone(),
            });
        } else {
            stretch.push(msg);
        }
    }
    emit_stretch(&mut out, &stretch);
    out
}

fn hop_is_work(rest: &str) -> bool {
    rest.lines().any(|line| {
        let t = line.trim();
        t.starts_with("HOST_CMD")
            || t.starts_with("COMPUTER_CMD")
            || t.starts_with("CONNECTOR_CMD")
            || t.starts_with("IMAGINE:")
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

fn emit_stretch(out: &mut Vec<ChatView>, stretch: &[&(String, String)]) {
    let mut last_final: Option<String> = None;
    let mut last_was_work = false;
    for (role, content) in stretch {
        if *role != "assistant" {
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
}

/// User-visible assistant prose. Thinking and HOST_CMD lines do not count.
pub fn assistant_prose(text: &str) -> String {
    visible_assistant(&strip_thinking(text))
}

fn visible_assistant(text: &str) -> String {
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
        if thought_chunk_is_attach_noise(chunk) {
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
    fn think_tags_and_merge_roundtrip() {
        let merged = merge_thinking("Need a snapshot.", "I'll look.");
        assert!(merged.starts_with("THINKING:"));
        assert!(merged.contains("I'll look."));
        assert_eq!(strip_thinking(&merged), "I'll look.");
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
        assert!(!is_workload_user("check the box"));
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
    fn imagine_prompt_stays_off_the_pane_until_final() {
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
}
