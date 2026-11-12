use std::path::PathBuf;

use grokhub_core::outcome::{self as oc, OutcomeResult};
use grokhub_core::run_metrics;

use super::*;
use crate::harness::{append_span, fixture_span, Span};

const NOW: u64 = 1_794_711_600_000;
const HOUR: u64 = 60 * 60 * 1000;

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("grokhub-chat-record-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn step(
    d: &std::path::Path,
    trace: &str,
    turn: u32,
    tool: &str,
    result: &str,
    ui: Option<bool>,
) -> Span {
    let mut s = fixture_span(NOW - 5_000, tool, "{}", "allow", result, ui, "").in_turn(trace, turn);
    s.session_id = trace.into();
    append_span(d, &s).unwrap();
    s
}

fn reply(d: &std::path::Path, trace: &str, turn: u32, text: &str) {
    let mut s = Span::reply(trace, text, &[]).in_turn(trace, turn);
    s.ts_ms = NOW - 1_000;
    append_span(d, &s).unwrap();
}

#[test]
fn the_signature_is_the_skill_else_the_asks_topic() {
    assert_eq!(
        task_signature("weekly-report", "anything"),
        "skill:weekly-report"
    );
    assert_eq!(
        task_signature("", "Summarize the March invoices"),
        "topic:summarize march invoic"
    );
    assert_eq!(task_signature("  ", ""), "");
}

#[test]
fn a_turn_end_writes_one_outcome_and_one_metrics_line_once() {
    let d = dir("turn");
    let v = step(&d, "shell-a", 1, "verify_script", "pass", None);
    reply(&d, "shell-a", 1, "Report filled. VERIFY_OK");
    assert_eq!(latest_step_turn(&d, "shell-a", NOW - 10_000), Some(1));
    assert_eq!(
        latest_step_turn(&d, "shell-a", NOW),
        None,
        "the step ran before this turn"
    );
    let o = record_turn(&d, "shell-a", 1, "skill:weekly-report", false, NOW, 90).expect("recorded");
    assert_eq!(
        (o.task.as_str(), o.signature.as_str(), o.result),
        ("shell-a:1", "skill:weekly-report", OutcomeResult::Success)
    );
    assert_eq!(o.reasons, vec!["verify_ok".to_string()]);
    assert_eq!(o.span_ids.len(), 2, "the verify step and the reply");
    assert_eq!(o.span_ids[0], v.span_ref());
    let runs = run_metrics::read_runs(&d);
    assert_eq!(
        (
            runs.len(),
            runs[0].run.as_str(),
            runs[0].class.as_str(),
            runs[0].result,
            runs[0].verified,
            runs[0].steps,
            runs[0].undos
        ),
        (
            1,
            "shell-a:1",
            "skill:weekly-report",
            OutcomeResult::Success,
            true,
            0,
            0
        )
    );
    assert_eq!(
        record_turn(&d, "shell-a", 1, "skill:weekly-report", false, NOW + 1, 90),
        None
    );
    assert_eq!(
        oc::read_outcome_lines(&d).len(),
        1,
        "a second turn end writes nothing"
    );
    assert_eq!(run_metrics::read_run_lines(&d).len(), 1);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_reply_only_turn_or_an_empty_signature_records_nothing() {
    let d = dir("reply-only");
    reply(&d, "shell-b", 1, "Hi there");
    assert_eq!(latest_step_turn(&d, "shell-b", 0), None);
    assert_eq!(
        record_turn(&d, "shell-b", 1, "topic:hi", false, NOW, 90),
        None
    );
    step(&d, "shell-b", 2, "click", "clicked", Some(true));
    assert_eq!(record_turn(&d, "shell-b", 2, "", false, NOW, 90), None);
    assert!(oc::read_outcome_lines(&d).is_empty());
    assert!(run_metrics::read_run_lines(&d).is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_ladder_pause_fails_the_turn() {
    let d = dir("paused");
    step(&d, "shell-p", 1, "click", "clicked", Some(true));
    let o = record_turn(&d, "shell-p", 1, "topic:export report", true, NOW, 90).unwrap();
    assert_eq!(o.result, OutcomeResult::Failure);
    assert_eq!(o.reasons, vec!["retry_exhausted".to_string()]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_correction_after_the_turn_makes_it_a_failure() {
    let d = dir("correction");
    step(&d, "shell-c", 3, "click", "clicked", Some(true));
    reply(&d, "shell-c", 3, "Here is the summary.");
    let sig = task_signature("", "Summarize the March invoices");
    let first = record_turn(&d, "shell-c", 3, &sig, false, NOW, 90).unwrap();
    assert_eq!(
        (first.signature.as_str(), first.result),
        ("topic:summarize march invoic", OutcomeResult::Pending)
    );
    assert_eq!(
        user_said(&d, "shell-c", "thanks, now April", NOW + 10, 90),
        None
    );
    assert_eq!(oc::read_outcome_lines(&d).len(), 1, "not a correction");
    assert_eq!(run_metrics::read_run_lines(&d).len(), 1);
    assert_eq!(
        user_said(
            &d,
            "shell-other",
            "No, that's the wrong sheet",
            NOW + 20,
            90
        ),
        None
    );
    let late = user_said(&d, "shell-c", "No, that's the wrong sheet", NOW + 30, 90).unwrap();
    assert_eq!(late.task, "shell-c:3");
    let all = oc::read_outcomes(&d);
    assert_eq!(all.len(), 1);
    assert_eq!(
        (all[0].result, all[0].reasons.clone()),
        (OutcomeResult::Failure, vec!["correction".to_string()])
    );
    let runs = run_metrics::read_runs(&d);
    assert_eq!(
        (
            runs.len(),
            runs[0].steps,
            runs[0].result,
            runs[0].corrections,
            runs[0].undos
        ),
        (1, 1, OutcomeResult::Failure, 1, 0),
        "the click counts as a step; the correction is a newer line"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_correction_hits_the_chats_newest_task_not_a_mission_line() {
    let d = dir("newest");
    step(&d, "shell-n", 1, "click", "clicked", Some(true));
    record_turn(&d, "shell-n", 1, "topic:first job", false, NOW - HOUR, 90).unwrap();
    let mut s2 = fixture_span(
        NOW - 2_000,
        "click",
        "{}",
        "allow",
        "clicked",
        Some(true),
        "",
    )
    .in_turn("shell-n", 2);
    s2.session_id = "shell-n".into();
    append_span(&d, &s2).unwrap();
    record_turn(&d, "shell-n", 2, "topic:second job", false, NOW, 90).unwrap();
    let mission = oc::TaskOutcome::finished(
        "shell-n",
        "topic:ship",
        &oc::OutcomeSignals::default(),
        NOW + 5,
    );
    oc::append_outcome(&d, &mission).unwrap();
    let late = user_said(
        &d,
        "shell-n",
        "That is wrong, it was the second job",
        NOW + 10,
        90,
    )
    .unwrap();
    assert_eq!(late.task, "shell-n:2");
    let results: Vec<(String, OutcomeResult)> = oc::read_outcomes(&d)
        .into_iter()
        .map(|o| (o.task, o.result))
        .collect();
    assert!(
        results.contains(&("shell-n:1".into(), OutcomeResult::Pending)),
        "{results:?}"
    );
    assert!(
        results.contains(&("shell-n:2".into(), OutcomeResult::Failure)),
        "{results:?}"
    );
    assert!(
        results.contains(&("shell-n".into(), OutcomeResult::Pending)),
        "{results:?}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn an_undo_fails_the_skills_tasks_from_the_last_day() {
    let d = dir("undo");
    step(&d, "shell-u", 1, "verify_script", "pass", None);
    reply(&d, "shell-u", 1, "Report filled. VERIFY_OK");
    record_turn(&d, "shell-u", 1, "skill:weekly-report", false, NOW, 90).unwrap();
    assert_eq!(after_undo(&d, "other-skill", NOW + HOUR, 90), 0);
    assert_eq!(after_undo(&d, "weekly-report", NOW + HOUR, 90), 1);
    let all = oc::read_outcomes(&d);
    assert_eq!(all.len(), 1, "still one record for the task");
    assert_eq!(
        (all[0].result, all[0].reasons.clone()),
        (OutcomeResult::Failure, vec!["undo".to_string()])
    );
    assert_eq!(
        oc::read_outcome_lines(&d).len(),
        2,
        "the undo is a superseding line"
    );
    let runs = run_metrics::read_runs(&d);
    assert_eq!(
        (
            runs.len(),
            runs[0].result,
            runs[0].undos,
            runs[0].corrections
        ),
        (1, OutcomeResult::Failure, 1, 0)
    );
    assert_eq!(run_metrics::read_run_lines(&d).len(), 2);
    assert_eq!(
        after_undo(&d, "weekly-report", NOW + HOUR + 1, 90),
        0,
        "already a failure"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn an_undo_a_day_later_leaves_the_task_alone() {
    let d = dir("undo-late");
    step(&d, "shell-v", 1, "verify_script", "pass", None);
    record_turn(&d, "shell-v", 1, "skill:weekly-report", false, NOW, 90).unwrap();
    assert_eq!(after_undo(&d, "weekly-report", NOW + 25 * HOUR, 90), 0);
    assert_eq!(oc::read_outcomes(&d)[0].result, OutcomeResult::Success);
    let _ = std::fs::remove_dir_all(&d);
}
