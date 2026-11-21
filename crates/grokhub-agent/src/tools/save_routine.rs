//! `save_routine`: saves the steps of this chat's last finished turn as a
//! routine ([`crate::routine_draft`]). The model names the inputs, the name
//! and the about; the steps themselves come from the turn as it ran. It
//! always asks first (the approval card is the person's yes), and saving
//! allows no change step: that stays the person's separate OK.

use serde_json::{json, Value};

use super::{ToolCtx, ToolOutput};
use crate::routine_consent::change_steps;
use crate::routine_draft::{self, Naming};

pub const NAME: &str = "save_routine";

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": NAME,
        "description": "Save the tool steps of the previous turn (the one before the user's latest message) as a routine the user can run again with no model: from chat with run_routine, or on a timer, a file or a webhook. Use it only when the user asks to save, keep or repeat what you just did. The steps are taken exactly as they ran; you only name it, say in one line what it does, and pick which literal values become inputs (each value must appear in a step's arguments). The user confirms before anything is saved.",
        "parameters": {
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Lowercase letters, digits and dashes, like disk-report."},
                "about": {"type": "string"},
                "inputs": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "value": {"type": "string", "description": "The literal text in the steps this input replaces; it becomes the default."},
                            "about": {"type": "string"}
                        },
                        "required": ["name", "value"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["name", "about"],
            "additionalProperties": false
        }
    })
}

pub fn naming(args: &Value) -> Naming {
    let text = |v: &Value, key: &str| {
        v.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    Naming {
        name: text(args, "name"),
        about: text(args, "about"),
        inputs: args
            .get("inputs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|i| (text(i, "name"), text(i, "value"), text(i, "about")))
            .collect(),
    }
}

pub fn run(ctx: &ToolCtx<'_>, args: &Value) -> ToolOutput {
    let Some(session) = ctx.session else {
        return ToolOutput::err("There is no chat here to save steps from.");
    };
    let items = match crate::session::resume_input(session) {
        Ok(items) => items,
        Err(e) => return ToolOutput::err(format!("Couldn't read this chat: {e}")),
    };
    save_from(&crate::perm::config_dir(), &items, args)
}

pub fn save_from(
    config_dir: &std::path::Path,
    items: &[crate::client::InputItem],
    args: &Value,
) -> ToolOutput {
    let calls = routine_draft::last_turn_calls(items);
    let routine = match routine_draft::draft(&calls, &naming(args)) {
        Ok(r) => r,
        Err(e) => return ToolOutput::err(format!("Not saved: {e}.")),
    };
    let saved = match routine_draft::save(config_dir, &routine, "saved from chat") {
        Ok(saved) => saved,
        Err(e) => return ToolOutput::err(format!("Not saved: {e}.")),
    };
    let mut out = match saved {
        Some(_) => format!(
            "Saved the routine {} with {} steps: {}.",
            routine.name,
            routine.steps.len(),
            routine
                .steps
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        None => format!("The routine {} was already saved exactly so.", routine.name),
    };
    let changes = change_steps(&routine);
    if !changes.is_empty() {
        out.push_str(&format!(
            " Its steps that change something ({}) are refused when it runs on its own until the user allows them.",
            changes
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    out.push_str(" Undo takes it back from the change history.");
    ToolOutput::ok(out)
}

#[cfg(test)]
#[path = "save_routine_tests.rs"]
mod tests;
