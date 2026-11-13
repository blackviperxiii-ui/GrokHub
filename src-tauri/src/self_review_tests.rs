use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use grokhub_agent::harness::WeeklyPass;
use grokhub_core::outcome::{self as oc, OutcomeResult, TaskOutcome};
use grokhub_core::scheduler_lease::{write_lease, LeaseOwner, SchedulerLease};
use grokhub_core::self_review::{CodeIdea, RankedProposal};
use grokhub_core::{UpdateCard, UpdateKind};

use super::*;

/// Sunday 15 Nov 2026, 03:00 UTC.
const SUN_3AM: u64 = 1_794_711_600_000;
/// Saturday 14 Nov 2026, 23:00 UTC.
const SAT_11PM: u64 = 1_794_697_200_000;
const DAY: u64 = 24 * 60 * 60 * 1000;
const PID: u32 = 4242;

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("shell-self-review-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn no_call(
    _: &Path,
    _: &[sr::SkillStats],
    _: Option<&hx::BrainAsk>,
    _: u64,
) -> Result<WeeklyPass, String> {
    panic!("no model call expected")
}

struct Feed(RefCell<Vec<UpdateCard>>);

impl Feed {
    fn new() -> Self {
        Self(RefCell::new(Vec::new()))
    }
    fn edit(&self, f: &mut dyn FnMut(&mut Vec<UpdateCard>) -> usize) -> Result<usize, String> {
        Ok(f(&mut self.0.borrow_mut()))
    }
    fn titles(&self) -> Vec<String> {
        self.0.borrow().iter().map(|c| c.title.clone()).collect()
    }
}

fn skill_run(dir: &Path, skill: &str, k: u64, ok: bool, at: u64) {
    let o = TaskOutcome {
        task: format!("{skill}:{k}"),
        signature: oc::skill_signature(skill),
        result: if ok {
            OutcomeResult::Success
        } else {
            OutcomeResult::Failure
        },
        finished_at: at,
        at,
        ..Default::default()
    };
    oc::append_outcome(dir, &o).unwrap();
}

fn write_skill(dir: &Path, name: &str, steps: &str) {
    let folder = dir.join("skills").join(name);
    std::fs::create_dir_all(&folder).unwrap();
    let md = grokhub_core::render_skill_md(&grokhub_core::SkillMd {
        name: name.into(),
        description: "a report".into(),
        instructions: steps.into(),
        ..Default::default()
    });
    std::fs::write(folder.join("SKILL.md"), md).unwrap();
}

#[test]
fn a_fresh_cabin_lease_or_no_clock_runs_nothing() {
    let dir = temp_dir("gate");
    write_lease(
        &dir,
        &SchedulerLease {
            owner: LeaseOwner::Cabin,
            pid: 7,
            beat_ms: SUN_3AM - 30_000,
        },
    )
    .unwrap();
    let feed = Feed::new();
    let look_now = |offset| look(&dir, PID, SUN_3AM, offset, &no_call, &|f| feed.edit(f));
    assert_eq!(look_now(Some(0)), Look::CabinOpen);
    assert_eq!(
        wr::last_day(&dir),
        None,
        "nothing claimed while the cabin is open"
    );
    // The cabin closed two minutes ago: its beat is stale.
    write_lease(
        &dir,
        &SchedulerLease {
            owner: LeaseOwner::Cabin,
            pid: 7,
            beat_ms: SUN_3AM - 120_000,
        },
    )
    .unwrap();
    assert_eq!(look_now(None), Look::NoClock);
    assert_eq!(wr::last_day(&dir), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sunday_at_the_dream_hour_runs_once_and_writes_the_shared_marker() {
    let dir = temp_dir("once");
    let feed = Feed::new();
    let at = |now| look(&dir, PID, now, Some(0), &no_call, &|f| feed.edit(f));
    assert_eq!(at(SAT_11PM), Look::NotDue);
    assert_eq!(at(SUN_3AM), Look::Ran { posted: 0 });
    assert_eq!(wr::last_day(&dir).as_deref(), Some("2026-11-15"));
    assert_eq!(at(SUN_3AM + 60 * 60_000), Look::NotDue);
    // UTC-6: 03:00 UTC is still Saturday 21:00 there.
    let west = temp_dir("west");
    assert_eq!(
        look(&west, PID, SUN_3AM, Some(-360), &no_call, &|f| feed.edit(f)),
        Look::NotDue
    );
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&west);
}

#[test]
fn a_night_the_cabin_already_reviewed_is_skipped() {
    let dir = temp_dir("cabin-did");
    std::fs::write(
        dir.join(wr::SELF_REVIEW_FILE),
        "{\n  \"last_day\": \"2026-11-15\",\n  \"pending\": []\n}",
    )
    .unwrap();
    let feed = Feed::new();
    assert_eq!(
        look(&dir, PID, SUN_3AM, Some(0), &no_call, &|f| feed.edit(f)),
        Look::NotDue
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_dream_hour_comes_from_app_json() {
    let dir = temp_dir("hour");
    assert_eq!(dream_hour(&dir), 3);
    std::fs::write(dir.join("app.json"), r#"{"dreamHour": 5}"#).unwrap();
    assert_eq!(dream_hour(&dir), 5);
    let feed = Feed::new();
    assert_eq!(
        look(&dir, PID, SUN_3AM, Some(0), &no_call, &|f| feed.edit(f)),
        Look::NotDue,
        "03:00 is before a 05:00 dream hour"
    );
    std::fs::write(dir.join("app.json"), r#"{"dreamHour": 99}"#).unwrap();
    assert_eq!(dream_hour(&dir), 23);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn failures_this_week_make_one_call_and_its_cards_land_in_the_feed() {
    let dir = temp_dir("call");
    write_skill(&dir, "report", "1. open report.md\n2. click Export\n");
    for k in 0..5u64 {
        skill_run(&dir, "report", k, k >= 3, SUN_3AM - (k + 1) * DAY / 2);
    }
    let calls = Cell::new(0);
    let call = |_: &Path, stats: &[sr::SkillStats], _: Option<&hx::BrainAsk>, _: u64| {
        calls.set(calls.get() + 1);
        assert_eq!(stats[0].numbers(), "3 of 5 report runs needed a fix");
        Ok(WeeklyPass {
            kept: vec![RankedProposal {
                skill: "report".into(),
                snippet: false,
                title: "Wait for Export".into(),
                steps: "1. open report.md 2. click Export 3. wait for the file".into(),
                numbers: "3 of 5 report runs needed a fix".into(),
                failures: 3,
            }],
            ideas: vec![CodeIdea {
                slug: "faster-export".into(),
                title: "Faster export".into(),
                problem: "Export waits".into(),
                evidence: "3 of 5 runs".into(),
            }],
            ..Default::default()
        })
    };
    let feed = Feed::new();
    assert_eq!(
        look(&dir, PID, SUN_3AM, Some(0), &call, &|f| feed.edit(f)),
        Look::Ran { posted: 2 }
    );
    assert_eq!(calls.get(), 1);
    assert_eq!(feed.titles(), vec!["Wait for Export", "Faster export"]);
    assert!(feed
        .0
        .borrow()
        .iter()
        .all(|c| c.kind == UpdateKind::Suggestion));
    let sources: Vec<String> = wr::pending(&dir).into_iter().map(|p| p.source).collect();
    assert_eq!(
        sources,
        vec!["self-review:patch:report", "self-review:code:faster-export"]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn quiet_mode_logs_the_weeks_cards_and_posts_none() {
    let dir = temp_dir("quiet");
    std::fs::write(dir.join("app.json"), r#"{"selfImproveQuiet": true}"#).unwrap();
    write_skill(&dir, "report", "1. open\n");
    for k in 0..4u64 {
        skill_run(&dir, "report", k, k >= 2, SUN_3AM - (k + 1) * DAY);
    }
    let call = |_: &Path, _: &[sr::SkillStats], _: Option<&hx::BrainAsk>, _: u64| {
        Ok(WeeklyPass {
            kept: vec![RankedProposal {
                skill: "report".into(),
                snippet: false,
                title: "Open then save".into(),
                steps: "1. open 2. save".into(),
                numbers: "2 of 4 report runs needed a fix".into(),
                failures: 2,
            }],
            ..Default::default()
        })
    };
    let feed = Feed::new();
    assert_eq!(
        look(&dir, PID, SUN_3AM, Some(0), &call, &|f| feed.edit(f)),
        Look::Ran { posted: 0 }
    );
    assert!(feed.titles().is_empty());
    assert!(wr::pending(&dir).is_empty());
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(wr::SELF_REVIEW_FILE)).unwrap())
            .unwrap();
    assert_eq!(raw["quiet_log"][0]["title"], "Open then save");
    assert_eq!(raw["quiet_log"][0]["day"], "2026-11-15");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_quiet_week_with_no_failures_makes_no_call() {
    let dir = temp_dir("calm");
    write_skill(&dir, "report", "1. open\n");
    skill_run(&dir, "report", 0, true, SUN_3AM - DAY);
    let feed = Feed::new();
    assert_eq!(
        look(&dir, PID, SUN_3AM, Some(0), &no_call, &|f| feed.edit(f)),
        Look::Ran { posted: 0 }
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_local_day_key_matches_the_cabins_last_day() {
    assert_eq!(local_day(SUN_3AM, 0), "2026-11-15");
    assert_eq!(local_day(SUN_3AM, -360), "2026-11-14");
    assert_eq!(local_day(SAT_11PM, 60), "2026-11-15");
}

#[test]
fn a_failed_model_call_releases_the_claim_so_the_next_tick_retries() {
    let dir = temp_dir("retry-call");
    write_skill(&dir, "report", "1. open report.md\n");
    for k in 0..3u64 {
        skill_run(&dir, "report", k, false, SUN_3AM - (k + 1) * DAY / 2);
    }
    let calls = Cell::new(0);
    let call = |_: &Path, _: &[sr::SkillStats], _: Option<&hx::BrainAsk>, _: u64| {
        calls.set(calls.get() + 1);
        if calls.get() == 1 {
            return Err("network offline".into());
        }
        Ok(WeeklyPass {
            kept: vec![RankedProposal {
                skill: "report".into(),
                snippet: false,
                title: "Retry worked".into(),
                steps: "1. open report.md 2. save".into(),
                numbers: "3 of 3 report runs needed a fix".into(),
                failures: 3,
            }],
            ..Default::default()
        })
    };
    let feed = Feed::new();
    assert_eq!(
        look(&dir, PID, SUN_3AM, Some(0), &call, &|f| feed.edit(f)),
        Look::Ran { posted: 0 }
    );
    assert_eq!(wr::last_day(&dir), None);
    assert_eq!(
        look(&dir, PID, SUN_3AM + 5 * 60_000, Some(0), &call, &|f| feed
            .edit(f)),
        Look::Ran { posted: 1 }
    );
    assert_eq!(calls.get(), 2);
    assert_eq!(feed.titles(), vec!["Retry worked"]);
    assert_eq!(wr::last_day(&dir).as_deref(), Some("2026-11-15"));
    let _ = std::fs::remove_dir_all(&dir);
}
