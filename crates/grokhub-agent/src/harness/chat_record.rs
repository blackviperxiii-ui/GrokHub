//! A chat's task outcomes and per-run metrics, written to the shared profile
//! files the weekly self-review and the skill-change trials judge. The cabin
//! keeps its own copy of this glue (its turn end, its "that was wrong" check
//! and its undo hook); this is the shared home any app can call. Unlike the
//! cabin, nothing here is held in memory: the chat's last task is read back
//! from `outcomes.jsonl`, so a call after a restart still finds it.

use std::path::Path;

use grokhub_core::outcome::{self as oc, TaskOutcome};
use grokhub_core::run_metrics;

use super::{audit_file, outcome_from_spans, read_spans_tail, run_metrics_from_spans, REPLY_TOOL};

/// How many span-log lines a turn end reads back, as the cabin does.
const SPAN_LINES: usize = 4_000;

/// The task a turn ran: its skill when one ran, else the topic of the ask.
/// Empty when neither says anything (nothing is recorded then).
pub fn task_signature(skill: &str, ask: &str) -> String {
    if !skill.trim().is_empty() {
        return oc::skill_signature(skill);
    }
    oc::topic_signature(ask)
}

/// The `chat:turn` task id for one turn.
pub fn task_id(trace: &str, turn: u32) -> String {
    format!("{trace}:{turn}")
}

/// The newest turn of `trace` that ran a step at or after `since` (a reply
/// alone is not a run). An app passes the time its turn started, so a turn
/// that ran no step never picks up an older one.
pub fn latest_step_turn(config_dir: &Path, trace: &str, since: u64) -> Option<u32> {
    let (spans, _) = read_spans_tail(config_dir, trace, SPAN_LINES);
    spans
        .iter()
        .filter(|s| s.chat_id == trace && s.tool != REPLY_TOOL && s.ts_ms >= since)
        .map(|s| s.turn)
        .max()
}

/// One outcome line and one metrics line for a finished turn that ran
/// steps, the cabin's turn-end record. `paused` means a retry ladder ran out.
/// Writes nothing (None) for an empty signature, a turn with no steps, or a
/// turn already on file, so it is safe to call at every turn end.
pub fn record_turn(
    config_dir: &Path,
    trace: &str,
    turn: u32,
    signature: &str,
    paused: bool,
    now: u64,
    keep_days: u32,
) -> Option<TaskOutcome> {
    if signature.is_empty() {
        return None;
    }
    let task = task_id(trace, turn);
    if oc::read_outcomes(config_dir).iter().any(|o| o.task == task) {
        return None;
    }
    let (spans, _) = read_spans_tail(config_dir, trace, SPAN_LINES);
    let spans: Vec<_> = spans.into_iter().filter(|s| s.turn == turn).collect();
    if !spans.iter().any(|s| s.tool != REPLY_TOOL) {
        return None;
    }
    let mut findings: Vec<String> = Vec::new();
    if let Ok(audit) = audit_file(config_dir, trace, Some((trace, turn))) {
        for w in audit.flagged.iter().filter(|w| w.window.turn == turn) {
            for f in &w.findings {
                if !findings.contains(&f.detector) {
                    findings.push(f.detector.clone());
                }
            }
        }
    }
    let outcome = outcome_from_spans(&task, signature, &spans, &findings, paused, now);
    oc::append_outcome(config_dir, &outcome).ok()?;
    let mut metrics = run_metrics_from_spans(&outcome, &spans);
    if let Some(line) = crate::brain::brief_for_turn(config_dir, trace, turn) {
        metrics.brain = line.run_brain();
    }
    let _ = run_metrics::append_run(config_dir, &metrics, keep_days);
    Some(outcome)
}

/// The chat's newest recorded task, if it has one.
pub fn last_task(config_dir: &Path, trace: &str) -> Option<TaskOutcome> {
    let prefix = format!("{trace}:");
    oc::read_outcomes(config_dir)
        .into_iter()
        .filter(|o| o.task.starts_with(&prefix))
        .max_by_key(|o| (o.finished_at, o.at))
}

/// The person's next line reads as "that was wrong": the chat's last task
/// becomes a failure (a superseding outcome line and a corrected metrics
/// line). Anything else writes nothing.
pub fn user_said(
    config_dir: &Path,
    trace: &str,
    text: &str,
    now: u64,
    keep_days: u32,
) -> Option<TaskOutcome> {
    if !oc::is_correction(text) {
        return None;
    }
    let last = last_task(config_dir, trace)?;
    let late = last.superseded(oc::REASON_CORRECTION, now);
    oc::append_outcome(config_dir, &late).ok()?;
    if let Some(run) = run_metrics::find_run(config_dir, &late.task) {
        let _ = run_metrics::append_run(config_dir, &run.corrected(late.at), keep_days);
    }
    Some(late)
}

/// A change to `skill` was undone: its tasks from the 24 hours before fail.
/// Returns how many tasks it marked.
pub fn after_undo(config_dir: &Path, skill: &str, now: u64, keep_days: u32) -> usize {
    let sig = oc::skill_signature(&grokhub_core::skill_dir_name(skill));
    let lates = oc::undo_supersedes(&oc::read_outcomes(config_dir), &sig, now);
    for late in &lates {
        let _ = oc::append_outcome(config_dir, late);
        if let Some(run) = run_metrics::find_run(config_dir, &late.task) {
            let _ = run_metrics::append_run(config_dir, &run.undone(now), keep_days);
        }
    }
    lates.len()
}

#[cfg(test)]
#[path = "chat_record_tests.rs"]
mod tests;
