use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionMode {
    Plan,
    Ask,
    Chat,
}

impl SessionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionMode::Plan => "plan",
            SessionMode::Ask => "ask",
            SessionMode::Chat => "chat",
        }
    }

    /// Grok Build ACP still names the default session `code`.
    pub fn acp_id(&self) -> &'static str {
        match self {
            SessionMode::Plan => "plan",
            SessionMode::Ask => "ask",
            SessionMode::Chat => "code",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "plan" => Some(SessionMode::Plan),
            "ask" => Some(SessionMode::Ask),
            "chat" | "code" | "normal" | "build" => Some(SessionMode::Chat),
            _ => None,
        }
    }

    pub fn cycle(self) -> Self {
        match self {
            SessionMode::Chat => SessionMode::Plan,
            SessionMode::Plan => SessionMode::Ask,
            SessionMode::Ask => SessionMode::Chat,
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
    /// Hook `ask` reason (or any other prompt body the CLI sent).
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AcpEvent {
    Ready { session_id: String },
    Thought(String),
    Text(String),
    Tool(ToolCard),
    Plan(String),
    Permission(PermissionAsk),
    Usage(crate::stream::GrokUsage),
    Commands(Vec<String>),
    Task { id: String, title: String, done: bool },
    Compact {
        started: bool,
        usage: crate::stream::GrokUsage,
        error: Option<String>,
    },
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

/// JSON-RPC 2.0 method-not-found. Grok Build closes stdio if a client-bound
/// request (fs/readTextFile, terminal/*) sits unanswered after session/new.
pub fn rpc_error(id: Value, code: i64, message: &str) -> JsonRpc {
    JsonRpc {
        jsonrpc: "2.0".into(),
        id: Some(id),
        method: None,
        params: None,
        result: None,
        error: Some(json!({ "code": code, "message": message })),
    }
}

pub fn method_not_found(id: Value) -> JsonRpc {
    rpc_error(id, -32601, "Method not found")
}

pub fn encode_line(msg: &JsonRpc) -> String {
    format!("{}\n", serde_json::to_string(msg).unwrap_or_else(|_| "{}".into()))
}

pub fn initialize_params() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "clientCapabilities": {
            "fs": { "readTextFile": false, "writeTextFile": false },
            "terminal": false
        },
        "clientInfo": { "name": "grokhub", "version": env!("CARGO_PKG_VERSION") }
    })
}

pub fn session_new_params(cwd: &str, yolo: bool, auto: bool, mode: SessionMode) -> Value {
    let mut meta = json!({
        "sessionMode": mode.acp_id(),
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

pub fn session_load_params(cwd: &str, session_id: &str, yolo: bool, auto: bool, mode: SessionMode) -> Value {
    let mut body = session_new_params(cwd, yolo, auto, mode);
    body["sessionId"] = json!(session_id);
    body["session_id"] = json!(session_id);
    body
}

pub fn prompt_params(session_id: &str, text: &str) -> Value {
    prompt_params_with_image(session_id, text, None)
}

/// Plus-button stills ride as ACP image blocks. `data_url` is `data:image/jpeg;base64,…`.
pub fn prompt_params_with_image(session_id: &str, text: &str, data_url: Option<&str>) -> Value {
    let mut prompt = vec![json!({ "type": "text", "text": text })];
    if let Some((mime, data)) = data_url.and_then(split_image_data_url) {
        prompt.push(json!({
            "type": "image",
            "mimeType": mime,
            "data": data,
        }));
    }
    json!({
        "sessionId": session_id,
        "prompt": prompt
    })
}

/// ACP image blocks want raw base64, not a data URL. Reject non-images and huge bodies.
pub fn split_image_data_url(url: &str) -> Option<(&str, &str)> {
    const IMAGE_B64_CAP: usize = 8 * 1024 * 1024;
    let rest = url.trim().strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    if data.is_empty() || data.len() > IMAGE_B64_CAP {
        return None;
    }
    if !meta.to_ascii_lowercase().contains("base64") {
        return None;
    }
    let mime = meta.split(';').next()?.trim();
    if !mime.starts_with("image/") || mime.len() > 64 {
        return None;
    }
    Some((mime, data))
}

/// grok login JWTs look like `header.payload.sig`. Console keys do not.
pub fn is_jwt_api_key(key: &str) -> bool {
    key.trim().bytes().filter(|b| *b == b'.').count() >= 2
}

pub fn pick_auth_method(auth_methods: &Value, api_key: &str) -> Option<String> {
    let ids: Vec<String> = auth_methods
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let has_api_key = !api_key.trim().is_empty();
    let jwt = is_jwt_api_key(api_key);
    // grok login JWTs belong on cached_token. A console key uses xai.api_key.
    if has_api_key && !jwt && ids.iter().any(|i| i == "xai.api_key") {
        return Some("xai.api_key".into());
    }
    if ids.iter().any(|i| i == "cached_token") {
        return Some("cached_token".into());
    }
    if ids.iter().any(|i| i == "grok.com") {
        return Some("grok.com".into());
    }
    if has_api_key && ids.iter().any(|i| i == "xai.api_key") {
        return Some("xai.api_key".into());
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
    let raw_title = update
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let loc = path_from_raw(update.get("rawInput").unwrap_or(&Value::Null));
    let title = pretty_tool_title(&raw_title, &kind, &loc);
    let mut images = Vec::new();
    if let Some(c) = update.get("content") {
        walk_images(c, &mut images);
    }
    let mut detail = tool_detail(update);
    if status.eq_ignore_ascii_case("failed") || status.eq_ignore_ascii_case("error") {
        let err = update
            .get("error")
            .or_else(|| update.get("message"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim();
        if !err.is_empty() && !detail.contains(err) {
            if detail.is_empty() {
                detail = err.to_string();
            } else {
                detail = format!("{detail}\n{err}");
            }
        }
    }
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

pub fn merge_tool_card(old: ToolCard, new: ToolCard) -> ToolCard {
    let title = if is_generic_tool_title(&new.title) {
        old.title
    } else {
        new.title
    };
    let kind = if new.kind.is_empty() { old.kind } else { new.kind };
    let status = if new.status.is_empty() {
        old.status
    } else {
        new.status
    };
    let detail = if looks_json_blob(&new.detail) {
        if looks_json_blob(&old.detail) {
            String::new()
        } else {
            old.detail
        }
    } else if new.detail.is_empty() {
        old.detail
    } else {
        new.detail
    };
    let diff = if new.diff.is_empty() { old.diff } else { new.diff };
    ToolCard {
        id: if new.id.is_empty() { old.id } else { new.id },
        title,
        kind,
        status,
        detail,
        diff,
        image_data_url: new.image_data_url.or(old.image_data_url),
    }
}

fn looks_json_blob(s: &str) -> bool {
    let t = s.trim();
    (t.starts_with('{') && t.ends_with('}')) || (t.starts_with('[') && t.ends_with(']'))
}

fn is_generic_tool_title(s: &str) -> bool {
    let t = s.trim();
    t.is_empty() || t.eq_ignore_ascii_case("tool")
}

fn pretty_tool_title(title: &str, kind: &str, loc: &str) -> String {
    let t = title.trim();
    if looks_json_blob(t) {
        // fall through to kind/path
    } else if !t.is_empty() && !is_generic_tool_title(t) {
        return shorten_tool_path(t);
    }
    let verb = match kind.to_ascii_lowercase().as_str() {
        "read" | "read_file" => "Read",
        "edit" | "edit_file" | "write" => "Edit",
        "delete" => "Delete",
        "execute" | "bash" | "terminal" | "shell" => "Run",
        "search" | "grep" => "Search",
        _ => "",
    };
    if !verb.is_empty() && !loc.is_empty() {
        return format!("{verb} `{loc}`");
    }
    if !loc.is_empty() {
        return loc.to_string();
    }
    if !verb.is_empty() {
        return verb.to_string();
    }
    "Tool".into()
}

fn path_from_raw(v: &Value) -> String {
    for key in [
        "target_file",
        "path",
        "file",
        "filePath",
        "file_path",
        "command",
        "cmd",
        "query",
    ] {
        if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
            let s = s.trim();
            if !s.is_empty() {
                return basename_or_tail(s);
            }
        }
    }
    String::new()
}

fn basename_or_tail(s: &str) -> String {
    let t = s.trim().trim_matches('`');
    let name = t.rsplit(['/', '\\']).next().unwrap_or(t);
    if name.chars().count() <= 42 {
        name.to_string()
    } else {
        format!("{}…", name.chars().take(41).collect::<String>())
    }
}

fn shorten_tool_path(title: &str) -> String {
    if let Some(start) = title.find('`') {
        if let Some(end) = title[start + 1..].find('`') {
            let path = &title[start + 1..start + 1 + end];
            let name = basename_or_tail(path);
            let rest = &title[start + 1 + end + 1..];
            return format!("{}`{name}`{rest}", &title[..start]);
        }
    }
    if title.len() <= 72 {
        return title.to_string();
    }
    format!("{}…", title.chars().take(71).collect::<String>())
}

fn tool_detail(update: &Value) -> String {
    if let Some(c) = update.get("content") {
        let text = tool_text_from_content(c);
        if !text.is_empty() && !looks_json_blob(&text) {
            return clip_tool_detail(&text);
        }
    }
    let loc = path_from_raw(update.get("rawInput").unwrap_or(&Value::Null));
    if !loc.is_empty() {
        return loc;
    }
    String::new()
}

fn tool_text_from_content(content: &Value) -> String {
    if let Some(s) = content.as_str() {
        return s.trim().to_string();
    }
    let Some(arr) = content.as_array() else {
        return String::new();
    };
    let mut out = String::new();
    for p in arr {
        let ty = p.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if ty == "diff" || ty == "image" {
            continue;
        }
        if let Some(t) = p.get("text").and_then(|x| x.as_str()) {
            push_tool_text(&mut out, t);
        }
        if let Some(inner) = p.get("content") {
            if let Some(t) = inner.get("text").and_then(|x| x.as_str()) {
                push_tool_text(&mut out, t);
            } else if let Some(t) = inner.as_str() {
                push_tool_text(&mut out, t);
            }
        }
    }
    out.trim().to_string()
}

fn push_tool_text(out: &mut String, t: &str) {
    let t = t.trim();
    if t.is_empty() || looks_json_blob(t) {
        return;
    }
    if !out.is_empty() {
        out.push(' ');
    }
    out.push_str(t);
}

fn clip_tool_detail(s: &str) -> String {
    let line = s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    if line.chars().count() <= 96 {
        return line.to_string();
    }
    format!("{}…", line.chars().take(95).collect::<String>())
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
        "available_commands_update" | "available_commands" => {
            let cmds = update
                .get("availableCommands")
                .or_else(|| update.get("commands"))
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| {
                            x.get("name")
                                .or_else(|| x.get("command"))
                                .and_then(|n| n.as_str())
                                .or_else(|| x.as_str())
                                .map(|s| s.trim().to_string())
                        })
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if cmds.is_empty() {
                None
            } else {
                Some(AcpEvent::Commands(cmds))
            }
        }
        "usage_update" | "turn_completed" => {
            let u = crate::stream::parse_usage(update);
            if u.is_empty() {
                None
            } else {
                Some(AcpEvent::Usage(u))
            }
        }
        "task_backgrounded" => Some(AcpEvent::Task {
            id: update
                .get("task_id")
                .or_else(|| update.get("tool_call_id"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            title: update
                .get("command")
                .or_else(|| update.get("title"))
                .and_then(|x| x.as_str())
                .unwrap_or("task")
                .chars()
                .take(80)
                .collect(),
            done: false,
        }),
        "task_completed" => Some(AcpEvent::Task {
            id: update
                .get("task_id")
                .or_else(|| update.pointer("/task_snapshot/task_id"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            title: update
                .get("command")
                .or_else(|| update.pointer("/task_snapshot/command"))
                .and_then(|x| x.as_str())
                .unwrap_or("task")
                .chars()
                .take(80)
                .collect(),
            done: true,
        }),
        "task_failed" | "todo_failed" => {
            let id = update
                .get("task_id")
                .or_else(|| update.get("tool_call_id"))
                .or_else(|| update.get("toolCallId"))
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let raw = update
                .get("error")
                .or_else(|| update.get("message"))
                .or_else(|| update.get("title"))
                .or_else(|| update.get("command"))
                .and_then(|x| x.as_str())
                .unwrap_or("task")
                .chars()
                .take(80)
                .collect::<String>();
            let title = if raw.to_ascii_lowercase().starts_with("failed") {
                raw
            } else {
                format!("Failed · {raw}")
            };
            Some(AcpEvent::Task {
                id,
                title,
                done: false,
            })
        }
        "auto_compact_started" => Some(AcpEvent::Compact {
            started: true,
            usage: crate::stream::parse_usage(update),
            error: None,
        }),
        "auto_compact_completed" => Some(AcpEvent::Compact {
            started: false,
            usage: crate::stream::parse_usage(update),
            error: None,
        }),
        "auto_compact_failed" => {
            let msg = update
                .get("message")
                .or_else(|| update.get("error"))
                .or_else(|| update.get("reason"))
                .and_then(|x| x.as_str())
                .unwrap_or("Compact failed")
                .trim()
                .to_string();
            Some(AcpEvent::Compact {
                started: false,
                usage: crate::stream::parse_usage(update),
                error: Some(if msg.is_empty() {
                    "Compact failed".into()
                } else {
                    msg
                }),
            })
        }
        "plan" => {
            if let Some(entries) = update.get("entries").and_then(|e| e.as_array()) {
                let lines: Vec<String> = entries
                    .iter()
                    .filter_map(|e| {
                        let c = e.get("content").and_then(|x| x.as_str())?.trim();
                        if c.is_empty() {
                            None
                        } else {
                            Some(c.to_string())
                        }
                    })
                    .collect();
                if !lines.is_empty() {
                    return Some(AcpEvent::Plan(lines.join(" · ")));
                }
            }
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
    let reason = params
        .get("reason")
        .or_else(|| params.get("message"))
        .or_else(|| params.get("permissionDecisionReason"))
        .or_else(|| params.get("hookReason"))
        .or_else(|| tool.get("reason"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    PermissionAsk {
        rpc_id: id,
        session_id,
        title,
        tool_call_id,
        reason,
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

pub fn permission_allow_always(id: Value) -> JsonRpc {
    response(
        id,
        json!({
            "outcome": { "outcome": "selected", "optionId": "allow-always" }
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
            pick_auth_method(&methods, "").as_deref(),
            Some("cached_token")
        );
        assert_eq!(
            pick_auth_method(&methods, "xai-console-key").as_deref(),
            Some("xai.api_key")
        );
        assert_eq!(
            pick_auth_method(&methods, "aaa.bbb.ccc").as_deref(),
            Some("cached_token"),
            "grok login JWT must not steal xai.api_key"
        );
        let alpha = json!([{ "id": "grok.com", "name": "Grok" }]);
        assert_eq!(
            pick_auth_method(&alpha, "").as_deref(),
            Some("grok.com"),
            "alpha advertises grok.com when logged out"
        );
    }

    #[test]
    fn prompt_sends_plus_button_image() {
        let text = prompt_params("s1", "hi");
        assert_eq!(text["prompt"].as_array().map(|a| a.len()), Some(1));
        assert_eq!(text["prompt"][0]["type"], "text");
        let url = "data:image/jpeg;base64,QQ==";
        let with = prompt_params_with_image("s1", "look", Some(url));
        assert_eq!(with["prompt"].as_array().map(|a| a.len()), Some(2));
        assert_eq!(with["prompt"][1]["type"], "image");
        assert_eq!(with["prompt"][1]["mimeType"], "image/jpeg");
        assert_eq!(with["prompt"][1]["data"], "QQ==");
        let ask = parse_permission(
            json!(1),
            &json!({
                "sessionId": "s1",
                "toolCall": { "title": "Run", "toolCallId": "c1" },
                "reason": "Confirm this deploy"
            }),
        );
        assert_eq!(ask.title, "Run");
        assert_eq!(ask.reason, "Confirm this deploy");
        assert!(split_image_data_url("data:text/plain;base64,QQ==").is_none());
        assert!(split_image_data_url("not-a-data-url").is_none());
        assert!(is_jwt_api_key("aaa.bbb.ccc"));
        assert!(!is_jwt_api_key("xai-console-key"));
        let reject = method_not_found(json!(77));
        assert_eq!(reject.id, Some(json!(77)));
        assert_eq!(reject.error.as_ref().unwrap()["code"], -32601);
        assert!(encode_line(&reject).contains("Method not found"));
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
    fn tool_card_hides_raw_json() {
        let pending = json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "t1",
            "title": "Read `/home/viper/.grok/installed-plugins/superpowers/SKILL.md`",
            "kind": "read",
            "status": "pending",
            "rawInput": { "target_file": "/home/viper/.grok/installed-plugins/superpowers/SKILL.md", "variant": "ReadFile" }
        });
        let card = parse_tool_card(&pending);
        assert!(card.title.contains("SKILL.md"), "{}", card.title);
        assert!(!card.title.contains("/home/viper"), "{}", card.title);
        assert!(!card.detail.contains('{'), "raw JSON leaked: {}", card.detail);
        assert!(!card.detail.contains("target_file"), "{}", card.detail);

        let done = json!({
            "sessionUpdate": "tool_call_update",
            "toolCallId": "t1",
            "status": "completed",
            "rawOutput": { "content": { "absolute_root_path": "/home/viper" } },
            "content": [{ "type": "content", "content": { "type": "text", "text": "# Skill\nUse this workflow." } }]
        });
        let next = parse_tool_card(&done);
        let merged = merge_tool_card(card, next);
        assert!(merged.title.contains("SKILL.md"), "{}", merged.title);
        assert_eq!(merged.status, "completed");
        assert!(!merged.detail.contains('{'), "{}", merged.detail);
        assert!(merged.detail.to_ascii_lowercase().contains("skill"), "{}", merged.detail);
    }

    #[test]
    fn mode_cycle() {
        assert_eq!(SessionMode::Chat.cycle(), SessionMode::Plan);
        assert_eq!(SessionMode::parse("chat"), Some(SessionMode::Chat));
        assert_eq!(SessionMode::parse("code"), Some(SessionMode::Chat));
        assert_eq!(SessionMode::Chat.as_str(), "chat");
        assert_eq!(SessionMode::Chat.acp_id(), "code");
        assert_eq!(SessionMode::parse("PLAN"), Some(SessionMode::Plan));
        let load = session_load_params("/home/j/GrokHub-Work", "sess-1", false, true, SessionMode::Chat);
        assert_eq!(load["cwd"], "/home/j/GrokHub-Work");
        assert_eq!(load["sessionId"], "sess-1");
        assert_eq!(load["_meta"]["sessionMode"], "code");
        let init = initialize_params();
        assert_eq!(init["clientCapabilities"]["terminal"], false);
        assert_eq!(init["clientCapabilities"]["fs"]["readTextFile"], false);
        assert_eq!(
            PermissionMode::parse("yolo"),
            Some(PermissionMode::AlwaysApprove)
        );
        assert!(PermissionMode::AlwaysApprove.auto_allows());
        assert!(PermissionMode::Auto.auto_allows());
        assert!(!PermissionMode::Ask.auto_allows());
    }
}
