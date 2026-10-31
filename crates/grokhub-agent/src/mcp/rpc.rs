//! JSON-RPC 2.0 messages for one MCP session. Newline-delimited on the wire.

use serde_json::{json, Value};

/// MCP revisions of the session era (an `initialize` handshake opens each
/// session), newest first. `initialize` asks for the first; the server's
/// answer must be one of these ([`negotiate`]). A later era that skips
/// `initialize` gets its own list and negotiation beside this one.
pub(crate) const SESSION_VERSIONS: [&str; 4] =
    ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// The revision this client asks for.
pub(crate) const LATEST: &str = SESSION_VERSIONS[0];

/// The revision for this session, from the server's `initialize` result.
/// The server answers the version asked for or another it speaks; an answer
/// outside [`SESSION_VERSIONS`] (or none) ends the connection.
pub(crate) fn negotiate(server: &str, init: &Value) -> Result<&'static str, String> {
    let answer = init
        .get("protocolVersion")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    if answer.is_empty() {
        return Err(format!(
            "MCP server `{server}` did not say which MCP version it speaks"
        ));
    }
    SESSION_VERSIONS
        .iter()
        .copied()
        .find(|known| *known == answer)
        .ok_or_else(|| {
            let shown: String = answer
                .chars()
                .filter(|c| !c.is_control())
                .take(40)
                .collect();
            format!(
                "MCP server `{server}` answered with MCP version {shown}; this client speaks {}",
                SESSION_VERSIONS.join(", ")
            )
        })
}

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

/// The model's text for a `tools/call` result, and whether it is a tool error
/// (`isError`). Text blocks come first; a `resource_link` becomes one line;
/// other blocks (image, audio, embedded resource) stay a `[kind]` marker.
/// `structuredContent` adds one compact JSON line unless a text block already
/// carries the same JSON (the spec's backwards-compatible copy).
pub(crate) fn tool_output(result: &Value) -> (String, bool) {
    let failed = result
        .get("isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mut text = String::new();
    let mut texts: Vec<&str> = Vec::new();
    if let Some(items) = result.get("content").and_then(|v| v.as_array()) {
        for item in items {
            match item.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                "" => {}
                "text" => {
                    if let Some(piece) = item.get("text").and_then(|v| v.as_str()) {
                        texts.push(piece);
                        push(&mut text, piece);
                    }
                }
                "resource_link" => push(&mut text, &resource_link_line(item)),
                kind => push(&mut text, &format!("[{kind}]")),
            }
        }
    }
    if let Some(structured) = result.get("structuredContent").filter(|v| !v.is_null()) {
        let copied = texts.iter().any(|piece| {
            serde_json::from_str::<Value>(piece.trim()).is_ok_and(|parsed| &parsed == structured)
        });
        if !copied {
            push(&mut text, &format!("structuredContent: {structured}"));
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

fn push(text: &mut String, piece: &str) {
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(piece);
}

/// `[resource_link] Title (name) <uri> (mime): description` on one line.
fn resource_link_line(item: &Value) -> String {
    let field = |key: &str, max: usize| {
        item.get(key)
            .and_then(Value::as_str)
            .map(|v| one_line(v, max))
            .filter(|v| !v.is_empty())
    };
    let name = field("name", 200);
    let title = field("title", 200);
    let mut line = "[resource_link]".to_string();
    match (title, name) {
        (Some(title), Some(name)) if title != name => line.push_str(&format!(" {title} ({name})")),
        (Some(label), _) | (None, Some(label)) => line.push_str(&format!(" {label}")),
        (None, None) => {}
    }
    if let Some(uri) = field("uri", 2048) {
        line.push_str(&format!(" <{uri}>"));
    }
    if let Some(mime) = field("mimeType", 100) {
        line.push_str(&format!(" ({mime})"));
    }
    if let Some(description) = field("description", 500) {
        line.push_str(&format!(": {description}"));
    }
    line
}

/// Trimmed, control characters (newlines too) turned to spaces, at most `max` chars.
fn one_line(text: &str, max: usize) -> String {
    text.trim()
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(max)
        .collect()
}

/// At most this many icons per tool.
const MAX_ICONS: usize = 4;
/// Longest icon `src` kept: an https URL or a `data:image/` URI.
const MAX_ICON_SRC: usize = 64 * 1024;
/// Largest `outputSchema` kept, serialized.
const MAX_OUTPUT_SCHEMA: usize = 16 * 1024;

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
            let annotations = item.get("annotations");
            // Display name: `title`, then `annotations.title` (the spec's order).
            let title = [item.get("title"), annotations.and_then(|a| a.get("title"))]
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|t| one_line(t, 200))
                .find(|t| !t.is_empty());
            let output_schema = item
                .get("outputSchema")
                .filter(|v| v.is_object() && v.to_string().len() <= MAX_OUTPUT_SCHEMA)
                .cloned();
            tools.push(RawTool {
                name: name.to_string(),
                title,
                description,
                input_schema,
                output_schema,
                hints: parse_hints(item),
                icons: parse_icons(item.get("icons")),
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

fn parse_hints(item: &Value) -> ToolHints {
    let hint = |key: &str| {
        item.get("annotations")
            .and_then(|a| a.get(key))
            .and_then(Value::as_bool)
    };
    ToolHints {
        // Older servers put `readOnlyHint` on the tool itself.
        read_only: hint("readOnlyHint")
            .or_else(|| item.get("readOnlyHint").and_then(Value::as_bool))
            .unwrap_or(false),
        destructive: hint("destructiveHint"),
        idempotent: hint("idempotentHint"),
        open_world: hint("openWorldHint"),
    }
}

/// Icons with an `https:` or `data:image/` source, at most [`MAX_ICONS`].
fn parse_icons(icons: Option<&Value>) -> Vec<ToolIcon> {
    let Some(items) = icons.and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|icon| {
            let src = icon.get("src")?.as_str()?.trim();
            let lower = src.get(..11).unwrap_or(src).to_ascii_lowercase();
            let allowed = lower.starts_with("https://") || lower.starts_with("data:image/");
            if !allowed
                || src.len() > MAX_ICON_SRC
                || src.chars().any(|c| c.is_whitespace() || c.is_control())
            {
                return None;
            }
            let mime_type = icon
                .get("mimeType")
                .and_then(Value::as_str)
                .map(|m| one_line(m, 100))
                .filter(|m| !m.is_empty());
            let sizes = icon
                .get("sizes")
                .and_then(Value::as_array)
                .map(|sizes| {
                    sizes
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|s| one_line(s, 20))
                        .filter(|s| !s.is_empty())
                        .take(8)
                        .collect()
                })
                .unwrap_or_default();
            Some(ToolIcon {
                src: src.to_string(),
                mime_type,
                sizes,
            })
        })
        .take(MAX_ICONS)
        .collect()
}

#[derive(Debug, Clone)]
pub(crate) struct RawTool {
    pub name: String,
    pub title: Option<String>,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
    pub hints: ToolHints,
    pub icons: Vec<ToolIcon>,
}

/// A tool's `annotations` hints. The server sets them, so they can only make
/// the gate stricter: `readOnlyHint` never skips a card, `destructiveHint:
/// true` keeps the card even under Always.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolHints {
    pub read_only: bool,
    pub destructive: Option<bool>,
    pub idempotent: Option<bool>,
    pub open_world: Option<bool>,
}

impl ToolHints {
    /// The server said `destructiveHint: true`. An absent hint is not
    /// destructive here, though the spec's default is true.
    pub fn is_destructive(&self) -> bool {
        self.destructive == Some(true)
    }
}

/// A tool icon from `tools/list`: `src` is an `https:` URL or a `data:image/` URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolIcon {
    pub src: String,
    pub mime_type: Option<String>,
    pub sizes: Vec<String>,
}

pub(crate) fn initialize_params() -> Value {
    json!({
        "protocolVersion": LATEST,
        "capabilities": {
            "elicitation": { "form": {}, "url": {} }
        },
        "clientInfo": {
            "name": "GrokHub",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(version: &str) -> Value {
        json!({"protocolVersion": version, "capabilities": {}, "serverInfo": {"name": "s", "version": "0"}})
    }

    #[test]
    fn initialize_asks_for_the_newest_and_accepts_every_session_version() {
        assert_eq!(initialize_params()["protocolVersion"], "2025-11-25");
        assert_eq!(negotiate("box", &answer("2025-11-25")), Ok("2025-11-25"));
        assert_eq!(negotiate("box", &answer("2025-06-18")), Ok("2025-06-18"));
        assert_eq!(negotiate("box", &answer("2025-03-26")), Ok("2025-03-26"));
        assert_eq!(negotiate("box", &answer("2024-11-05")), Ok("2024-11-05"));
    }

    #[test]
    fn an_unknown_or_missing_version_is_a_clear_error() {
        assert_eq!(
            negotiate("box", &answer("2026-07-28")),
            Err("MCP server `box` answered with MCP version 2026-07-28; this client speaks 2025-11-25, 2025-06-18, 2025-03-26, 2024-11-05".to_string())
        );
        assert_eq!(
            negotiate("box", &answer("1.0\n\u{1b}[31m")),
            Err("MCP server `box` answered with MCP version 1.0[31m; this client speaks 2025-11-25, 2025-06-18, 2025-03-26, 2024-11-05".to_string())
        );
        assert_eq!(
            negotiate("box", &json!({"capabilities": {}})),
            Err("MCP server `box` did not say which MCP version it speaks".to_string())
        );
        assert_eq!(
            negotiate("box", &json!({"protocolVersion": 20251125})),
            Err("MCP server `box` did not say which MCP version it speaks".to_string())
        );
    }

    #[test]
    fn tools_list_keeps_title_output_schema_annotations_and_safe_icons() {
        let big = format!("data:image/png;base64,{}", "A".repeat(MAX_ICON_SRC));
        let (tools, cursor) = parse_tools(&json!({
            "tools": [
                {
                    "name": "get_weather",
                    "title": "Weather Information Provider",
                    "description": "Get current weather",
                    "inputSchema": {"type": "object", "properties": {"location": {"type": "string"}}},
                    "outputSchema": {"type": "object", "properties": {"temperature": {"type": "number"}}},
                    "annotations": {
                        "title": "Ignored when title is set",
                        "readOnlyHint": false,
                        "destructiveHint": true,
                        "idempotentHint": true,
                        "openWorldHint": false
                    },
                    "icons": [
                        {"src": "https://example.com/weather.png", "mimeType": "image/png", "sizes": ["48x48", "96x96"]},
                        {"src": "http://example.com/plain.png"},
                        {"src": "javascript:alert(1)"},
                        {"src": "data:text/html;base64,PGI+"},
                        {"src": "file:///etc/passwd"},
                        {"src": "https://example.com/has space.png"},
                        {"src": big},
                        {"src": "DATA:image/svg+xml;base64,PHN2Zy8+", "sizes": ["any"]},
                        {"mimeType": "image/png"}
                    ]
                },
                {
                    "name": "old_tool",
                    "description": "Older shape",
                    "readOnlyHint": true,
                    "annotations": {"title": "Old Tool"},
                    "outputSchema": "not an object",
                    "icons": "nope"
                },
                {
                    "name": "many_icons",
                    "inputSchema": {"type": "object"},
                    "icons": [
                        {"src": "https://a.example/1.png"},
                        {"src": "https://a.example/2.png"},
                        {"src": "https://a.example/3.png"},
                        {"src": "https://a.example/4.png"},
                        {"src": "https://a.example/5.png"}
                    ]
                }
            ],
            "nextCursor": " next "
        }));
        assert_eq!(cursor.as_deref(), Some("next"));
        assert_eq!(tools.len(), 3);

        let weather = &tools[0];
        assert_eq!(weather.name, "get_weather");
        assert_eq!(
            weather.title.as_deref(),
            Some("Weather Information Provider")
        );
        assert_eq!(
            weather.output_schema,
            Some(json!({"type": "object", "properties": {"temperature": {"type": "number"}}}))
        );
        assert_eq!(
            weather.hints,
            ToolHints {
                read_only: false,
                destructive: Some(true),
                idempotent: Some(true),
                open_world: Some(false),
            }
        );
        assert!(weather.hints.is_destructive());
        assert_eq!(
            weather.icons,
            vec![
                ToolIcon {
                    src: "https://example.com/weather.png".into(),
                    mime_type: Some("image/png".into()),
                    sizes: vec!["48x48".into(), "96x96".into()],
                },
                ToolIcon {
                    src: "DATA:image/svg+xml;base64,PHN2Zy8+".into(),
                    mime_type: None,
                    sizes: vec!["any".into()],
                },
            ]
        );

        let old = &tools[1];
        assert_eq!(old.title.as_deref(), Some("Old Tool"));
        assert_eq!(old.output_schema, None);
        assert_eq!(
            old.hints,
            ToolHints {
                read_only: true,
                ..ToolHints::default()
            }
        );
        assert!(!old.hints.is_destructive());
        assert!(old.icons.is_empty());
        assert_eq!(
            old.input_schema,
            json!({"type": "object", "properties": {}})
        );

        let srcs: Vec<&str> = tools[2].icons.iter().map(|i| i.src.as_str()).collect();
        assert_eq!(
            srcs,
            vec![
                "https://a.example/1.png",
                "https://a.example/2.png",
                "https://a.example/3.png",
                "https://a.example/4.png"
            ]
        );
        assert_eq!(tools[2].title, None);
    }

    #[test]
    fn structured_content_adds_one_compact_line_unless_a_text_block_copies_it() {
        let copied = json!({
            "content": [{"type": "text", "text": "{\"temperature\": 22.5, \"conditions\": \"Partly cloudy\"}"}],
            "structuredContent": {"temperature": 22.5, "conditions": "Partly cloudy"}
        });
        assert_eq!(
            tool_output(&copied),
            (
                "{\"temperature\": 22.5, \"conditions\": \"Partly cloudy\"}".to_string(),
                false
            )
        );
        let summary = json!({
            "content": [{"type": "text", "text": "Sunny, 22.5°C"}],
            "structuredContent": {"temperature": 22.5, "conditions": "Sunny"}
        });
        assert_eq!(
            tool_output(&summary).0,
            "Sunny, 22.5°C\nstructuredContent: {\"conditions\":\"Sunny\",\"temperature\":22.5}"
        );
        let bare = json!({"content": [], "structuredContent": {"ok": true}});
        assert_eq!(tool_output(&bare).0, "structuredContent: {\"ok\":true}");
        let failed = json!({
            "content": [{"type": "text", "text": "Invalid date"}],
            "structuredContent": {"field": "date"},
            "isError": true
        });
        assert_eq!(
            tool_output(&failed),
            (
                "Invalid date\nstructuredContent: {\"field\":\"date\"}".to_string(),
                true
            )
        );
    }

    #[test]
    fn resource_links_read_as_one_line_and_other_blocks_stay_markers() {
        let result = json!({
            "content": [
                {"type": "text", "text": "Found it"},
                {
                    "type": "resource_link",
                    "uri": "file:///project/src/main.rs",
                    "name": "main.rs",
                    "description": "Primary application entry point",
                    "mimeType": "text/x-rust"
                },
                {
                    "type": "resource_link",
                    "uri": "https://example.com/r/1",
                    "name": "r1",
                    "title": "Report\nOne"
                },
                {"type": "resource_link", "name": "no-uri"},
                {"type": "image", "data": "/w==", "mimeType": "image/png"},
                {"type": "audio", "data": "AA==", "mimeType": "audio/wav"},
                {"type": "resource", "resource": {"uri": "file:///a", "text": "x"}}
            ]
        });
        assert_eq!(
            tool_output(&result),
            (
                "Found it\n\
                 [resource_link] main.rs <file:///project/src/main.rs> (text/x-rust): Primary application entry point\n\
                 [resource_link] Report One (r1) <https://example.com/r/1>\n\
                 [resource_link] no-uri\n\
                 [image]\n[audio]\n[resource]"
                    .to_string(),
                false
            )
        );
        assert_eq!(
            tool_output(&json!({"content": []})),
            ("(empty)".to_string(), false)
        );
    }
}
