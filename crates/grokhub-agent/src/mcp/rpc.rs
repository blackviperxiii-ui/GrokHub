//! JSON-RPC 2.0 messages for one MCP session. Newline-delimited on the wire.

use serde_json::{json, Value};

pub(crate) const PROTOCOL: &str = "2025-03-26";

pub(crate) fn request(id: u64, method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    })
}

pub(crate) fn notification(method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    })
}

pub(crate) fn id_matches(msg: &Value, id: u64) -> bool {
    msg.get("method").is_none()
        && msg.get("id").and_then(|item| {
            item.as_u64()
                .or_else(|| item.as_i64().and_then(|n| u64::try_from(n).ok()))
        }) == Some(id)
}

pub(crate) fn rpc_error(msg: &Value) -> Option<String> {
    let err = msg.get("error")?;
    let message = err
        .get("message")
        .and_then(|item| item.as_str())
        .unwrap_or("MCP error");
    let code = err.get("code").and_then(|item| item.as_i64());
    Some(match code {
        Some(code) => format!("{message} ({code})"),
        None => message.to_string(),
    })
}

pub(crate) fn tool_output(result: &Value) -> (String, bool) {
    let failed = result
        .get("isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mut text = String::new();
    if let Some(items) = result.get("content").and_then(|v| v.as_array()) {
        for item in items {
            let kind = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if kind == "text" {
                if let Some(piece) = item.get("text").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(piece);
                }
            } else if !kind.is_empty() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&format!("[{kind}]"));
            }
        }
    }
    if text.is_empty() {
        if let Some(obj) = result.as_object() {
            if obj.len() == 1 && obj.contains_key("content") {
                text = "(empty)".into();
            } else if !obj.is_empty() {
                text = result.to_string();
            }
        }
    }
    if text.chars().count() > 40_000 {
        text = text.chars().take(40_000).collect();
        text.push_str("\n…");
    }
    (text, failed)
}

pub(crate) fn parse_tools(result: &Value) -> (Vec<RawTool>, Option<String>) {
    let mut tools = Vec::new();
    if let Some(items) = result.get("tools").and_then(|v| v.as_array()) {
        for item in items {
            let Some(name) = item.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            if name.trim().is_empty() {
                continue;
            }
            let description = item
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let input_schema = item
                .get("inputSchema")
                .or_else(|| item.get("input_schema"))
                .cloned()
                .filter(|v| v.is_object())
                .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
            let read_only = item
                .get("annotations")
                .and_then(|v| v.get("readOnlyHint"))
                .or_else(|| item.get("readOnlyHint"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            tools.push(RawTool {
                name: name.to_string(),
                description,
                input_schema,
                read_only,
            });
        }
    }
    let cursor = result
        .get("nextCursor")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    (tools, cursor)
}

#[derive(Debug, Clone)]
pub(crate) struct RawTool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub read_only: bool,
}

pub(crate) fn initialize_params() -> Value {
    json!({
        "protocolVersion": PROTOCOL,
        "capabilities": {
            "elicitation": { "form": {}, "url": {} }
        },
        "clientInfo": {
            "name": "GrokHub",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}
