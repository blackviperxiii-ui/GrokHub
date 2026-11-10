use std::fs;

use super::decide::{Area, Outcome, Proposal, Situation, State};
use super::habit::{self, Part};
use super::ladder::Undo;
use super::review::{digest, faults, report, Fault};
use super::score::WEEK_MS;
use super::signal::Subject;
use super::{sources, Engine};
use crate::outcome::OutcomeResult;
use crate::run_metrics::RunMetrics;

const HOUR: u64 = 60 * 60 * 1_000;
const DAY: u64 = 24 * HOUR;
/// A Thursday midnight UTC, so the eight weeks below are whole weeks.
const START: u64 = (1_800_000_000_000 / WEEK_MS + 1) * WEEK_MS;

fn run(skill: &str, model: &str, ok: bool, at: u64) -> RunMetrics {
    RunMetrics {
        run: format!("{skill}:{at}"),
        class: format!("skill:{skill}"),
        result: if ok {
            OutcomeResult::Success
        } else {
            OutcomeResult::Failure
        },
        model: model.into(),
        wall_ms: 2_000,
        finished_at: at,
        at,
        ..RunMetrics::default()
    }
}

/// One finished run, as the run log's writer sends it.
fn log(engine: &mut Engine, m: &RunMetrics) {
    let hour = ((m.at % DAY) / HOUR) as u32;
    for s in sources::run(m)
        .into_iter()
        .chain(sources::run_habit(m, hour, false))
    {
        engine.record(s);
    }
    engine.flush(m.at);
}

/// The app changes one setting on its own and the user answers.
fn setting_change(state: &mut State, day: u64, kept: bool, at: u64) {
    let rule = Subject::new("rule", &format!("settings/day-{day}")).unwrap();
    state.decide(
        &Proposal {
            area: Area::Settings,
            rule: rule.clone(),
            confidence: 0.85,
            evidence: 14.0,
            undo: Undo::OneClick,
            value: 0.5,
            hard: false,
            helps: None,
            hurts: None,
            tested: false,
        },
        Situation::default(),
        at,
    );
    let outcome = if kept {
        Outcome::Accepted
    } else {
        Outcome::Declined
    };
    state.outcome(&rule, outcome, at);
}

/// Eight weeks of one made-up person, through the real sources, engine and
/// decision state. Every morning they run their report skill and it works.
/// Every afternoon a second skill runs on a weaker model and fails three
/// days in four. For four weeks they keep what the app changes in Settings;
/// for the next four they say no two days in three.
fn eight_weeks(dir: &std::path::Path) -> (Engine, State, u64) {
    let (mut engine, mut state) = (Engine::open(dir), State::default());
    for day in 0..56u64 {
        let midnight = START + day * DAY;
        log(
            &mut engine,
            &run("weekly-report", "big-model", true, midnight + 8 * HOUR),
        );
        log(
            &mut engine,
            &run("flaky", "weak-model", day % 4 == 0, midnight + 14 * HOUR),
        );
        setting_change(
            &mut state,
            day,
            day < 28 || day % 3 == 0,
            midnight + 20 * HOUR,
        );
    }
    (engine, state, START + 56 * DAY - HOUR)
}

#[test]
fn a_simulated_user_ends_with_their_habit_the_apps_faults_and_a_report_without_their_names() {
    let dir = std::env::temp_dir().join(format!("grokhub-learn-sim-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (engine, state, now) = eight_weeks(&dir);
    assert_eq!(engine.dropped(), 0);

    // The morning habit is found, and only that one.
    let mornings: Vec<String> = habit::due(engine.beliefs(), Part::Morning, false, now)
        .into_iter()
        .map(|h| h.skill)
        .collect();
    assert_eq!(mornings, vec!["weekly-report".to_string()]);

    // The app's own faults, the worst first. The run counts are what is left
    // of 56 daily runs after the 30-day fade.
    let found = faults(engine.beliefs(), state.score(), now);
    assert_eq!(
        found,
        vec![
            Fault::Unreliable {
                subject: "model:weak-model".into(),
                good: 8,
                runs: 32,
            },
            Fault::Unreliable {
                subject: "skill:flaky".into(),
                good: 8,
                runs: 32,
            },
            Fault::Slipped {
                area: Area::Settings,
                now: 32,
                before: 100,
                resting: true,
            },
        ]
    );

    // The weekly self-review reads all three.
    assert_eq!(
        digest(&found),
        "What the app measured about itself (ids and counts only):\n\
         - `model:weak-model` went well in 8 of its last 32 runs (25%).\n\
         - `skill:flaky` went well in 8 of its last 32 runs (25%).\n\
         - Settings: 32% of the app's own changes were kept in the last four weeks, down from 100%. It only offers for now.\n"
    );
    // The report for the developers leaves the user's skill out.
    assert_eq!(
        report(&found, " Loadout 9.9.9 ").as_deref(),
        Some(
            "## What the app measured going wrong\n\n\
             - `model:weak-model` went well in 8 of its last 32 runs (25%).\n\
             - Settings: 32% of the app's own changes were kept in the last four weeks, down from 100%. It only offers for now.\n\n\
             ---\nMeasured by Loadout 9.9.9 on one PC. Ids and counts only.\n"
        )
    );
}

#[test]
fn a_good_week_has_no_faults_no_digest_and_no_report() {
    let dir = std::env::temp_dir().join(format!("grokhub-learn-sim-good-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let (mut engine, mut state) = (Engine::open(&dir), State::default());
    for day in 0..20u64 {
        let at = START + day * DAY + 9 * HOUR;
        // One failure in five is not a fault, and nine bad runs are too few.
        log(&mut engine, &run("tidy-up", "big-model", day % 5 != 0, at));
        if day < 9 {
            log(&mut engine, &run("rare", "new-model", false, at + HOUR));
        }
        setting_change(&mut state, day, true, at);
    }
    let found = faults(engine.beliefs(), state.score(), START + 20 * DAY);
    assert_eq!(found, Vec::new());
    assert_eq!(digest(&found), "");
    assert_eq!(report(&found, "Loadout 9.9.9"), None);
    // A fault in the user's own skill alone is for the self-review, not for
    // the developers.
    let own = [Fault::Unreliable {
        subject: "skill:my-skill".into(),
        good: 2,
        runs: 12,
    }];
    assert_eq!(report(&own, "Loadout 9.9.9"), None);
    assert_eq!(
        digest(&own),
        "What the app measured about itself (ids and counts only):\n\
         - `skill:my-skill` went well in 2 of its last 12 runs (16%).\n"
    );
}
