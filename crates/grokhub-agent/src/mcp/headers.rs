//! Streamable HTTP request headers of the stateless era (MCP 2026-07-28,
//! "Request Metadata"): `Mcp-Method`, `Mcp-Name`, and the `Mcp-Param-*`
//! headers a tool asks for with `x-mcp-header` in its `inputSchema`.

use base64::Engine as _;
use serde_json::Value;

/// One `x-mcp-header` annotation: where the value sits in the arguments
/// (a chain of `properties` keys) and the header's name part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParamHeader {
    pub path: Vec<String>,
    pub name: String,
}

/// Deepest schema nesting walked for annotations.
const MAX_DEPTH: usize = 32;

/// Keywords whose values are data, not schemas.
const DATA_KEYWORDS: [&str; 4] = ["default", "examples", "const", "enum"];

/// The tool's `x-mcp-header` annotations, or why the tool must be left out:
/// an annotation off a plain `properties` chain, on a non-primitive (or
/// `number`) property, empty, not an HTTP token, or repeated (any case).
pub(crate) fn param_headers(schema: &Value) -> Result<Vec<ParamHeader>, String> {
    let mut found = Vec::new();
    walk(schema, Some(Vec::new()), 0, &mut found)?;
    found.sort_by(|a, b| a.name.cmp(&b.name));
    let mut seen: Vec<String> = Vec::new();
    for header in &found {
        let lower = header.name.to_ascii_lowercase();
        if seen.contains(&lower) {
            return Err(format!("x-mcp-header {} is repeated", header.name));
        }
        seen.push(lower);
    }
    Ok(found)
}

/// `path` is `Some` while the chain from the root is `properties` keys only.
fn walk(
    node: &Value,
    path: Option<Vec<String>>,
    depth: usize,
    found: &mut Vec<ParamHeader>,
) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    match node {
        Value::Array(items) => {
            for item in items {
                walk(item, None, depth + 1, found)?;
            }
            Ok(())
        }
        Value::Object(map) => {
            if let Some(name) = map.get("x-mcp-header") {
                found.push(annotation(map, name, path.as_deref())?);
            }
            for (key, value) in map {
                if DATA_KEYWORDS.contains(&key.as_str()) || key == "x-mcp-header" {
                    continue;
                }
                if key == "properties" {
                    if let Some(props) = value.as_object() {
                        for (prop, schema) in props {
                            let next = path.as_ref().map(|p| {
                                let mut p = p.clone();
                                p.push(prop.clone());
                                p
                            });
                            walk(schema, next, depth + 1, found)?;
                        }
                        continue;
                    }
                }
                walk(value, None, depth + 1, found)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn annotation(
    schema: &serde_json::Map<String, Value>,
    name: &Value,
    path: Option<&[String]>,
) -> Result<ParamHeader, String> {
    let name = name.as_str().unwrap_or("");
    let Some(path) = path.filter(|p| !p.is_empty()) else {
        return Err(format!(
            "x-mcp-header {} is not on a plain properties path",
            shown(name)
        ));
    };
    if name.is_empty() || !name.bytes().all(is_tchar) {
        return Err(format!("x-mcp-header {} is not a header name", shown(name)));
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("string" | "integer" | "boolean") => Ok(ParamHeader {
            path: path.to_vec(),
            name: name.to_string(),
        }),
        _ => Err(format!(
            "x-mcp-header {name} is on a property that is not a string, integer or boolean"
        )),
    }
}

fn shown(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(40)
        .collect()
}

/// RFC 9110 `tchar`.
fn is_tchar(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}

/// Largest integer sent in a header (JavaScript's safe range).
const SAFE_INT: i64 = (1 << 53) - 1;

/// `Mcp-Param-{name}` for each annotation whose argument is present: strings
/// as is, integers in decimal, booleans as `true` / `false`, then
/// [`encode`]. A missing or `null` value (or one of another type) sends none.
pub(crate) fn param_values(headers: &[ParamHeader], args: &Value) -> Vec<(String, String)> {
    headers
        .iter()
        .filter_map(|header| {
            let mut value = args;
            for key in &header.path {
                value = value.get(key)?;
            }
            let text = match value {
                Value::String(text) => text.clone(),
                Value::Bool(flag) => flag.to_string(),
                Value::Number(n) => n
                    .as_i64()
                    .filter(|n| (-SAFE_INT..=SAFE_INT).contains(n))?
                    .to_string(),
                _ => return None,
            };
            Some((format!("Mcp-Param-{}", header.name), encode(&text)))
        })
        .collect()
}

/// A header value as is when it is plain visible ASCII (space and tab
/// inside), else `=?base64?…?=` of its UTF-8. A plain value that looks like
/// the sentinel is encoded too.
pub(crate) fn encode(value: &str) -> String {
    let plain = value
        .bytes()
        .all(|b| b == b'\t' || (0x20..=0x7e).contains(&b))
        && !value.starts_with([' ', '\t'])
        && !value.ends_with([' ', '\t'])
        && !(value.starts_with("=?base64?") && value.ends_with("?="));
    if plain {
        value.to_string()
    } else {
        let packed = base64::engine::general_purpose::STANDARD.encode(value.as_bytes());
        format!("=?base64?{packed}?=")
    }
}

/// `Mcp-Method`, plus `Mcp-Name` for the methods that name a tool, resource
/// or prompt, from a JSON-RPC request body. A notification gets none.
pub(crate) fn standard(body: &Value) -> Vec<(String, String)> {
    let Some(method) = body.get("method").and_then(Value::as_str) else {
        return Vec::new();
    };
    if body.get("id").is_none() {
        return Vec::new();
    }
    let mut out = vec![("Mcp-Method".to_string(), method.to_string())];
    let key = match method {
        "tools/call" | "prompts/get" => "name",
        "resources/read" => "uri",
        _ => return out,
    };
    if let Some(name) = body
        .pointer(&format!("/params/{key}"))
        .and_then(Value::as_str)
    {
        out.push(("Mcp-Name".to_string(), encode(name)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn header_values_follow_the_spec_encoding_table() {
        assert_eq!(encode("us-west1"), "us-west1");
        assert_eq!(encode("Hello, 世界"), "=?base64?SGVsbG8sIOS4lueVjA==?=");
        assert_eq!(encode(" padded "), "=?base64?IHBhZGRlZCA=?=");
        assert_eq!(encode("line1\nline2"), "=?base64?bGluZTEKbGluZTI=?=");
        assert_eq!(
            encode("=?base64?literal?="),
            "=?base64?PT9iYXNlNjQ/bGl0ZXJhbD89?="
        );
    }

    #[test]
    fn standard_headers_name_the_method_and_the_tool() {
        let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "get_weather"}});
        assert_eq!(
            standard(&call),
            vec![
                ("Mcp-Method".to_string(), "tools/call".to_string()),
                ("Mcp-Name".to_string(), "get_weather".to_string())
            ]
        );
        let read = json!({"id": 2, "method": "resources/read", "params": {"uri": "file:///projects/myapp/config.json"}});
        assert_eq!(
            standard(&read)[1],
            (
                "Mcp-Name".to_string(),
                "file:///projects/myapp/config.json".to_string()
            )
        );
        let list = json!({"id": 3, "method": "tools/list", "params": {}});
        assert_eq!(
            standard(&list),
            vec![("Mcp-Method".to_string(), "tools/list".to_string())]
        );
        let note = json!({"method": "notifications/cancelled", "params": {}});
        assert!(standard(&note).is_empty());
    }

    #[test]
    fn x_mcp_header_annotations_become_param_headers() {
        let schema = json!({
            "type": "object",
            "properties": {
                "region": {"type": "string", "x-mcp-header": "Region"},
                "query": {"type": "string"},
                "limit": {"type": "integer", "x-mcp-header": "Limit"},
                "opts": {"type": "object", "properties": {
                    "dry": {"type": "boolean", "x-mcp-header": "Dry-Run"}
                }},
                "note": {"type": "string", "default": {"x-mcp-header": "Ignored"}}
            },
            "required": ["region", "query"]
        });
        let headers = param_headers(&schema).unwrap();
        assert_eq!(
            headers,
            vec![
                ParamHeader {
                    path: vec!["opts".into(), "dry".into()],
                    name: "Dry-Run".into()
                },
                ParamHeader {
                    path: vec!["limit".into()],
                    name: "Limit".into()
                },
                ParamHeader {
                    path: vec!["region".into()],
                    name: "Region".into()
                },
            ]
        );
        let args =
            json!({"region": "us-west1", "query": "SELECT 1", "limit": 42, "opts": {"dry": false}});
        assert_eq!(
            param_values(&headers, &args),
            vec![
                ("Mcp-Param-Dry-Run".to_string(), "false".to_string()),
                ("Mcp-Param-Limit".to_string(), "42".to_string()),
                ("Mcp-Param-Region".to_string(), "us-west1".to_string()),
            ]
        );
        // The value goes as sent (the server checks it against the body);
        // null, absent, a float or an object sends no header.
        let sparse = json!({"region": "Zürich", "limit": null, "opts": {"dry": "yes"}});
        assert_eq!(
            param_values(&headers, &sparse),
            vec![
                ("Mcp-Param-Dry-Run".to_string(), "yes".to_string()),
                (
                    "Mcp-Param-Region".to_string(),
                    "=?base64?WsO8cmljaA==?=".to_string()
                )
            ]
        );
        assert_eq!(
            param_values(&headers, &json!({"limit": 1.5, "opts": {"dry": {"x": 1}}})),
            Vec::<(String, String)>::new()
        );
        assert_eq!(
            param_values(&headers, &json!({"limit": 9_007_199_254_740_992_i64})),
            Vec::<(String, String)>::new()
        );
    }

    #[test]
    fn a_bad_x_mcp_header_rejects_the_tool() {
        let cases = [
            (
                json!({"properties": {"a": {"type": "array", "items": {"type": "string", "x-mcp-header": "A"}}}}),
                "x-mcp-header A is not on a plain properties path",
            ),
            (
                json!({"properties": {"a": {"oneOf": [{"type": "string", "x-mcp-header": "A"}]}}}),
                "x-mcp-header A is not on a plain properties path",
            ),
            (
                json!({"type": "string", "x-mcp-header": "Root"}),
                "x-mcp-header Root is not on a plain properties path",
            ),
            (
                json!({"properties": {"a": {"type": "number", "x-mcp-header": "A"}}}),
                "x-mcp-header A is on a property that is not a string, integer or boolean",
            ),
            (
                json!({"properties": {"a": {"type": "string", "x-mcp-header": ""}}}),
                "x-mcp-header  is not a header name",
            ),
            (
                json!({"properties": {"a": {"type": "string", "x-mcp-header": "Bad\r\nX"}}}),
                "x-mcp-header Bad  X is not a header name",
            ),
            (
                json!({"properties": {
                    "a": {"type": "string", "x-mcp-header": "Region"},
                    "b": {"type": "string", "x-mcp-header": "region"}
                }}),
                "x-mcp-header region is repeated",
            ),
        ];
        for (schema, why) in cases {
            assert_eq!(param_headers(&schema), Err(why.to_string()), "{schema}");
        }
        assert_eq!(param_headers(&json!({"type": "object"})), Ok(Vec::new()));
    }
}
