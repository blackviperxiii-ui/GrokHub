//! The Routines list on the Automations page (script-first runtime, phase
//! 6): each saved routine with what starts it, its steps, whether its change
//! steps are allowed, whether it is on, and its runs and model calls. Allow,
//! Undo and Run now come from the person's click here.

use std::path::Path;

use grokhub_agent::harness::UndoAsk;
use grokhub_agent::routine_consent::{self, Standing};
use grokhub_agent::routine_log;
use grokhub_agent::routine_triggers::{self, Start};
use grokhub_core::routine::{Routine, Trigger};
use serde::Serialize;
use serde_json::Map;

use crate::automations::RunResult;
use crate::scheduler::Route;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Consent {
    /// Every step only reads: nothing to allow.
    ReadOnly,
    Allowed,
    /// Allowed once, but the routine changed since.
    Edited,
    NotAllowed,
}

impl From<Standing> for Consent {
    fn from(s: Standing) -> Self {
        match s {
            Standing::ReadOnly => Self::ReadOnly,
            Standing::Allowed => Self::Allowed,
            Standing::Edited => Self::Edited,
            Standing::NotAllowed => Self::NotAllowed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub name: String,
    pub about: String,
    /// `daily 09:00`, `webhook deploys`, `new *.csv in /home/user/in`, `chat`.
    pub triggers: Vec<String>,
    pub steps: usize,
    /// The change steps, as `id: tool`.
    pub changes: Vec<String>,
    /// `None` when the routine doesn't load.
    pub consent: Option<Consent>,
    pub on: bool,
    pub runs: u32,
    pub model_calls: u32,
    pub last_at_ms: Option<u64>,
    pub last_ok: Option<bool>,
    pub last_line: Option<String>,
    /// Why the routine can't run; empty when it can.
    pub problems: Vec<String>,
}

fn trigger_label(t: &Trigger) -> String {
    match t {
        Trigger::Timer(timer) => String::from(*timer),
        Trigger::Webhook(hook) => format!("webhook {hook}"),
        Trigger::File { folder, glob } => format!("new {glob} in {folder}"),
        Trigger::Chat => "chat".into(),
    }
}

pub fn rows(config_dir: &Path) -> Vec<Row> {
    let stats = routine_log::stats(config_dir);
    routine_triggers::all(config_dir)
        .into_iter()
        .map(|(dir, loaded)| {
            let folder = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let (name, about, triggers, steps, changes, consent, problems) = match &loaded {
                Ok(r) => (
                    r.name.clone(),
                    r.about.clone(),
                    r.triggers.iter().map(trigger_label).collect(),
                    r.steps.len(),
                    routine_consent::change_steps(r)
                        .into_iter()
                        .map(|c| format!("{}: {}", c.id, c.tool))
                        .collect(),
                    Some(routine_consent::standing(config_dir, r).into()),
                    Vec::new(),
                ),
                Err(problems) => (
                    folder,
                    String::new(),
                    Vec::new(),
                    0,
                    Vec::new(),
                    None,
                    problems.clone(),
                ),
            };
            let s = stats.get(&name).cloned().unwrap_or_default();
            Row {
                on: routine_triggers::is_on(config_dir, &name),
                runs: s.runs,
                model_calls: s.model_calls,
                last_at_ms: s.last.as_ref().map(|l| l.at_ms),
                last_ok: s.last.as_ref().map(|l| l.ok),
                last_line: s.last.map(|l| l.line),
                name,
                about,
                triggers,
                steps,
                changes,
                consent,
                problems,
            }
        })
        .collect()
}

fn find(config_dir: &Path, name: &str) -> Result<(std::path::PathBuf, Routine), String> {
    routine_triggers::routines(config_dir)
        .into_iter()
        .find(|(_, r)| r.name == name)
        .ok_or_else(|| format!("No routine named {name}"))
}

/// Allows routine `name`'s change steps as they are now.
pub fn allow(config_dir: &Path, name: &str) -> Result<(), String> {
    let (_, r) = find(config_dir, name)?;
    routine_consent::grant(config_dir, &r, grokhub_core::now_ms())
}

pub fn revoke(config_dir: &Path, name: &str) -> Result<(), String> {
    routine_consent::revoke(config_dir, name).map(|_| ())
}

/// Undoes the newest saved change to routine `name`, from the person's click.
pub fn undo(config_dir: &Path, name: &str) -> Result<String, String> {
    grokhub_agent::routine_draft::undo(config_dir, name, UndoAsk::from_click())
}

/// Runs routine `name` now, as a trigger would, with its inputs' defaults.
pub fn run_now(
    config_dir: &Path,
    name: &str,
    route: &dyn Fn() -> Result<Route, String>,
) -> Result<RunResult, String> {
    let (dir, routine) = find(config_dir, name)?;
    let start = Start {
        name: routine.name.clone(),
        dir,
        routine,
        inputs: Map::new(),
        why: "run now".into(),
    };
    Ok(crate::routine_runs::run(config_dir, &start, None, route))
}

#[cfg(test)]
#[path = "routines_page_tests.rs"]
mod tests;
