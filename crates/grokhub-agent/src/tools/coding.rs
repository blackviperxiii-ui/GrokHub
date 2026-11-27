//! Lean coding tool profile for local models (Order 27 / local-first item 8).
//!
//! A local coding turn (Code tab, or a Task / Repair Brief) sends only the
//! code tools below and a lean fixed prompt. Frontier and Hangout chat-only
//! turns are unchanged. `repo_map` is allowlisted so it appears once that
//! tool is registered; until then schemas simply omit it.

use std::cell::Cell;
use std::path::Path;

use crate::brain::{Brief, Intent};
use crate::client::{ContentPart, InputItem};
use crate::prompt::{lean_template_body, template_body};

use super::set::ToolSet;

thread_local! {
    static FORCE: Cell<Option<bool>> = const { Cell::new(None) };
}

/// Pins [`wants`] for tests that keep a scripted (non-`local:`) client.
#[cfg(test)]
pub(crate) struct ForceGuard(Option<bool>);

#[cfg(test)]
impl ForceGuard {
    pub(crate) fn set(on: Option<bool>) -> Self {
        Self(FORCE.with(|f| f.replace(on)))
    }
}

#[cfg(test)]
impl Drop for ForceGuard {
    fn drop(&mut self) {
        FORCE.with(|f| f.set(self.0));
    }
}

/// Code tools a local coding turn offers, in stable-prefix order.
///
/// Canvas item 8: read, grep, glob, edit, shell, tests, repo_map.
/// `list_dir` stays with the read/find set; `get_command_or_subagent_output`
/// covers reading test command output; edit is `write` + `search_replace`.
pub const CODING_TOOLS: &[&str] = &[
    "read_file",
    "list_dir",
    "grep",
    "glob",
    "get_command_or_subagent_output",
    "repo_map",
    "write",
    "search_replace",
    "run_terminal_command",
];

/// How many of [`CODING_TOOLS`] are registered today.
#[cfg(test)]
pub(crate) const CODING_TOOLS_LIVE: usize = CODING_TOOLS.len();

/// Allowlist for a local coding turn.
pub fn tool_set() -> ToolSet {
    ToolSet::only_strict(CODING_TOOLS.iter().copied())
}

/// True when this turn should use the lean coding profile: a local model on
/// a Code tab session or a Task / Repair Brief.
pub fn wants(session: &str, model: &str, brief: Option<&Brief>) -> bool {
    if let Some(on) = FORCE.with(Cell::get) {
        return on;
    }
    if !crate::route::local::is_local(model) && !crate::route::live::step_on_device() {
        return false;
    }
    if crate::code_guard::is_set(session) {
        return true;
    }
    brief.is_some_and(|b| matches!(b.intent, Intent::Task | Intent::Repair))
}

/// Drop every tool outside [`CODING_TOOLS`] (Order 27 local path).
pub fn filter_tools(tools: &mut Vec<serde_json::Value>) {
    tools.retain(|t| {
        t["name"]
            .as_str()
            .is_some_and(|n| CODING_TOOLS.contains(&n))
    });
}

/// True when this on-device request is a coding turn (Code tab or Task/Repair).
pub fn wants_on_device(session: &str) -> bool {
    if crate::code_guard::is_set(session) {
        return true;
    }
    crate::checkpoint::turn_brief(session)
        .is_some_and(|b| matches!(b.intent, Intent::Task | Intent::Repair))
}

/// Replace the full fixed template in the first system message with the lean
/// one, keeping cabin rules, AGENTS.md and the rest of the compose tail.
pub fn apply_lean_prompt(input: &mut [InputItem]) {
    let full = template_body();
    let lean = lean_template_body();
    let full_trim = full.trim();
    let lean_trim = lean.trim();
    for item in input {
        let InputItem::Message { role, content } = item else {
            continue;
        };
        if role != "system" {
            continue;
        }
        for part in content.iter_mut() {
            let ContentPart::InputText(text) = part else {
                continue;
            };
            if text.starts_with(lean_trim) {
                return;
            }
            if let Some(rest) = text.strip_prefix(full_trim) {
                *text = format!("{lean_trim}{rest}");
                return;
            }
        }
        return;
    }
}

/// Append a ranked tree-sitter repository map to the system message on local
/// coding turns so the model starts with the workspace structure within budget.
pub fn apply_repo_map(
    input: &mut [InputItem],
    workspace: &Path,
    user_text: &str,
    token_budget: usize,
) {
    let (focus_files, query_terms) = crate::repo_map::extract_prompt_hints(user_text);
    let map = crate::repo_map::build(
        workspace,
        &crate::repo_map::RepoMapConfig {
            token_budget,
            focus_files,
            query_terms,
            ..Default::default()
        },
    );
    if map.rendered.is_empty() {
        return;
    }
    for item in input {
        let InputItem::Message { role, content } = item else {
            continue;
        };
        if role != "system" {
            continue;
        }
        for part in content.iter_mut() {
            let ContentPart::InputText(text) = part else {
                continue;
            };
            if text.contains("<repo_map ") {
                return;
            }
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&map.rendered);
            return;
        }
        return;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::Brief;
    use crate::client::{ContentPart, InputItem};
    use serde_json::Value;

    fn brief(intent: Intent) -> Brief {
        Brief {
            intent,
            confidence: 100,
            ..Default::default()
        }
    }

    #[test]
    fn coding_tools_are_the_literal_canvas_set() {
        assert_eq!(
            CODING_TOOLS,
            &[
                "read_file",
                "list_dir",
                "grep",
                "glob",
                "get_command_or_subagent_output",
                "repo_map",
                "write",
                "search_replace",
                "run_terminal_command",
            ]
        );
        assert_eq!(CODING_TOOLS_LIVE, 9);
        assert!(CODING_TOOLS.contains(&"repo_map"));
    }

    #[test]
    fn local_task_and_code_tab_want_lean_chat_does_not() {
        assert!(wants("s", "local:qwen", Some(&brief(Intent::Task))));
        assert!(wants("s", "local:qwen", Some(&brief(Intent::Repair))));
        assert!(!wants("s", "local:qwen", Some(&brief(Intent::Chat))));
        assert!(!wants("s", "local:qwen", Some(&brief(Intent::Research))));
        assert!(!wants("s", "grok-4.7", Some(&brief(Intent::Task))));
        assert!(!wants("s", "local:qwen", None));
        crate::code_guard::set("code-lean", Some(std::sync::Arc::new(|_, _, _| None)));
        assert!(wants("code-lean", "local:devstral", None));
        assert!(!wants("code-lean", "grok-4.7", None));
        crate::code_guard::set("code-lean", None);
    }

    #[test]
    fn lean_prompt_replaces_the_full_template_and_keeps_the_tail() {
        let full = template_body();
        let lean = lean_template_body();
        assert!(lean.len() < full.len());
        let mut input = vec![InputItem::Message {
            role: "system".into(),
            content: vec![ContentPart::InputText(format!(
                "{}\n\nCabin rule.\n\n<agents>\nrules\n</agents>",
                full.trim()
            ))],
        }];
        apply_lean_prompt(&mut input);
        let text = match &input[0] {
            InputItem::Message { content, .. } => match &content[0] {
                ContentPart::InputText(t) => t.clone(),
                _ => panic!("text"),
            },
            _ => panic!("system"),
        };
        assert!(text.starts_with(lean.trim()), "{text}");
        assert!(text.contains("Cabin rule."));
        assert!(text.contains("<agents>\nrules\n</agents>"));
        apply_lean_prompt(&mut input);
        let again = match &input[0] {
            InputItem::Message { content, .. } => match &content[0] {
                ContentPart::InputText(t) => t.clone(),
                _ => panic!("text"),
            },
            _ => panic!("system"),
        };
        assert_eq!(again, text);
    }

    #[test]
    fn apply_repo_map_appends_once_and_respects_budget() {
        let dir = std::env::temp_dir().join(format!("grokhub-coding-rm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            "pub fn target_worker() -> i32 { 7 }\n",
        )
        .unwrap();

        let mut input = vec![InputItem::Message {
            role: "system".into(),
            content: vec![ContentPart::InputText("System intro.".into())],
        }];
        apply_repo_map(&mut input, &dir, "fix target_worker in src/lib.rs", 256);
        let text = match &input[0] {
            InputItem::Message { content, .. } => match &content[0] {
                ContentPart::InputText(t) => t.clone(),
                _ => panic!("text"),
            },
            _ => panic!("system"),
        };
        assert!(text.contains("<repo_map "), "{text}");
        assert!(text.contains("target_worker"), "{text}");

        // Idempotent across turns in the same wire build.
        apply_repo_map(&mut input, &dir, "fix target_worker in src/lib.rs", 256);
        let again = match &input[0] {
            InputItem::Message { content, .. } => match &content[0] {
                ContentPart::InputText(t) => t.clone(),
                _ => panic!("text"),
            },
            _ => panic!("system"),
        };
        assert_eq!(again, text);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn coding_set_allows_only_code_tools() {
        let set = tool_set();
        for name in CODING_TOOLS {
            assert!(set.allows(name), "{name}");
        }
        assert!(!set.allows("web_search"));
        assert!(!set.allows("image_generate"));
        assert!(!set.allows("todo_write"));
        assert!(!set.allows("skill"));
        assert!(!set.allows("tool_search"));
    }

    #[test]
    fn coding_schemas_are_the_live_code_tools_and_smaller_than_full() {
        use crate::gate::{Gate, PermMode};
        use crate::tools::{schemas_for, schemas_for_job};
        let _legacy = crate::tools::registry::LegacyPromptGuard::set();
        let gate = Gate {
            mode: PermMode::Always,
            readonly_session: false,
            attended: true,
            desktop: false,
        };
        let full = schemas_for(&gate);
        let coding = schemas_for_job(&gate, &tool_set());
        let names: Vec<&str> = coding
            .iter()
            .filter_map(|t: &Value| t["name"].as_str())
            .collect();
        assert_eq!(
            names,
            vec![
                "read_file",
                "list_dir",
                "grep",
                "glob",
                "get_command_or_subagent_output",
                "repo_map",
                "write",
                "search_replace",
                "run_terminal_command",
            ]
        );
        assert_eq!(names.len(), CODING_TOOLS_LIVE);
        let full_bytes = serde_json::to_vec(&full).unwrap().len();
        let coding_bytes = serde_json::to_vec(&coding).unwrap().len();
        assert!(
            coding_bytes * 2 < full_bytes,
            "coding payload should be less than half of full: full={full_bytes} coding={coding_bytes}"
        );
    }
}
