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
    /// What Claude Code offers for "Always allow" (allow rules such as
    /// `Bash(npm test *)`, accepting edits, another folder), each set to
    /// last for this Claude Code session only: the app keeps them itself.
    pub always: Vec<Value>,
}

impl ToolAsk {
    /// Whether the app's own gate counts this call as a hard stop (a delete,
    /// a force push, the app's config), read as the app's matching tool:
    /// `Bash` as a command, `Write` and the edit tools as file writes. A hard
    /// stop can only be allowed on this computer, never from a paired chat.
    pub fn is_hard(&self) -> bool {
        let native = match self.tool.as_str() {
            "Bash" => "run_terminal_command",
            "Write" => "write",
            "Edit" | "MultiEdit" | "NotebookEdit" => "search_replace",
            _ => return false,
        };
        crate::harness::hard_class(native, &self.input.to_string()).is_some()
    }
}

/// A `control_request` line: a tool to ask about, or another request this
/// app doesn't take (answered with an error so the turn doesn't hang).
#[derive(Debug, Clone, PartialEq)]
pub enum Control {
    Ask(ToolAsk),
    Other { request_id: String },
}

/// The tools whose "Always allow" may accept every edit in the folder.
const EDIT_TOOLS: [&str; 4] = ["Edit", "Write", "MultiEdit", "NotebookEdit"];

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
    let always = req["permission_suggestions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| match s["type"].as_str() {
            Some("addRules") => s["behavior"] == "allow",
            // Accepting every edit is kept only for an edit itself: for a
            // command (`touch`), it would let much more than it showed.
            Some("setMode") => s["mode"] == "acceptEdits" && EDIT_TOOLS.contains(&tool.as_str()),
            Some("addDirectories") => true,
            _ => false,
        })
        .map(|s| {
            let mut s = s.clone();
            s["destination"] = json!("session");
            s
        })
        .collect();
    Some(Control::Ask(ToolAsk {
        request_id,
        tool,
        input,
        action,
        reason,
        always,
    }))
}

/// The allow rules among kept "Always allow" updates, as `--allowedTools`
/// takes them.
pub fn allowed_rules(always: &[Value]) -> Vec<String> {
    always
        .iter()
        .filter(|u| u["type"] == "addRules")
        .flat_map(|u| u["rules"].as_array().into_iter().flatten())
        .filter_map(rule_text)
        .collect()
}

/// Edits were allowed always: later turns run in `acceptEdits`.
pub fn accepts_edits(always: &[Value]) -> bool {
    always
        .iter()
        .any(|u| u["type"] == "setMode" && u["mode"] == "acceptEdits")
}

/// Folders allowed always, as `--add-dir` takes them.
pub fn added_dirs(always: &[Value]) -> Vec<String> {
    always
        .iter()
        .filter(|u| u["type"] == "addDirectories")
        .flat_map(|u| u["directories"].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
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
        PermAnswer::Allow => json!({
            "behavior": "allow",
            "updatedInput": ask.input,
        }),
        // Applied for the rest of this turn too, not only the next ones.
        PermAnswer::Always => json!({
            "behavior": "allow",
            "updatedInput": ask.input,
            "updatedPermissions": ask.always,
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
    fn a_delete_or_a_force_push_is_a_hard_stop_and_a_touch_is_not() {
        assert!(!touch().is_hard());
        let ask = |tool: &str, input: Value| ToolAsk {
            tool: tool.into(),
            input,
            ..touch()
        };
        assert!(ask("Bash", json!({"command": "rm -rf ~/old"})).is_hard());
        assert!(ask("Bash", json!({"command": "git push --force origin main"})).is_hard());
        assert!(!ask("Bash", json!({"command": "cargo test"})).is_hard());
        assert!(!ask("Read", json!({"file_path": "/home/user/my-app/x.rs"})).is_hard());
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
                always: vec![
                    json!({"type": "addRules", "rules": [{"toolName": "Bash", "ruleContent": "touch probe-file *"}], "behavior": "allow", "destination": "session"}),
                    json!({"type": "addDirectories", "directories": ["/home/user/elsewhere"], "destination": "session"}),
                ],
            }
        );
        let always = touch().always;
        assert_eq!(
            allowed_rules(&always),
            vec!["Bash(touch probe-file *)".to_string()]
        );
        assert_eq!(
            added_dirs(&always),
            vec!["/home/user/elsewhere".to_string()]
        );
        assert!(!accepts_edits(&always));
    }

    #[test]
    fn a_command_never_turns_on_accepting_every_edit() {
        let line = TOUCH.replace(
            r#""permission_suggestions":["#,
            r#""permission_suggestions":[{"type":"setMode","mode":"acceptEdits","destination":"session"},"#,
        );
        let Some(Control::Ask(ask)) = control(&line) else {
            panic!()
        };
        assert!(!accepts_edits(&ask.always), "{:?}", ask.always);
        assert_eq!(
            allowed_rules(&ask.always),
            vec!["Bash(touch probe-file *)".to_string()]
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
        let always: Value = serde_json::from_str(&answer_line(&ask, PermAnswer::Always)).unwrap();
        assert_eq!(always["response"]["response"]["behavior"], "allow");
        assert_eq!(
            always["response"]["response"]["updatedPermissions"],
            json!(ask.always),
            "applies for the rest of this turn"
        );
        assert_eq!(
            allow["response"]["response"].get("updatedPermissions"),
            None
        );
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
    fn an_edit_keeps_accept_edits_and_drops_deny_rules_and_other_modes() {
        let line = r#"{"type":"control_request","request_id":"r3","request":{"subtype":"can_use_tool","tool_name":"Write","input":{"file_path":"notes.md"},"permission_suggestions":[{"type":"setMode","mode":"acceptEdits","destination":"session"},{"type":"setMode","mode":"bypassPermissions","destination":"session"},{"type":"addRules","behavior":"allow","rules":[{"toolName":"Write"}]},{"type":"addRules","behavior":"deny","rules":[{"toolName":"Bash","ruleContent":"rm *"}]}]}}"#;
        let Some(Control::Ask(ask)) = control(line) else {
            panic!()
        };
        assert_eq!(ask.action, "notes.md");
        assert_eq!(ask.reason, "");
        assert_eq!(
            ask.always,
            vec![
                json!({"type": "setMode", "mode": "acceptEdits", "destination": "session"}),
                json!({"type": "addRules", "behavior": "allow", "rules": [{"toolName": "Write"}], "destination": "session"}),
            ]
        );
        assert!(accepts_edits(&ask.always));
        assert_eq!(allowed_rules(&ask.always), vec!["Write".to_string()]);
        assert_eq!(added_dirs(&ask.always), Vec::<String>::new());
    }
}
