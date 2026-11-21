//! What each routine run did, for the Routines list (script-first runtime,
//! phase 6): when it ran, what started it, whether it finished, and how many
//! model calls it made. Kept in `{config}/routines/runs.jsonl`, newest last,
//! at most [`KEEP`] lines. A line holds no step output beyond the run's
//! last line, clipped and with secrets redacted.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use grokhub_core::routine;
use serde::{Deserialize, Serialize};

use crate::routine_run::{Ended, Report};

pub const RUNS_FILE: &str = "runs.jsonl";
/// Lines kept; older ones go on the next write.
pub const KEEP: usize = 2_000;
const LINE_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRow {
    pub name: String,
    pub at_ms: u64,
    /// `timer`, a file's path, `webhook <hook>`, `chat` or `run now`.
    pub why: String,
    /// Finished, or stopped early as planned. A stop or a hand-over to a
    /// full assistant turn is not ok.
    pub ok: bool,
    pub model_calls: u32,
    /// The run's last line: its result, or why it stopped.
    pub line: String,
}

/// Per routine: runs, model calls, and the newest run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stats {
    pub runs: u32,
    pub model_calls: u32,
    pub last: Option<RunRow>,
}

fn path(config_dir: &Path) -> PathBuf {
    routine::routines_dir(config_dir).join(RUNS_FILE)
}

/// The row for a finished run. A dry run is not a run.
pub fn row(name: &str, why: &str, report: &Report, at_ms: u64) -> Option<RunRow> {
    if report.dry_run {
        return None;
    }
    let text = grokhub_core::redact_secrets(&report.text());
    let last = text
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");
    let line = match last.char_indices().nth(LINE_CHARS) {
        Some((cut, _)) => format!("{}…", &last[..cut]),
        None => last.to_string(),
    };
    Some(RunRow {
        name: name.to_string(),
        at_ms,
        why: why.to_string(),
        ok: matches!(report.ended, Ended::Finished | Ended::Done { .. }),
        model_calls: report.model_calls,
        line,
    })
}

pub fn read(config_dir: &Path) -> Vec<RunRow> {
    std::fs::read_to_string(path(config_dir))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Adds one run, keeping the newest [`KEEP`].
pub fn append(config_dir: &Path, row: &RunRow) -> Result<(), String> {
    let mut rows = read(config_dir);
    rows.push(row.clone());
    let from = rows.len().saturating_sub(KEEP);
    let mut text = String::new();
    for r in &rows[from..] {
        text.push_str(&serde_json::to_string(r).map_err(|e| e.to_string())?);
        text.push('\n');
    }
    grokhub_core::pulse_writer::write_atomic(&path(config_dir), &text)
}

/// Logs `report` as a run of `name`; a dry run logs nothing.
pub fn record(config_dir: &Path, name: &str, why: &str, report: &Report) {
    if let Some(row) = row(name, why, report, grokhub_core::now_ms()) {
        let _ = append(config_dir, &row);
    }
}

pub fn stats(config_dir: &Path) -> BTreeMap<String, Stats> {
    let mut out: BTreeMap<String, Stats> = BTreeMap::new();
    for r in read(config_dir) {
        let s = out.entry(r.name.clone()).or_default();
        s.runs += 1;
        s.model_calls += r.model_calls;
        s.last = Some(r);
    }
    out
}

#[cfg(test)]
#[path = "routine_log_tests.rs"]
mod tests;
