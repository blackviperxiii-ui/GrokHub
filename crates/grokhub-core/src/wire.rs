//! Chat wire types shared by the native engine and the app: turn events,
//! tool cards, permission and form asks, session modes and usage.

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

    /// Ask requires approval before shell, edit, or write. An unwatched run
    /// cannot show Allow / Deny.
    pub fn needs_approval(self) -> bool {
        matches!(self, Self::Ask)
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
    /// Tool name plus the raw-input keys the cabin pre-check reads (shell
    /// `command`, MCP `name`, desktop `x` / `y`, and a computer-use frame's
    /// key chord, app, window, and field descriptors). Typed text and other
    /// args stay out.
    pub raw_input: String,
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
    /// One plain line: the command, path, or site. Not a raw tool dump.
    pub action: String,
    /// Hook `ask` reason (or any other prompt body the CLI sent).
    pub reason: String,
    /// The agent's own "reject once" option, if it offered one. Deny selects it so
    /// the agent hears a refusal; `cancelled` means the turn was stopped.
    pub reject_option: Option<String>,
}

/// Grok Build 1.0.17 `x.ai/mcp/elicit` — MCP tool needs a form or URL from you.
#[derive(Debug, Clone, PartialEq)]
pub struct ElicitAsk {
    pub rpc_id: Value,
    pub session_id: String,
    pub tool_call_id: String,
    pub server_name: String,
    pub message: String,
    /// `form` or `url`.
    pub mode: String,
    pub url: String,
    pub elicitation_id: String,
    /// First string field on a form, if the schema has one.
    pub field_name: Option<String>,
    pub field_title: String,
    /// Password, token, or other secret. The value stays out of the transcript.
    pub secret: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AcpEvent {
    Ready { session_id: String },
    Thought(String),
    Text(String),
    Tool(ToolCard),
    Plan(String),
    /// A findings card body (`grokhub_core::findings`), shown after the reply.
    Findings(String),
    Permission(PermissionAsk),
    Elicit(ElicitAsk),
    ElicitComplete { elicitation_id: String, server_name: String },
    Usage(GrokUsage),
    Commands(Vec<String>),
    Task { id: String, title: String, done: bool },
    Compact {
        started: bool,
        usage: GrokUsage,
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
    if status.eq_ignore_ascii_case("input_required") && detail.is_empty() {
        detail = "Waiting for input".into();
    }
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
        raw_input: raw_summary(update),
    }
}

/// Raw-input keys a computer-use frame keeps for the cabin pre-check: key
/// chords, the app and window, and field descriptors and secret flags (the
/// same keys `harness::credential_field` reads).
const CU_RAW_KEYS: &[&str] = &[
    "keys", "key", "app", "window", "role", "ax_role", "label", "ax_label", "aria_label", "field", "field_name",
    "placeholder", "target", "element", "autocomplete", "input_type", "id", "selector", "secret", "sensitive",
    "is_secret", "is_password", "password_field", "masked",
];

/// One kept computer-use value: a string capped at 80 chars, a bool or
/// number, or a nested field object (`{"field":{"role":…}}`) holding only
/// these keys plus `name`. Typed `text` and `value` are never kept.
fn cu_keep(v: &Value, depth: u8) -> Option<Value> {
    match v {
        Value::String(s) => Some(Value::from(s.chars().take(80).collect::<String>())),
        Value::Bool(_) | Value::Number(_) => Some(v.clone()),
        Value::Object(o) if depth < 2 => {
            let kept: serde_json::Map<String, Value> = o
                .iter()
                .filter(|(k, _)| k.as_str() == "name" || CU_RAW_KEYS.contains(&k.as_str()))
                .filter_map(|(k, v)| cu_keep(v, depth + 1).map(|v| (k.clone(), v)))
                .collect();
            (!kept.is_empty()).then_some(Value::Object(kept))
        }
        _ => None,
    }
}

/// See [`ToolCard::raw_input`].
pub fn raw_summary(update: &Value) -> String {
    let raw = update.get("rawInput").unwrap_or(&Value::Null);
    let mut m = serde_json::Map::new();
    if let Some(n) = update.get("toolName").and_then(|v| v.as_str()) {
        m.insert("tool".into(), Value::from(n));
    }
    for k in ["command", "name", "x", "y"] {
        if let Some(v) = raw.get(k).filter(|v| v.is_string() || v.is_number()) {
            m.insert(k.into(), v.clone());
        }
    }
    // Spike-1c path D: what the hard-class check reads on a computer-use
    // frame. Key chords, the app and window, and words that describe the
    // target field. Never the typed `text` or `value`.
    for k in CU_RAW_KEYS {
        if let Some(keep) = raw.get(*k).and_then(|v| cu_keep(v, 0)) {
            m.insert((*k).into(), keep);
        }
    }
    if m.is_empty() {
        String::new()
    } else {
        Value::Object(m).to_string()
    }
}

pub fn merge_tool_card(old: ToolCard, new: ToolCard) -> ToolCard {
    let title = if is_generic_tool_title(&new.title) {
        old.title
    } else {
        new.title
    };
    let kind = if new.kind.is_empty() { old.kind } else { new.kind };
    // A status flip with no new content must not keep a status-word detail
    // ("Waiting for input", "running"). The chip is the one status. Real
    // content on the update still replaces it.
    let status_flipped = !new.status.is_empty() && new.status != old.status;
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
    } else if new.detail.is_empty()
        && status_flipped
        && crate::tool_detail_is_status(&old.detail)
    {
        String::new()
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
        raw_input: if new.raw_input.is_empty() {
            old.raw_input
        } else {
            new.raw_input
        },
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
            let u = parse_usage(update);
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
            usage: parse_usage(update),
            error: None,
        }),
        "auto_compact_completed" => Some(AcpEvent::Compact {
            started: false,
            usage: parse_usage(update),
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
                usage: parse_usage(update),
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
    let action = permission_action_line(tool);
    PermissionAsk {
        rpc_id: id,
        session_id,
        title,
        tool_call_id,
        action,
        reason,
        reject_option: permission_option(params, "reject_once"),
    }
}

/// `optionId` of the first offered `session/request_permission` option of `kind`
/// (`allow_once`, `allow_always`, `reject_once`, `reject_always`).
pub fn permission_option(params: &Value, kind: &str) -> Option<String> {
    params
        .get("options")?
        .as_array()?
        .iter()
        .find(|o| {
            o.get("kind")
                .and_then(|k| k.as_str())
                .is_some_and(|k| k.replace('-', "_").eq_ignore_ascii_case(kind))
        })?
        .get("optionId")?
        .as_str()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

/// The command, path, or site on one line. JSON tool dumps stay off the card.
pub fn permission_action_line(tool: &Value) -> String {
    let raw = tool
        .get("rawInput")
        .or_else(|| tool.get("raw_input"))
        .unwrap_or(&Value::Null);
    if let Some(cmd) = first_action_str(raw, &["command", "cmd", "shell"]) {
        return one_action_line(&cmd);
    }
    if let Some(url) = first_action_str(raw, &["url", "uri", "href", "site"]) {
        return one_action_line(&site_label(&url));
    }
    if let Some(path) = first_action_str(
        raw,
        &[
            "path",
            "file",
            "filePath",
            "file_path",
            "target_file",
            "target",
        ],
    ) {
        return one_action_line(&path);
    }
    let title = tool
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if title.is_empty() || looks_json_blob(title) {
        return String::new();
    }
    if let Some(site) = url_in_text(title) {
        return one_action_line(&site_label(&site));
    }
    one_action_line(title)
}

fn first_action_str(v: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(s) = v.get(*key).and_then(|x| x.as_str()) {
            let s = s.trim();
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

fn one_action_line(s: &str) -> String {
    let line = s
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if line.chars().count() <= 120 {
        line
    } else {
        format!("{}…", line.chars().take(119).collect::<String>())
    }
}

fn site_label(url: &str) -> String {
    let t = url.trim();
    let rest = t
        .strip_prefix("https://")
        .or_else(|| t.strip_prefix("http://"))
        .unwrap_or("");
    if rest.is_empty() {
        return t.to_string();
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = host.split('@').next_back().unwrap_or(host);
    if host.is_empty() {
        t.to_string()
    } else {
        host.to_string()
    }
}

fn url_in_text(s: &str) -> Option<String> {
    for word in s.split_whitespace() {
        let word = word.trim_matches(|c: char| matches!(c, '`' | '"' | '\'' | ',' | ')' | '('));
        if word.starts_with("https://") || word.starts_with("http://") {
            return Some(word.to_string());
        }
    }
    None
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

/// You chose Deny: select the agent's reject option. The agent reports a refusal.
pub fn permission_reject(id: Value, option_id: &str) -> JsonRpc {
    response(
        id,
        json!({
            "outcome": { "outcome": "selected", "optionId": option_id }
        }),
    )
}

/// The turn is stopping (Stop, a failed stream, a finished or replayed turn), so the
/// ask is withdrawn. Agents report this as "User cancelled", so it is not a Deny.
pub fn permission_cancel(id: Value) -> JsonRpc {
    response(
        id,
        json!({
            "outcome": { "outcome": "cancelled" }
        }),
    )
}

pub fn parse_elicit(id: Value, params: &Value) -> ElicitAsk {
    let mode = params
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("form")
        .to_ascii_lowercase();
    let schema = params.get("requestedSchema").unwrap_or(&Value::Null);
    let (field_name, field_title, secret) = first_form_field(schema);
    ElicitAsk {
        rpc_id: id,
        session_id: params
            .get("sessionId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        tool_call_id: params
            .get("toolCallId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        server_name: params
            .get("serverName")
            .and_then(|v| v.as_str())
            .unwrap_or("MCP")
            .to_string(),
        message: params
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string(),
        mode,
        url: params
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        elicitation_id: params
            .get("elicitationId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        field_name,
        field_title,
        secret,
    }
}

fn first_form_field(schema: &Value) -> (Option<String>, String, bool) {
    let props = match schema.get("properties").and_then(|v| v.as_object()) {
        Some(p) if !p.is_empty() => p,
        _ => return (None, String::new(), false),
    };
    let required: Vec<String> = schema
        .get("required")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let pick = required
        .iter()
        .find(|k| {
            props.get(k.as_str()).and_then(|p| p.get("type")).and_then(|t| t.as_str())
                == Some("string")
        })
        .cloned()
        .or_else(|| {
            props.iter().find_map(|(k, p)| {
                if p.get("type").and_then(|t| t.as_str()) == Some("string") {
                    Some(k.clone())
                } else {
                    None
                }
            })
        });
    let Some(name) = pick else {
        return (None, String::new(), false);
    };
    let prop = props.get(&name);
    let title = prop
        .and_then(|p| p.get("title").or_else(|| p.get("description")))
        .and_then(|v| v.as_str())
        .unwrap_or(name.as_str())
        .trim()
        .to_string();
    let secret = prop.is_some_and(|p| field_asks_for_secret(&name, &title, p));
    (Some(name), title, secret)
}

/// Connector and MCP forms that ask for a password, token, or key.
pub fn field_asks_for_secret(name: &str, title: &str, prop: &Value) -> bool {
    if prop.get("writeOnly").and_then(|v| v.as_bool()) == Some(true) {
        return true;
    }
    if prop.get("format").and_then(|v| v.as_str()) == Some("password") {
        return true;
    }
    let blob = format!("{name} {title}").to_ascii_lowercase();
    [
        "password",
        "passwd",
        "secret",
        "token",
        "api_key",
        "apikey",
        "credential",
    ]
    .iter()
    .any(|k| blob.contains(k))
}

pub fn parse_elicit_complete(params: &Value) -> (String, String) {
    let id = params
        .get("elicitationId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let server = params
        .get("serverName")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    (id, server)
}

pub fn elicit_accept(id: Value, content: Option<Value>) -> JsonRpc {
    let mut body = json!({ "outcome": "accept" });
    if let Some(c) = content {
        body["content"] = c;
    }
    response(id, body)
}

pub fn elicit_decline(id: Value) -> JsonRpc {
    response(id, json!({ "outcome": "decline" }))
}

pub fn elicit_cancel(id: Value) -> JsonRpc {
    response(id, json!({ "outcome": "cancel" }))
}


/// One finished turn of a background or night run: its session, reply, thinking and usage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SingleTurn {
    pub session_id: String,
    pub text: String,
    pub thought: String,
    pub usage: GrokUsage,
    pub stop_reason: String,
}

/// One event from a running turn: streamed text, a tool card, usage, the end.
#[derive(Debug, Clone, PartialEq)]
pub enum GrokPEvent {
    Thought(String),
    Text(String),
    Tool(ToolCard),
    Usage(GrokUsage),
    Plan(String),
    Compact {
        started: bool,
        usage: GrokUsage,
        error: Option<String>,
    },
    Commands(Vec<String>),
    Task { id: String, title: String, done: bool },
    Recovering(String),
    End(SingleTurn),
    Err(String),
}

/// One finished unattended turn as JSON: `sessionId`, `text`, `thought`,
/// `stopReason`, and usage. Leading log lines before the object are skipped.
pub fn parse_single_turn(stdout: &str) -> Result<SingleTurn, String> {
    let trimmed = stdout.trim();
    let json = if let Some(i) = trimmed.find('{') {
        &trimmed[i..]
    } else {
        trimmed
    };
    let v: Value = serde_json::from_str(json).map_err(|e| format!("turn json: {e}"))?;
    let session_id = v
        .get("sessionId")
        .or_else(|| v.get("session_id"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if session_id.is_empty() {
        return Err("turn missing sessionId".into());
    }
    let text = v
        .get("text")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let thought = v
        .get("thought")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() && thought.is_empty() {
        return Err("turn empty reply".into());
    }
    let mut usage = parse_usage(&v);
    if usage.stop_reason.is_empty() {
        usage.stop_reason = v
            .get("stopReason")
            .or_else(|| v.get("stop_reason"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
    }
    let stop_reason = usage.stop_reason.clone();
    Ok(SingleTurn {
        session_id,
        text,
        thought,
        usage,
        stop_reason,
    })
}

/// Server-reported spend and context. Grok Build 1.0.12+ includes reasoning.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GrokUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub total_tokens: u64,
    pub num_turns: u32,
    pub context_tokens_used: u64,
    pub context_window_tokens: u64,
    pub stop_reason: String,
    /// Millionths of a dollar, when the native engine reported a cost. Zero for the CLI.
    pub cost_in_usd_ticks: i64,
    /// "SuperGrok pool" or "API credits" from the native engine. Empty on the CLI path.
    pub meter: String,
}

impl GrokUsage {
    pub fn is_empty(&self) -> bool {
        self.input_tokens == 0
            && self.output_tokens == 0
            && self.reasoning_tokens == 0
            && self.total_tokens == 0
            && self.context_tokens_used == 0
            && self.context_window_tokens == 0
    }

    pub fn context_used(&self) -> u64 {
        if self.context_tokens_used > 0 {
            self.context_tokens_used
        } else {
            self.context_total()
        }
    }

    pub fn context_window(&self) -> u64 {
        if self.context_window_tokens > 0 {
            self.context_window_tokens
        } else {
            500_000
        }
    }

    pub fn context_total(&self) -> u64 {
        if self.total_tokens > 0 {
            self.total_tokens
        } else {
            self.input_tokens
                + self.cache_read_input_tokens
                + self.cache_creation_input_tokens
                + self.output_tokens
        }
    }

    pub fn merge(&mut self, other: &GrokUsage) {
        if other.input_tokens > 0 {
            self.input_tokens = other.input_tokens;
        }
        if other.output_tokens > 0 {
            self.output_tokens = other.output_tokens;
        }
        if other.reasoning_tokens > 0 {
            self.reasoning_tokens = other.reasoning_tokens;
        }
        if other.cache_read_input_tokens > 0 {
            self.cache_read_input_tokens = other.cache_read_input_tokens;
        }
        if other.cache_creation_input_tokens > 0 {
            self.cache_creation_input_tokens = other.cache_creation_input_tokens;
        }
        if other.total_tokens > 0 {
            self.total_tokens = other.total_tokens;
        }
        if other.num_turns > 0 {
            self.num_turns = other.num_turns;
        }
        if other.context_tokens_used > 0 {
            self.context_tokens_used = other.context_tokens_used;
        }
        if other.context_window_tokens > 0 {
            self.context_window_tokens = other.context_window_tokens;
        }
        if !other.stop_reason.is_empty() {
            self.stop_reason = other.stop_reason.clone();
        }
        if other.cost_in_usd_ticks != 0 {
            self.cost_in_usd_ticks = other.cost_in_usd_ticks;
        }
        if !other.meter.is_empty() {
            self.meter = other.meter.clone();
        }
    }
}

pub fn grok_context_line(u: &GrokUsage) -> String {
    if u.is_empty() {
        return String::new();
    }
    let used = u.context_used();
    let window = u.context_window();
    let pct = (used.min(window) * 100).checked_div(window).unwrap_or(100) as u32;
    let mut s = format!("{pct}% · {}/{}", compact_k(used), compact_k(window));
    if u.reasoning_tokens > 0 {
        s.push_str(&format!(" · {} think", compact_k(u.reasoning_tokens)));
    }
    if !s.is_empty() && !u.meter.is_empty() {
        s.push_str(" · ");
        s.push_str(&u.meter);
    }
    s
}

pub fn grok_usage_line(u: &GrokUsage) -> String {
    if u.is_empty() {
        return String::new();
    }
    let mut s = format!(
        "grok {} in / {} out",
        compact_k(u.input_tokens),
        compact_k(u.output_tokens)
    );
    if u.reasoning_tokens > 0 {
        s.push_str(&format!(" / {} think", compact_k(u.reasoning_tokens)));
    }
    if u.cache_read_input_tokens > 0 {
        s.push_str(&format!(" / {} cache", compact_k(u.cache_read_input_tokens)));
    }
    s
}

pub fn turn_footer(stop_reason: &str, usage: &GrokUsage) -> String {
    let reason = stop_reason.trim();
    let ctx = grok_context_line(usage);
    let head = match reason {
        "" | "end_turn" => {
            if ctx.is_empty() {
                return String::new();
            }
            "Done"
        }
        "cancelled" | "canceled" => "Cancelled",
        "max_tokens" => "Truncated — Grok is continuing",
        "max_turn_requests" | "max_turns_reached" => "Max turns",
        "refusal" => "Refused",
        other => other,
    };
    if ctx.is_empty() {
        head.to_string()
    } else {
        format!("{head} · {ctx}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamErrorKind {
    Fatal,
    Transient,
    TruncationContinue,
    CreditLimit,
}

pub fn classify_stream_error(msg: &str) -> StreamErrorKind {
    let l = msg.to_ascii_lowercase();
    if l.contains("credit")
        || l.contains("quota")
        || l.contains("usage limit")
        || l.contains("upgrade tier")
        || (l.contains("limit") && (l.contains("upsell") || l.contains("out of")))
    {
        StreamErrorKind::CreditLimit
    } else if l.contains("shorter answer")
        || l.contains("max_output")
        || l.contains("max_tokens")
        || (l.contains("truncat") && (l.contains("output") || l.contains("response") || l.contains("token")))
    {
        StreamErrorKind::TruncationContinue
    } else if ["500", "502", "503", "504"].iter().any(|c| {
        l.split(|ch: char| !ch.is_ascii_digit()).any(|w| w == *c)
    }) || l.contains("5xx")
        || l.contains("stall")
        || l.contains("dropped")
        || l.contains("timed out")
        || l.contains("timeout")
        || l.contains("unavailable")
        || l.contains("connection reset")
        || l.contains("econnreset")
        || l.contains("temporarily")
        || l.contains("try again later")
        || l.contains("unreachable")
        || l.contains("coordinator")
    {
        StreamErrorKind::Transient
    } else {
        StreamErrorKind::Fatal
    }
}

pub fn retry_status_line(msg: &str) -> String {
    let t = msg.trim();
    if t.is_empty() {
        return "Retrying…".into();
    }
    let l = t.to_ascii_lowercase();
    if l.starts_with("retry") {
        t.to_string()
    } else {
        format!("Retry: {t}")
    }
}

pub fn rewrite_truncation_error(msg: &str) -> String {
    match classify_stream_error(msg) {
        StreamErrorKind::TruncationContinue => {
            "Output hit the token limit. Grok is continuing automatically.".into()
        }
        StreamErrorKind::CreditLimit => {
            "Credit limit reached. Try Again retries the last prompt.".into()
        }
        StreamErrorKind::Transient => {
            let l = msg.to_ascii_lowercase();
            if l.contains("unreachable") || l.contains("coordinator") {
                "Subagent coordinator busy — retrying.".into()
            } else {
                "Grok hit a transient inference error and is retrying.".into()
            }
        }
        StreamErrorKind::Fatal => msg.to_string(),
    }
}

fn compact_k(n: u64) -> String {
    if n >= 1000 {
        format!("{}k", (n + 500) / 1000)
    } else {
        n.to_string()
    }
}

pub fn parse_usage(v: &Value) -> GrokUsage {
    let body = v.get("usage").unwrap_or(v);
    let mut u = GrokUsage {
        input_tokens: json_u64(body, &["input_tokens", "inputTokens"]),
        output_tokens: json_u64(body, &["output_tokens", "outputTokens"]),
        reasoning_tokens: json_u64(body, &["reasoning_tokens", "reasoningTokens"]),
        cache_read_input_tokens: json_u64(
            body,
            &["cache_read_input_tokens", "cacheReadInputTokens", "cachedReadTokens"],
        ),
        cache_creation_input_tokens: json_u64(
            body,
            &[
                "cache_creation_input_tokens",
                "cacheCreationInputTokens",
                "cacheCreationTokens",
            ],
        ),
        total_tokens: json_u64(body, &["total_tokens", "totalTokens"]),
        num_turns: json_u64(v, &["num_turns", "numTurns"]).min(u32::MAX as u64) as u32,
        context_tokens_used: json_u64(
            v,
            &["context_tokens_used", "contextTokensUsed", "tokens_used", "tokensUsed"],
        ),
        context_window_tokens: json_u64(
            v,
            &[
                "context_window_tokens",
                "contextWindowTokens",
                "context_window",
                "contextWindow",
            ],
        ),
        stop_reason: json_str(v, &["stopReason", "stop_reason"]),
        cost_in_usd_ticks: json_i64(body, &["cost_in_usd_ticks", "costInUsdTicks"]),
        meter: json_str(body, &["meter"]),
    };
    if u.num_turns == 0 {
        u.num_turns = json_u64(body, &["num_turns", "numTurns", "modelCalls"]).min(u32::MAX as u64) as u32;
    }
    if u.context_tokens_used == 0 {
        u.context_tokens_used = json_u64(body, &["context_tokens_used", "contextTokensUsed"]);
    }
    if u.context_window_tokens == 0 {
        u.context_window_tokens = json_u64(body, &["context_window", "contextWindow", "contextWindowTokens"]);
    }
    if u.stop_reason.is_empty() {
        u.stop_reason = json_str(body, &["stopReason", "stop_reason"]);
    }
    if u.total_tokens == 0 {
        u.total_tokens = u.context_total();
    }
    u
}

pub fn parse_signals_json(raw: &str) -> Option<GrokUsage> {
    let v: Value = serde_json::from_str(raw).ok()?;
    let u = GrokUsage {
        context_tokens_used: json_u64(&v, &["contextTokensUsed", "context_tokens_used"]),
        context_window_tokens: json_u64(&v, &["contextWindowTokens", "context_window_tokens"]),
        num_turns: json_u64(&v, &["turnCount", "turn_count"]).min(u32::MAX as u64) as u32,
        ..GrokUsage::default()
    };
    if u.context_tokens_used == 0 && u.context_window_tokens == 0 {
        None
    } else {
        Some(u)
    }
}

fn json_u64(v: &Value, keys: &[&str]) -> u64 {
    for k in keys {
        let Some(x) = v.get(*k) else { continue };
        if let Some(n) = x.as_u64() {
            return n;
        }
        if let Some(n) = x.as_i64() {
            return n.max(0) as u64;
        }
        if let Some(n) = x.as_f64() {
            return n.max(0.0) as u64;
        }
    }
    0
}

fn json_i64(v: &Value, keys: &[&str]) -> i64 {
    for k in keys {
        let Some(x) = v.get(*k) else { continue };
        if let Some(n) = x.as_i64() {
            return n;
        }
        if let Some(n) = x.as_u64() {
            return n as i64;
        }
        if let Some(n) = x.as_f64() {
            return n as i64;
        }
    }
    0
}

fn json_str(v: &Value, keys: &[&str]) -> String {
    for k in keys {
        if let Some(s) = v.get(*k).and_then(|x| x.as_str()).map(str::trim).filter(|s| !s.is_empty()) {
            return s.to_string();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_turn_stamps_session_and_text() {
        let raw = r#"{
            "text": "pong",
            "stopReason": "end_turn",
            "sessionId": "01a024f8-7606-74a2-8331-57a5177822eb",
            "thought": "say pong"
        }"#;
        let t = parse_single_turn(raw).expect("json");
        assert_eq!(t.session_id, "01a024f8-7606-74a2-8331-57a5177822eb");
        assert_eq!(t.text, "pong");
        assert_eq!(t.thought, "say pong");
        let spent = parse_single_turn(
            r#"{
            "text": "pong",
            "stopReason": "end_turn",
            "sessionId": "01a024f8-7606-74a2-8331-57a5177822eb",
            "usage": {"input_tokens": 18007, "output_tokens": 45, "reasoning_tokens": 40, "total_tokens": 18052},
            "num_turns": 1
        }"#,
        )
        .expect("usage");
        assert_eq!(spent.usage.reasoning_tokens, 40);
        assert_eq!(spent.usage.total_tokens, 18052);
        assert_eq!(spent.stop_reason, "end_turn");
        let noisy = format!("debug line\n{raw}\n");
        assert_eq!(parse_single_turn(&noisy).unwrap().text, "pong");
        assert!(parse_single_turn("{}").is_err());
    }

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
        assert_eq!(ask.action, "Run");
        assert_eq!(ask.reason, "Confirm this deploy");
        let elicit = parse_elicit(
            json!(2),
            &json!({
                "sessionId": "s1",
                "toolCallId": "mcp-elicit-1",
                "serverName": "github",
                "message": "Need email",
                "mode": "form",
                "requestedSchema": {
                    "type": "object",
                    "properties": { "email": { "type": "string", "title": "Email" } },
                    "required": ["email"]
                }
            }),
        );
        assert_eq!(elicit.server_name, "github");
        assert_eq!(elicit.field_name.as_deref(), Some("email"));
        assert!(!elicit.secret, "an email field is not a secret");
        assert_eq!(elicit_accept(json!(2), Some(json!({"email": "a@b.com"}))).result.unwrap()["outcome"], "accept");
        assert_eq!(elicit_decline(json!(3)).result.unwrap()["outcome"], "decline");
        let url = parse_elicit(
            json!(4),
            &json!({
                "sessionId": "s1",
                "serverName": "linear",
                "message": "Login",
                "mode": "url",
                "url": "https://example.com/auth",
                "elicitationId": "el-1"
            }),
        );
        assert_eq!(url.mode, "url");
        assert_eq!(url.url, "https://example.com/auth");
        let (cid, srv) = parse_elicit_complete(&json!({
            "elicitationId": "el-1",
            "serverName": "linear"
        }));
        assert_eq!(cid, "el-1");
        assert_eq!(srv, "linear");
        let waiting = parse_tool_card(&json!({
            "toolCallId": "t1",
            "title": "github",
            "status": "input_required"
        }));
        assert_eq!(waiting.status, "input_required");
        assert_eq!(waiting.detail, "Waiting for input");
        let done = parse_tool_card(&json!({
            "toolCallId": "t1",
            "title": "click",
            "status": "completed"
        }));
        assert_eq!(done.detail, "");
        let cleared = merge_tool_card(waiting.clone(), done);
        assert_eq!(cleared.status, "completed");
        assert_eq!(cleared.detail, "");
        let with_text = parse_tool_card(&json!({
            "toolCallId": "t1",
            "title": "click",
            "status": "completed",
            "content": [{ "type": "content", "content": { "type": "text", "text": "3 matches" } }]
        }));
        let kept = merge_tool_card(waiting, with_text);
        assert_eq!(kept.status, "completed");
        assert_eq!(kept.detail, "3 matches");
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
    fn computer_use_raw_keeps_field_words_and_chords_never_typed_text() {
        let u = json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "t9",
            "title": "computer_type",
            "kind": "other",
            "status": "pending",
            "rawInput": { "text": "hunter22", "value": "hunter22", "label": "Password", "secret": true, "keys": "ctrl+v", "nested": { "a": 1 } }
        });
        let card = parse_tool_card(&u);
        let raw: Value = serde_json::from_str(&card.raw_input).unwrap();
        assert_eq!(raw, json!({ "label": "Password", "secret": true, "keys": "ctrl+v" }));
        assert!(!card.raw_input.contains("hunter22"), "{}", card.raw_input);
    }

    #[test]
    fn computer_use_raw_keeps_field_id_and_nested_field_objects_never_their_text() {
        let u = json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "t10",
            "title": "Tool",
            "toolName": "keyboard_type",
            "status": "pending",
            "rawInput": {
                "text": "hunter22",
                "id": "login-pass",
                "field": { "role": "AXSecureTextField", "name": "pw", "value": "hunter22", "deep": { "label": "x" } }
            }
        });
        let raw: Value = serde_json::from_str(&parse_tool_card(&u).raw_input).unwrap();
        assert_eq!(
            raw,
            json!({ "tool": "keyboard_type", "id": "login-pass", "field": { "role": "AXSecureTextField", "name": "pw" } })
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
        assert!(PermissionMode::Ask.needs_approval());
        assert!(!PermissionMode::Auto.needs_approval());
        assert!(!PermissionMode::AlwaysApprove.needs_approval());
    }

    #[test]
    fn ask_card_says_the_command_path_or_site() {
        let cmd = parse_permission(
            json!(1),
            &json!({
                "sessionId": "s1",
                "toolCall": {
                    "title": "{\"tool\":\"bash\",\"raw\":true}",
                    "toolCallId": "c1",
                    "rawInput": { "command": "git status --short" }
                }
            }),
        );
        assert_eq!(cmd.action, "git status --short");
        let path = permission_action_line(&json!({
            "title": "tool",
            "rawInput": { "path": "C:\\Users\\j\\notes.md" }
        }));
        assert_eq!(path, "C:\\Users\\j\\notes.md");
        let site = permission_action_line(&json!({
            "title": "Fetch",
            "rawInput": { "url": "https://github.com/blackviperxiii-ui/GrokHub/pull/1" }
        }));
        assert_eq!(site, "github.com");
        let titled = permission_action_line(&json!({
            "title": "Open https://example.com/inbox"
        }));
        assert_eq!(titled, "example.com");
        let dump = permission_action_line(&json!({ "title": "{\"ok\":true}" }));
        assert!(dump.is_empty(), "a raw tool dump is not the action line: {dump}");
        let test = permission_action_line(&json!({
            "title": "Run",
            "rawInput": { "command": "[ -f /etc/os-release ] && cat /etc/os-release" }
        }));
        assert_eq!(test, "[ -f /etc/os-release ] && cat /etc/os-release");
        let group = permission_action_line(&json!({
            "rawInput": { "command": "{ echo hi; ls; }" }
        }));
        assert_eq!(group, "{ echo hi; ls; }");
    }

    #[test]
    fn deny_rejects_and_only_a_stop_cancels() {
        let ask = parse_permission(
            json!(7),
            &json!({
                "sessionId": "s1",
                "toolCall": { "title": "Run", "toolCallId": "c1" },
                "options": [
                    { "optionId": "allow-once", "name": "Allow", "kind": "allow_once" },
                    { "optionId": "allow-always", "name": "Always", "kind": "allow_always" },
                    { "optionId": "no-thanks", "name": "Reject", "kind": "reject_once" },
                    { "optionId": "never", "name": "Never", "kind": "reject_always" }
                ]
            }),
        );
        assert_eq!(
            ask.reject_option.as_deref(),
            Some("no-thanks"),
            "Deny picks the agent's own reject_once id, not reject_always"
        );
        let deny = permission_reject(json!(7), "no-thanks").result.unwrap();
        assert_eq!(deny["outcome"]["outcome"], "selected");
        assert_eq!(deny["outcome"]["optionId"], "no-thanks");
        let stop = permission_cancel(json!(7)).result.unwrap();
        assert_eq!(
            stop["outcome"]["outcome"], "cancelled",
            "cancelled is a stop: Grok reports it as \"User cancelled\""
        );
        let bare = parse_permission(
            json!(8),
            &json!({ "sessionId": "s1", "toolCall": { "title": "Run", "toolCallId": "c2" } }),
        );
        assert_eq!(bare.reject_option, None);
        let dashed = permission_option(
            &json!({ "options": [{ "optionId": " r1 ", "kind": "Reject-Once" }] }),
            "reject_once",
        );
        assert_eq!(dashed.as_deref(), Some("r1"));
    }

    #[test]
    fn secret_elicit_is_marked_and_email_is_not() {
        let secret = parse_elicit(
            json!(4),
            &json!({
                "sessionId": "s1",
                "serverName": "github",
                "message": "Need a token",
                "mode": "form",
                "requestedSchema": {
                    "type": "object",
                    "properties": {
                        "api_token": { "type": "string", "title": "API token", "writeOnly": true }
                    },
                    "required": ["api_token"]
                }
            }),
        );
        assert!(secret.secret);
        assert_eq!(secret.field_name.as_deref(), Some("api_token"));
        assert!(field_asks_for_secret(
            "password",
            "Password",
            &json!({ "type": "string", "format": "password" })
        ));
        assert!(!field_asks_for_secret("email", "Email", &json!({ "type": "string" })));
    }
}
