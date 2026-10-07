//! Grok Build skills, MCP servers, plugins, and marketplace catalog.

use crate::locate::{grok_home, grok_user_stdout_timeout};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct GrokCatalog {
    pub skills: Vec<GrokSkillRow>,
    pub mcp: Vec<GrokMcpRow>,
    pub plugins: Vec<GrokPluginRow>,
    pub workflows: Vec<GrokWorkflowRow>,
    pub hooks: Vec<GrokHookRow>,
    /// `None` when inspect omitted `projectTrusted`. The untrusted note stays off.
    pub project_trusted: Option<bool>,
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

/// Where a hook file was loaded from. Unknown `source.type` values are `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookOrigin {
    User,
    Project,
    Other,
}

impl HookOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrokHookRow {
    pub event: String,
    pub hook_type: String,
    pub target: String,
    pub origin: HookOrigin,
    pub path: String,
    pub matcher: Option<String>,
}

/// One MCP row after `grok mcp doctor --json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpDoctorStatus {
    Connected,
    NeedsSignIn,
    Error(String),
}

const MCP_DOCTOR_DETAIL_MAX: usize = 120;

impl McpDoctorStatus {
    /// `Connected`, `Needs sign-in`, or `Error: ` plus at most 120 characters.
    pub fn label(&self) -> String {
        match self {
            Self::Connected => "Connected".to_string(),
            Self::NeedsSignIn => "Needs sign-in".to_string(),
            Self::Error(detail) => {
                format!("Error: {}", clip_chars(detail.trim(), MCP_DOCTOR_DETAIL_MAX))
            }
        }
    }
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

pub fn parse_inspect_hooks(v: &Value) -> Vec<GrokHookRow> {
    let Some(arr) = v.get("hooks").and_then(|x| x.as_array()) else {
        return Vec::new();
    };
    arr.iter().filter_map(hook_row).collect()
}

fn hook_row(h: &Value) -> Option<GrokHookRow> {
    let event = h.get("event").and_then(|x| x.as_str()).unwrap_or("").trim();
    if event.is_empty() {
        return None;
    }
    let src = h.get("source").cloned().unwrap_or(Value::Null);
    let kind = src.get("type").and_then(|x| x.as_str()).unwrap_or("");
    let origin = match kind {
        "user" => HookOrigin::User,
        "project" => HookOrigin::Project,
        _ => HookOrigin::Other,
    };
    let matcher = match h.get("matcher") {
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        _ => None,
    };
    Some(GrokHookRow {
        event: event.to_string(),
        hook_type: h
            .get("hookType")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        target: h
            .get("target")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        origin,
        path: src
            .get("path")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        matcher,
    })
}

pub fn parse_inspect_project_trusted(v: &Value) -> Option<bool> {
    v.get("projectTrusted").and_then(|x| x.as_bool())
}

/// `grok mcp doctor --json`. `None` when `text` is not a JSON object with a
/// `servers` array (leading or trailing noise included). A failed check whose
/// detail contains "Auth required" is sign-in. `healthy: true` is connected.
pub fn parse_mcp_doctor(text: &str) -> Option<HashMap<String, McpDoctorStatus>> {
    let v: Value = serde_json::from_str(text.trim()).ok()?;
    let servers = v.get("servers")?.as_array()?;
    let mut out = HashMap::new();
    for server in servers {
        let Some(name) = server
            .get("name")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|n| !n.is_empty())
        else {
            continue;
        };
        out.insert(name.to_string(), mcp_doctor_status(server));
    }
    Some(out)
}

fn mcp_doctor_status(server: &Value) -> McpDoctorStatus {
    if server.get("healthy").and_then(|x| x.as_bool()) == Some(true) {
        return McpDoctorStatus::Connected;
    }
    let mut auth = false;
    let mut first_fail: Option<String> = None;
    if let Some(checks) = server.get("checks").and_then(|x| x.as_array()) {
        for check in checks {
            if check.get("passed").and_then(|x| x.as_bool()).unwrap_or(true) {
                continue;
            }
            let detail = check
                .get("detail")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .trim();
            if detail.to_ascii_lowercase().contains("auth required") {
                auth = true;
            }
            if first_fail.is_none() && !detail.is_empty() {
                first_fail = Some(clip_chars(detail, MCP_DOCTOR_DETAIL_MAX));
            }
        }
    }
    if auth {
        return McpDoctorStatus::NeedsSignIn;
    }
    McpDoctorStatus::Error(first_fail.unwrap_or_else(|| "doctor reported unhealthy".to_string()))
}

fn clip_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
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
/// 1.0.14 can list a different id per reasoning-effort level on the same line.
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
        for tok in line.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',' || c == ':')
        {
            let id = tok.trim().trim_end_matches(['*', '-']);
            if id.starts_with("grok-") && !out.iter().any(|x| x == id) {
                out.push(id.to_string());
            }
        }
    }
    out
}

/// One-line 1.0.14 inspect notes (Claude bypass lock is advisory, not enforced).
pub fn inspect_advisory(v: &Value) -> String {
    let ver = v
        .get("grokVersion")
        .or_else(|| v.get("version"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim();
    let channel = v
        .get("channel")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim();
    let advisory = v
        .pointer("/permissions/claudeBypassLockAdvisory")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let mut parts = Vec::new();
    if !ver.is_empty() {
        parts.push(if channel.is_empty() {
            format!("Grok Build {ver}")
        } else {
            format!("Grok Build {ver} ({channel})")
        });
    }
    if advisory {
        parts.push("Claude bypass lock is advisory, not enforced".into());
    }
    parts.join(" · ")
}

pub fn skill_source_label(skill: &GrokSkillRow) -> String {
    match skill.source.as_str() {
        "bundled" => "Built-in".into(),
        "plugin" if !skill.plugin.is_empty() => format!("Plugin · {}", skill.plugin),
        "plugin" => "Plugin".into(),
        other => other.to_string(),
    }
}

/// MCP startup (chrome-devtools is 90s in `~/.grok`) must finish before the
/// Connectors page decides the list is empty.
const CATALOG_CMD_SECS: u64 = 120;

pub fn load_grok_catalog(bin: &Path, cwd: &Path) -> Result<GrokCatalog, String> {
    // Overlap the three commands. Serial 3×120s kept Skills on Loading… past a
    // short QA wait. Each command still has CATALOG_CMD_SECS (chrome-devtools).
    let bin_mcp = bin.to_path_buf();
    let cwd_mcp = cwd.to_path_buf();
    let mcp_job = std::thread::spawn(move || {
        grok_user_stdout_timeout(
            &bin_mcp,
            &cwd_mcp,
            &["mcp", "list", "--json"],
            CATALOG_CMD_SECS,
        )
        .unwrap_or_default()
    });
    let bin_plug = bin.to_path_buf();
    let cwd_plug = cwd.to_path_buf();
    let plug_job = std::thread::spawn(move || {
        grok_user_stdout_timeout(
            &bin_plug,
            &cwd_plug,
            &["plugin", "list", "--json", "--available"],
            CATALOG_CMD_SECS,
        )
        .unwrap_or_default()
    });
    let inspect_res = grok_user_stdout_timeout(bin, cwd, &["inspect", "--json"], CATALOG_CMD_SECS);
    let mcp_text = mcp_job.join().unwrap_or_default();
    let plug_text = plug_job.join().unwrap_or_default();
    let inspect = inspect_res?;
    let inspect_v: Value = serde_json::from_str(inspect.trim()).unwrap_or(Value::Null);
    let skills = parse_inspect_skills(&inspect_v);
    let hooks = parse_inspect_hooks(&inspect_v);
    let project_trusted = parse_inspect_project_trusted(&inspect_v);
    let workflows = parse_workflows(&skills, grok_home(), cwd);
    Ok(GrokCatalog {
        skills,
        mcp: parse_mcp_list(&mcp_text),
        plugins: parse_plugin_list(&plug_text),
        workflows,
        hooks,
        project_trusted,
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
        let effort = parse_models_list(
            "Available models:\n  * grok-4.6 (default)\n    high: grok-4.6-reasoning\n  - grok-4.5\n",
        );
        assert!(effort.contains(&"grok-4.6".into()), "{effort:?}");
        assert!(effort.contains(&"grok-4.6-reasoning".into()), "{effort:?}");
        assert!(effort.contains(&"grok-4.5".into()), "{effort:?}");
        let note = inspect_advisory(&serde_json::json!({
            "grokVersion": "1.0.14",
            "channel": "alpha",
            "permissions": { "claudeBypassLockAdvisory": true }
        }));
        assert!(note.contains("1.0.14"), "{note}");
        assert!(note.contains("advisory"), "{note}");
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

    #[test]
    fn parse_inspect_hooks_reads_user_null_project_and_other() {
        let fixture = serde_json::json!({
            "event": "pre_tool_use",
            "hookType": "command",
            "target": "echo hi",
            "source": {"type": "user", "path": "~/.grok/hooks"},
            "matcher": "Bash"
        });
        let null_matcher = serde_json::json!({
            "event": "post_tool_use",
            "hookType": "http",
            "target": "https://example.test/hook",
            "source": {"type": "user", "path": "~/.grok/hooks"},
            "matcher": null
        });
        let v = serde_json::json!({
            "projectTrusted": false,
            "hooks": [
                fixture,
                null_matcher,
                {
                    "event": "session_start",
                    "hookType": "command",
                    "target": "echo project",
                    "source": {"type": "project", "path": ".grok/hooks"},
                    "matcher": "Edit"
                },
                {
                    "event": "stop",
                    "hookType": "command",
                    "target": "echo other",
                    "source": {"type": "plugin", "path": "/tmp/x"},
                    "matcher": "Read"
                }
            ]
        });
        let rows = parse_inspect_hooks(&v);
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].event, "pre_tool_use");
        assert_eq!(rows[0].hook_type, "command");
        assert_eq!(rows[0].target, "echo hi");
        assert_eq!(rows[0].origin, HookOrigin::User);
        assert_eq!(rows[0].path, "~/.grok/hooks");
        assert_eq!(rows[0].matcher.as_deref(), Some("Bash"));
        assert_eq!(rows[1].hook_type, "http");
        assert_eq!(rows[1].matcher, None);
        assert_eq!(rows[1].origin, HookOrigin::User);
        assert_eq!(rows[2].origin, HookOrigin::Project);
        assert_eq!(rows[2].path, ".grok/hooks");
        assert_eq!(rows[3].origin, HookOrigin::Other);
        assert_eq!(parse_inspect_project_trusted(&v), Some(false));
        assert_eq!(
            parse_inspect_project_trusted(&serde_json::json!({"projectTrusted": true})),
            Some(true)
        );
        assert_eq!(parse_inspect_project_trusted(&serde_json::json!({})), None);
        assert!(parse_inspect_hooks(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn load_grok_catalog_reads_hooks_from_the_same_inspect() {
        let src = include_str!("catalog.rs");
        let load = src
            .split("pub fn load_grok_catalog(")
            .nth(1)
            .and_then(|s| s.split("pub fn parse_workflows(").next())
            .expect("load_grok_catalog");
        assert_eq!(
            load.matches("\"inspect\"").count(),
            1,
            "hooks must come from the inspect call already made: {load}"
        );
        assert!(load.contains("parse_inspect_hooks"), "{load}");
        assert!(load.contains("parse_inspect_project_trusted"), "{load}");
        assert_eq!(
            load.matches("std::thread::spawn").count(),
            2,
            "mcp list and plugin list must overlap inspect: {load}"
        );
        let first_spawn = load.find("std::thread::spawn").expect("spawn");
        let inspect_at = load.find("\"inspect\"").expect("inspect");
        let mcp_join = load.find("mcp_job.join()").expect("join mcp");
        assert!(
            first_spawn < inspect_at && inspect_at < mcp_join,
            "inspect must start before either join, so the three commands overlap: {load}"
        );
    }

    #[test]
    fn parse_mcp_doctor_maps_healthy_auth_required_and_errors() {
        let healthy = r#"{"name":"fs","transport":"stdio","target":"npx -y @modelcontextprotocol/server-filesystem /tmp","source":"config","checks":[{"label":"server started","passed":true,"detail":"0.1s"},{"label":"handshake","passed":true,"detail":"ok"}],"healthy":true}"#;
        let auth = r#"{"sources":[],"servers":[{"name":"linear","transport":"http","target":"https://mcp.linear.app/mcp","source":"config","checks":[{"label":"server started","passed":true,"detail":"0.2s"},{"label":"handshake failed","passed":false,"detail":"Send message error Transport [rmcp::transport::worker::WorkerTransport<rmcp::transport::streamable_http_client::StreamableHttpClientWorker<xai_grok_mcp::mcp_http_client::McpHttpClient<rmcp::transport::auth::AuthClient<reqwest::async_impl::client::Client>>>>] error: Auth required, when send initialize request","hint":"check server logs"}],"healthy":false}],"healthy_count":0,"failing_count":1}"#;
        let healthy_doc = format!(r#"{{"servers":[{healthy}]}}"#);
        let map = parse_mcp_doctor(&healthy_doc).expect("healthy json");
        assert_eq!(map["fs"].label(), "Connected");
        let auth_map = parse_mcp_doctor(auth).expect("auth json");
        assert_eq!(auth_map["linear"], McpDoctorStatus::NeedsSignIn);
        assert_eq!(auth_map["linear"].label(), "Needs sign-in");

        let shouted = serde_json::json!({
            "servers": [{
                "name": "linear",
                "healthy": false,
                "checks": [{"label": "handshake failed", "passed": false, "detail": "AUTH REQUIRED"}]
            }]
        });
        let shouted_map = parse_mcp_doctor(&shouted.to_string()).unwrap();
        assert_eq!(shouted_map["linear"].label(), "Needs sign-in");

        let refused = serde_json::json!({
            "servers": [{
                "name": "db",
                "healthy": false,
                "checks": [{"label": "connect", "passed": false, "detail": "connection refused"}]
            }]
        });
        let refused_map = parse_mcp_doctor(&refused.to_string()).unwrap();
        assert_eq!(refused_map["db"].label(), "Error: connection refused");

        let long = "你".repeat(180);
        let clipped = serde_json::json!({
            "servers": [{
                "name": "bad",
                "healthy": false,
                "checks": [{"label": "connect", "passed": false, "detail": long}]
            }]
        });
        let clipped_map = parse_mcp_doctor(&clipped.to_string()).unwrap();
        let label = clipped_map["bad"].label();
        let shown = label.strip_prefix("Error: ").expect("error prefix");
        assert_eq!(shown.chars().count(), 120);
        assert!(shown.chars().all(|c| c == '你'), "{shown}");

        let bare = serde_json::json!({
            "servers": [{
                "name": "z",
                "healthy": false,
                "checks": [{"label": "server started", "passed": true, "detail": "0.1s"}]
            }]
        });
        let bare_map = parse_mcp_doctor(&bare.to_string()).unwrap();
        assert_eq!(bare_map["z"].label(), "Error: doctor reported unhealthy");
        let no_checks = serde_json::json!({"servers": [{"name": "z", "healthy": false}]});
        assert_eq!(
            parse_mcp_doctor(&no_checks.to_string()).unwrap()["z"].label(),
            "Error: doctor reported unhealthy"
        );

        let auth_later = serde_json::json!({
            "servers": [{
                "name": "linear",
                "healthy": false,
                "checks": [
                    {"passed": false, "detail": "connection refused"},
                    {"passed": false, "detail": "Auth required, when send initialize request"}
                ]
            }]
        });
        assert_eq!(
            parse_mcp_doctor(&auth_later.to_string()).unwrap()["linear"].label(),
            "Needs sign-in"
        );
        let auth_on_pass = serde_json::json!({
            "servers": [{
                "name": "db",
                "healthy": false,
                "checks": [
                    {"passed": true, "detail": "Auth required"},
                    {"passed": false, "detail": "connection refused"}
                ]
            }]
        });
        assert_eq!(
            parse_mcp_doctor(&auth_on_pass.to_string()).unwrap()["db"].label(),
            "Error: connection refused"
        );

        assert!(parse_mcp_doctor("not json").is_none());
        assert!(parse_mcp_doctor("").is_none());
        assert!(parse_mcp_doctor("{\"servers\":1}").is_none());
        assert!(parse_mcp_doctor("ok\n{\"servers\":[]}").is_none());
        assert!(parse_mcp_doctor("{\"servers\":[]}\ntrailing").is_none());
        assert!(parse_mcp_doctor("  {\"servers\":[]}  ").unwrap().is_empty());
    }
}
