use std::path::PathBuf;

use grokhub_agent::harness::{append_span, fixture_span};
use grokhub_core::outcome::{self as oc, OutcomeResult};
use grokhub_core::run_metrics;

use super::*;

const NOW: u64 = 1_794_711_600_000;
const HOUR: u64 = 60 * 60 * 1000;

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("grokhub-chat-trials-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn step(d: &std::path::Path, chat: &str, turn: u32, at: u64) {
    let mut s =
        fixture_span(at, "click", "{}", "allow", "clicked", Some(true), "").in_turn(chat, turn);
    s.session_id = chat.into();
    append_span(d, &s).unwrap();
}

fn skill(d: &std::path::Path, name: &str, slash: &str) {
    let md = grokhub_core::render_skill_md(&grokhub_core::SkillMd {
        name: name.into(),
        slash: slash.into(),
        trigger: slash.into(),
        instructions: "1. Open report.md".into(),
        ..Default::default()
    });
    let at = d.join("skills").join(name);
    std::fs::create_dir_all(&at).unwrap();
    std::fs::write(at.join("SKILL.md"), md).unwrap();
}

#[test]
fn keep_days_reads_the_cabins_key() {
    let d = dir("keep");
    assert_eq!(keep_days(&d), 90);
    std::fs::write(d.join("app.json"), r#"{"amrRetentionDays": 180}"#).unwrap();
    assert_eq!(keep_days(&d), 180);
    std::fs::write(d.join("app.json"), r#"{"amr_retention_days": 0}"#).unwrap();
    assert_eq!(keep_days(&d), 0);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_skill_message_is_signed_by_the_skill_else_by_its_topic() {
    let d = dir("sig");
    skill(&d, "weekly-report", "/weekly-report");
    assert_eq!(
        signature(&d, "/weekly-report for March"),
        "skill:weekly-report"
    );
    assert_eq!(
        signature(&d, "Follow skill weekly-report"),
        "skill:weekly-report"
    );
    assert_eq!(
        signature(&d, "Summarize the March invoices"),
        "topic:summarize march invoic"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_turn_end_records_this_turns_steps_once() {
    let d = dir("turn");
    step(&d, "shell-t", 1, NOW - HOUR);
    assert_eq!(
        turn_end(
            &d,
            "shell-t",
            "Summarize the March invoices",
            NOW - 1_000,
            NOW
        ),
        None,
        "this turn ran no step; the old one is left alone"
    );
    step(&d, "shell-t", 2, NOW - 500);
    let o = turn_end(
        &d,
        "shell-t",
        "Summarize the March invoices",
        NOW - 1_000,
        NOW,
    )
    .unwrap();
    assert_eq!(
        (o.task.as_str(), o.signature.as_str(), o.result),
        (
            "shell-t:2",
            "topic:summarize march invoic",
            OutcomeResult::Pending
        )
    );
    assert_eq!(turn_end(&d, "shell-t", "again", NOW - 1_000, NOW + 1), None);
    assert_eq!(oc::read_outcome_lines(&d).len(), 1);
    assert_eq!(run_metrics::read_runs(&d)[0].run, "shell-t:2");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn that_was_wrong_then_an_undo_both_land_as_failures() {
    let d = dir("late");
    skill(&d, "weekly-report", "/weekly-report");
    step(&d, "shell-w", 1, NOW - 500);
    turn_end(&d, "shell-w", "/weekly-report", NOW - 1_000, NOW).unwrap();
    assert_eq!(said(&d, "shell-w", "thanks", NOW + 1), None);
    let late = said(&d, "shell-w", "No, that's the wrong sheet", NOW + 2).unwrap();
    assert_eq!(late.reasons, vec!["correction".to_string()]);
    let run = &run_metrics::read_runs(&d)[0];
    assert_eq!((run.corrections, run.result), (1, OutcomeResult::Failure));
    assert_eq!(
        skill_undone(&d, "weekly-report", NOW + HOUR),
        0,
        "already a failure"
    );

    step(&d, "shell-w", 2, NOW + 2 * HOUR);
    turn_end(
        &d,
        "shell-w",
        "/weekly-report",
        NOW + 2 * HOUR - 1,
        NOW + 2 * HOUR + 1,
    )
    .unwrap();
    assert_eq!(skill_undone(&d, "weekly-report", NOW + 3 * HOUR), 1);
    let runs = run_metrics::read_runs(&d);
    let two = runs.iter().find(|r| r.run == "shell-w:2").unwrap();
    assert_eq!((two.undos, two.result), (1, OutcomeResult::Failure));
    let _ = std::fs::remove_dir_all(&d);
}
