//! Settings → Permissions (shell parity): the saved command rules every
//! project shares and this project's remembered Allow-always grants, in
//! the same files the cabin and the agent's gate read. Deny wins over ask,
//! and ask wins over allow; dangerous commands still ask.

use std::path::Path;

use grokhub_agent::perm::{self, Action, ConfigGuard};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Allow,
    Ask,
    Deny,
}

impl From<Kind> for Action {
    fn from(kind: Kind) -> Self {
        match kind {
            Kind::Allow => Action::Allow,
            Kind::Ask => Action::Ask,
            Kind::Deny => Action::Deny,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleRow {
    /// "allow", "ask" or "deny".
    pub action: &'static str,
    /// The rule as typed, like `Bash(git *)`.
    pub rule: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    /// Saved for every project, in order.
    pub rules: Vec<RuleRow>,
    /// From this project's .claude/settings.json; read only.
    pub imported: Vec<RuleRow>,
    /// Commands remembered with Allow always for this project.
    pub grants: Vec<String>,
}

fn row(rule: &perm::Rule) -> RuleRow {
    RuleRow {
        action: rule.action.as_str(),
        rule: rule.source.clone(),
    }
}

fn page(config_dir: &Path, workspace: &Path) -> Page {
    Page {
        rules: perm::load_rules(config_dir).iter().map(row).collect(),
        imported: perm::load_claude_project(workspace)
            .unwrap_or_default()
            .iter()
            .map(row)
            .collect(),
        grants: perm::load_grants(config_dir, workspace),
    }
}

pub fn load(config_dir: &Path, workspace: &Path) -> Page {
    let _config = ConfigGuard::set(config_dir);
    page(config_dir, workspace)
}

pub fn add_rule(
    config_dir: &Path,
    workspace: &Path,
    rule: &str,
    kind: Kind,
) -> Result<Page, String> {
    let _config = ConfigGuard::set(config_dir);
    let parsed = perm::parse_rule(rule.trim(), kind.into())
        .map_err(|e| format!("That rule could not be read: {e}."))?;
    let mut rules = perm::load_rules(config_dir);
    rules.push(parsed);
    perm::save_rules(config_dir, &rules)?;
    Ok(page(config_dir, workspace))
}

/// Removes the rule at `index`, if it still says `rule` (the cabin may have
/// changed the list since this page loaded).
pub fn remove_rule(
    config_dir: &Path,
    workspace: &Path,
    index: usize,
    rule: &str,
) -> Result<Page, String> {
    let _config = ConfigGuard::set(config_dir);
    let mut rules = perm::load_rules(config_dir);
    if rules.get(index).map(|r| r.source.as_str()) != Some(rule) {
        return Err("That rule changed since this page loaded. Look again.".into());
    }
    rules.remove(index);
    perm::save_rules(config_dir, &rules)?;
    Ok(page(config_dir, workspace))
}

/// Allow always for a command in this project. Dangerous and read-only
/// commands are not stored.
pub fn remember(config_dir: &Path, workspace: &Path, command: &str) -> Result<Page, String> {
    let _config = ConfigGuard::set(config_dir);
    let args = serde_json::json!({ "command": command.trim() }).to_string();
    if !perm::remember_allow_always(workspace, "run_terminal_command", &args)? {
        return Err("That command was not remembered: it is dangerous, read-only or empty.".into());
    }
    Ok(page(config_dir, workspace))
}

pub fn forget(config_dir: &Path, workspace: &Path, command: &str) -> Result<Page, String> {
    let _config = ConfigGuard::set(config_dir);
    perm::remove_grant(workspace, command)?;
    Ok(page(config_dir, workspace))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("gh-shell-perm-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (cfg, ws) = (root.join("cfg"), root.join("ws"));
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::create_dir_all(ws.join(".git")).unwrap();
        (root, cfg, ws)
    }

    fn rule(action: &'static str, rule: &str) -> RuleRow {
        RuleRow {
            action,
            rule: rule.into(),
        }
    }

    #[test]
    fn rules_add_in_order_and_remove_by_place() {
        let (root, cfg, ws) = scratch("rules");
        assert_eq!(
            load(&cfg, &ws),
            Page {
                rules: vec![],
                imported: vec![],
                grants: vec![]
            }
        );
        add_rule(&cfg, &ws, " Bash(git *) ", Kind::Allow).unwrap();
        let page = add_rule(&cfg, &ws, "WebFetch(domain:example.com)", Kind::Deny).unwrap();
        assert_eq!(
            page.rules,
            vec![
                rule("allow", "Bash(git *)"),
                rule("deny", "WebFetch(domain:example.com)")
            ]
        );
        assert_eq!(
            remove_rule(&cfg, &ws, 0, "WebFetch(domain:example.com)"),
            Err("That rule changed since this page loaded. Look again.".to_string())
        );
        let page = remove_rule(&cfg, &ws, 0, "Bash(git *)").unwrap();
        assert_eq!(
            page.rules,
            vec![rule("deny", "WebFetch(domain:example.com)")]
        );
        assert_eq!(perm::load_rules(&cfg).len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_bad_rule_is_refused_in_words() {
        let (root, cfg, ws) = scratch("bad");
        assert_eq!(
            add_rule(&cfg, &ws, "Nope(x)", Kind::Allow),
            Err("That rule could not be read: unknown tool prefix: Nope.".to_string())
        );
        assert_eq!(load(&cfg, &ws).rules, vec![]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn grants_are_per_project_and_dangerous_ones_are_not_kept() {
        let (root, cfg, ws) = scratch("grants");
        let page = remember(&cfg, &ws, "cargo build").unwrap();
        assert_eq!(page.grants, vec!["cargo build".to_string()]);
        let other = root.join("other");
        std::fs::create_dir_all(other.join(".git")).unwrap();
        assert_eq!(load(&cfg, &other).grants, Vec::<String>::new());
        assert_eq!(
            remember(&cfg, &ws, "rm -rf /"),
            Err(
                "That command was not remembered: it is dangerous, read-only or empty.".to_string()
            )
        );
        let page = forget(&cfg, &ws, "cargo build").unwrap();
        assert_eq!(page.grants, Vec::<String>::new());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_projects_claude_settings_show_read_only() {
        let (root, cfg, ws) = scratch("claude");
        std::fs::create_dir_all(ws.join(".claude")).unwrap();
        std::fs::write(
            ws.join(".claude").join("settings.json"),
            r#"{"permissions":{"allow":["Bash(npm test)"],"deny":["Read(.env)"]}}"#,
        )
        .unwrap();
        let page = load(&cfg, &ws);
        assert_eq!(
            page.imported
                .iter()
                .map(|r| (r.action, r.rule.as_str()))
                .collect::<Vec<_>>(),
            vec![("allow", "Bash(npm test)"), ("deny", "Read(.env)")]
        );
        assert_eq!(page.rules, vec![]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
