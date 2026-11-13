//! Settings → Behavior, "What Loadout changed on its own": the cabin's Undo,
//! Keep and Restore rows (`change_undo.rs`, `skill_undo.rs`) for skills,
//! connections and automations. Rows and result lines come from
//! [`crate::changes`], so they read the same as `/skills changes` and every
//! undo, keep or restore goes through the change ledger with a click's
//! `UndoAsk`. The page sends these only from a pointer click.

use std::path::Path;

use grokhub_agent::harness::ChangeKind;
use serde::Serialize;
use tauri::State;

use crate::changes::{self, Row};
use crate::ShellState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    /// "skill", "connection" or "automation": what the page sends back.
    pub kind: String,
    pub title: String,
    pub rows: Vec<Row>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeRowsView {
    pub heading: String,
    pub empty: String,
    pub groups: Vec<Group>,
}

const KINDS: [(ChangeKind, &str); 3] = [
    (ChangeKind::Skill, "Skills"),
    (ChangeKind::Connection, "Connections"),
    (ChangeKind::Automation, "Automations"),
];

pub fn heading() -> String {
    format!(
        "What {} changed on its own",
        grokhub_core::brand::product_name()
    )
}

pub fn empty_line() -> String {
    format!(
        "Nothing to undo. When {} changes a skill, connection or automation on its own, it shows here with Undo and Keep.",
        grokhub_core::brand::product_name()
    )
}

fn kind_of(kind: &str) -> Result<ChangeKind, String> {
    KINDS
        .iter()
        .map(|(k, _)| *k)
        .find(|k| k.as_str() == kind)
        .ok_or_else(|| format!("No change rows for {kind}."))
}

/// Only the kinds with something to undo, keep or restore.
pub fn view(config_dir: &Path, now_ms: u64) -> ChangeRowsView {
    let groups = KINDS
        .iter()
        .map(|(kind, title)| Group {
            kind: kind.as_str().to_string(),
            title: (*title).to_string(),
            rows: changes::rows(config_dir, *kind, now_ms),
        })
        .filter(|g| !g.rows.is_empty())
        .collect();
    ChangeRowsView {
        heading: heading(),
        empty: empty_line(),
        groups,
    }
}

pub fn undo(config_dir: &Path, kind: &str, id: &str) -> Result<String, String> {
    changes::undo(config_dir, kind_of(kind)?, id, true)
}

pub fn keep(config_dir: &Path, kind: &str, id: &str) -> Result<String, String> {
    changes::keep(config_dir, kind_of(kind)?, id)
}

pub fn restore(config_dir: &Path, kind: &str, id: &str) -> Result<String, String> {
    if kind_of(kind)? != ChangeKind::Skill {
        return Err("Only a removed skill can be restored.".into());
    }
    changes::restore_skill(config_dir, id, true)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    pub view: ChangeRowsView,
    pub status: String,
}

fn answer(config_dir: &Path, done: Result<String, String>) -> Answer {
    Answer {
        view: view(config_dir, grokhub_core::now_ms()),
        status: done.unwrap_or_else(|e| e),
    }
}

#[tauri::command]
pub(crate) fn get_change_rows(state: State<'_, ShellState>) -> ChangeRowsView {
    view(&state.config_dir, grokhub_core::now_ms())
}

#[tauri::command]
pub(crate) fn change_row_undo(state: State<'_, ShellState>, kind: String, id: String) -> Answer {
    answer(&state.config_dir, undo(&state.config_dir, &kind, &id))
}

#[tauri::command]
pub(crate) fn change_row_keep(state: State<'_, ShellState>, kind: String, id: String) -> Answer {
    answer(&state.config_dir, keep(&state.config_dir, &kind, &id))
}

#[tauri::command]
pub(crate) fn change_row_restore(state: State<'_, ShellState>, kind: String, id: String) -> Answer {
    answer(&state.config_dir, restore(&state.config_dir, &kind, &id))
}

#[cfg(test)]
#[path = "change_rows_tests.rs"]
mod tests;
