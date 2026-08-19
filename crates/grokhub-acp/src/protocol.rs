use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    Plan,
    Ask,
    Code,
}

impl SessionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionMode::Plan => "plan",
            SessionMode::Ask => "ask",
            SessionMode::Code => "code",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "plan" => Some(SessionMode::Plan),
            "ask" => Some(SessionMode::Ask),
            "code" | "normal" | "build" => Some(SessionMode::Code),
            _ => None,
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            SessionMode::Code => SessionMode::Plan,
            SessionMode::Plan => SessionMode::Ask,
            SessionMode::Ask => SessionMode::Code,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    Ask,
    Auto,
    AlwaysApprove,
}

impl PermissionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            PermissionMode::Ask => "ask",
            PermissionMode::Auto => "auto",
            PermissionMode::AlwaysApprove => "always-approve",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ask" | "normal" => Some(PermissionMode::Ask),
            "auto" => Some(PermissionMode::Auto),
            "always-approve" | "always" | "yolo" => Some(PermissionMode::AlwaysApprove),
            _ => None,
        }
    }

    /// Auto and Always answer ACP permission prompts in the cabin.
    /// Ask leaves the Allow / Deny / Always bar up.
    pub fn auto_allows(self) -> bool {
        matches!(self, Self::AlwaysApprove | Self::Auto)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCard {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub status: String,
    pub detail: String,
    pub diff: String,
    pub image_data_url: Option<String>,
}

impl ToolCard {
    pub fn is_computer_use(&self) -> bool {
        let t = format!("{} {}", self.title, self.kind).to_ascii_lowercase();
        t.contains("computer")
            || t.contains("screenshot")
            || t.contains("snapshot")
            || t.contains("click")
            || t.contains("mouse")
            || t.contains("desktop")
            || t.contains("browser")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionAsk {
    pub rpc_id: Value,
    pub session_id: String,
    pub title: String,
    pub tool_call_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AcpEvent {
    Ready { session_id: String },
    Thought(String),
    Text(String),
    Tool(ToolCard),
    Plan(String),
    Permission(PermissionAsk),
    Done { stop_reason: String },
    Err(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpc {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
}

pub fn request(id: u64, method: &str, params: Value) -> JsonRpc {
    JsonRpc {
        jsonrpc: "2.0".into(),
        id: Some(json!(id)),
        method: Some(method.into()),
        params: Some(params),
        result: None,
        error: None,
    }
}

pub fn response(id: Value, result: Value) -> JsonRpc {
    JsonRpc {
        jsonrpc: "2.0".into(),
        id: Some(id),
        method: None,
        params: None,
        result: Some(result),
        error: None,
    }
}

pub fn notification(method: &str, params: Value) -> JsonRpc {
    JsonRpc {
        jsonrpc: "2.0".into(),
        id: None,
        method: Some(method.into()),
        params: Some(params),
        result: None,
        error: None,
    }
}

pub fn encode_line(msg: &JsonRpc) -> String {
    format!("{}\n", serde_json::to_string(msg).unwrap_or_else(|_| "{}".into()))
}

pub fn initialize_params() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "clientCapabilities": {
            "fs": { "readTextFile": true, "writeTextFile": true },
            "terminal": true
        },
        "clientInfo": { "name": "grokhub", "version": env!("CARGO_PKG_VERSION") }
    })
}

pub fn session_new_params(cwd: &str, yolo: bool, auto: bool, mode: SessionMode) -> Value {
    let mut meta = json!({
        "sessionMode": mode.as_str(),
    });
    if yolo {
        meta["yoloMode"] = json!(true);
    }
    if auto {
        meta["autoMode"] = json!(true);
    }
    json!({
        "cwd": cwd,
        "mcpServers": [],
        "_meta": meta
    })
}

pub fn prompt_params(session_id: &str, text: &str) -> Value {
    json!({
        "sessionId": session_id,
        "prompt": [{ "type": "text", "text": text }]
    })
}

pub fn pick_auth_method(auth_methods: &Value, has_api_key: bool) -> Option<String> {
    let ids: Vec<String> = auth_methods
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    if has_api_key && ids.iter().any(|i| i == "xai.api_key") {
        return Some("xai.api_key".into());
    }
    if ids.iter().any(|i| i == "cached_token") {
        return Some("cached_token".into());
    }
    ids.first().cloned()
}

pub fn image_data_url_from_value(v: &Value) -> Option<String> {
    if let Some(url) = v.get("dataUrl").or_else(|| v.get("data_url")).and_then(|x| x.as_str()) {
        if url.starts_with("data:image") {
            return Some(url.to_string());
        }
    }
    let mime = v
        .get("mimeType")
        .or_else(|| v.get("mime_type"))
        .and_then(|x| x.as_str())
        .unwrap_or("image/jpeg");
    if let Some(data) = v.get("data").and_then(|x| x.as_str()) {
        if !data.is_empty() && !data.starts_with("data:") {
            return Some(format!("data:{mime};base64,{data}"));
        }
        if data.starts_with("data:image") {
            return Some(data.to_string());
        }
    }
    if let Some(url) = v.get("url").and_then(|x| x.as_str()) {
        if url.starts_with("data:image") {
            return Some(url.to_string());
        }
    }
    None
}

pub fn walk_images(v: &Value, out: &mut Vec<String>) {
    if let Some(url) = image_data_url_from_value(v) {
        out.push(url);
    }
    match v {
        Value::Array(a) => {
            for x in a {
                walk_images(x, out);
            }
        }
        Value::Object(m) => {
            for x in m.values() {
                walk_images(x, out);
            }
        }
        _ => {}
    }
}

pub fn parse_tool_card(update: &Value) -> ToolCard {
    let id = update
        .get("toolCallId")
        .or_else(|| update.get("tool_call_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let title = update
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("tool")
        .to_string();
    let kind = update
        .get("kind")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let status = update
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("pending")
        .to_string();
    let mut images = Vec::new();
    if let Some(c) = update.get("content") {
        walk_images(c, &mut images);
    }
    let detail = update
        .get("rawInput")
        .or_else(|| update.get("rawOutput"))
        .map(|v| v.to_string())
        .unwrap_or_default();
    let diff = update
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|a| {
            a.iter().find_map(|p| {
                if p.get("type").and_then(|t| t.as_str()) == Some("diff") {
                    Some(p.get("diff").and_then(|d| d.as_str()).unwrap_or("").to_string())
                } else {
                    None
                }
            })
        })
        .unwrap_or_default();
    ToolCard {
        id,
        title,
        kind,
        status,
        detail,
        diff,
        image_data_url: images.pop(),
    }
}

pub fn parse_session_update(params: &Value) -> Option<AcpEvent> {
    let update = params.get("update").unwrap_or(params);
    let kind = update
        .get("sessionUpdate")
        .or_else(|| update.get("session_update"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    match kind {
        "agent_message_chunk" => {
            let t = update
                .pointer("/content/text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if t.is_empty() {
                None
            } else {
                Some(AcpEvent::Text(t))
            }
        }
        "agent_thought_chunk" => {
            let t = update
                .pointer("/content/text")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if t.is_empty() {
                None
            } else {
                Some(AcpEvent::Thought(t))
            }
        }
        "tool_call" | "tool_call_update" => Some(AcpEvent::Tool(parse_tool_card(update))),
        "plan" => {
            let t = update
                .get("title")
                .or_else(|| update.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or("plan")
                .to_string();
            Some(AcpEvent::Plan(t))
        }
        _ => None,
    }
}

pub fn parse_permission(id: Value, params: &Value) -> PermissionAsk {
    let session_id = params
        .get("sessionId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let tool = params.get("toolCall").unwrap_or(params);
    let title = tool
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("tool")
        .to_string();
    let tool_call_id = tool
        .get("toolCallId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    PermissionAsk {
        rpc_id: id,
        session_id,
        title,
        tool_call_id,
    }
}

pub fn permission_allow(id: Value) -> JsonRpc {
    response(
        id,
        json!({
            "outcome": { "outcome": "selected", "optionId": "allow-once" }
        }),
    )
}

pub fn permission_deny(id: Value) -> JsonRpc {
    response(
        id,
        json!({
            "outcome": { "outcome": "cancelled" }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_prefers_cached_without_key() {
        let methods = json!([{ "id": "xai.api_key" }, { "id": "cached_token" }]);
        assert_eq!(
            pick_auth_method(&methods, false).as_deref(),
            Some("cached_token")
        );
        assert_eq!(
            pick_auth_method(&methods, true).as_deref(),
            Some("xai.api_key")
        );
    }

    #[test]
    fn parses_text_and_thought() {
        let u = json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "text": "hi" }
        });
        assert_eq!(parse_session_update(&u), Some(AcpEvent::Text("hi".into())));
        let t = json!({
            "update": {
                "sessionUpdate": "agent_thought_chunk",
                "content": { "text": "hmm" }
            }
        });
        assert_eq!(parse_session_update(&t), Some(AcpEvent::Thought("hmm".into())));
    }

    #[test]
    fn tool_image_and_computer() {
        let u = json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "t1",
            "title": "computer_screenshot",
            "kind": "other",
            "status": "completed",
            "content": [{ "type": "image", "mimeType": "image/jpeg", "data": "AAAA" }]
        });
        let card = parse_tool_card(&u);
        assert!(card.is_computer_use());
        assert_eq!(
            card.image_data_url.as_deref(),
            Some("data:image/jpeg;base64,AAAA")
        );
    }

    #[test]
    fn mode_cycle() {
        assert_eq!(SessionMode::Code.cycle(), SessionMode::Plan);
        assert_eq!(SessionMode::parse("PLAN"), Some(SessionMode::Plan));
        assert_eq!(
            PermissionMode::parse("yolo"),
            Some(PermissionMode::AlwaysApprove)
        );
        assert!(PermissionMode::AlwaysApprove.auto_allows());
        assert!(PermissionMode::Auto.auto_allows());
        assert!(!PermissionMode::Ask.auto_allows());
    }
}
