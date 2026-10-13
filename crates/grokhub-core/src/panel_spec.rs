//! Declarative generative-UI panel spec: a model emits JSON describing a small
//! read-mostly panel (headings, text, lists, tables, key-value rows, buttons,
//! inputs, progress). This module validates shape and safety only; the cabin
//! paints it later and buttons reference action ids — they never run commands
//! directly, so hard approvals stay on the existing path.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Longest serialized panel JSON the parser accepts.
pub const PANEL_JSON_MAX: usize = 24_000;
/// Most top-level elements in one panel.
pub const ELEMENTS_MAX: usize = 48;
/// Longest user-visible string (body text, table cells, list items).
pub const TEXT_MAX: usize = 2_000;
/// Longest label, title, key, or short id.
pub const SHORT_MAX: usize = 120;
/// List items per list element.
pub const LIST_ITEMS_MAX: usize = 32;
/// Columns per table (including header).
pub const TABLE_COLS_MAX: usize = 8;
/// Data rows per table (header not counted).
pub const TABLE_ROWS_MAX: usize = 32;
/// Rows per key-value element.
pub const KV_PAIRS_MAX: usize = 24;
/// Nesting depth of the parsed JSON value (arrays/objects).
pub const JSON_MAX_DEPTH: usize = 16;
/// Longest `action` id on a button.
pub const ACTION_ID_MAX: usize = 64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PanelSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub elements: Vec<PanelElement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PanelElement {
    Heading(HeadingEl),
    Text(TextEl),
    List(ListEl),
    Table(TableEl),
    KeyValue(KeyValueEl),
    Button(ButtonEl),
    Input(InputEl),
    Progress(ProgressEl),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeadingEl {
    #[serde(default = "default_heading_level")]
    pub level: u8,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextEl {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListEl {
    #[serde(default)]
    pub ordered: bool,
    pub items: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TableEl {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyValueEl {
    pub pairs: Vec<KeyValuePair>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ButtonEl {
    pub label: String,
    /// Approval-gated action id only; never a URL or shell command.
    pub action: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputEl {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgressEl {
    pub label: String,
    /// Fraction complete, 0.0..=1.0.
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyValuePair {
    pub key: String,
    pub value: String,
}

fn default_heading_level() -> u8 {
    2
}

/// Parse and validate panel JSON. Unknown fields are rejected at deserialize time.
pub fn parse_panel_spec(json: &str) -> Result<PanelSpec, String> {
    if json.len() > PANEL_JSON_MAX {
        return Err(format!(
            "panel JSON is over {PANEL_JSON_MAX} bytes (got {len})",
            len = json.len()
        ));
    }
    let value: Value = serde_json::from_str(json).map_err(|e| format!("panel JSON: {e}"))?;
    let depth = json_value_depth(&value);
    if depth > JSON_MAX_DEPTH {
        return Err(format!(
            "panel JSON nesting is {depth} levels; max is {JSON_MAX_DEPTH}"
        ));
    }
    let spec: PanelSpec = serde_json::from_value(value).map_err(|e| format!("panel spec: {e}"))?;
    validate_panel_spec(&spec)?;
    Ok(spec)
}

/// Re-check a parsed spec (for callers that deserialized elsewhere).
pub fn validate_panel_spec(spec: &PanelSpec) -> Result<(), String> {
    if let Some(title) = &spec.title {
        short_text(title, "title")?;
    }
    if spec.elements.is_empty() {
        return Err("panel must include at least one element".into());
    }
    if spec.elements.len() > ELEMENTS_MAX {
        return Err(format!(
            "panel has {n} elements; max is {ELEMENTS_MAX}",
            n = spec.elements.len()
        ));
    }
    for (i, el) in spec.elements.iter().enumerate() {
        validate_element(el).map_err(|e| format!("element {i}: {e}"))?;
    }
    Ok(())
}

fn validate_element(el: &PanelElement) -> Result<(), String> {
    match el {
        PanelElement::Heading(h) => {
            if !(1..=3).contains(&h.level) {
                return Err(format!(
                    "heading level must be 1..=3 (got {level})",
                    level = h.level
                ));
            }
            body_text(&h.text, "heading")?;
        }
        PanelElement::Text(t) => body_text(&t.text, "text")?,
        PanelElement::List(l) => {
            if l.items.is_empty() {
                return Err("list must include at least one item".into());
            }
            if l.items.len() > LIST_ITEMS_MAX {
                return Err(format!(
                    "list has {n} items; max is {LIST_ITEMS_MAX}",
                    n = l.items.len()
                ));
            }
            for (j, item) in l.items.iter().enumerate() {
                body_text(item, &format!("list item {j}"))?;
            }
        }
        PanelElement::Table(t) => {
            if t.headers.is_empty() {
                return Err("table must include at least one header".into());
            }
            if t.headers.len() > TABLE_COLS_MAX {
                return Err(format!(
                    "table has {n} columns; max is {TABLE_COLS_MAX}",
                    n = t.headers.len()
                ));
            }
            for (j, h) in t.headers.iter().enumerate() {
                short_text(h, &format!("table header {j}"))?;
            }
            if t.rows.len() > TABLE_ROWS_MAX {
                return Err(format!(
                    "table has {n} rows; max is {TABLE_ROWS_MAX}",
                    n = t.rows.len()
                ));
            }
            let cols = t.headers.len();
            for (r, row) in t.rows.iter().enumerate() {
                if row.len() != cols {
                    return Err(format!(
                        "table row {r} has {got} cells; expected {cols}",
                        got = row.len()
                    ));
                }
                for (c, cell) in row.iter().enumerate() {
                    body_text(cell, &format!("table cell {r},{c}"))?;
                }
            }
        }
        PanelElement::KeyValue(kv) => {
            if kv.pairs.is_empty() {
                return Err("key_value must include at least one pair".into());
            }
            if kv.pairs.len() > KV_PAIRS_MAX {
                return Err(format!(
                    "key_value has {n} pairs; max is {KV_PAIRS_MAX}",
                    n = kv.pairs.len()
                ));
            }
            for (j, pair) in kv.pairs.iter().enumerate() {
                short_text(&pair.key, &format!("key {j}"))?;
                body_text(&pair.value, &format!("value {j}"))?;
            }
        }
        PanelElement::Button(b) => {
            short_text(&b.label, "button label")?;
            action_id(&b.action, "button action")?;
        }
        PanelElement::Input(i) => {
            action_id(&i.id, "input id")?;
            short_text(&i.label, "input label")?;
            if let Some(p) = &i.placeholder {
                short_text(p, "input placeholder")?;
            }
            if let Some(v) = &i.value {
                body_text(v, "input value")?;
            }
        }
        PanelElement::Progress(p) => {
            short_text(&p.label, "progress label")?;
            if !p.value.is_finite() || !(0.0..=1.0).contains(&p.value) {
                return Err(format!(
                    "progress value must be a finite number from 0.0 to 1.0 (got {value})",
                    value = p.value
                ));
            }
        }
    }
    Ok(())
}

fn short_text(s: &str, field: &str) -> Result<(), String> {
    let t = s.trim();
    if t.is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if t.chars().count() > SHORT_MAX {
        return Err(format!(
            "{field} is over {SHORT_MAX} characters ({n})",
            n = t.chars().count()
        ));
    }
    plain_line(s, field)?;
    Ok(())
}

fn body_text(s: &str, field: &str) -> Result<(), String> {
    if s.chars().count() > TEXT_MAX {
        return Err(format!(
            "{field} is over {TEXT_MAX} characters ({n})",
            n = s.chars().count()
        ));
    }
    plain_line(s, field)?;
    Ok(())
}

fn plain_line(s: &str, field: &str) -> Result<(), String> {
    if s.contains('<') || s.contains('>') {
        return Err(format!("{field} must not contain raw HTML"));
    }
    let lower = s.to_ascii_lowercase();
    if lower.contains("<script") || lower.contains("javascript:") {
        return Err(format!("{field} must not contain scripts"));
    }
    scan_urls(s, field)?;
    Ok(())
}

fn scan_urls(s: &str, field: &str) -> Result<(), String> {
    let mut i = 0usize;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        if s[i..].starts_with("http://") {
            return Err(format!("{field} must use https URLs, not http"));
        }
        if let Some(rest) = s[i..].strip_prefix("https://") {
            let url = take_url(rest);
            https_url(url).map_err(|e| format!("{field}: {e}"))?;
            i += "https://".len() + url.len();
            continue;
        }
        if s[i..].starts_with("//") {
            return Err(format!("{field} must not use protocol-relative URLs"));
        }
        i += 1;
    }
    Ok(())
}

fn take_url(rest: &str) -> &str {
    rest.split(|c: char| c.is_whitespace() || matches!(c, ')' | ']' | '"' | '\'' | '<' | '>'))
        .next()
        .unwrap_or(rest)
}

fn https_url(url: &str) -> Result<(), String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("https URL is empty".into());
    }
    let authority = url.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return Err("https URL is missing a host".into());
    }
    if url.contains("javascript:") {
        return Err("https URL must not contain javascript:".into());
    }
    Ok(())
}

fn action_id(s: &str, field: &str) -> Result<(), String> {
    let id = s.trim();
    if id.is_empty() || id.chars().count() > ACTION_ID_MAX {
        return Err(format!(
            "{field} must be 1..={ACTION_ID_MAX} characters (got {n})",
            n = id.chars().count()
        ));
    }
    let mut chars = id.chars();
    let first = chars
        .next()
        .ok_or_else(|| format!("{field} must not be empty"))?;
    if !first.is_ascii_lowercase() {
        return Err(format!(
            "{field} must start with a lowercase letter (got {:?})",
            first
        ));
    }
    if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_')) {
        return Err(format!(
            "{field} may only use lowercase letters, digits, dashes, and underscores"
        ));
    }
    Ok(())
}

fn json_value_depth(v: &Value) -> usize {
    match v {
        Value::Array(items) => 1 + items.iter().map(json_value_depth).max().unwrap_or(0),
        Value::Object(map) => 1 + map.values().map(json_value_depth).max().unwrap_or(0),
        _ => 1,
    }
}

/// JSON Schema for prompts (draft 2020-12). Buttons name action ids only.
pub fn panel_spec_json_schema() -> Value {
    serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://grokhub.local/schemas/panel_spec.json",
        "title": "GrokHubPanelSpec",
        "description": "A small declarative UI panel. Buttons reference action ids; the cabin runs actions only through the normal approval flow.",
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "title": { "type": "string", "maxLength": SHORT_MAX },
            "elements": {
                "type": "array",
                "minItems": 1,
                "maxItems": ELEMENTS_MAX,
                "items": { "$ref": "#/$defs/element" }
            }
        },
        "required": ["elements"],
        "$defs": {
            "plainText": {
                "type": "string",
                "maxLength": TEXT_MAX,
                "description": "Plain text only: no HTML tags or scripts; http:// URLs are forbidden; https:// URLs are allowed."
            },
            "shortText": {
                "type": "string",
                "maxLength": SHORT_MAX
            },
            "actionId": {
                "type": "string",
                "maxLength": ACTION_ID_MAX,
                "pattern": "^[a-z][a-z0-9_-]*$"
            },
            "element": {
                "oneOf": [
                    {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["type", "text"],
                        "properties": {
                            "type": { "const": "heading" },
                            "level": { "type": "integer", "minimum": 1, "maximum": 3, "default": 2 },
                            "text": { "$ref": "#/$defs/plainText" }
                        }
                    },
                    {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["type", "text"],
                        "properties": {
                            "type": { "const": "text" },
                            "text": { "$ref": "#/$defs/plainText" }
                        }
                    },
                    {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["type", "items"],
                        "properties": {
                            "type": { "const": "list" },
                            "ordered": { "type": "boolean", "default": false },
                            "items": {
                                "type": "array",
                                "minItems": 1,
                                "maxItems": LIST_ITEMS_MAX,
                                "items": { "$ref": "#/$defs/plainText" }
                            }
                        }
                    },
                    {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["type", "headers", "rows"],
                        "properties": {
                            "type": { "const": "table" },
                            "headers": {
                                "type": "array",
                                "minItems": 1,
                                "maxItems": TABLE_COLS_MAX,
                                "items": { "$ref": "#/$defs/shortText" }
                            },
                            "rows": {
                                "type": "array",
                                "maxItems": TABLE_ROWS_MAX,
                                "items": {
                                    "type": "array",
                                    "items": { "$ref": "#/$defs/plainText" }
                                }
                            }
                        }
                    },
                    {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["type", "pairs"],
                        "properties": {
                            "type": { "const": "key_value" },
                            "pairs": {
                                "type": "array",
                                "minItems": 1,
                                "maxItems": KV_PAIRS_MAX,
                                "items": {
                                    "type": "object",
                                    "additionalProperties": false,
                                    "required": ["key", "value"],
                                    "properties": {
                                        "key": { "$ref": "#/$defs/shortText" },
                                        "value": { "$ref": "#/$defs/plainText" }
                                    }
                                }
                            }
                        }
                    },
                    {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["type", "label", "action"],
                        "properties": {
                            "type": { "const": "button" },
                            "label": { "$ref": "#/$defs/shortText" },
                            "action": { "$ref": "#/$defs/actionId" }
                        }
                    },
                    {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["type", "id", "label"],
                        "properties": {
                            "type": { "const": "input" },
                            "id": { "$ref": "#/$defs/actionId" },
                            "label": { "$ref": "#/$defs/shortText" },
                            "placeholder": { "$ref": "#/$defs/shortText" },
                            "value": { "$ref": "#/$defs/plainText" }
                        }
                    },
                    {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["type", "label", "value"],
                        "properties": {
                            "type": { "const": "progress" },
                            "label": { "$ref": "#/$defs/shortText" },
                            "value": { "type": "number", "minimum": 0.0, "maximum": 1.0 }
                        }
                    }
                ]
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_json() -> &'static str {
        r#"{
  "title": "Disk check",
  "elements": [
    { "type": "heading", "level": 2, "text": "Summary" },
    { "type": "text", "text": "Root is 72% full. Details: https://example.com/docs" },
    { "type": "list", "ordered": true, "items": ["Check logs", "Archive old builds"] },
    {
      "type": "table",
      "headers": ["Mount", "Use"],
      "rows": [["/", "72%"], ["/home", "41%"]]
    },
    {
      "type": "key_value",
      "pairs": [{ "key": "Host", "value": "beast" }]
    },
    { "type": "button", "label": "Approve cleanup", "action": "run-cleanup" },
    {
      "type": "input",
      "id": "note",
      "label": "Note",
      "placeholder": "Optional",
      "value": ""
    },
    { "type": "progress", "label": "Scan", "value": 0.25 }
  ]
}"#
    }

    #[test]
    fn valid_spec_round_trips() {
        let spec = parse_panel_spec(sample_json()).expect("sample parses");
        let again = serde_json::to_string(&spec).expect("serialize");
        let round = parse_panel_spec(&again).expect("re-parse");
        assert_eq!(spec, round);
    }

    #[test]
    fn minimal_spec_round_trips() {
        let json = r#"{"elements":[{"type":"text","text":"Hello"}]}"#;
        let spec = parse_panel_spec(json).unwrap();
        let round = parse_panel_spec(&serde_json::to_string(&spec).unwrap()).unwrap();
        assert_eq!(spec, round);
    }

    #[test]
    fn json_schema_exports_for_prompts() {
        let schema = panel_spec_json_schema();
        assert_eq!(schema["title"], "GrokHubPanelSpec");
        assert!(schema["$defs"]["element"]["oneOf"].is_array());
        let s = serde_json::to_string(&schema).unwrap();
        assert!(s.contains("key_value"));
        assert!(s.contains("actionId"));
    }

    #[test]
    fn rejects_unknown_top_level_field() {
        let err =
            parse_panel_spec(r#"{"extra":1,"elements":[{"type":"text","text":"x"}]}"#).unwrap_err();
        assert!(err.contains("unknown field"), "{err}");
    }

    #[test]
    fn rejects_unknown_element_field() {
        let err =
            parse_panel_spec(r#"{"elements":[{"type":"text","text":"ok","onclick":"evil"}]}"#)
                .unwrap_err();
        assert!(err.contains("unknown field"), "{err}");
    }

    #[test]
    fn rejects_unknown_element_type() {
        let err = parse_panel_spec(r#"{"elements":[{"type":"chart","text":"nope"}]}"#).unwrap_err();
        assert!(err.contains("panel spec"), "{err}");
    }

    #[test]
    fn rejects_oversized_json() {
        let big = "a".repeat(PANEL_JSON_MAX + 1);
        let json = format!(r#"{{"elements":[{{"type":"text","text":"{big}"}}]}}"#);
        let err = parse_panel_spec(&json).unwrap_err();
        assert!(err.contains("over"), "{err}");
    }

    #[test]
    fn rejects_deep_json() {
        let mut value = serde_json::json!({
            "elements": [{ "type": "text", "text": "leaf" }]
        });
        for _ in 0..JSON_MAX_DEPTH {
            value = serde_json::json!({ "nested": value });
        }
        let err = parse_panel_spec(&value.to_string()).unwrap_err();
        assert!(err.contains("nesting"), "{err}");
    }

    #[test]
    fn rejects_too_many_elements() {
        let items = (0..ELEMENTS_MAX + 1)
            .map(|_| r#"{"type":"text","text":"x"}"#)
            .collect::<Vec<_>>()
            .join(",");
        let json = format!(r#"{{"elements":[{items}]}}"#);
        let err = parse_panel_spec(&json).unwrap_err();
        assert!(err.contains("elements"), "{err}");
    }

    #[test]
    fn rejects_html_in_text() {
        let err =
            parse_panel_spec(r#"{"elements":[{"type":"text","text":"<b>x</b>"}]}"#).unwrap_err();
        assert!(err.contains("HTML"), "{err}");
    }

    #[test]
    fn rejects_script_in_text() {
        let err = parse_panel_spec(r#"{"elements":[{"type":"text","text":"<script>alert(1)"}]}"#)
            .unwrap_err();
        assert!(err.contains("HTML") || err.contains("script"), "{err}");
    }

    #[test]
    fn rejects_javascript_url_in_text() {
        let err =
            parse_panel_spec(r#"{"elements":[{"type":"text","text":"javascript:alert(1)"}]}"#)
                .unwrap_err();
        assert!(err.contains("script"), "{err}");
    }

    #[test]
    fn rejects_http_url_in_text() {
        let err =
            parse_panel_spec(r#"{"elements":[{"type":"text","text":"see http://example.com"}]}"#)
                .unwrap_err();
        assert!(err.contains("https"), "{err}");
    }

    #[test]
    fn rejects_protocol_relative_url() {
        let err = parse_panel_spec(r#"{"elements":[{"type":"text","text":"//cdn.example/x"}]}"#)
            .unwrap_err();
        assert!(err.contains("protocol-relative"), "{err}");
    }

    #[test]
    fn rejects_button_without_action_id_shape() {
        let err =
            parse_panel_spec(r#"{"elements":[{"type":"button","label":"Go","action":"Bad Id"}]}"#)
                .unwrap_err();
        assert!(err.contains("action"), "{err}");
    }

    #[test]
    fn rejects_progress_out_of_range() {
        let err = parse_panel_spec(r#"{"elements":[{"type":"progress","label":"x","value":1.5}]}"#)
            .unwrap_err();
        assert!(err.contains("progress"), "{err}");
    }

    #[test]
    fn rejects_table_column_mismatch() {
        let err = parse_panel_spec(
            r#"{"elements":[{"type":"table","headers":["a","b"],"rows":[["only-one"]]}]}"#,
        )
        .unwrap_err();
        assert!(err.contains("cells"), "{err}");
    }

    #[test]
    fn rejects_empty_elements() {
        let err = parse_panel_spec(r#"{"elements":[]}"#).unwrap_err();
        assert!(err.contains("at least one"), "{err}");
    }

    #[test]
    fn rejects_heading_level_out_of_range() {
        let err = parse_panel_spec(r#"{"elements":[{"type":"heading","level":4,"text":"Nope"}]}"#)
            .unwrap_err();
        assert!(err.contains("heading level"), "{err}");
    }

    #[test]
    fn rejects_body_over_text_max() {
        let body = "a".repeat(TEXT_MAX + 1);
        let json = format!(r#"{{"elements":[{{"type":"text","text":"{body}"}}]}}"#);
        let err = parse_panel_spec(&json).unwrap_err();
        assert!(err.contains("over"), "{err}");
    }
}
