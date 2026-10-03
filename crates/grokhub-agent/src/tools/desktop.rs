//! Desktop tool schemas. The app supplies the screen driver through [`crate::DesktopOps`].

use serde_json::{json, Value};

pub const NAMES: &[&str] = &["screenshot", "click", "move", "drag", "scroll", "type", "key"];

pub fn schemas() -> Vec<Value> {
    let monitor = json!({
        "type": "string",
        "description": "Monitor id, or \"all\". Defaults to \"all\". Coordinates are pixels in the last screenshot of that monitor."
    });
    let button = json!({
        "type": "string",
        "enum": ["left", "right", "middle"],
        "description": "Mouse button. Defaults to left."
    });
    let out = vec![
        tool(
            "screenshot",
            "Capture the screen as an image. The text is geometry for later clicks.",
            json!({ "monitor": monitor }),
            &[],
        ),
        tool(
            "click",
            "Click at screenshot coordinates. double repeats the click.",
            json!({
                "x": {"type": "number"},
                "y": {"type": "number"},
                "button": button,
                "double": {"type": "boolean"},
                "monitor": monitor
            }),
            &["x", "y"],
        ),
        tool(
            "move",
            "Move the pointer to screenshot coordinates.",
            json!({
                "x": {"type": "number"},
                "y": {"type": "number"},
                "monitor": monitor
            }),
            &["x", "y"],
        ),
        tool(
            "drag",
            "Press, move in steps, and release.",
            json!({
                "from_x": {"type": "number"},
                "from_y": {"type": "number"},
                "to_x": {"type": "number"},
                "to_y": {"type": "number"},
                "button": button,
                "monitor": monitor
            }),
            &["from_x", "from_y", "to_x", "to_y"],
        ),
        tool(
            "scroll",
            "Move the pointer, then scroll. Positive dy is up. Positive dx is right.",
            json!({
                "x": {"type": "number"},
                "y": {"type": "number"},
                "dx": {"type": "number"},
                "dy": {"type": "number"},
                "monitor": monitor
            }),
            &["x", "y"],
        ),
        tool(
            "type",
            "Type Unicode text by injecting keystrokes. Does not read the clipboard.",
            json!({ "text": {"type": "string"} }),
            &["text"],
        ),
        tool(
            "key",
            "Press a key combo, for example Return, ctrl+c, or alt+F4.",
            json!({ "keys": {"type": "string"} }),
            &["keys"],
        ),
    ];
    debug_assert!(out.iter().all(|tool| {
        tool.get("name")
            .and_then(|name| name.as_str())
            .is_some_and(|name| NAMES.contains(&name))
    }));
    debug_assert_eq!(out.len(), NAMES.len());
    out
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "function",
        "name": name,
        "description": description,
        "parameters": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        }
    })
}
