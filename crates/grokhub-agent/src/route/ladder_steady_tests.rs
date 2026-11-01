//! Steady effort (opt-in): E1 climbs only after two failed tool steps in a
//! row, a chat class stops at `high` on E1, and the next clean step takes E1's
//! rung back (`DE4`). Off keeps E1 as it was.

use grokhub_core::model_registry::EFFORT_LADDER;

use super::ladder::{
    advance, advance_holding, start_rung, user_facing, Band, EffortSettings, EffortState, Obs,
    Steer, PREPARE_HARD,
};
use super::policy::class_row;

fn band(class: &str) -> Band {
    Band::of(class_row(class).unwrap(), 0)
}

/// One turn in `class`: the opening call, then one tool step per entry of
/// `errors` (failed tools in that step). Every call before a step came back
/// fine. Returns each step's rung and rules.
fn turn(class: &str, errors: &[u32], steady: bool) -> Vec<(&'static str, Vec<String>)> {
    turn_holding(class, errors, steady, false)
}

fn turn_holding(
    class: &str,
    errors: &[u32],
    steady: bool,
    hold_drops: bool,
) -> Vec<(&'static str, Vec<String>)> {
    let b = band(class);
    let start = || start_rung(b, 0.5, Steer::None, user_facing(class));
    let first = Obs {
        new_turn: true,
        steady,
        ..Obs::default()
    };
    let (mut s, _) = advance_holding(None, class, b, start(), &first, hold_drops);
    let mut out = Vec::new();
    for n in errors {
        s.last_ok = true;
        let obs = Obs {
            tool_errors: *n,
            steady,
            ..Obs::default()
        };
        let (next, pick) = advance_holding(Some(&s), class, b, start(), &obs, hold_drops);
        out.push((EFFORT_LADDER[pick.rung], pick.rules));
        s = next;
    }
    out
}

fn rules(r: &[&str]) -> Vec<String> {
    r.iter().map(|s| s.to_string()).collect()
}

#[test]
fn steady_off_still_climbs_on_a_single_tool_error() {
    assert_eq!(
        turn("chat:default", &[1, 0], false),
        vec![("high", rules(&["E1"])), ("high", rules(&[]))]
    );
    assert_eq!(
        turn("chat:default", &[1, 1, 1, 0], false),
        vec![
            ("high", rules(&["E1"])),
            ("xhigh", rules(&["E1"])),
            ("xhigh", rules(&["E1:ceiling"])),
            ("xhigh", rules(&[])),
        ]
    );
}

#[test]
fn steady_one_error_does_not_climb_and_two_in_a_row_climb_once() {
    assert_eq!(
        turn("chat:default", &[1, 0], true),
        vec![("medium", rules(&[])), ("medium", rules(&[]))]
    );
    assert_eq!(
        turn("debug", &[1, 1], true),
        vec![("high", rules(&[])), ("xhigh", rules(&["E1"]))]
    );
    // A clean step between two errors resets the streak: no climb.
    assert_eq!(
        turn("debug", &[1, 0, 1], true),
        vec![
            ("high", rules(&[])),
            ("high", rules(&[])),
            ("high", rules(&[]))
        ]
    );
}

#[test]
fn steady_clean_step_after_the_climb_drops_one_rung() {
    assert_eq!(
        turn("debug", &[1, 1, 0, 0], true),
        vec![
            ("high", rules(&[])),
            ("xhigh", rules(&["E1"])),
            ("high", rules(&["DE4"])),
            ("high", rules(&[])),
        ]
    );
    // A step whose call did not come back fine is not clean: no DE4 yet.
    let b = band("debug");
    let start = || start_rung(b, 0.5, Steer::None, true);
    let steady = |tool_errors: u32| Obs {
        tool_errors,
        steady: true,
        ..Obs::default()
    };
    let first = Obs {
        new_turn: true,
        ..steady(0)
    };
    let (mut s, _) = advance(None, "debug", b, start(), &first);
    for _ in 0..2 {
        s.last_ok = true;
        s = advance(Some(&s), "debug", b, start(), &steady(1)).0;
    }
    s.last_ok = false;
    let (s, p) = advance(Some(&s), "debug", b, start(), &steady(0));
    assert_eq!((EFFORT_LADDER[p.rung], p.rules), ("xhigh", rules(&[])));
    assert_eq!((s.fail_streak, s.e1_raised), (0, true));
}

#[test]
fn steady_chat_class_never_passes_high_on_tool_errors() {
    assert_eq!(
        turn("chat:default", &[1, 1, 1, 1, 0], true),
        vec![
            ("medium", rules(&[])),
            ("high", rules(&["E1"])),
            ("high", rules(&["E1:steady_cap"])),
            ("high", rules(&["E1:steady_cap"])),
            ("medium", rules(&["DE4"])),
        ]
    );
    // chat:quick's own ceiling (medium) sits under high.
    assert_eq!(
        turn("chat:quick", &[1, 1, 1], true),
        vec![
            ("low", rules(&[])),
            ("medium", rules(&["E1"])),
            ("medium", rules(&["E1:ceiling"])),
        ]
    );
}

#[test]
fn steady_keeps_prepare_hard_at_its_high_floor_and_care_first_holds_de4() {
    assert_eq!(
        turn(PREPARE_HARD, &[1, 1, 0, 0], true),
        vec![
            ("high", rules(&[])),
            ("xhigh", rules(&["E1"])),
            ("xhigh", rules(&[])),
            ("xhigh", rules(&[])),
        ],
        "prepare:hard never steps down"
    );
    let b = band(PREPARE_HARD);
    let (_, p) = advance(
        None,
        PREPARE_HARD,
        b,
        start_rung(b, 0.0, Steer::Quicker, true),
        &Obs {
            new_turn: true,
            steady: true,
            ..Obs::default()
        },
    );
    assert_eq!(EFFORT_LADDER[p.rung], "high");
    // Care-first holds chat drops, DE4 included.
    assert_eq!(
        turn_holding("chat:default", &[1, 1, 0], true, true),
        vec![
            ("medium", rules(&[])),
            ("high", rules(&["E1"])),
            ("high", rules(&[]))
        ]
    );
}

#[test]
fn steady_streak_starts_over_on_a_new_turn() {
    let b = band("debug");
    let start = || start_rung(b, 0.5, Steer::None, true);
    let obs = |new_turn: bool, tool_errors: u32| Obs {
        new_turn,
        tool_errors,
        steady: true,
        ..Obs::default()
    };
    let (s, _) = advance(None, "debug", b, start(), &obs(true, 0));
    let (s, _) = advance(Some(&s), "debug", b, start(), &obs(false, 1));
    assert_eq!(s.fail_streak, 1);
    let (s, _) = advance(Some(&s), "debug", b, start(), &obs(true, 0));
    let (s, p) = advance(Some(&s), "debug", b, start(), &obs(false, 1));
    assert_eq!((EFFORT_LADDER[p.rung], s.fail_streak), ("high", 1));
}

#[test]
fn a_fresh_state_has_no_streak() {
    assert_eq!(
        (
            EffortState::default().fail_streak,
            EffortState::default().e1_raised
        ),
        (0, false)
    );
}

#[test]
fn effort_settings_read_steady_effort_from_app_json() {
    let dir = std::env::temp_dir().join(format!("gh-steady-effort-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(!EffortSettings::load(&dir).steady, "missing file is off");
    assert!(!EffortSettings::default().steady);
    let app = dir.join("app.json");
    std::fs::write(&app, r#"{"deviceName":"cabin"}"#).unwrap();
    assert!(!EffortSettings::load(&dir).steady, "missing key is off");
    std::fs::write(&app, r#"{"deviceName":"cabin","steadyEffort":true}"#).unwrap();
    assert!(EffortSettings::load(&dir).steady);
    std::fs::write(&app, r#"{"steadyEffort":"yes"}"#).unwrap();
    assert!(!EffortSettings::load(&dir).steady, "a non-bool is off");
    std::fs::write(&app, "not json").unwrap();
    assert!(!EffortSettings::load(&dir).steady, "bad json is off");
    let _ = std::fs::remove_dir_all(&dir);
}
