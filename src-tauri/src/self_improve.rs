//! Settings → Behavior → self-improvement, the same as the cabin's
//! (`self_review_ui.rs`, `change_history_ui.rs`): the Quiet self-review
//! switch with the suggestions it logged, the history of changes the weekly
//! pass made (trying, kept, rolled back, ended, reverted) with a click Revert
//! on the newest kept one, the runs per area, and the code ideas the user
//! saved with Copy as GitHub issue.
//!
//! The switch is `selfImproveQuiet` in `app.json` and the log and ideas are
//! the cabin's `self_review.json`, so both apps read the same. This module
//! only reads that file. A revert goes through
//! `grokhub_agent::harness::revert_version` with the click's `UndoAsk`.

use std::path::Path;

use grokhub_agent::harness::{self as hx, VersionStatus};
use grokhub_core::run_metrics::{self, DomainRow};
use grokhub_core::self_review::{self as sr, QuietEntry};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::ShellState;

/// The cabin's switch in `app.json`.
pub const QUIET_KEY: &str = "selfImproveQuiet";
/// The cabin's weekly pass state.
pub const SELF_REVIEW_FILE: &str = "self_review.json";
/// History rows Settings shows, newest first.
pub const HISTORY_ROWS: usize = 8;
/// Logged suggestions Settings shows.
pub const QUIET_LINES: usize = 5;
pub const QUIET_ROW: &str = "Quiet self-review";
pub const QUIET_HINT: &str = "The Sunday self-review logs its suggestions here instead of posting Pulse cards. Nothing is applied either way.";
pub const HISTORY_EMPTY: &str =
    "No self-changes yet. The Sunday self-review proposes them as Pulse cards.";
const DOMAINS_HEAD: &str = "Runs by area (runs on file):";

/// What this module reads of `self_review.json`. Unknown fields are left
/// alone (nothing is written back).
#[derive(Debug, Clone, Default, Deserialize)]
struct ReviewFile {
    #[serde(default)]
    quiet_log: Vec<QuietEntry>,
    #[serde(default)]
    saved_ideas: Vec<SavedIdea>,
}

/// A code-change idea the user saved from its Pulse card.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedIdea {
    pub title: String,
    pub text: String,
    #[serde(default)]
    pub at: u64,
}

fn review_file(config_dir: &Path) -> ReviewFile {
    std::fs::read_to_string(config_dir.join(SELF_REVIEW_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// One history line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRow {
    pub title: String,
    pub hint: String,
    /// Set on a kept version that is still the newest change on its target.
    pub revert: Option<String>,
}

/// One bar per area.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DomainBar {
    pub domain: String,
    pub runs: usize,
}

/// The whole section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfImproveView {
    pub quiet: bool,
    /// Under the switch while it is on.
    pub quiet_note: Option<String>,
    pub history: Vec<HistoryRow>,
    /// "Runs by area (runs on file):" and one line per area.
    pub domains: Option<String>,
    pub bars: Vec<DomainBar>,
    /// Newest first.
    pub ideas: Vec<SavedIdea>,
}

fn status_word(s: VersionStatus) -> &'static str {
    match s {
        VersionStatus::Canary => "Trying",
        VersionStatus::Kept => "Kept",
        VersionStatus::RolledBack => "Rolled back",
        VersionStatus::Ended => "Ended",
        VersionStatus::Reverted => "Reverted",
    }
}

/// Newest first. Revert goes only on the newest kept version of a target.
pub fn history_rows(versions: &[hx::Version], now: u64) -> Vec<HistoryRow> {
    let mut rows = Vec::new();
    for (i, v) in versions.iter().enumerate().rev().take(HISTORY_ROWS) {
        let newest = !versions[i + 1..]
            .iter()
            .any(|o| o.kind == v.kind && o.id == v.id);
        let mut hint = format!("{} · {}", v.diff, grokhub_core::pulse::ago_label(v.at, now));
        if !v.note.is_empty() {
            hint = format!("{} · {hint}", v.note);
        }
        rows.push(HistoryRow {
            title: format!("{}: {}", status_word(v.status), v.label),
            hint,
            revert: (newest && v.status == VersionStatus::Kept).then(|| v.version.clone()),
        });
    }
    rows
}

/// What Settings shows under the switch.
pub fn quiet_log_note(log: &[QuietEntry]) -> String {
    let lines = sr::quiet_log_lines(log, QUIET_LINES);
    if lines.is_empty() {
        "Nothing logged yet. The self-review runs on Sunday at dream time.".into()
    } else {
        format!("Logged suggestions, newest first:\n{}", lines.join("\n"))
    }
}

/// One compact line per area the runs on file name. `None` before any run
/// has one.
pub fn domain_block(rows: &[DomainRow]) -> Option<String> {
    if rows.is_empty() {
        return None;
    }
    let lines: Vec<String> = rows.iter().map(DomainRow::line).collect();
    Some(format!("{DOMAINS_HEAD}\n{}", lines.join("\n")))
}

pub fn view(config_dir: &Path, now: u64) -> SelfImproveView {
    let quiet = grokhub_core::app_json::bool_or(config_dir, QUIET_KEY, false);
    let file = review_file(config_dir);
    let domains = run_metrics::domain_rows(&run_metrics::read_runs(config_dir));
    SelfImproveView {
        quiet,
        quiet_note: quiet.then(|| quiet_log_note(&file.quiet_log)),
        history: history_rows(&hx::read_versions(config_dir), now),
        domains: domain_block(&domains),
        bars: domains
            .iter()
            .map(|r| DomainBar {
                domain: r.domain.clone(),
                runs: r.runs,
            })
            .collect(),
        ideas: file.saved_ideas.into_iter().rev().collect(),
    }
}

/// Revert on a kept history row: the user's click undoes it through the
/// ledger. The line says what happened.
pub fn revert(config_dir: &Path, version: &str, now: u64) -> Result<String, String> {
    hx::revert_version(
        config_dir,
        &config_dir.join("skills"),
        version,
        hx::UndoAsk::from_click(),
        now,
    )
    .map(|v| v.note)
}

/// The issue text for a saved idea, with the home folder, emails and keys
/// taken out. Nothing is sent: the user pastes it on GitHub.
pub fn issue(config_dir: &Path, title: &str, home: Option<&str>) -> Result<String, String> {
    let file = review_file(config_dir);
    let idea = file
        .saved_ideas
        .iter()
        .find(|i| i.title == title)
        .ok_or("That idea isn't saved any more.")?;
    Ok(sr::issue_text(
        &idea.title,
        &idea.text,
        env!("CARGO_PKG_VERSION"),
        home,
    ))
}

/// "Issue copied…", after the page put the text on the clipboard.
pub fn issue_copied() -> String {
    format!(
        "Issue copied. Paste it on GitHub yourself; {} sent nothing.",
        grokhub_core::brand::product_name()
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueText {
    pub text: String,
    pub copied: String,
}

#[tauri::command]
pub(crate) fn get_self_improve(state: State<'_, ShellState>) -> SelfImproveView {
    view(&state.config_dir, grokhub_core::now_ms())
}

#[tauri::command]
pub(crate) fn set_self_improve_quiet(
    state: State<'_, ShellState>,
    on: bool,
) -> Result<SelfImproveView, String> {
    grokhub_core::app_json::patch(&state.config_dir, &[(QUIET_KEY, on.into())])?;
    Ok(view(&state.config_dir, grokhub_core::now_ms()))
}

#[tauri::command]
pub(crate) fn self_improve_revert(
    state: State<'_, ShellState>,
    version: String,
) -> Result<String, String> {
    revert(&state.config_dir, &version, grokhub_core::now_ms())
}

#[tauri::command]
pub(crate) fn self_improve_issue(
    state: State<'_, ShellState>,
    title: String,
) -> Result<IssueText, String> {
    let home = grokhub_core::user_home().map(|h| h.to_string_lossy().into_owned());
    Ok(IssueText {
        text: issue(&state.config_dir, &title, home.as_deref())?,
        copied: issue_copied(),
    })
}

#[cfg(test)]
#[path = "self_improve_tests.rs"]
mod tests;
