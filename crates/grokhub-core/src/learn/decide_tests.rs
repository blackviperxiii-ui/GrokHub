use super::decide::*;
use super::ladder::Undo;
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
