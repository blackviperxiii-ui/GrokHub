//! Claude Code asking the app before a tool runs. With
//! `--permission-prompt-tool stdio`, a call the permission mode doesn't
//! settle comes as a `control_request` line (`can_use_tool`), and the turn
//! waits for a `control_response` line on stdin. Pure: lines in, lines out.

use serde_json::{json, Value};

use crate::gate::PermAnswer;

/// One tool Claude Code wants to run, waiting on the user.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolAsk {
    pub request_id: String,
    pub tool: String,
    /// The tool's input, sent back unchanged when the user allows it.
    pub input: Value,
    /// One plain line: the command, file or link.
    pub action: String,
    /// What Claude said the call is for, when it said.
    pub reason: String,
    /// Rules Claude Code offers for "Always allow", like `Bash(npm test *)`.
    pub rules: Vec<String>,
}

/// A `control_request` line: a tool to ask about, or another request this
/// app doesn't take (answered with an error so the turn doesn't hang).
#[derive(Debug, Clone, PartialEq)]
pub enum Control {
    Ask(ToolAsk),
    Other { request_id: String },
}

/// The control request on this line, if it is one.
pub fn control(line: &str) -> Option<Control> {
    let msg: Value = serde_json::from_str(line.trim()).ok()?;
    if msg["type"] != "control_request" {
        return None;
    }
    let request_id = msg["request_id"]
        .as_str()
        .filter(|s| !s.is_empty())?
        .to_string();
    let req = &msg["request"];
    if req["subtype"] != "can_use_tool" {
        return Some(Control::Other { request_id });
    }
    let tool = req["tool_name"].as_str().unwrap_or_default().to_string();
    let input = req["input"].clone();
    let action = super::stream::input_summary(&input);
    let reason = req["description"]
        .as_str()
        .or_else(|| req["decision_reason"].as_str())
        .unwrap_or_default()
        .to_string();
    let rules = req["permission_suggestions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["type"] == "addRules" && s["behavior"] == "allow")
        .flat_map(|s| s["rules"].as_array().into_iter().flatten())
        .filter_map(rule_text)
        .collect();
    Some(Control::Ask(ToolAsk {
        request_id,
        tool,
        input,
        action,
        reason,
        rules,
    }))
}

/// `Bash(npm test *)` from `{toolName, ruleContent}`; just the tool when
/// the rule has no content.
fn rule_text(rule: &Value) -> Option<String> {
    let tool = rule["toolName"].as_str().filter(|s| !s.is_empty())?;
    Some(
        match rule["ruleContent"].as_str().filter(|s| !s.is_empty()) {
            Some(content) => format!("{tool}({content})"),
            None => tool.to_string(),
        },
    )
}

/// The line that answers `ask`. A stopped turn denies and interrupts.
pub fn answer_line(ask: &ToolAsk, answer: PermAnswer) -> String {
    let response = match answer {
        PermAnswer::Allow | PermAnswer::Always => json!({
            "behavior": "allow",
            "updatedInput": ask.input,
        }),
        PermAnswer::Deny => json!({
            "behavior": "deny",
            "message": "The user said no to this step. Don't try it another way; ask them what to do instead.",
        }),
        PermAnswer::Cancel => json!({
            "behavior": "deny",
            "message": "The user stopped the turn.",
            "interrupt": true,
        }),
    };
    success(&ask.request_id, response)
}

/// The line that turns down a control request this app doesn't take.
pub fn refuse_line(request_id: &str) -> String {
    json!({
        "type": "control_response",
        "response": {
            "subtype": "error",
            "request_id": request_id,
            "error": "This app doesn't handle that request.",
        },
    })
    .to_string()
}

fn success(request_id: &str, response: Value) -> String {
    json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": request_id,
            "response": response,
        },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shaped like Claude Code 2.1's line for `touch probe-file`.
    const TOUCH: &str = r#"{"type":"control_request","request_id":"5b8715fc-34ec-4892-bace-d6cd2f60cea9","request":{"subtype":"can_use_tool","tool_name":"Bash","display_name":"Bash","input":{"command":"touch probe-file && ls probe-file","description":"Create probe file and list it"},"description":"Create probe file and list it","permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"touch probe-file *"}],"behavior":"allow","destination":"localSettings"},{"type":"addDirectories","directories":["/home/user/elsewhere"],"destination":"session"}]}}"#;

    fn touch() -> ToolAsk {
        match control(TOUCH) {
            Some(Control::Ask(ask)) => ask,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_tool_request_reads_as_an_ask_with_its_command_and_rule() {
        assert_eq!(
            touch(),
            ToolAsk {
                request_id: "5b8715fc-34ec-4892-bace-d6cd2f60cea9".into(),
                tool: "Bash".into(),
                input: json!({"command": "touch probe-file && ls probe-file", "description": "Create probe file and list it"}),
                action: "touch probe-file && ls probe-file".into(),
                reason: "Create probe file and list it".into(),
                rules: vec!["Bash(touch probe-file *)".into()],
            }
        );
    }

    #[test]
    fn allowing_sends_the_input_back_and_denying_tells_claude_why() {
        let ask = touch();
        let allow: Value = serde_json::from_str(&answer_line(&ask, PermAnswer::Allow)).unwrap();
        assert_eq!(
            allow,
            json!({"type": "control_response", "response": {"subtype": "success", "request_id": "5b8715fc-34ec-4892-bace-d6cd2f60cea9", "response": {"behavior": "allow", "updatedInput": {"command": "touch probe-file && ls probe-file", "description": "Create probe file and list it"}}}})
        );
        let deny: Value = serde_json::from_str(&answer_line(&ask, PermAnswer::Deny)).unwrap();
        assert_eq!(deny["response"]["response"]["behavior"], "deny");
        assert_eq!(deny["response"]["response"].get("interrupt"), None);
        let stop: Value = serde_json::from_str(&answer_line(&ask, PermAnswer::Cancel)).unwrap();
        assert_eq!(stop["response"]["response"]["interrupt"], true);
    }

    #[test]
    fn other_requests_are_refused_and_other_lines_are_not_requests() {
        let hook =
            r#"{"type":"control_request","request_id":"r2","request":{"subtype":"hook_callback"}}"#;
        assert_eq!(
            control(hook),
            Some(Control::Other {
                request_id: "r2".into()
            })
        );
        let refused: Value = serde_json::from_str(&refuse_line("r2")).unwrap();
        assert_eq!(refused["response"]["subtype"], "error");
        assert_eq!(refused["response"]["request_id"], "r2");
        assert_eq!(control(r#"{"type":"result"}"#), None);
        assert_eq!(control("not json"), None);
        assert_eq!(
            control(r#"{"type":"control_request","request":{"subtype":"can_use_tool"}}"#),
            None,
            "no id to answer"
        );
    }

    #[test]
    fn a_rule_without_content_is_the_whole_tool() {
        let line = r#"{"type":"control_request","request_id":"r3","request":{"subtype":"can_use_tool","tool_name":"Write","input":{"file_path":"notes.md"},"permission_suggestions":[{"type":"addRules","behavior":"allow","rules":[{"toolName":"Write"}]},{"type":"addRules","behavior":"deny","rules":[{"toolName":"Bash","ruleContent":"rm *"}]}]}}"#;
        let Some(Control::Ask(ask)) = control(line) else {
            panic!()
        };
        assert_eq!(ask.action, "notes.md");
        assert_eq!(ask.reason, "");
        assert_eq!(ask.rules, vec!["Write".to_string()]);
    }
}
