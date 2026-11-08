//! `models_check` (read-only): the online-model connection check
//! ([`crate::route::check`]) as a tool, so "is my Claude key working?" gets
//! a live answer instead of a guess. It never sends chat text.

use serde_json::{json, Value};

use super::ToolOutput;

pub const NAME: &str = "models_check";

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": NAME,
        "description": "Check the online models the user connected (Grok sign-in, and any Claude, OpenRouter or other provider key): whether each key is saved and allowed, and, live, whether the provider accepts the key and answers a one-word test. Sends no chat text. Use it whenever the user asks to test a key or a connection, or which models you can use; report its result as it reads. live false reads saved settings only.",
        "parameters": {
            "type": "object",
            "properties": {
                "live": {"type": "boolean"}
            },
            "required": [],
            "additionalProperties": false
        }
    })
}

pub fn run(args: &Value) -> ToolOutput {
    let live = args.get("live").and_then(Value::as_bool).unwrap_or(true);
    let dir = crate::perm::config_dir();
    let check = crate::route::check::run(&dir, live, grokhub_core::now_ms());
    ToolOutput::ok(crate::route::check::report(&check, false))
}
