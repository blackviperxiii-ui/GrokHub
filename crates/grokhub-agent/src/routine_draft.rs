//! Save as routine (script-first runtime, phase 5). The tool calls of a
//! chat's last finished turn become a draft routine by a fixed pass: each
//! call that went through is one step, with the arguments it ran with. The
//! chat model names the inputs (which literal values become
//! `{{inputs.…}}`), the name and the one-line about. The draft is saved
//! only after the person says yes on the approval card, and it goes through
//! the change ledger, so `grokhub-hub routine undo` takes it back.
//!
//! Saving never allows a change step: that is [`crate::routine_consent`].

use std::path::Path;

use grokhub_core::routine::{self, Input, InputKind, Routine, Step, StepKind};
use serde_json::{Map, Value};

use crate::client::InputItem;
use crate::harness::{self, Change, Origin, RoutineTarget};

/// The tools a saved step may be. Planning, memory and chat tools stay out.
pub const SAVED_TOOLS: &[(&str, &str)] = &[
    ("read_file", "read"),
    ("list_dir", "list"),
    ("grep", "grep"),
    ("glob", "glob"),
    ("run_terminal_command", "run"),
    ("write", "write"),
    ("search_replace", "edit"),
    ("web_fetch", "fetch"),
    ("web_search", "search"),
];
/// Steps one routine keeps, at most.
pub const MAX_STEPS: usize = 30;
/// A value shorter than this becomes an input only where it is a whole
/// argument, never inside a longer text.
const SHORT_VALUE: usize = 3;

/// One call that went through.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub tool: String,
    pub args: Map<String, Value>,
}

/// What the chat model names.
#[derive(Debug, Clone, PartialEq)]
pub struct Naming {
    pub name: String,
    pub about: String,
    /// Input name, the literal value it replaces, and what it is.
    pub inputs: Vec<(String, String, String)>,
}

/// Outputs that mean the call didn't run, or ran and failed.
fn failed(tool: &str, output: &str) -> bool {
    let out = output.trim_start();
    let refused = [
        "User rejected",
        "User cancelled",
        crate::gate::NOT_EXECUTED,
        crate::gate::READ_ONLY_PHASE,
        crate::run::TRUNCATED_TOOL,
        crate::run::BAD_ARGS_TOOL,
    ];
    if refused.iter().any(|p| out.starts_with(p))
        || (out.starts_with("Tool `") && out.contains("` was not executed"))
    {
        return true;
    }
    // A command's first line is its exit code.
    tool == "run_terminal_command"
        && out
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("exit "))
            .is_some_and(|code| code.trim() != "0")
}

/// The calls of the last turn in `items` (after its last user message) that
/// ran and worked, in order, for the tools a routine may keep.
pub fn last_turn_calls(items: &[InputItem]) -> Vec<Call> {
    let start = items
        .iter()
        .rposition(|i| matches!(i, InputItem::Message { role, .. } if role == "user"))
        .map_or(0, |at| at + 1);
    let turn = &items[start..];
    let output_of = |id: &str| {
        turn.iter().find_map(|i| match i {
            InputItem::FunctionCallOutput { call_id, output } if call_id == id => {
                Some(output.as_str())
            }
            _ => None,
        })
    };
    turn.iter()
        .filter_map(|i| match i {
            InputItem::FunctionCall {
                call_id,
                name,
                arguments,
            } => Some((call_id, name, arguments)),
            _ => None,
        })
        .filter(|(_, name, _)| SAVED_TOOLS.iter().any(|(t, _)| t == name))
        .filter(|(id, name, _)| output_of(id).is_some_and(|out| !failed(name, out)))
        .filter_map(
            |(_, name, arguments)| match serde_json::from_str(arguments) {
                Ok(Value::Object(args)) => Some(Call {
                    tool: name.clone(),
                    args,
                }),
                _ => None,
            },
        )
        .collect()
}

/// Replace `value` with `template` in every string inside `v`. Returns how
/// many places changed.
fn swap(v: &mut Value, value: &str, template: &str) -> usize {
    match v {
        Value::String(s) if s == value => {
            *s = template.to_string();
            1
        }
        Value::String(s) if value.chars().count() >= SHORT_VALUE && s.contains(value) => {
            *s = s.replace(value, template);
            1
        }
        Value::Array(items) => items.iter_mut().map(|i| swap(i, value, template)).sum(),
        Value::Object(map) => map.values_mut().map(|i| swap(i, value, template)).sum(),
        _ => 0,
    }
}

/// The draft routine for `calls`, named by `naming`. It runs from chat and
/// shows its last step's output.
pub fn draft(calls: &[Call], naming: &Naming) -> Result<Routine, String> {
    if calls.is_empty() {
        return Err(
            "the last turn ran no steps a routine can keep (files, commands, edits, web reads)"
                .into(),
        );
    }
    if calls.len() > MAX_STEPS {
        return Err(format!(
            "the last turn ran {} steps; a routine keeps at most {MAX_STEPS}",
            calls.len()
        ));
    }
    let mut args: Vec<Value> = calls
        .iter()
        .map(|c| Value::Object(c.args.clone()))
        .collect();
    if args.iter().any(|a| a.to_string().contains("{{")) {
        return Err("a step's text holds \"{{\", which a routine would read as a blank".into());
    }
    let mut inputs = std::collections::BTreeMap::new();
    for (name, value, about) in &naming.inputs {
        if value.is_empty() {
            return Err(format!("inputs.{name} has no value"));
        }
        let template = format!("{{{{inputs.{name}}}}}");
        let hits: usize = args.iter_mut().map(|a| swap(a, value, &template)).sum();
        if hits == 0 {
            return Err(format!("\"{value}\" for inputs.{name} isn't in any step"));
        }
        inputs.insert(
            name.clone(),
            Input {
                kind: InputKind::String,
                default: Some(Value::String(value.clone())),
                about: about.clone(),
            },
        );
    }
    let mut used: Vec<String> = Vec::new();
    let mut steps = Vec::new();
    for (call, args) in calls.iter().zip(args) {
        let short = SAVED_TOOLS
            .iter()
            .find(|(t, _)| *t == call.tool)
            .map_or("step", |(_, s)| *s);
        let n = used.iter().filter(|u| u.as_str() == short).count();
        used.push(short.to_string());
        let id = if n == 0 {
            short.to_string()
        } else {
            format!("{short}_{}", n + 1)
        };
        let Value::Object(args) = args else {
            unreachable!("built from objects")
        };
        steps.push(Step {
            id,
            kind: StepKind::Tool {
                tool: call.tool.clone(),
                args,
            },
        });
    }
    let last = steps.last().map(|s| s.id.clone()).unwrap_or_default();
    let r = Routine {
        version: 1,
        name: naming.name.trim().to_string(),
        about: naming.about.trim().to_string(),
        triggers: vec![routine::Trigger::Chat],
        inputs,
        steps,
        result: Some(format!("{{{{{last}.stdout}}}}")),
    };
    let problems = r.problems(&crate::routine_run::step_tool);
    if problems.is_empty() {
        Ok(r)
    } else {
        Err(problems.join("; "))
    }
}

/// Saves `r` through the change ledger. `Ok(None)`: it was already saved
/// exactly so.
pub fn save(config_dir: &Path, r: &Routine, reason: &str) -> Result<Option<Change>, String> {
    let text = serde_json::to_string_pretty(r).map_err(|e| e.to_string())?;
    let target = RoutineTarget {
        config_dir,
        name: &r.name,
    };
    let file = harness::ChangeTarget::file(&target)?;
    harness::record_change(config_dir, &target, Origin::User, reason, || {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        grokhub_core::pulse_writer::write_atomic(&file, &text)
    })
}

/// Undoes the newest saved change to routine `name`. `ask` is the person's
/// own typing or click.
pub fn undo(config_dir: &Path, name: &str, ask: harness::UndoAsk) -> Result<String, String> {
    let target = RoutineTarget { config_dir, name };
    harness::undo_change(config_dir, &target, ask)?;
    Ok(if harness::ChangeTarget::file(&target)?.exists() {
        format!("Undid the newest change to the routine {name}.")
    } else {
        format!("Removed the routine {name}. Its text is kept in the change history.")
    })
}

#[cfg(test)]
#[path = "routine_draft_tests.rs"]
mod tests;
