// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! Read a project's `.claude/settings.json` permissions block. Nothing else.

use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::perm::rules::{parse_rule, Action, Rule};

const MAX_BYTES: u64 = 1_048_576;

pub fn import_claude_json(text: &str) -> Vec<Rule> {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(perms) = value.get("permissions") else {
        return Vec::new();
    };
    let mut rules = Vec::new();
    for (action, key) in [
        (Action::Allow, "allow"),
        (Action::Deny, "deny"),
        (Action::Ask, "ask"),
    ] {
        let Some(entries) = perms.get(key).and_then(Value::as_array) else {
            continue;
        };
        for entry in entries {
            let Some(text) = entry.as_str() else {
                continue;
            };
            if let Ok(rule) = parse_rule(text, action) {
                rules.push(rule);
            }
        }
    }
    rules
}

pub fn load_project(workspace: &Path) -> Option<Vec<Rule>> {
    let claude = workspace.join(".claude");
    let dir_meta = fs::symlink_metadata(&claude).ok()?;
    if !dir_meta.file_type().is_dir() {
        return None;
    }
    let path = claude.join("settings.json");
    let meta = fs::symlink_metadata(&path).ok()?;
    if !meta.file_type().is_file() || meta.len() > MAX_BYTES {
        return None;
    }
    let text = fs::read_to_string(&path).ok()?;
    Some(import_claude_json(&text))
}
