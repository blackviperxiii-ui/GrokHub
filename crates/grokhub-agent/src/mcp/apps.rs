//! MCP Apps (SEP-1865): tools that point at a `ui://` HTML resource the host
//! renders in a sandboxed iframe. Helpers parse `_meta.ui`, check visibility,
//! decode the resource, and build the host half of the postMessage handshake.
//! No I/O — the client reads the resource and the UI hosts the iframe.

use serde_json::{json, Value};

/// Official MCP Apps MIME type for HTML UI resources.
pub const MIME: &str = "text/html;profile=mcp-app";

/// Extension id hosts advertise in client capabilities / discover `_meta`.
pub const EXTENSION: &str = "io.modelcontextprotocol/ui";

/// MCP Apps protocol version for `ui/initialize`.
pub const PROTOCOL_VERSION: &str = "2026-01-26";

/// Longest HTML document kept for an in-chat app (512 KiB).
pub const MAX_HTML: usize = 512 * 1024;

/// Who may call a tool that carries `_meta.ui`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Model,
    App,
}

/// `_meta.ui` on a tool from `tools/list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiMeta {
    pub resource_uri: String,
    /// Empty means the default: both model and app.
    pub visibility: Vec<Visibility>,
}

impl UiMeta {
    pub fn model_visible(&self) -> bool {
        self.visibility.is_empty() || self.visibility.contains(&Visibility::Model)
    }

    pub fn app_visible(&self) -> bool {
        self.visibility.is_empty() || self.visibility.contains(&Visibility::App)
    }
}

/// A decoded UI resource ready for the chat host.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpAppView {
    pub server: String,
    pub tool: String,
    pub resource_uri: String,
    pub html: String,
    pub prefers_border: bool,
    /// Tool arguments the host sends as `ui/notifications/tool-input`.
    pub tool_input: Value,
    /// The `tools/call` result the host sends as `ui/notifications/tool-result`.
    pub tool_result: Value,
}

/// Parse `_meta.ui` from a `tools/list` tool item. `None` when there is no
/// usable `resourceUri` under `ui://`.
pub fn parse_ui_meta(item: &Value) -> Option<UiMeta> {
    let ui = item.pointer("/_meta/ui")?;
    let uri = ui
        .get("resourceUri")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|u| u.starts_with("ui://") && u.len() > "ui://".len())?;
    if uri.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    let visibility = match ui.get("visibility").and_then(Value::as_array) {
        Some(items) => items
            .iter()
            .filter_map(Value::as_str)
            .filter_map(|v| match v {
                "model" => Some(Visibility::Model),
                "app" => Some(Visibility::App),
                _ => None,
            })
            .collect(),
        None => Vec::new(),
    };
    Some(UiMeta {
        resource_uri: uri.to_string(),
        visibility,
    })
}

/// Whether a tool with this meta (or none) is offered to the model.
pub fn model_visible(ui: Option<&UiMeta>) -> bool {
    ui.map(UiMeta::model_visible).unwrap_or(true)
}

/// Whether a tool with this meta (or none) may be called from the app iframe.
pub fn app_visible(ui: Option<&UiMeta>) -> bool {
    ui.map(UiMeta::app_visible).unwrap_or(false)
}

/// Decode a `resources/read` result into HTML for an MCP App. Requires a
/// `ui://` URI and the MCP Apps MIME type.
pub fn decode_resource(uri: &str, result: &Value) -> Result<(String, bool), String> {
    if !uri.starts_with("ui://") {
        return Err("MCP App resource must use a ui:// URI".into());
    }
    let contents = result
        .get("contents")
        .and_then(Value::as_array)
        .ok_or_else(|| "resources/read returned no contents".to_string())?;
    let item = contents
        .iter()
        .find(|c| c.get("uri").and_then(Value::as_str) == Some(uri))
        .or_else(|| contents.first())
        .ok_or_else(|| "resources/read returned empty contents".to_string())?;
    let mime = item
        .get("mimeType")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if mime != MIME {
        return Err(format!("MCP App resource mimeType must be {MIME}"));
    }
    let html = if let Some(text) = item.get("text").and_then(Value::as_str) {
        text.to_string()
    } else if let Some(blob) = item.get("blob").and_then(Value::as_str) {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(blob.trim())
            .map_err(|_| "MCP App resource blob was not valid base64".to_string())?;
        String::from_utf8(bytes)
            .map_err(|_| "MCP App resource blob was not UTF-8 HTML".to_string())?
    } else {
        return Err("MCP App resource had neither text nor blob".into());
    };
    if html.trim().is_empty() {
        return Err("MCP App resource HTML was empty".into());
    }
    if html.len() > MAX_HTML {
        return Err(format!("MCP App HTML is larger than {MAX_HTML} bytes"));
    }
    let prefers_border = item
        .pointer("/_meta/ui/prefersBorder")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok((html, prefers_border))
}

/// Default CSP for an MCP App with no `_meta.ui.csp` (block network).
pub fn default_csp() -> String {
    [
        "default-src 'none'",
        "script-src 'unsafe-inline' 'unsafe-eval'",
        "style-src 'unsafe-inline'",
        "img-src data: blob:",
        "font-src data:",
        "connect-src 'none'",
        "frame-ancestors 'none'",
        "base-uri 'none'",
        "form-action 'none'",
    ]
    .join("; ")
}

/// HTML wrapped with a CSP meta tag when the document has no Content-Security-Policy.
pub fn with_csp(html: &str, csp: &str) -> String {
    if html
        .to_ascii_lowercase()
        .contains("content-security-policy")
    {
        return html.to_string();
    }
    let meta = format!("<meta http-equiv=\"Content-Security-Policy\" content=\"{csp}\">");
    if let Some(at) = html.to_ascii_lowercase().find("<head>") {
        let insert = at + "<head>".len();
        let mut out = String::with_capacity(html.len() + meta.len());
        out.push_str(&html[..insert]);
        out.push_str(&meta);
        out.push_str(&html[insert..]);
        out
    } else {
        format!("<!DOCTYPE html><html><head>{meta}</head><body>{html}</body></html>")
    }
}

/// Result payload for a view's `ui/initialize` request.
pub fn host_initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "hostInfo": {
            "name": "GrokHub",
            "version": env!("CARGO_PKG_VERSION")
        },
        "hostCapabilities": {
            "serverTools": {},
            "serverResources": {}
        },
        "hostContext": {
            "theme": "system",
            "displayMode": "inline"
        }
    })
}

/// Client capability object fragment that advertises MCP Apps support.
pub fn client_capability() -> Value {
    json!({
        "mimeTypes": [MIME]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_ui_meta_reads_uri_and_visibility() {
        let item = json!({
            "name": "show",
            "_meta": {
                "ui": {
                    "resourceUri": "ui://inventory/dashboard",
                    "visibility": ["model", "app"]
                }
            }
        });
        let meta = parse_ui_meta(&item).unwrap();
        assert_eq!(meta.resource_uri, "ui://inventory/dashboard");
        assert!(meta.model_visible());
        assert!(meta.app_visible());
    }

    #[test]
    fn app_only_tools_are_hidden_from_the_model() {
        let item = json!({
            "name": "restock",
            "_meta": { "ui": { "resourceUri": "ui://x/y", "visibility": ["app"] } }
        });
        let meta = parse_ui_meta(&item).unwrap();
        assert!(!meta.model_visible());
        assert!(meta.app_visible());
        assert!(!model_visible(Some(&meta)));
        assert!(app_visible(Some(&meta)));
    }

    #[test]
    fn decode_resource_accepts_text_html_profile() {
        let result = json!({
            "contents": [{
                "uri": "ui://demo/app",
                "mimeType": MIME,
                "text": "<!DOCTYPE html><html><body>hi</body></html>",
                "_meta": { "ui": { "prefersBorder": true } }
            }]
        });
        let (html, border) = decode_resource("ui://demo/app", &result).unwrap();
        assert!(html.contains("hi"));
        assert!(border);
    }

    #[test]
    fn decode_resource_rejects_wrong_mime() {
        let result = json!({
            "contents": [{
                "uri": "ui://demo/app",
                "mimeType": "text/html",
                "text": "<html></html>"
            }]
        });
        let err = decode_resource("ui://demo/app", &result).unwrap_err();
        assert!(err.contains(MIME), "{err}");
    }

    #[test]
    fn with_csp_injects_into_head() {
        let html = with_csp(
            "<html><head></head><body>x</body></html>",
            "default-src 'none'",
        );
        assert!(html.contains("Content-Security-Policy"));
        assert!(html.contains("default-src 'none'"));
    }

    #[test]
    fn host_initialize_names_grokhub() {
        let result = host_initialize_result();
        assert_eq!(result["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(result["hostInfo"]["name"], "GrokHub");
        assert!(result["hostCapabilities"].get("serverTools").is_some());
    }
}
