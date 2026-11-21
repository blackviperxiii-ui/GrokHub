//! `run_routine`: runs one of the person's saved routines from chat
//! (script-first runtime, phase 4). The steps run with no model, each
//! through the same gate as the model's own calls. The chat can't answer a
//! step's ask yet, so a step that changes something is refused; read-only
//! routines run to the end. Only routines with the `chat` trigger are
//! listed, and the tool only shows when there is one.

use std::path::Path;

use grokhub_core::routine::{InputKind, Routine};
use serde_json::{json, Map, Value};

use super::{ToolCtx, ToolOutput};
use crate::routine_run::{self, Ended, RunSpec};

pub const NAME: &str = "run_routine";
/// Most routines the description lists.
const LISTED: usize = 40;

pub fn visible(config_dir: &Path) -> bool {
    !crate::routine_triggers::for_chat(config_dir).is_empty()
}

pub fn schema() -> Value {
    schema_in(&crate::perm::config_dir())
}

pub fn schema_in(config_dir: &Path) -> Value {
    let list = crate::routine_triggers::for_chat(config_dir);
    let names: Vec<&str> = list.iter().map(|(_, r)| r.name.as_str()).collect();
    let rows: Vec<String> = list.iter().take(LISTED).map(|(_, r)| row(r)).collect();
    json!({
        "type": "function",
        "name": NAME,
        "description": format!(
            "Run one of the user's saved routines: fixed steps that run with no model, each checked like your own tool calls. \
    Use it when the user asks for what a routine does; it is faster and cheaper than doing the steps yourself. \
    Pass inputs by name; one left out takes its default. A step that would change something is refused here, and the result says so. \
    Report the result as it reads.\nRoutines:\n{}",
            rows.join("\n")
        ),
        "parameters": {
            "type": "object",
            "properties": {
                "name": {"type": "string", "enum": names},
                "inputs": {"type": "object"},
                "dry_run": {"type": "boolean"}
            },
            "required": ["name"],
            "additionalProperties": false
        }
    })
}

/// `- name: about (inputs: a string, b integer = 3)`.
fn row(r: &Routine) -> String {
    let mut line = format!("- {}", r.name);
    if !r.about.is_empty() {
        line.push_str(&format!(": {}", r.about));
    }
    if !r.inputs.is_empty() {
        let inputs: Vec<String> = r
            .inputs
            .iter()
            .map(|(name, i)| match &i.default {
                Some(d) => format!("{name} {} = {d}", i.kind.name()),
                None => format!("{name} {}", i.kind.name()),
            })
            .collect();
        line.push_str(&format!(" (inputs: {})", inputs.join(", ")));
    }
    line
}

pub fn run(ctx: &ToolCtx<'_>, args: &Value) -> ToolOutput {
    run_in(&crate::perm::config_dir(), ctx, args)
}

pub fn run_in(config_dir: &Path, ctx: &ToolCtx<'_>, args: &Value) -> ToolOutput {
    let name = args.get("name").and_then(Value::as_str).unwrap_or_default();
    let list = crate::routine_triggers::for_chat(config_dir);
    let Some((dir, routine)) = list.iter().find(|(_, r)| r.name == name) else {
        let names: Vec<&str> = list.iter().map(|(_, r)| r.name.as_str()).collect();
        return ToolOutput::err(format!(
            "No routine named \"{name}\". Routines: {}.",
            if names.is_empty() {
                "none".to_string()
            } else {
                names.join(", ")
            }
        ));
    };
    let given = args.get("inputs").and_then(Value::as_object);
    let inputs = match typed(routine, given) {
        Ok(v) => v,
        Err(e) => return ToolOutput::err(e),
    };
    let report = routine_run::run(&RunSpec {
        routine,
        dir,
        workspace: ctx.workspace,
        inputs,
        dry_run: args
            .get("dry_run")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        approve: None,
        desktop: ctx.desktop,
        stop: ctx.stop,
        brain: None,
        words: None,
    });
    // Step outputs are outside text.
    let text = super::fence::fence("routine", &report.text());
    match report.ended {
        Ended::Stopped { .. } => ToolOutput::err(text),
        _ => ToolOutput::ok(text),
    }
}

/// The model's inputs, typed by the routine: text for a number or a list is
/// read as JSON, as `--set` reads it.
fn typed(
    routine: &Routine,
    given: Option<&Map<String, Value>>,
) -> Result<Map<String, Value>, String> {
    let mut out = Map::new();
    for (key, v) in given.into_iter().flatten() {
        let input = routine
            .inputs
            .get(key)
            .ok_or_else(|| format!("{} has no input named {key}", routine.name))?;
        let v = match v {
            Value::String(s) if input.kind != InputKind::String => serde_json::from_str(s.trim())
                .ok()
                .filter(|v| input.kind.accepts(v))
                .ok_or_else(|| {
                    let kind = input.kind.name();
                    let a = if kind.starts_with(['a', 'e', 'i', 'o', 'u']) {
                        "an"
                    } else {
                        "a"
                    };
                    format!("inputs.{key} must be {a} {kind}")
                })?,
            other => other.clone(),
        };
        out.insert(key.clone(), v);
    }
    Ok(out)
}

#[cfg(test)]
#[path = "run_routine_tests.rs"]
mod tests;
