//! Grok Build skills, MCP servers, plugins, and marketplace catalog.

use crate::locate::{grok_home, grok_user_stdout_timeout};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct GrokCatalog {
    pub skills: Vec<GrokSkillRow>,
    pub mcp: Vec<GrokMcpRow>,
    pub plugins: Vec<GrokPluginRow>,
    pub workflows: Vec<GrokWorkflowRow>,
}

#[derive(Debug, Clone)]
pub struct GrokSkillRow {
    pub name: String,
    pub description: String,
    pub source: String,
    pub plugin: String,
    pub user_invocable: bool,
}

#[derive(Debug, Clone)]
pub struct GrokMcpRow {
    pub name: String,
    pub enabled: bool,
    pub target: String,
}

#[derive(Debug, Clone)]
pub struct GrokPluginRow {
    pub name: String,
    pub status: String,
    pub enabled: bool,
    pub marketplace: String,
    pub description: String,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct GrokWorkflowRow {
    pub name: String,
    pub source: String,
    pub description: String,
}

pub fn parse_inspect_skills(v: &Value) -> Vec<GrokSkillRow> {
    let Some(arr) = v.get("skills").and_then(|x| x.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|s| {
            let name = s.get("name").and_then(|x| x.as_str())?.trim();
            if name.is_empty() {
                return None;
            }
            let src = s.get("source").cloned().unwrap_or(Value::Null);
            let kind = src
                .get("type")
                .and_then(|x| x.as_str())
                .unwrap_or("user")
                .to_string();
            let plugin = src
                .get("plugin_name")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            Some(GrokSkillRow {
                name: name.to_string(),
                description: s
                    .get("description")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .chars()
                    .take(220)
                    .collect(),
                source: kind,
                plugin,
                user_invocable: s
                    .get("userInvocable")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(true),
            })
        })
        .collect()
}

pub fn parse_mcp_list(text: &str) -> Vec<GrokMcpRow> {
    let Ok(v) = serde_json::from_str::<Value>(text.trim()) else {
        return Vec::new();
    };
    let arr = v.as_array().cloned().unwrap_or_default();
    arr.iter()
        .filter_map(|s| {
            let name = s.get("name").and_then(|x| x.as_str())?.trim();
            if name.is_empty() {
                return None;
            }
            let mut target = s
                .get("target")
                .or_else(|| s.get("command"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if let Some(args) = s.get("args").and_then(|x| x.as_array()) {
                let extra: Vec<&str> = args.iter().filter_map(|x| x.as_str()).collect();
                if !extra.is_empty() {
                    if target.is_empty() {
                        target = extra.join(" ");
                    } else {
                        target = format!("{target} {}", extra.join(" "));
                    }
                }
            }
            Some(GrokMcpRow {
                name: name.to_string(),
                enabled: s.get("enabled").and_then(|x| x.as_bool()).unwrap_or(true),
                target,
            })
        })
        .collect()
}

pub fn parse_plugin_list(text: &str) -> Vec<GrokPluginRow> {
    let Ok(v) = serde_json::from_str::<Value>(text.trim()) else {
        return Vec::new();
    };
    let arr = v.as_array().cloned().unwrap_or_default();
    arr.iter()
        .filter_map(|s| {
            let name = s.get("name").and_then(|x| x.as_str())?.trim();
            if name.is_empty() {
                return None;
            }
            let status = s
                .get("status")
                .and_then(|x| x.as_str())
                .unwrap_or("installed")
                .to_string();
            Some(GrokPluginRow {
                name: name.to_string(),
                status: status.clone(),
                enabled: s
                    .get("enabled")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(status == "installed"),
                marketplace: s
                    .get("marketplace")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                description: s
                    .get("description")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .chars()
                    .take(220)
                    .collect(),
                source: s
                    .get("source")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect()
}

/// Parse `grok models` text (no `--json` on grok 1.0.8).
pub fn parse_models_list(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim().trim_start_matches('*').trim_start_matches('-').trim();
        if line.is_empty() || line.to_ascii_lowercase().starts_with("you are logged") {
            continue;
        }
        if line.to_ascii_lowercase().starts_with("default model")
            || line.to_ascii_lowercase().starts_with("available models")
        {
            continue;
        }
        let id = line
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_end_matches(|c| c == ':' || c == ',')
            .to_string();
        if id.starts_with("grok-") && !out.iter().any(|x| x == &id) {
            out.push(id);
        }
    }
    out
}

pub fn skill_source_label(skill: &GrokSkillRow) -> String {
    match skill.source.as_str() {
        "bundled" => "Built-in".into(),
        "plugin" if !skill.plugin.is_empty() => format!("Plugin · {}", skill.plugin),
        "plugin" => "Plugin".into(),
        other => other.to_string(),
    }
}

pub fn load_grok_catalog(bin: &Path, cwd: &Path) -> Result<GrokCatalog, String> {
    let inspect = grok_user_stdout_timeout(bin, cwd, &["inspect", "--json"], 20)?;
    let inspect_v: Value = serde_json::from_str(inspect.trim()).unwrap_or(Value::Null);
    let mcp_text =
        grok_user_stdout_timeout(bin, cwd, &["mcp", "list", "--json"], 20).unwrap_or_default();
    let plug_text = grok_user_stdout_timeout(
        bin,
        cwd,
        &["plugin", "list", "--json", "--available"],
        45,
    )
    .unwrap_or_default();
    let skills = parse_inspect_skills(&inspect_v);
    let workflows = parse_workflows(&skills, grok_home(), cwd);
    Ok(GrokCatalog {
        skills,
        mcp: parse_mcp_list(&mcp_text),
        plugins: parse_plugin_list(&plug_text),
        workflows,
    })
}

pub fn parse_workflows(skills: &[GrokSkillRow], grok_home: Option<PathBuf>, cwd: &Path) -> Vec<GrokWorkflowRow> {
    let mut out = Vec::new();
    for s in skills {
        let n = s.name.to_ascii_lowercase();
        if n.contains("workflow") || n == "execute-plan" || n == "writing-plans" || n == "executing-plans" {
            out.push(GrokWorkflowRow {
                name: s.name.clone(),
                source: skill_source_label(s),
                description: s.description.clone(),
            });
        }
    }
    for dir in [
        grok_home.map(|h| h.join("workflows")),
        Some(cwd.join(".grok/workflows")),
    ]
    .into_iter()
    .flatten()
    {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("rhai") {
                continue;
            }
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("workflow")
                .to_string();
            if out.iter().any(|w| w.name == name) {
                continue;
            }
            out.push(GrokWorkflowRow {
                name,
                source: dir.display().to_string(),
                description: "Rhai workflow".into(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_inspect_skills_reads_bundled_and_plugin() {
        let v = serde_json::json!({
            "skills": [
                {
                    "name": "create-skill",
                    "description": "Scaffold a Grok skill",
                    "source": {"type": "bundled"},
                    "userInvocable": true
                },
                {
                    "name": "base44-cli",
                    "description": "Base44 CLI",
                    "source": {"type": "plugin", "plugin_name": "base44"},
                    "userInvocable": true
                }
            ]
        });
        let rows = parse_inspect_skills(&v);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "create-skill");
        assert_eq!(skill_source_label(&rows[0]), "Built-in");
        assert_eq!(skill_source_label(&rows[1]), "Plugin · base44");
    }

    #[test]
    fn parse_mcp_and_plugins_from_cli_json() {
        let mcp = parse_mcp_list(
            r#"[{"name":"chrome-devtools","command":"npx","args":["-y","chrome-devtools-mcp@1.6.0"],"enabled":true}]"#,
        );
        assert_eq!(mcp.len(), 1);
        assert!(mcp[0].enabled);
        assert_eq!(mcp[0].name, "chrome-devtools");
        assert!(
            mcp[0].target.contains("chrome-devtools-mcp@1.6.0"),
            "MCP tile must show the real command line: {}",
            mcp[0].target
        );
        let plugs = parse_plugin_list(
            r#"[{"status":"installed","name":"superpowers","marketplace":"xAI Official","source":"https://github.com/obra/superpowers.git"},{"status":"available","name":"vercel","description":"Vercel deploy","marketplace":"xAI Official"}]"#,
        );
        assert_eq!(plugs.len(), 2);
        assert_eq!(plugs[0].status, "installed");
        assert_eq!(plugs[1].status, "available");
        assert!(plugs[1].description.contains("Vercel"));
    }

    #[test]
    fn parse_models_list_reads_grok_cli_text() {
        let text = "You are logged in with grok.com.\n\nDefault model: grok-4.6\n\nAvailable models:\n  * grok-4.6 (default)\n  - grok-4.5\n";
        let rows = parse_models_list(text);
        assert_eq!(rows, vec!["grok-4.6".to_string(), "grok-4.5".to_string()]);
    }

    #[test]
    fn parse_workflows_picks_inspect_skills_and_rhai() {
        let skills = parse_inspect_skills(&serde_json::json!({
            "skills": [
                {"name":"create-workflow","description":"Author a Rhai workflow","source":{"type":"bundled"}},
                {"name":"review","description":"Code review","source":{"type":"bundled"}}
            ]
        }));
        let dir = std::env::temp_dir().join(format!("grokhub-wf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("workflows")).unwrap();
        std::fs::write(dir.join("workflows/nightly.rhai"), "let meta = #{ name: \"nightly\" };").unwrap();
        let rows = parse_workflows(&skills, Some(dir.clone()), Path::new("/tmp"));
        assert!(rows.iter().any(|w| w.name == "create-workflow"), "{rows:?}");
        assert!(rows.iter().any(|w| w.name == "nightly"), "{rows:?}");
        assert!(!rows.iter().any(|w| w.name == "review"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
