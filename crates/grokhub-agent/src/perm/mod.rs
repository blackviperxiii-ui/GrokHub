// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! Permission engine. Deny beats ask, and ask beats allow.
//! The engine only loosens gate v0 when an explicit allow rule matches a
//! non-dangerous command, or when every segment is on the read-only list.
//! Dangerous commands still prompt. Unattended mode still denies everything
//! else. Remembered grants apply only while someone is there to have approved them.

mod claude;
mod matchers;
mod risk;
mod rules;
mod split;
mod store;

use std::path::Path;

use crate::gate::{self, Decision, Gate};

pub use claude::{import_claude_json, load_project as load_claude_project};
pub use rules::{parse_rule, Action, PatMode, Rule, RuleParseError, Tool};
pub use split::{analyze, peeled_primary, Facts, Seg};

pub(crate) use risk::{git_words_are_read_only_query, git_words_have_unsafe_query_option};
pub use store::{
    add_grant_at, config_dir, load_all_grants, load_grants, load_rules, project_key,
    remember_grant, remove_grant, save_rules, ConfigGuard,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    pub rules: Vec<Rule>,
    pub grants: Vec<String>,
}

impl Policy {
    pub fn empty() -> Self {
        Self {
            rules: Vec::new(),
            grants: Vec::new(),
        }
    }

    /// Saved rules, the project's `.claude/settings.json` permissions, and this project's grants.
    pub fn load(workspace: &Path) -> Self {
        let dir = store::config_dir();
        let mut rules = store::load_rules(&dir);
        if let Some(imported) = claude::load_project(workspace) {
            rules.extend(imported);
        }
        let grants = store::load_grants(&dir, workspace);
        Self { rules, grants }
    }
}

/// Overlay `base` (gate v0) with the engine. `base` is returned whenever the
/// engine does not have a stricter answer or an explicit allow.
pub fn govern(
    base: Decision,
    gate: &Gate,
    name: &str,
    arguments: &str,
    workspace: &Path,
    policy: &Policy,
) -> Decision {
    if !gate::is_readonly(name)
        && (gate.readonly_session || gate::is_desktop(name) || !gate::is_known(name))
    {
        return base;
    }
    if name == "monitor" {
        if let Some(command) = command_arg(arguments) {
            let facts = split::analyze(&command);
            let texts = bash_texts(&command, &facts);
            if rule_hit(policy, Action::Deny, Tool::Bash, &texts) {
                return Decision::Refuse(gate::unattended_deny(name));
            }
        }
        return base;
    }
    let Some(kind) = tool_kind(name) else {
        return base;
    };
    let paths = collect_paths(name, arguments);
    if paths
        .iter()
        .any(|path| matchers::path_escapes(workspace, path))
    {
        return Decision::Refuse("path escapes the workspace".into());
    }
    if kind == Tool::Bash {
        let command = command_arg(arguments).unwrap_or_default();
        let facts = split::analyze(&command);
        let texts = bash_texts(&command, &facts);
        if rule_hit(policy, Action::Deny, kind, &texts) {
            return Decision::Refuse(gate::unattended_deny(name));
        }
        if facts.segments.is_none() || facts.dangerous {
            return prompt(gate, name);
        }
        if rule_hit(policy, Action::Ask, kind, &texts) {
            return prompt(gate, name);
        }
        if allow_bash(policy, &facts) {
            return Decision::Run;
        }
        if gate.attended && grant_covers(policy, &command, &facts) {
            return Decision::Run;
        }
        if readonly_shell(&facts) {
            return Decision::Run;
        }
        return base;
    }
    if rule_hit_paths(policy, Action::Deny, kind, workspace, &paths) {
        return Decision::Refuse(gate::unattended_deny(name));
    }
    if rule_hit_paths(policy, Action::Ask, kind, workspace, &paths) {
        return prompt(gate, name);
    }
    if allow_paths(policy, kind, workspace, &paths) {
        return Decision::Run;
    }
    base
}

/// Persist Allow-always for a shell call. Dangerous, unsplittable, and
/// read-only commands are not stored. Returns whether a grant was written.
pub fn remember_allow_always(
    workspace: &Path,
    name: &str,
    arguments: &str,
) -> Result<bool, String> {
    if name != "run_terminal_command" {
        return Ok(false);
    }
    let Some(command) = command_arg(arguments) else {
        return Ok(false);
    };
    let facts = split::analyze(&command);
    let Some(segs) = facts.segments else {
        return Ok(false);
    };
    if facts.dangerous || segs.iter().any(|seg| seg.dangerous || !seg.eligible) {
        return Ok(false);
    }
    let mut wrote = false;
    for seg in segs {
        if seg.readonly || seg.command.trim().is_empty() {
            continue;
        }
        store::remember_grant(workspace, &seg.command)?;
        wrote = true;
    }
    Ok(wrote)
}

pub fn mcp_matches(rule: &Rule, tool_name: &str) -> bool {
    matchers::tool_reaches(rule.tool, Tool::Mcp)
        && matchers::rule_matches_text(rule, tool_name, rule.action != Action::Allow)
}

pub fn webfetch_matches(rule: &Rule, url: &str) -> bool {
    if !matchers::tool_reaches(rule.tool, Tool::WebFetch) {
        return false;
    }
    match rule.pattern.as_deref() {
        None | Some("*") | Some("") => true,
        Some(pattern) if rule.mode == PatMode::Domain => matchers::domain_matches(pattern, url),
        Some(pattern) => {
            matchers::domain_matches(pattern, url) || matchers::glob_match(pattern, url, false)
        }
    }
}

/// An explicit ask rule matched. Auto-review must not override it.
pub(crate) fn explicit_ask(policy: &Policy, name: &str, arguments: &str, workspace: &Path) -> bool {
    if crate::mcp::is_mcp_call(name, arguments) {
        return crate::mcp::has_ask_rule(policy, name, arguments);
    }
    let Some(kind) = tool_kind(name) else {
        return false;
    };
    if kind == Tool::Bash {
        let command = command_arg(arguments).unwrap_or_default();
        let facts = split::analyze(&command);
        let texts = bash_texts(&command, &facts);
        return rule_hit(policy, Action::Ask, kind, &texts);
    }
    let paths = collect_paths(name, arguments);
    rule_hit_paths(policy, Action::Ask, kind, workspace, &paths)
}

fn prompt(gate: &Gate, name: &str) -> Decision {
    if gate.attended {
        Decision::Ask
    } else {
        Decision::Refuse(gate::unattended_deny(name))
    }
}

fn tool_kind(name: &str) -> Option<Tool> {
    match name {
        "run_terminal_command" => Some(Tool::Bash),
        "read_file" | "list_dir" => Some(Tool::Read),
        "grep" | "glob" => Some(Tool::Grep),
        "write" | "search_replace" => Some(Tool::Edit),
        _ => None,
    }
}

fn command_arg(arguments: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(arguments).ok()?;
    let text = value.get("command")?.as_str()?.trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn collect_paths(name: &str, arguments: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(arguments) else {
        return Vec::new();
    };
    let keys: &[&str] = match name {
        "read_file" => &["target_file"],
        "list_dir" | "grep" | "glob" => &["path"],
        "write" => &["path", "file_path", "target_file"],
        "search_replace" => &["file_path", "path", "target_file"],
        _ => &[],
    };
    let mut paths = Vec::new();
    for key in keys {
        if let Some(text) = value.get(*key).and_then(|item| item.as_str()) {
            let text = text.trim();
            if !text.is_empty() && text != "." && !paths.iter().any(|have| have == text) {
                paths.push(text.to_string());
            }
        }
    }
    if name == "glob" {
        if let Some(pattern) = value.get("pattern").and_then(|item| item.as_str()) {
            if pattern_needs_confine(pattern) && !paths.iter().any(|have| have == pattern) {
                paths.push(pattern.to_string());
            }
        }
    }
    paths
}

fn pattern_needs_confine(pattern: &str) -> bool {
    let pattern = pattern.trim().replace('\\', "/");
    pattern.starts_with('~')
        || pattern.starts_with('/')
        || pattern.split('/').any(|part| part == "..")
        || (pattern.len() >= 3
            && pattern.as_bytes().get(1) == Some(&b':')
            && pattern.as_bytes()[0].is_ascii_alphabetic()
            && pattern.as_bytes().get(2) == Some(&b'/'))
}

fn bash_texts(raw: &str, facts: &Facts) -> Vec<String> {
    let mut texts = vec![raw.trim().to_string()];
    if let Some(segs) = &facts.segments {
        for seg in segs {
            push_unique(&mut texts, seg.command.clone());
        }
    }
    for probe in &facts.probes {
        push_unique(&mut texts, probe.clone());
    }
    texts
}

fn push_unique(texts: &mut Vec<String>, text: String) {
    if !text.is_empty() && !texts.iter().any(|have| have == &text) {
        texts.push(text);
    }
}

fn rule_hit(policy: &Policy, action: Action, kind: Tool, texts: &[String]) -> bool {
    let wide = action != Action::Allow;
    policy.rules.iter().any(|rule| {
        rule.action == action
            && matchers::tool_reaches(rule.tool, kind)
            && texts.iter().any(|text| match_rule_text(rule, text, wide))
    })
}

fn match_rule_text(rule: &Rule, text: &str, wide: bool) -> bool {
    match rule.pattern.as_deref() {
        None | Some("*") | Some("") => true,
        Some(pattern) if rule.mode == PatMode::Domain => matchers::domain_matches(pattern, text),
        Some(_) => matchers::rule_matches_text(rule, text, wide),
    }
}

fn rule_hit_paths(
    policy: &Policy,
    action: Action,
    kind: Tool,
    workspace: &Path,
    paths: &[String],
) -> bool {
    policy.rules.iter().any(|rule| {
        rule.action == action
            && matchers::tool_reaches(rule.tool, kind)
            && path_rule_hit(rule, workspace, paths)
    })
}

fn path_rule_hit(rule: &Rule, workspace: &Path, paths: &[String]) -> bool {
    if rule.mode == PatMode::Domain {
        return false;
    }
    match rule.pattern.as_deref() {
        None | Some("*") | Some("") => true,
        Some(_) => paths
            .iter()
            .any(|path| matchers::path_rule_matches(rule, workspace, path)),
    }
}

fn allow_bash(policy: &Policy, facts: &Facts) -> bool {
    let Some(segs) = &facts.segments else {
        return false;
    };
    if segs.is_empty() || facts.dangerous || segs.iter().any(|seg| seg.dangerous || !seg.eligible) {
        return false;
    }
    segs.iter().all(|seg| {
        policy.rules.iter().any(|rule| {
            rule.action == Action::Allow
                && matchers::tool_reaches(rule.tool, Tool::Bash)
                && rule.mode != PatMode::Domain
                && matchers::rule_matches_text(rule, &seg.command, false)
        })
    })
}

fn grant_covers(policy: &Policy, raw: &str, facts: &Facts) -> bool {
    let Some(segs) = &facts.segments else {
        return false;
    };
    if segs.is_empty() || facts.dangerous || segs.iter().any(|seg| seg.dangerous || !seg.eligible) {
        return false;
    }
    if policy.grants.iter().any(|grant| grant.trim() == raw.trim()) {
        return true;
    }
    segs.iter().all(|seg| {
        seg.readonly
            || policy.grants.iter().any(|grant| {
                let grant = grant.trim();
                grant == seg.command
                    || split::peeled_primary(grant).as_deref() == Some(seg.command.as_str())
            })
    })
}

fn readonly_shell(facts: &Facts) -> bool {
    facts.segments.as_ref().is_some_and(|segs| {
        !segs.is_empty()
            && !facts.dangerous
            && segs
                .iter()
                .all(|seg| seg.readonly && seg.eligible && !seg.dangerous)
    })
}

fn allow_paths(policy: &Policy, kind: Tool, workspace: &Path, paths: &[String]) -> bool {
    if paths.is_empty() {
        return policy.rules.iter().any(|rule| {
            rule.action == Action::Allow
                && matchers::tool_reaches(rule.tool, kind)
                && rule.mode != PatMode::Domain
                && matches!(rule.pattern.as_deref(), None | Some("*") | Some(""))
        });
    }
    paths.iter().all(|path| {
        !matchers::path_escapes(workspace, path)
            && policy.rules.iter().any(|rule| {
                rule.action == Action::Allow
                    && matchers::tool_reaches(rule.tool, kind)
                    && matchers::path_rule_matches(rule, workspace, path)
            })
    })
}

#[cfg(test)]
mod tests;
