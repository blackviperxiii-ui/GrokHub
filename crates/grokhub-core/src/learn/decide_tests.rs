use super::decide::*;
use super::ladder::Undo;
use super::now::Snapshot;
use super::score::{Score, Trend, MIN_OUTCOMES, SLIP_REST_MS, WEEK_MS};
use super::signal::Subject;

const T: u64 = 1_800_000_000_000;
const HOUR: u64 = 60 * 60 * 1_000;
const DAY: u64 = 24 * HOUR;

fn rule(id: &str) -> Subject {
    Subject::new("rule", id).expect("rule id")
}

/// A one-click settings change the app is sure of.
fn sure(id: &str) -> Proposal {
    Proposal {
        area: Area::Settings,
        rule: rule(id),
        confidence: 0.85,
        evidence: 14.0,
        undo: Undo::OneClick,
        value: 0.5,
        hard: false,
        helps: None,
        hurts: None,
    }
}

fn calm() -> Situation {
    Situation::default()
}

fn answer(s: &mut State, p: &Proposal, at: u64) -> (Verdict, Reason) {
    let d = s.decide(p, calm(), at);
    (d.verdict, d.reason)
}

/// A state where `id` has had its three yes answers.
fn graduated(id: &str) -> State {
    let mut s = State::default();
    for _ in 0..SHADOW_PASSES {
        assert_eq!(s.outcome(&rule(id), Outcome::Accepted, T), None);
    }
    s
}

#[test]
fn a_new_rule_only_offers_until_three_yes_answers_in_a_row() {
    let mut s = State::default();
    let p = sure("text-size");
    assert_eq!(answer(&mut s, &p, T), (Verdict::Offer, Reason::Shadow));
    s.outcome(&p.rule, Outcome::Accepted, T);
    s.outcome(&p.rule, Outcome::Accepted, T);
    assert!(!s.graduated(&p.rule));
    assert_eq!(answer(&mut s, &p, T), (Verdict::Offer, Reason::Shadow));
    s.outcome(&p.rule, Outcome::Accepted, T);
    assert!(s.graduated(&p.rule));
    assert_eq!(answer(&mut s, &p, T), (Verdict::Act, Reason::Sure));
    // Another rule has earned nothing from this one.
    assert_eq!(
        answer(&mut s, &sure("theme"), T),
        (Verdict::Offer, Reason::Shadow)
    );
}

#[test]
fn a_no_rests_the_rule_a_month_and_starts_its_shadow_over() {
    let mut s = State::default();
    let p = sure("text-size");
    s.outcome(&p.rule, Outcome::Accepted, T);
    s.outcome(&p.rule, Outcome::Accepted, T);
    s.outcome(&p.rule, Outcome::Declined, T);
    assert_eq!(
        answer(&mut s, &p, T + 29 * DAY),
        (Verdict::Wait, Reason::Resting)
    );
    assert_eq!(
        answer(&mut s, &p, T + 30 * DAY),
        (Verdict::Offer, Reason::Shadow)
    );
    s.outcome(&p.rule, Outcome::Accepted, T + 30 * DAY);
    assert!(
        !s.graduated(&p.rule),
        "one yes after a no is one, not three"
    );
}

#[test]
fn each_term_names_its_own_reason() {
    let p = sure("text-size");
    let cases: Vec<(&str, Proposal, Situation, Verdict, Reason)> = vec![
        (
            "not sure",
            Proposal {
                confidence: 0.49,
                ..p.clone()
            },
            calm(),
            Verdict::Wait,
            Reason::NotSureYet,
        ),
        (
            "offer line",
            Proposal {
                confidence: 0.79,
                ..p.clone()
            },
            calm(),
            Verdict::Offer,
            Reason::SureEnoughToOffer,
        ),
        (
            "no undo",
            Proposal {
                undo: Undo::Never,
                confidence: 1.0,
                ..p.clone()
            },
            calm(),
            Verdict::Offer,
            Reason::NoUndo,
        ),
        (
            "bigger change, too few sightings",
            Proposal {
                undo: Undo::Restore,
                confidence: 0.95,
                evidence: 9.0,
                ..p.clone()
            },
            calm(),
            Verdict::Offer,
            Reason::SureEnoughToOffer,
        ),
        (
            "halt",
            p.clone(),
            Situation {
                halted: true,
                ..calm()
            },
            Verdict::Wait,
            Reason::Halted,
        ),
        (
            "quiet",
            p.clone(),
            Situation {
                quiet: true,
                ..calm()
            },
            Verdict::Wait,
            Reason::Quiet,
        ),
        (
            "busy",
            p.clone(),
            Situation {
                busy: true,
                ..calm()
            },
            Verdict::Wait,
            Reason::Busy,
        ),
        ("all clear", p.clone(), calm(), Verdict::Act, Reason::Sure),
    ];
    for (label, proposal, now, verdict, reason) in cases {
        let mut s = graduated("text-size");
        let d = s.decide(&proposal, now, T);
        assert_eq!((d.verdict, d.reason), (verdict, reason), "{label}");
    }
}

#[test]
fn a_hard_change_never_acts_however_sure_and_however_trusted() {
    let mut s = graduated("clear-downloads");
    s.set_dial(Area::Settings, Dial::Act);
    let hard = Proposal {
        hard: true,
        helps: None,
        hurts: None,
        confidence: 1.0,
        evidence: 10_000.0,
        value: 1.0,
        ..sure("clear-downloads")
    };
    for undo in [Undo::OneClick, Undo::Restore, Undo::Never] {
        let p = Proposal {
            undo,
            ..hard.clone()
        };
        assert_eq!(
            answer(&mut s, &p, T),
            (Verdict::Offer, Reason::AlwaysAsks),
            "{undo:?}"
        );
    }
    assert_eq!(s.budget_left(T), ACTS_PER_DAY, "nothing was spent");
}

#[test]
fn the_dial_narrows_one_area_and_leaves_the_others() {
    let mut s = graduated("text-size");
    let p = sure("text-size");
    assert_eq!(s.dial(Area::Settings), Dial::Act);
    s.set_dial(Area::Settings, Dial::Offer);
    assert_eq!(answer(&mut s, &p, T), (Verdict::Offer, Reason::DialOffer));
    s.set_dial(Area::Settings, Dial::Watch);
    assert_eq!(answer(&mut s, &p, T), (Verdict::Wait, Reason::DialWatch));
    let skills = Proposal {
        area: Area::Skills,
        ..p.clone()
    };
    assert_eq!(answer(&mut s, &skills, T), (Verdict::Act, Reason::Sure));
}

#[test]
fn five_changes_a_day_and_the_most_helpful_go_first() {
    let mut s = State::default();
    let ids = ["a", "b", "c", "d", "e", "f"];
    for id in ids {
        for _ in 0..SHADOW_PASSES {
            s.outcome(&rule(id), Outcome::Accepted, T);
        }
    }
    let proposals: Vec<Proposal> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| Proposal {
            value: 0.1 * (i as f32 + 1.0),
            ..sure(id)
        })
        .collect();
    let out = s.decide_all(proposals, calm(), T);
    let got: Vec<(&str, Verdict, Reason)> = out
        .iter()
        .map(|d| (d.rule.as_str(), d.verdict, d.reason))
        .collect();
    assert_eq!(
        got,
        vec![
            ("rule:f", Verdict::Act, Reason::Sure),
            ("rule:e", Verdict::Act, Reason::Sure),
            ("rule:d", Verdict::Act, Reason::Sure),
            ("rule:c", Verdict::Act, Reason::Sure),
            ("rule:b", Verdict::Act, Reason::Sure),
            ("rule:a", Verdict::Offer, Reason::Budget),
        ]
    );
    assert_eq!(s.budget_left(T), 0);
    assert_eq!(s.budget_left(T + DAY), 5);
}

#[test]
fn two_undos_turn_a_rule_off_for_good() {
    let mut s = graduated("text-size");
    let p = sure("text-size");
    assert_eq!(answer(&mut s, &p, T), (Verdict::Act, Reason::Sure));
    s.outcome(&p.rule, Outcome::Undone, T + HOUR);
    for _ in 0..SHADOW_PASSES {
        s.outcome(&p.rule, Outcome::Accepted, T + 2 * DAY);
    }
    assert_eq!(
        answer(&mut s, &p, T + 5 * DAY),
        (Verdict::Act, Reason::Sure)
    );
    s.outcome(&p.rule, Outcome::Undone, T + 6 * DAY);
    for _ in 0..10 {
        s.outcome(&p.rule, Outcome::Accepted, T + 7 * DAY);
    }
    assert_eq!(
        answer(&mut s, &p, T + 400 * DAY),
        (Verdict::Never, Reason::TurnedOff)
    );
}

#[test]
fn two_undos_in_a_day_pause_everything_and_name_what_to_put_back() {
    let mut s = State::default();
    for id in ["a", "b", "c", "d"] {
        for _ in 0..SHADOW_PASSES {
            s.outcome(&rule(id), Outcome::Accepted, T);
        }
        assert_eq!(
            answer(&mut s, &sure(id), T + HOUR),
            (Verdict::Act, Reason::Sure),
            "{id}"
        );
    }
    assert_eq!(s.outcome(&rule("a"), Outcome::Undone, T + 2 * HOUR), None);
    let alarm = s
        .outcome(&rule("b"), Outcome::Undone, T + 3 * HOUR)
        .expect("the second undo trips it");
    assert_eq!(alarm.paused_until_ms, T + 3 * HOUR + PAUSE_MS);
    assert_eq!(alarm.put_back, vec![rule("d"), rule("c")]);
    assert!(s.paused(T + 3 * HOUR));
    assert_eq!(
        answer(&mut s, &sure("c"), T + 4 * HOUR),
        (Verdict::Offer, Reason::Paused)
    );
    // A third undo while paused raises no second alarm.
    assert_eq!(s.outcome(&rule("c"), Outcome::Undone, T + 5 * HOUR), None);
    assert!(!s.paused(T + 3 * HOUR + PAUSE_MS));
    assert_eq!(
        answer(&mut s, &sure("d"), T + 3 * HOUR + PAUSE_MS),
        (Verdict::Act, Reason::Sure)
    );
}

#[test]
fn undos_a_day_apart_do_not_trip_the_watchdog_and_resume_lifts_a_pause() {
    let mut s = State::default();
    assert_eq!(s.outcome(&rule("a"), Outcome::Undone, T), None);
    assert_eq!(s.outcome(&rule("b"), Outcome::Undone, T + DAY), None);
    assert!(!s.paused(T + DAY));
    assert!(s
        .outcome(&rule("c"), Outcome::Undone, T + DAY + HOUR)
        .is_some());
    assert!(s.paused(T + DAY + HOUR));
    s.resume();
    assert!(!s.paused(T + DAY + HOUR));
}

#[test]
fn the_log_keeps_every_act_and_only_changes_of_any_other_answer() {
    let mut s = State::default();
    let p = sure("text-size");
    for _ in 0..3 {
        s.decide(&p, calm(), T);
    }
    assert_eq!(s.log().count(), 1, "the same answer is logged once");
    for _ in 0..SHADOW_PASSES {
        s.outcome(&p.rule, Outcome::Accepted, T);
    }
    s.decide(&p, calm(), T + 1);
    s.decide(&p, calm(), T + 2);
    let log: Vec<(u64, Verdict, Reason)> =
        s.log().map(|d| (d.at_ms, d.verdict, d.reason)).collect();
    assert_eq!(
        log,
        vec![
            (T, Verdict::Offer, Reason::Shadow),
            (T + 1, Verdict::Act, Reason::Sure),
            (T + 2, Verdict::Act, Reason::Sure),
        ]
    );
    for i in 0..(LOG_CAP as u64 + 50) {
        s.decide(&sure(&format!("r{i}")), calm(), T + 10 + i);
    }
    assert_eq!(s.log().count(), LOG_CAP);
}

#[test]
fn the_state_survives_the_file_and_forget_keeps_only_the_dials() {
    let dir = std::env::temp_dir().join(format!("decide-file-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(load(&dir), State::default());
    let p = sure("text-size");
    let first = with_state(&dir, |s| {
        s.set_dial(Area::Router, Dial::Offer);
        for _ in 0..SHADOW_PASSES {
            s.outcome(&p.rule, Outcome::Accepted, T);
        }
        s.decide(&p, calm(), T)
    });
    assert_eq!((first.verdict, first.reason), (Verdict::Act, Reason::Sure));
    // A second app on the same folder sees the dial, the pass and the spend.
    let again = load(&dir);
    assert_eq!(again.dial(Area::Router), Dial::Offer);
    assert!(again.graduated(&p.rule));
    assert_eq!(again.budget_left(T), 4);
    assert!(
        std::fs::metadata(super::store::dir(&dir).join(FILE))
            .expect("file")
            .len()
            < 1_024
    );
    with_state(&dir, State::forget_learned);
    let cleared = load(&dir);
    assert_eq!(cleared.dial(Area::Router), Dial::Offer);
    assert!(!cleared.graduated(&p.rule));
    assert_eq!(cleared.log().count(), 0);
    assert_eq!(cleared.budget_left(T), 5);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_chat_view_lists_the_dials_and_says_why_in_plain_words() {
    let mut s = State::default();
    assert!(text(&s, T).contains(NO_DECISIONS));
    s.set_dial(Area::Skills, Dial::Offer);
    s.decide(&sure("text-size"), calm(), T);
    let out = text(&s, T);
    assert!(out.contains("- Skills: offer `skills`\n"), "{out}");
    assert!(out.contains("- Settings: act `settings`\n"), "{out}");
    assert!(
        out.contains(
            "- Asked: It's new at this, so it asks until you've said yes three times. `rule:text-size`\n"
        ),
        "{out}"
    );
    assert!(out.ends_with(TRUST_USAGE), "{out}");
    assert_eq!(Area::parse("Local-AI"), Some(Area::LocalAi));
    assert_eq!(Dial::parse("ACT"), Some(Dial::Act));
    assert_eq!(Dial::parse("always"), None);
}

#[test]
fn the_page_shows_every_dial_and_the_newest_decisions_first() {
    use super::view;
    let mut s = graduated("habit/tidy/morning");
    s.set_dial(Area::Router, Dial::Watch);
    let empty = view::trust(&State::default(), Snapshot::default(), T);
    assert_eq!(empty.dials.len(), 6);
    assert!(empty.dials.iter().all(|d| d.dial == Dial::Act));
    assert_eq!((empty.left_today, empty.decisions.len()), (ACTS_PER_DAY, 0));

    answer(&mut s, &sure("settings/text-size"), T);
    answer(&mut s, &sure("habit/tidy/morning"), T + HOUR);
    let page = view::trust(&s, Snapshot::default(), T + HOUR);
    assert_eq!(
        page.dials[2],
        view::DialRow {
            area: Area::Router,
            label: "Model choice",
            dial: Dial::Watch,
            went: None,
            before: None,
            trend: Trend::TooEarly,
            resting: false,
        }
    );
    assert_eq!((page.asking_first, page.left_today), (false, 4));
    assert_eq!(
        page.decisions,
        vec![
            view::DecisionRow {
                at_ms: T + HOUR,
                title: "Skill tidy in the morning".into(),
                did: "Did it",
                why: "It was sure, you said yes to this before, and it can be undone.",
            },
            view::DecisionRow {
                at_ms: T,
                title: "Settings text size".into(),
                did: "Asked",
                why: "It's new at this, so it asks until you've said yes three times.",
            },
        ]
    );
    assert_eq!(
        serde_json::to_value(&page.dials[2]).unwrap(),
        serde_json::json!({
            "area": "router", "label": "Model choice", "dial": "watch",
            "went": null, "before": null, "trend": "too_early", "resting": false,
        })
    );
    // Only the newest lines are shown.
    for n in 0..20 {
        answer(&mut s, &sure(&format!("settings/n{n}")), T + 2 * HOUR);
    }
    assert_eq!(
        view::trust(&s, Snapshot::default(), T + 2 * HOUR)
            .decisions
            .len(),
        view::DECISIONS_SHOWN
    );
}

/// `good` yes answers and `bad` no answers to `id`'s offers in the week of
/// `at`, after one decision so the rule knows its area.
fn answers(s: &mut State, id: &str, good: u32, bad: u32, at: u64) {
    answer(s, &sure(id), at);
    for _ in 0..good {
        s.outcome(&rule(id), Outcome::Accepted, at);
    }
    for _ in 0..bad {
        s.outcome(&rule(id), Outcome::Declined, at);
    }
}

#[test]
fn the_scoreboard_compares_four_weeks_with_the_four_before() {
    let mut s = State::default();
    let settings = |s: &State, at: u64| s.score().score(Area::Settings, at);
    assert_eq!(
        settings(&s, T),
        Score {
            now: None,
            before: None,
            trend: Trend::TooEarly
        }
    );
    // An outcome for a rule no proposal came from has no area to count in.
    s.outcome(&rule("stray"), Outcome::Accepted, T);
    assert_eq!(settings(&s, T).now, None);

    answers(&mut s, "a", 9, 1, T);
    assert_eq!(
        settings(&s, T),
        Score {
            now: Some(90),
            before: None,
            trend: Trend::TooEarly
        }
    );
    // Four weeks on, those counts are the span before.
    let later = T + 4 * WEEK_MS;
    answers(&mut s, "b", 8, 2, later);
    assert_eq!(
        settings(&s, later),
        Score {
            now: Some(80),
            before: Some(90),
            trend: Trend::Steady
        }
    );
    answers(&mut s, "c", 0, 5, later);
    assert_eq!(
        settings(&s, later),
        Score {
            now: Some(53),
            before: Some(90),
            trend: Trend::Worse
        }
    );
    // Other areas are untouched, and counts older than both spans drop.
    assert_eq!(s.score().score(Area::Skills, later).now, None);
    let much_later = T + 8 * WEEK_MS;
    answers(&mut s, "d", 1, 0, much_later);
    assert_eq!(
        settings(&s, much_later),
        Score {
            now: Some(100),
            before: Some(53),
            trend: Trend::TooEarly
        }
    );
    assert_eq!(MIN_OUTCOMES, 10);
}

#[test]
fn an_area_that_got_worse_only_offers_for_a_month_and_carry_on_lifts_it() {
    let mut s = graduated("x");
    answers(&mut s, "a", 10, 0, T);
    let later = T + 4 * WEEK_MS;
    // A no rests its own rule, so each no here is a different rule.
    for n in 0..5 {
        answers(&mut s, &format!("n{n}"), 1, 1, later);
    }
    assert_eq!(s.score().score(Area::Settings, later).trend, Trend::Worse);
    assert!(!s.score().resting(Area::Settings, later));
    assert_eq!(
        answer(&mut s, &sure("x"), later),
        (Verdict::Offer, Reason::Slipped)
    );
    assert!(s.score().resting(Area::Settings, later));
    // Another area still acts.
    let mut skills = sure("y");
    skills.area = Area::Skills;
    for _ in 0..SHADOW_PASSES {
        s.outcome(&skills.rule, Outcome::Accepted, later);
    }
    let d = s.decide(&skills, calm(), later);
    assert_eq!((d.verdict, d.reason), (Verdict::Act, Reason::Sure));
    // The rest ends on its own once the numbers are no longer worse.
    let rested = later + SLIP_REST_MS;
    assert!(!s.score().resting(Area::Settings, rested));
    assert_eq!(
        answer(&mut s, &sure("x"), rested),
        (Verdict::Act, Reason::Sure)
    );
    // Carry on lifts a rest at once.
    let mut again = graduated("x");
    answers(&mut again, "a", 10, 0, T);
    for n in 0..5 {
        answers(&mut again, &format!("n{n}"), 1, 1, later);
    }
    assert_eq!(
        answer(&mut again, &sure("x"), later),
        (Verdict::Offer, Reason::Slipped)
    );
    again.resume();
    assert!(!again.score().resting(Area::Settings, later));
    let shown = text(&again, later);
    assert!(
        shown.contains("**The last four weeks**\n- Settings: 50% went your way, down from 100%\n"),
        "{shown}"
    );
}

#[test]
fn a_goal_the_user_named_moves_a_proposal_up_the_queue() {
    let mut s = State::default();
    let cheap = Proposal {
        value: 0.4,
        helps: Some(Goal::Cheapest),
        hurts: Some(Goal::Fastest),
        ..sure("cheap")
    };
    let fast = Proposal {
        value: 0.5,
        helps: Some(Goal::Fastest),
        ..sure("fast")
    };
    let order = |s: &mut State| -> Vec<Subject> {
        s.decide_all(vec![cheap.clone(), fast.clone()], calm(), T)
            .into_iter()
            .map(|d| d.rule)
            .collect()
    };
    // No goals: the more helpful one goes first.
    assert_eq!((s.worth(&cheap), s.worth(&fast)), (0.4 * 0.85, 0.5 * 0.85));
    assert_eq!(order(&mut s), vec![rule("fast"), rule("cheap")]);
    // Cheapest first: 0.34 * 1.5 beats 0.425.
    s.set_goals(&[Goal::Cheapest, Goal::Cheapest]);
    assert_eq!(s.goals(), [Goal::Cheapest]);
    assert_eq!(s.worth(&cheap), 0.4 * 0.85 * 1.5);
    assert_eq!(order(&mut s), vec![rule("cheap"), rule("fast")]);
    // Fastest first, cheapest second: the cheap one loses 0.5 and gains 0.25.
    s.set_goals(&[Goal::Fastest, Goal::Cheapest]);
    assert_eq!(s.worth(&cheap), 0.4 * 0.85 * 0.75);
    assert_eq!(s.worth(&fast), 0.5 * 0.85 * 1.5);
    assert_eq!(order(&mut s), vec![rule("fast"), rule("cheap")]);
    // Goals are the user's own, so Forget everything keeps them.
    s.forget_learned();
    assert_eq!(s.goals(), [Goal::Fastest, Goal::Cheapest]);
    let shown = text(&s, T);
    assert!(
        shown.contains(
            "**What matters most to you**\n1. Fastest answers `fastest`\n2. Lowest cost `cheapest`\n"
        ),
        "{shown}"
    );
    s.set_goals(&[]);
    assert!(text(&s, T).contains(NO_GOALS));
    // The page names a goal with an underscore, the cabin with a dash.
    assert_eq!(
        serde_json::to_string(&Goal::BestQuality).unwrap(),
        "\"best_quality\""
    );
    assert_eq!(Goal::parse("Best-Quality"), Some(Goal::BestQuality));
}

#[test]
fn the_snapshot_is_one_byte_and_a_game_counts_as_busy() {
    let all = Snapshot {
        quiet: true,
        busy: true,
        halted: true,
        gaming: true,
        on_battery: true,
    };
    assert_eq!(Snapshot::default().bits(), 0);
    assert_eq!(all.bits(), 31);
    assert_eq!(Snapshot::from_bits(31), all);
    let gaming = Snapshot {
        gaming: true,
        ..Snapshot::default()
    };
    assert_eq!(Snapshot::from_bits(gaming.bits()), gaming);
    assert_eq!(
        gaming.situation(),
        Situation {
            quiet: false,
            busy: true,
            halted: false
        }
    );
    let battery = Snapshot {
        on_battery: true,
        ..Snapshot::default()
    };
    assert_eq!(battery.situation(), Situation::default());
    let mut s = graduated("x");
    assert_eq!(
        {
            let d = s.decide(&sure("x"), gaming.situation(), T);
            (d.verdict, d.reason)
        },
        (Verdict::Wait, Reason::Busy)
    );
    assert_eq!(
        serde_json::to_value(super::view::trust(&s, gaming, T).now).unwrap(),
        serde_json::json!({
            "quiet": false, "busy": false, "halted": false, "gaming": true, "onBattery": false,
        })
    );
}
