// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! Native MCP server file. Only `mcpServers` / `mcp_servers` definitions are kept.
//! A credential-looking file name is refused before it is opened.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Map, Value};

use crate::perm::config_dir;

const FILE_NAME: &str = "mcp.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerDef {
    pub transport: TransportDef,
    pub enabled: bool,
    pub startup_timeout: Duration,
    pub tool_timeout: Duration,
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportDef {
    Stdio {
        command: String,
        args: Vec<String>,
        env: BTreeMap<String, String>,
        cwd: Option<PathBuf>,
    },
    Http {
        url: String,
        sse: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub added: Vec<String>,
    pub skipped: Vec<String>,
}

impl ImportReport {
    pub fn summary(&self) -> String {
        if self.added.is_empty() && self.skipped.is_empty() {
            return "No MCP server definitions found".into();
        }
        let mut parts = Vec::new();
        if !self.added.is_empty() {
            parts.push(format!("Imported {}", self.added.join(", ")));
        }
        if !self.skipped.is_empty() {
            parts.push(format!("Kept existing {}", self.skipped.join(", ")));
        }
        parts.join(". ")
    }
}

pub fn config_file() -> PathBuf {
    config_dir().join(FILE_NAME)
}

pub fn load_servers() -> BTreeMap<String, ServerDef> {
    let text = fs::read_to_string(config_file()).unwrap_or_default();
    servers_from_document(&text).unwrap_or_default()
}

pub fn import_documents(docs: &[String]) -> Result<ImportReport, String> {
    let mut incoming = BTreeMap::new();
    for doc in docs {
        for (name, def) in servers_from_document(doc)? {
            incoming.entry(name).or_insert(def);
        }
    }
    merge_servers(incoming)
}

pub fn import_paths(paths: &[PathBuf]) -> Result<ImportReport, String> {
    let mut docs = Vec::new();
    for path in paths {
        docs.push(read_mcp_text(path)?);
    }
    import_documents(&docs)
}

pub fn servers_from_document(text: &str) -> Result<BTreeMap<String, ServerDef>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(BTreeMap::new());
    }
    let value: Value =
        serde_json::from_str(text).map_err(|err| format!("MCP config is not JSON: {err}"))?;
    let Some(root) = value.as_object() else {
        return Ok(BTreeMap::new());
    };
    let block = root
        .get("mcpServers")
        .or_else(|| root.get("mcp_servers"))
        .and_then(|item| item.as_object());
    let Some(block) = block else {
        return Ok(BTreeMap::new());
    };
    let mut out = BTreeMap::new();
    for (name, spec) in block {
        let name = name.trim();
        if name.is_empty() || is_desktop_server(name) {
            continue;
        }
        if let Some(def) = parse_server(spec) {
            out.insert(name.to_string(), def);
        }
    }
    Ok(out)
}

fn merge_servers(incoming: BTreeMap<String, ServerDef>) -> Result<ImportReport, String> {
    let path = config_file();
    let mut existing = load_file_map(&path);
    let mut added = Vec::new();
    let mut skipped = Vec::new();
    for (name, def) in incoming {
        if existing.contains_key(&name) {
            skipped.push(name);
            continue;
        }
        if let Some(spec) = server_to_json(&def) {
            existing.insert(name.clone(), spec);
            added.push(name);
        }
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let body = json!({ "mcpServers": existing });
    let text = serde_json::to_string_pretty(&body).map_err(|err| err.to_string())?;
    fs::write(&path, text).map_err(|err| err.to_string())?;
    crate::mcp::invalidate();
    Ok(ImportReport { added, skipped })
}

fn load_file_map(path: &Path) -> Map<String, Value> {
    let Ok(text) = fs::read_to_string(path) else {
        return Map::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Map::new();
    };
    value
        .get("mcpServers")
        .or_else(|| value.get("mcp_servers"))
        .and_then(|item| item.as_object())
        .cloned()
        .unwrap_or_default()
}

fn server_to_json(def: &ServerDef) -> Option<Value> {
    let mut spec = Map::new();
    match &def.transport {
        TransportDef::Stdio {
            command,
            args,
            env,
            cwd,
        } => {
            spec.insert("command".into(), json!(command));
            if !args.is_empty() {
                spec.insert("args".into(), json!(args));
            }
            if !env.is_empty() {
                spec.insert("env".into(), json!(env));
            }
            if let Some(cwd) = cwd {
                spec.insert("cwd".into(), json!(cwd.display().to_string()));
            }
        }
        TransportDef::Http { url, sse } => {
            spec.insert("url".into(), json!(url));
            spec.insert("type".into(), json!(if *sse { "sse" } else { "http" }));
            if !def.headers.is_empty() {
                spec.insert("headers".into(), json!(def.headers));
            }
        }
    }
    if !def.enabled {
        spec.insert("enabled".into(), json!(false));
    }
    Some(Value::Object(spec))
}

fn parse_server(spec: &Value) -> Option<ServerDef> {
    let spec = spec.as_object()?;
    let kept = keep_server_fields(spec);
    let enabled = kept
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let startup_timeout = secs_field(&kept, "startup_timeout_sec", "startupTimeoutSec", 15);
    let tool_timeout = secs_field(&kept, "tool_timeout_sec", "toolTimeoutSec", 60);
    let mut headers = string_map(kept.get("headers"));
    if let Some(token) = kept
        .get("bearerToken")
        .or_else(|| kept.get("bearer_token"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        headers
            .entry("Authorization".into())
            .or_insert_with(|| format!("Bearer {token}"));
    }
    let kind = kept
        .get("type")
        .or_else(|| kept.get("transport"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let url = kept
        .get("url")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let command = kept
        .get("command")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let sse = matches!(kind.as_str(), "sse" | "sse-legacy" | "legacy-sse");
    let http = sse
        || matches!(
            kind.as_str(),
            "http" | "streamable-http" | "streamable_http"
        );
    let transport = if let Some(url) = url {
        if kind == "stdio" || (!http && !kind.is_empty() && command.is_some()) {
            return None;
        }
        TransportDef::Http { url, sse }
    } else {
        let command = command?;
        TransportDef::Stdio {
            command,
            args: string_list(kept.get("args")),
            env: string_map(kept.get("env")),
            cwd: kept
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
        }
    };
    Some(ServerDef {
        transport,
        enabled,
        startup_timeout,
        tool_timeout,
        headers,
    })
}

fn keep_server_fields(spec: &Map<String, Value>) -> Map<String, Value> {
    const KEYS: &[&str] = &[
        "command",
        "args",
        "env",
        "cwd",
        "url",
        "type",
        "transport",
        "headers",
        "enabled",
        "startup_timeout_sec",
        "startupTimeoutSec",
        "tool_timeout_sec",
        "toolTimeoutSec",
        "bearerToken",
        "bearer_token",
    ];
    let mut out = Map::new();
    for key in KEYS {
        if let Some(value) = spec.get(*key) {
            out.insert((*key).to_string(), value.clone());
        }
    }
    out
}

fn secs_field(spec: &Map<String, Value>, a: &str, b: &str, default: u64) -> Duration {
    let secs = spec
        .get(a)
        .or_else(|| spec.get(b))
        .and_then(|v| v.as_u64())
        .filter(|n| *n > 0)
        .unwrap_or(default);
    Duration::from_secs(secs.min(600))
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn string_map(value: Option<&Value>) -> BTreeMap<String, String> {
    let Some(obj) = value.and_then(|v| v.as_object()) else {
        return BTreeMap::new();
    };
    obj.iter()
        .filter_map(|(k, v)| v.as_str().map(|text| (k.clone(), text.to_string())))
        .collect()
}

/// The cabin's own desktop MCP server. Native chats drive the desktop in-process,
/// behind the desktop switch, Ask, Halt and the lock screen, so this server is never
/// imported or started as an MCP server (that would route around those gates).
pub fn is_desktop_server(name: &str) -> bool {
    name.trim().to_ascii_lowercase().replace('_', "-") == grokhub_core::DESKTOP_MCP_SERVER
}

pub fn read_mcp_text(path: &Path) -> Result<String, String> {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if forbidden_file_name(name) {
        return Err(format!("refusing to read credential file {name}"));
    }
    fs::read_to_string(path).map_err(|err| err.to_string())
}

fn forbidden_file_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == concat!("auth", ".json")
        || lower == concat!("secrets", ".json")
        || lower.contains("credential")
        || lower.ends_with(".pem")
        || lower.ends_with(".key")
}

pub fn target_label(def: &ServerDef) -> String {
    match &def.transport {
        TransportDef::Stdio { command, args, .. } => {
            if args.is_empty() {
                command.clone()
            } else {
                format!("{command} {}", args.join(" "))
            }
        }
        TransportDef::Http { url, sse } => {
            if *sse {
                format!("sse {url}")
            } else {
                format!("http {url}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cabin_desktop_server_is_never_loaded_or_imported() {
        let doc = r#"{"mcpServers":{
            "grokhub-desktop":{"command":"grokhub","args":["--mcp-desktop"]},
            "GrokHub_Desktop":{"command":"grokhub","args":["--mcp-desktop"]},
            "files":{"command":"files-mcp"}
        }}"#;
        let servers = servers_from_document(doc).unwrap();
        assert_eq!(servers.keys().collect::<Vec<_>>(), vec!["files"]);
        assert!(is_desktop_server("grokhub-desktop"));
        assert!(is_desktop_server(" GROKHUB_DESKTOP "));
        assert!(!is_desktop_server("grokhub-desktop-extra"));
    }
}
