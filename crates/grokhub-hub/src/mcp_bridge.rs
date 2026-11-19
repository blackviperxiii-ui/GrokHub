//! Opt-in `--mcp-bridge` stdio MCP server for local IDE clients (Claude Code,
//! Cursor). Clean-room inspiration from OpenClaw's tools-serve pattern only —
//! no network listen, no copied code.
//!
//! Tools are read-only:
//! - `memory_search`: unsealed AMR nodes (`nodes/<id>.md`) only
//! - `automation_status`: `automations.json` status
//!
//! `.sealed` nodes are never opened or returned ([`at_rest`] denial). The
//! store is opened without a sealer, and this module only reads `.md` files.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use grokhub_core::amr::{AmrStore, Node};
use grokhub_core::automation::Automation;
use serde_json::{json, Value};

/// MCP server name. Clients see tools as `grokhub-bridge__<tool>`.
pub const BRIDGE_MCP_SERVER: &str = "grokhub-bridge";

const PROTOCOLS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const LINE_CAP: usize = 1 << 20;
const SEARCH_CAP: usize = 20;
const AUTO_STORE_CAP: u64 = 2 * 1024 * 1024;

const SEALED_DENIED: &str = "Sealed memory nodes are not available over the MCP bridge.";

/// One JSON-RPC request line in, at most one reply line out.
pub struct BridgeServer<'a> {
    config_dir: &'a Path,
}

impl<'a> BridgeServer<'a> {
    pub fn new(config_dir: &'a Path) -> Self {
        Self { config_dir }
    }

    pub fn handle_line(&self, line: &str) -> Option<String> {
        let v: Value = match serde_json::from_str(line.trim()) {
            Ok(v) => v,
            Err(_) => return Some(rpc_error(&Value::Null, -32700, "Parse error")),
        };
        let id = v.get("id").cloned().filter(|i| !i.is_null())?;
        let method = v.get("method").and_then(|m| m.as_str())?;
        let params = v.get("params").cloned().unwrap_or(Value::Null);
        Some(match method {
            "initialize" => {
                let asked = params
                    .get("protocolVersion")
                    .and_then(|p| p.as_str())
                    .unwrap_or("");
                let version = if PROTOCOLS.contains(&asked) {
                    asked
                } else {
                    PROTOCOLS[0]
                };
                rpc_result(
                    &id,
                    json!({
                        "protocolVersion": version,
                        "capabilities": { "tools": { "listChanged": false } },
                        "serverInfo": {
                            "name": BRIDGE_MCP_SERVER,
                            "version": env!("CARGO_PKG_VERSION"),
                        },
                    }),
                )
            }
            "ping" => rpc_result(&id, json!({})),
            "tools/list" => rpc_result(&id, json!({ "tools": bridge_tools() })),
            "tools/call" => {
                let tool = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let args = params
                    .get("arguments")
                    .cloned()
                    .filter(Value::is_object)
                    .unwrap_or_else(|| json!({}));
                let (ok, text) = self.call(tool, &args);
                tool_reply(&id, ok, &text)
            }
            _ => rpc_error(&id, -32601, "Method not found"),
        })
    }

    fn call(&self, tool: &str, args: &Value) -> (bool, String) {
        match tool {
            "memory_search" => {
                let query = args
                    .get("query")
                    .and_then(|q| q.as_str())
                    .unwrap_or("")
                    .trim();
                if query.is_empty() {
                    return (false, "query is required".into());
                }
                match search_unsealed(self.config_dir, query) {
                    Ok(text) => (true, text),
                    Err(err) => (false, err),
                }
            }
            "automation_status" => {
                let id = args
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                match automation_status(self.config_dir, id) {
                    Ok(text) => (true, text),
                    Err(err) => (false, err),
                }
            }
            _ => (false, format!("unknown tool `{tool}`")),
        }
    }
}

fn bridge_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "memory_search",
            "description": "Search unsealed GrokHub AMR memory (plain nodes only). Sealed personal nodes are never returned.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Words to find in plain memory nodes."
                    }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "automation_status",
            "description": "List GrokHub automation status (id, schedule, enabled, health). Read-only.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Optional automation id. Omit to list every automation."
                    }
                }
            }
        }),
    ]
}

/// Search plain `nodes/<id>.md` only. Never opens `.sealed` files, even when a
/// sealer could decrypt them.
pub fn search_unsealed(config_dir: &Path, query: &str) -> Result<String, String> {
    let raw = query.trim();
    let query = raw.to_ascii_lowercase();
    if query.is_empty() {
        return Err("query is required".into());
    }
    let nodes_dir = config_dir.join("amr").join("nodes");
    if !nodes_dir.is_dir() {
        return Ok("no matches".into());
    }
    // Exact id of a sealed-only node: refuse, do not open.
    if sealed_path(&nodes_dir, raw).is_file() && !plain_path(&nodes_dir, raw).is_file() {
        return Err(SEALED_DENIED.into());
    }
    let hits = collect_plain_hits(&nodes_dir, &query)?;
    if hits.is_empty() {
        return Ok("no matches".into());
    }
    let mut out = String::new();
    for (id, line) in hits {
        out.push_str(&format!("{id}\t{line}\n"));
    }
    Ok(out.trim_end().to_string())
}

fn plain_path(nodes_dir: &Path, id: &str) -> PathBuf {
    nodes_dir.join(format!("{id}.md"))
}

fn sealed_path(nodes_dir: &Path, id: &str) -> PathBuf {
    nodes_dir.join(format!("{id}.sealed"))
}

fn collect_plain_hits(nodes_dir: &Path, query: &str) -> Result<Vec<(String, String)>, String> {
    let Ok(read) = fs::read_dir(nodes_dir) else {
        return Ok(Vec::new());
    };
    let mut files: Vec<PathBuf> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
        .collect();
    files.sort();
    let mut hits = Vec::new();
    for path in files {
        // Never follow a sibling .sealed; only the .md path is opened.
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if sealed_path(nodes_dir, &id).is_file() {
            // A node must not be both plain and sealed; if a .sealed sibling
            // exists, refuse that id entirely.
            continue;
        }
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let Ok(node) = Node::from_markdown(&text) else {
            continue;
        };
        if node.sensitivity.sealed() {
            // Frontmatter claims sealed but the file is .md — still withhold.
            continue;
        }
        for line in match_plain(&node, query) {
            hits.push((node.id.clone(), line));
            if hits.len() >= SEARCH_CAP {
                return Ok(hits);
            }
        }
    }
    Ok(hits)
}

fn match_plain(node: &Node, query: &str) -> Vec<String> {
    let mut out = Vec::new();
    if node.id.to_ascii_lowercase().contains(query) {
        out.push(format!("id: {}", node.id));
    }
    for tag in &node.tags {
        if tag.to_ascii_lowercase().contains(query) {
            out.push(format!("tag: {tag}"));
        }
    }
    for line in node.body.lines() {
        if line.to_ascii_lowercase().contains(query) {
            out.push(line.to_string());
        }
    }
    out
}

/// Read-only automation status from `{config}/automations.json`.
pub fn automation_status(config_dir: &Path, id: Option<&str>) -> Result<String, String> {
    let list = load_automations(&config_dir.join("automations.json"))?;
    let rows: Vec<&Automation> = match id {
        Some(want) => list.iter().filter(|a| a.id == want).collect(),
        None => list.iter().collect(),
    };
    if let Some(want) = id
        && rows.is_empty()
    {
        return Err(format!("automation {want} not found"));
    }
    if rows.is_empty() {
        return Ok("no automations".into());
    }
    let mut out = String::new();
    for row in rows {
        let cadence = if row.schedule == "heartbeat" {
            format!("every {} min", row.heartbeat_every_min.max(1))
        } else if row.time.is_empty() {
            row.schedule.clone()
        } else {
            format!("{} {}", row.schedule, row.time)
        };
        let enabled = if row.enabled { "on" } else { "off" };
        let health = match row.health.outcome {
            grokhub_core::automation::AutoOutcome::None => "clear".to_string(),
            grokhub_core::automation::AutoOutcome::Started => "started".into(),
            grokhub_core::automation::AutoOutcome::Ok => "ok".into(),
            grokhub_core::automation::AutoOutcome::Failed => {
                if row.health.error.is_empty() {
                    "failed".into()
                } else {
                    format!("failed: {}", row.health.error)
                }
            }
            grokhub_core::automation::AutoOutcome::Stopped => "stopped".into(),
        };
        let last = row
            .last_run
            .map(|ms| ms.to_string())
            .unwrap_or_else(|| "-".into());
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\truns:{}\tlast:{}\t{}\n",
            row.id, enabled, cadence, health, row.run_count, last, row.name
        ));
    }
    Ok(out.trim_end().to_string())
}

fn load_automations(path: &Path) -> Result<Vec<Automation>, String> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let len = fs::metadata(path).map_err(|e| e.to_string())?.len();
    if len > AUTO_STORE_CAP {
        return Err("automations.json is too large".into());
    }
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&text).map_err(|e| format!("automations.json: {e}"))
}

fn rpc_result(id: &Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

fn rpc_error(id: &Value, code: i64, msg: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": msg } }).to_string()
}

fn tool_reply(id: &Value, ok: bool, text: &str) -> String {
    rpc_result(
        id,
        json!({ "content": [{ "type": "text", "text": text }], "isError": !ok }),
    )
}

fn emit(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

/// Serve `grokhub-bridge` on stdin/stdout until stdin closes. Stdio only.
pub fn run_stdio(dir: &Path) -> i32 {
    let _ = AmrStore::at(dir.join("amr"));
    let server = BridgeServer::new(dir);
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    loop {
        let line = match grokhub_core::desktop_mcp::read_line_capped(&mut reader, LINE_CAP) {
            Ok(None) => break,
            Ok(Some(l)) => l,
            Err(msg) => {
                eprintln!("mcp-bridge: {msg}");
                emit(&rpc_error(&Value::Null, -32700, "Parse error"));
                continue;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = server.handle_line(&line) {
            emit(&reply);
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::amr::{NodeDraft, NodeType, Sensitivity};

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "gh-bridge-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|t| t.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn plain_draft(id: &str, body: &str) -> NodeDraft {
        NodeDraft {
            id: id.into(),
            node_type: NodeType::Fact,
            created: "2026-11-19T00:00:00Z".into(),
            updated: "2026-11-19T00:00:00Z".into(),
            source: "user".into(),
            confidence: 0.9,
            tags: vec!["dock".into()],
            body: body.into(),
            sensitivity: Sensitivity::Plain,
            consent_ref: String::new(),
        }
    }

    /// Hex-labeled fake sealer so a sealed file would open if the bridge used it.
    struct OpenSealer;
    impl grokhub_core::amr::Sealer for OpenSealer {
        fn seal(&self, aad: &str, plain: &str) -> Result<String, String> {
            Ok(format!("fake:{}:{}", hex_encode(aad), hex_encode(plain)))
        }
        fn open(&self, aad: &str, sealed: &str) -> Result<String, String> {
            let rest = sealed
                .strip_prefix(&format!("fake:{}:", hex_encode(aad)))
                .ok_or("wrong node")?;
            String::from_utf8(hex_decode(rest)?).map_err(|e| e.to_string())
        }
    }

    fn hex_encode(s: &str) -> String {
        s.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
    }
    fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string()))
            .collect()
    }

    #[test]
    fn memory_search_finds_plain_nodes_and_skips_sealed() {
        let dir = temp("search");
        let sealer = std::sync::Arc::new(OpenSealer);
        let store = AmrStore::at(dir.join("amr")).with_sealer(sealer);
        store.init().unwrap();
        store
            .remember(&plain_draft("fact-dock", "Dock layout is plain.\n"))
            .unwrap();
        store
            .remember(&NodeDraft {
                sensitivity: Sensitivity::Personal,
                ..plain_draft("fact-secret", "Secret harbor code is Pier 9.\n")
            })
            .unwrap();
        assert!(dir.join("amr/nodes/fact-secret.sealed").is_file());
        assert!(!dir.join("amr/nodes/fact-secret.md").exists());

        // Full store recall (with sealer) would find the sealed node.
        let with_key = store.recall("pier");
        assert_eq!(with_key.len(), 1);
        assert_eq!(with_key[0].id, "fact-secret");

        // Bridge must not.
        let found = search_unsealed(&dir, "dock").unwrap();
        assert!(found.contains("fact-dock"), "{found}");
        assert!(!found.contains("Pier"), "{found}");
        assert!(!found.contains("fact-secret"), "{found}");

        // Body text that lives only inside a sealed node is invisible.
        assert_eq!(search_unsealed(&dir, "pier").as_deref(), Ok("no matches"));

        // Exact sealed id lookup is denied, never opened.
        assert_eq!(
            search_unsealed(&dir, "fact-secret").unwrap_err(),
            SEALED_DENIED
        );

        // Plaintext stuffed into a .sealed file is still refused (never read).
        fs::write(
            dir.join("amr/nodes/leaked.sealed"),
            "---\nid: leaked\ntype: fact\ncreated: 2026-11-19T00:00:00Z\nupdated: 2026-11-19T00:00:00Z\nsource: user\nconfidence: 0.9\ntags: [x]\n---\nLeaked plaintext pier.\n",
        )
        .unwrap();
        assert_eq!(search_unsealed(&dir, "leaked").unwrap_err(), SEALED_DENIED);
        assert_eq!(
            search_unsealed(&dir, "plaintext").as_deref(),
            Ok("no matches")
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn automation_status_lists_enabled_and_health() {
        let dir = temp("auto");
        fs::write(
            dir.join("automations.json"),
            r#"[{"id":"a1","name":"Inbox","schedule":"heartbeat","time":"","heartbeatEveryMin":15,"instructions":"check mail","enabled":true,"runCount":3,"health":{"outcome":"ok"}}]"#,
        )
        .unwrap();
        let text = automation_status(&dir, None).unwrap();
        assert!(text.contains("a1\ton\tevery 15 min\tok\truns:3"), "{text}");
        assert!(text.contains("Inbox"), "{text}");
        assert_eq!(
            automation_status(&dir, Some("missing")).unwrap_err(),
            "automation missing not found"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn initialize_names_the_bridge_server() {
        let dir = temp("init");
        let server = BridgeServer::new(&dir);
        let reply = server
            .handle_line(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{}}}"#,
            )
            .unwrap();
        let v: Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(v["result"]["serverInfo"]["name"], BRIDGE_MCP_SERVER);
        assert_eq!(v["result"]["protocolVersion"], "2025-06-18");
        let list = server
            .handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#)
            .unwrap();
        let tools = serde_json::from_str::<Value>(&list).unwrap();
        let names: Vec<&str> = tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["memory_search", "automation_status"]);
        let _ = fs::remove_dir_all(&dir);
    }
}
