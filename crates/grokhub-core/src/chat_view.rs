//! Clean chat surface. The model still sees HOST_RESULT; the user sees thought, tools, and the answer.

use crate::chat::extract_host_cmds;
use crate::connector::extract_connector_cmds;

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
    t.starts_with("HOST_RESULT") || t.starts_with("HOST_DIFF") || t.starts_with("CONNECTOR_RESULT")
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
    for (i, (role, content)) in messages.iter().enumerate() {
        match role.as_str() {
            "user" => {
                if let Some(tool) = tool_from_workload(content) {
                    out.push(tool);
                } else if !is_workload_user(content) {
                    out.push(ChatView {
                        kind: ChatKind::User,
                        title: String::new(),
                        body: content.clone(),
                    });
                }
            }
            "assistant" => {
                let (thought, rest) = split_thought(content);
                if !thought.is_empty() {
                    out.push(ChatView {
                        kind: ChatKind::Thought,
                        title: "Thought".into(),
                        body: thought,
                    });
                }
                let prose = visible_assistant(&rest);
                if !prose.is_empty() {
                    out.push(ChatView {
                        kind: ChatKind::Assistant,
                        title: String::new(),
                        body: prose,
                    });
                }
                let next_is_result = messages
                    .get(i + 1)
                    .is_some_and(|(r, c)| r == "user" && is_workload_user(c));
                if !next_is_result {
                    for cmd in extract_host_cmds(&rest) {
                        out.push(ChatView {
                            kind: ChatKind::Tool,
                            title: "Host".into(),
                            body: cmd,
                        });
                    }
                    for c in extract_connector_cmds(&rest) {
                        out.push(ChatView {
                            kind: ChatKind::Tool,
                            title: connector_title(&c.connector_id),
                            body: format!("{} {}", c.tool, c.args)
                                .trim()
                                .to_string(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn connector_title(id: &str) -> String {
    match id {
        "github" | "gh" => "GitHub".into(),
        other => other.to_string(),
    }
}

fn is_protocol_line(line: &str) -> bool {
    let t = line.trim();
    t.starts_with("HOST_CMD")
        || t.starts_with("CONNECTOR_CMD")
        || t.starts_with("WORK_PIN:")
        || t.starts_with("WORK_UPDATE:")
        || t.starts_with("VERIFY_OK")
        || t.starts_with("GOAL_COMPLETE")
        || t.starts_with("GOAL_BLOCKED")
        || t.starts_with("CONSULT:")
}

fn visible_assistant(text: &str) -> String {
    let mut lines: Vec<&str> = text
        .lines()
        .filter(|line| !is_protocol_line(line))
        .collect();
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    lines.join("\n").trim().to_string()
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

fn tool_from_workload(content: &str) -> Option<ChatView> {
    let t = content.trim_start();
    if t.starts_with("HOST_DIFF") {
        return None;
    }
    if t.starts_with("HOST_RESULT") {
        return Some(tool_from_host_result(t));
    }
    if t.starts_with("CONNECTOR_RESULT") {
        return Some(tool_from_connector_result(t));
    }
    None
}

fn tool_from_host_result(content: &str) -> ChatView {
    let mut cmd = String::new();
    for line in content.lines() {
        let line = line.trim();
        if let Some(c) = line.strip_prefix("$ ") {
            cmd = c.to_string();
            break;
        }
    }
    if cmd.is_empty() {
        cmd = "host".into();
    }
    ChatView {
        kind: ChatKind::Tool,
        title: "Host".into(),
        body: cmd,
    }
}

fn tool_from_connector_result(content: &str) -> ChatView {
    let rest = content
        .trim_start()
        .strip_prefix("CONNECTOR_RESULT (facts only):")
        .or_else(|| content.trim_start().strip_prefix("CONNECTOR_RESULT:"))
        .unwrap_or(content)
        .trim();
    let first = rest.lines().next().unwrap_or("connector").trim();
    let mut parts = first.splitn(2, char::is_whitespace);
    let id = parts.next().unwrap_or("github");
    let body = parts.next().unwrap_or("").trim();
    ChatView {
        kind: ChatKind::Tool,
        title: connector_title(id),
        body: if body.is_empty() {
            first.to_string()
        } else {
            body.to_string()
        },
    }
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
                "THINKING:\nNeed a snapshot.\n\nI'll look.\nHOST_CMD: uname -a\nVERIFY_OK\n".into(),
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
                ChatKind::Assistant,
                ChatKind::Tool,
                ChatKind::Assistant,
            ]
        );
        assert_eq!(v[0].body, "check the box");
        assert!(v[1].body.contains("snapshot"));
        assert_eq!(v[1].title, "Thought");
        assert_eq!(v[2].body, "I'll look.");
        assert!(!v[2].body.contains("HOST_CMD"));
        assert!(!v[2].body.contains("VERIFY_OK"));
        assert_eq!(v[3].title, "Host");
        assert!(v[3].body.contains("uname -a"));
        assert!(!v.iter().any(|x| x.body.contains("HOST_RESULT")));
        assert!(!v.iter().any(|x| x.body.contains("HOST_DIFF")));
        assert_eq!(v[4].body, "You're on Linux cabin.");
    }

    #[test]
    fn pending_host_cmd_is_a_tool_until_the_receipt() {
        let msgs = vec![
            ("user".into(), "go".into()),
            ("assistant".into(), "On it.\nHOST_CMD: ls /tmp\n".into()),
        ];
        let v = visible_chat(&msgs);
        assert_eq!(
            kinds(&v),
            vec![ChatKind::User, ChatKind::Assistant, ChatKind::Tool]
        );
        assert_eq!(v[1].body, "On it.");
        assert_eq!(v[2].title, "Host");
        assert_eq!(v[2].body, "ls /tmp");
    }

    #[test]
    fn connector_receipt_is_a_github_tool() {
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
        assert!(v.iter().any(|x| x.kind == ChatKind::Tool && x.title == "GitHub"));
        assert!(!v.iter().any(|x| x.body.contains("CONNECTOR_RESULT")));
        assert!(!v.iter().any(|x| x.body.contains("CONNECTOR_CMD")));
    }

    #[test]
    fn think_tags_and_merge_roundtrip() {
        let merged = merge_thinking("Need a snapshot.", "I'll look.");
        assert!(merged.starts_with("THINKING:"));
        assert!(merged.contains("I'll look."));
        assert_eq!(strip_thinking(&merged), "I'll look.");
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
        assert!(is_workload_user("HOST_DIFF:\n- a"));
        assert!(!is_workload_user("check the box"));
    }

    #[test]
    fn imagine_prompt_stays_visible() {
        let v = visible_chat(&[(
            "assistant".into(),
            "IMAGINE: a cabin at night\nHOST_CMD: true\n".into(),
        )]);
        assert!(v.iter().any(|x| x.kind == ChatKind::Assistant && x.body.contains("IMAGINE:")));
        assert!(v.iter().any(|x| x.kind == ChatKind::Tool && x.body == "true"));
        assert!(!v.iter().any(|x| x.body.contains("HOST_CMD")));
    }
}
