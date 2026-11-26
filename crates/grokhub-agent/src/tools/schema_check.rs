//! Argument check for a local tool call. llama-server turns the same JSON
//! schema into a grammar when it is started with `--jinja`. A call that still
//! misses the schema is sent back once; the next miss is a plain error.

use serde_json::Value;

/// What the model and the chat see after the one repair has already been used.
pub const PLAIN_SCHEMA_ERROR: &str = "not run: the tool arguments do not match the schema";

/// `already_repaired` is true when this turn already sent one schema miss back.
pub fn schema_message(already_repaired: bool, why: &str) -> String {
    if already_repaired {
        PLAIN_SCHEMA_ERROR.to_string()
    } else {
        format!(
            "not run: tool arguments do not match the schema ({why}). Send this call once more with arguments that match."
        )
    }
}

/// Why `arguments` miss the schema of `name` in `tools`.
///
/// `None` when the call is not a schema problem: the tool is not in the list
/// (dispatch owns that), or the arguments are not a JSON object (the parse
/// path owns that). An empty string is an empty object, same as tool dispatch.
pub fn schema_miss(tools: &[Value], name: &str, arguments: &str) -> Option<String> {
    let params = tool_parameters(tools, name)?;
    let value = if arguments.trim().is_empty() {
        Value::Object(Default::default())
    } else {
        match serde_json::from_str::<Value>(arguments) {
            Ok(v) if v.is_object() => v,
            _ => return None,
        }
    };
    check_value(&params, &value, "").err()
}

fn tool_parameters(tools: &[Value], name: &str) -> Option<Value> {
    tools.iter().find_map(|tool| {
        let function = tool.get("function").unwrap_or(tool);
        if function.get("name").and_then(Value::as_str) != Some(name) {
            return None;
        }
        Some(
            function
                .get("parameters")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}})),
        )
    })
}

fn check_value(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    if let Some(kinds) = schema.get("type") {
        let ok = match kinds {
            Value::String(one) => type_ok(one, value),
            Value::Array(list) => list
                .iter()
                .filter_map(Value::as_str)
                .any(|one| type_ok(one, value)),
            _ => true,
        };
        if !ok {
            let expect = match kinds {
                Value::String(one) => type_phrase(one),
                other => format!("a {other}"),
            };
            let at = if path.is_empty() { "value" } else { path };
            return Err(format!("{at} is not {expect}"));
        }
    }
    if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
        if !allowed.iter().any(|item| item == value) {
            let at = if path.is_empty() { "value" } else { path };
            return Err(format!("{at} is not one of the allowed values"));
        }
    }
    if value.is_object() && (schema.get("properties").is_some() || schema.get("required").is_some())
    {
        return check_object(schema, value, path);
    }
    if let (Some(items), Some(list)) = (schema.get("items"), value.as_array()) {
        for (i, item) in list.iter().enumerate() {
            let child = if path.is_empty() {
                format!("[{i}]")
            } else {
                format!("{path}[{i}]")
            };
            check_value(items, item, &child)?;
        }
    }
    Ok(())
}

fn check_object(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    let Some(obj) = value.as_object() else {
        return Ok(());
    };
    let properties = schema.get("properties").and_then(Value::as_object);
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for name in required.iter().filter_map(Value::as_str) {
            if obj.get(name).is_none_or(Value::is_null) {
                return Err(format!("missing {name}"));
            }
        }
    }
    if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
        if let Some(properties) = properties {
            let mut unknown: Vec<&str> = obj
                .keys()
                .map(String::as_str)
                .filter(|key| !properties.contains_key(*key))
                .collect();
            unknown.sort_unstable();
            if let Some(name) = unknown.first() {
                return Err(format!("unknown field {name}"));
            }
        }
    }
    if let Some(properties) = properties {
        for (name, spec) in properties {
            if let Some(child) = obj.get(name) {
                let at = if path.is_empty() {
                    name.clone()
                } else {
                    format!("{path}.{name}")
                };
                check_value(spec, child, &at)?;
            }
        }
    }
    Ok(())
}

fn type_phrase(spec: &str) -> String {
    match spec {
        "integer" => "an integer".into(),
        "object" => "an object".into(),
        "array" => "an array".into(),
        other => format!("a {other}"),
    }
}

fn type_ok(spec: &str, value: &Value) -> bool {
    match spec {
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "null" => value.is_null(),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools() -> Vec<Value> {
        vec![serde_json::json!({
            "type": "function",
            "name": "read_file",
            "parameters": {
                "type": "object",
                "properties": {
                    "target_file": {"type": "string"},
                    "limit": {"type": "integer"}
                },
                "required": ["target_file"],
                "additionalProperties": false
            }
        })]
    }

    #[test]
    fn a_literal_malformed_read_misses_the_schema_and_one_repair_then_a_plain_error() {
        let tools = tools();
        let missing = schema_miss(&tools, "read_file", "{}").unwrap();
        assert_eq!(missing, "missing target_file");
        assert_eq!(
            schema_message(false, &missing),
            "not run: tool arguments do not match the schema (missing target_file). Send this call once more with arguments that match."
        );
        let wrong = schema_miss(&tools, "read_file", r#"{"target_file":1}"#).unwrap();
        assert_eq!(wrong, "target_file is not a string");
        let extra = schema_miss(
            &tools,
            "read_file",
            r#"{"target_file":"note.txt","mode":"raw"}"#,
        )
        .unwrap();
        assert_eq!(extra, "unknown field mode");
        let fraction = schema_miss(
            &tools,
            "read_file",
            r#"{"target_file":"note.txt","limit":1.5}"#,
        )
        .unwrap();
        assert_eq!(fraction, "limit is not an integer");
        assert_eq!(
            schema_miss(&tools, "read_file", r#"{"target_file":"note.txt"}"#),
            None
        );
        assert_eq!(
            schema_message(true, "missing target_file"),
            PLAIN_SCHEMA_ERROR
        );
        assert_eq!(
            schema_miss(&tools, "read_file", "not json"),
            None,
            "a non-object stays on the parse path"
        );
        assert_eq!(
            schema_miss(&tools, "not_a_tool", "{}"),
            None,
            "an unknown tool stays on the dispatch path"
        );
    }
}
