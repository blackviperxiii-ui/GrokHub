//! Brain 2 acceptance: the Brief's goal changes the starting effort for the
//! same call class, the reason lands in the route log and `/why`, nothing
//! prompts, and the ladder still escalates on failure and drops back after
//! clean work.

use grokhub_core::model_registry::store::save_registry;
use grokhub_core::model_registry::{Credential, Listing, ModelMeta, Registry, SourceKind};

use super::goal::{pin_goal, set_goal, RouteGoal};
use super::guard::in_holdout;
use super::ladder::note_tool_error;
use super::live::{self, RouteCall, RouteDone};
use super::log::why_text;

const NOW: u64 = 1_000_000;

fn fleet() -> Registry {
    let mut reg = Registry::default();
    reg.entitlement.credential = Credential::Plan;
    let m = ModelMeta {
        id: "grok-4.3".into(),
        efforts: Some(vec![
            "low".into(),
            "medium".into(),
            "high".into(),
            "xhigh".into(),
        ]),
        tool_calling: Some(true),
        api_shape: Some("responses".into()),
        ..ModelMeta::default()
    };
    reg.apply_refresh(&[Listing::new(SourceKind::XaiApi, vec![m])], &[], 1);
    reg
}

fn call<'a>(session: &'a str, episode: &'a str, text: &'a str) -> RouteCall<'a> {
    RouteCall {
        provider: "xai",
        class: "chat:default",
        model: "grok-4.3",
        episode,
        session,
        text,
        needs_tools: true,
        ..RouteCall::default()
    }
}

fn ok() -> RouteDone {
    RouteDone {
        ok: true,
        http: 200,
        ..RouteDone::default()
    }
}

#[test]
fn the_goal_changes_the_effort_for_the_same_class_and_the_reason_is_logged() {
    let dir = crate::harness::test_dir("brain2-goal");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet()).unwrap();
    let _ = (live::take_premium_ask(), live::take_budget_ask());
    for ep in [
        "g2-ep-none",
        "g2-ep-q1",
        "g2-ep-speed",
        "g2-ep-cost",
        "g2-ep-st1",
    ] {
        assert!(
            !in_holdout(ep),
            "{ep} sits in the holdout; pick another name"
        );
    }
    set_goal(
        "g2-s-quality",
        Some(RouteGoal::of("quality", "mistakes can break the system").unwrap()),
    );
    set_goal(
        "g2-s-speed",
        Some(RouteGoal::of("speed", "a quick answer").unwrap()),
    );
    set_goal(
        "g2-s-cost",
        Some(RouteGoal::of("cost", "routine work").unwrap()),
    );
    set_goal(
        "g2-s-steer",
        Some(RouteGoal::of("cost", "routine work").unwrap()),
    );
    let effort = |s: &str, ep: &str, text: &str| {
        let d = live::decide(&dir, &call(s, ep, text), NOW);
        (
            d.route.effort.clone().unwrap_or_default(),
            d.route.rule_ids.clone(),
            d,
        )
    };
    let (none, rules, d) = effort("g2-s-none", "g2-ep-none", "debug and refactor the config");
    assert_eq!(none, "medium", "{rules:?}");
    assert!(!rules.iter().any(|r| r.starts_with("goal:")), "{rules:?}");
    assert_eq!(d.goal, None);
    let (quality, rules, d) = effort("g2-s-quality", "g2-ep-q1", "debug and refactor the config");
    assert_eq!(quality, "high", "{rules:?}");
    assert!(rules.contains(&"goal:quality".to_string()), "{rules:?}");
    assert_eq!(
        d.route.reason,
        "balanced · High: everyday chat, and this one needs care."
    );
    let rec = live::route_log(
        &dir,
        &call("g2-s-quality", "g2-ep-q1", "debug and refactor the config"),
        &d,
        &ok(),
    );
    assert_eq!(
        (rec.goal.as_str(), rec.goal_reason.as_str()),
        ("quality", "mistakes can break the system")
    );
    let why = why_text(&dir);
    assert!(
        why.contains("Goal: quality (mistakes can break the system)."),
        "{why}"
    );
    let (speed, rules, _) = effort("g2-s-speed", "g2-ep-speed", "debug and refactor the config");
    assert_eq!(speed, "low", "{rules:?}");
    assert!(rules.contains(&"goal:speed".to_string()), "{rules:?}");
    let (cost, rules, _) = effort("g2-s-cost", "g2-ep-cost", "debug and refactor the config");
    assert_eq!(cost, "low", "{rules:?}");
    // The user's own words win over the goal.
    let (steered, rules, _) = effort(
        "g2-s-steer",
        "g2-ep-st1",
        "think hard and debug and refactor the config",
    );
    assert_eq!(steered, "xhigh", "{rules:?}");
    assert!(!rules.iter().any(|r| r.starts_with("goal:")), "{rules:?}");
    // No prompt: neither a premium nor a budget ask was raised.
    assert_eq!(live::take_premium_ask(), None);
    assert!(!live::take_budget_ask());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_goal_start_still_escalates_on_failure_and_drops_back_after_clean_steps() {
    let dir = crate::harness::test_dir("brain2-ladder");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet()).unwrap();
    let (s, ep) = ("g2-s-ladder", "g2-ep-ladder");
    assert!(!in_holdout(ep));
    set_goal(
        s,
        Some(RouteGoal::of("quality", "mistakes can break the system").unwrap()),
    );
    let c = call(s, ep, "debug and refactor the config");
    let step = || {
        let d = live::decide(&dir, &c, NOW);
        live::route_log(&dir, &c, &d, &ok());
        (
            d.route.effort.clone().unwrap_or_default(),
            d.route.rule_ids.clone(),
        )
    };
    assert_eq!(step().0, "high");
    note_tool_error();
    let (up, rules) = step();
    assert_eq!(up, "xhigh", "{rules:?}");
    assert!(rules.contains(&"E1".to_string()), "{rules:?}");
    let mut seen = Vec::new();
    for _ in 0..3 {
        seen.push(step().0);
    }
    // The E1 step went fine too, so the third clean step after it drops back.
    assert_eq!(
        seen,
        vec!["xhigh", "xhigh", "high"],
        "DE1 drops back after three clean steps"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_background_runs_cost_goal_is_logged_but_never_moves_its_effort() {
    let dir = crate::harness::test_dir("brain4-goal");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet()).unwrap();
    let (s, ep) = ("b4-s-active", "b4-ep-active");
    assert!(!in_holdout(s) && !in_holdout(ep), "pick another name");
    pin_goal(s, Some(RouteGoal::of("cost", "user active").unwrap()));
    // The unattended class has no table row: the call's own model and effort go out.
    let bg = RouteCall {
        class: crate::unattended::UNATTENDED_CLASS,
        effort: Some("low"),
        ..call(s, "", "summarize the feed")
    };
    let d = live::decide(&dir, &bg, NOW);
    assert_eq!(d.goal, RouteGoal::of("cost", "user active"));
    assert_eq!(
        (d.send_model("grok-3-mini-fast"), d.send_effort(Some("low"))),
        ("grok-3-mini-fast".to_string(), Some("low".to_string()))
    );
    let rec = live::route_log(&dir, &bg, &d, &ok());
    assert_eq!(
        (rec.goal.as_str(), rec.goal_reason.as_str()),
        ("cost", "user active")
    );
    // A listed background class keeps its own start: the goal does not move it.
    let sum = RouteCall {
        class: "background:summarize",
        ..call(s, ep, "summarize the feed")
    };
    let d = live::decide(&dir, &sum, NOW);
    assert!(
        !d.route.rule_ids.iter().any(|r| r.starts_with("goal:")),
        "{:?}",
        d.route.rule_ids
    );
    pin_goal(s, None);
    // The Brief's own background row stays out of a background call's record, as before.
    set_goal(
        s,
        Some(RouteGoal::of("cost", "user isn't waiting").unwrap()),
    );
    assert_eq!(live::decide(&dir, &bg, NOW).goal, None);
    set_goal(s, None);
    let _ = std::fs::remove_dir_all(dir);
}
