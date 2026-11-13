//! The learner on real files: arms from the registry, the propensity log a
//! routed chat turn leaves when the next turn settles it, every feedback
//! signal, and a learned arm steering the route through the router's filters.
//! The pure choice and the offline estimators are in [`super::super::ask_eval`].

use std::collections::BTreeMap;

use grokhub_core::model_registry::seed::seed_prices;
use grokhub_core::model_registry::store::{save_registry, write_json};
use grokhub_core::model_registry::{Credential, Listing, ModelMeta, Registry, SourceKind};

use super::*;
use crate::route::ask::{read, AskContext};
use crate::route::guard::in_holdout;
use crate::route::live::{self, RouteCall, RouteDone};

fn meta(id: &str, efforts: &[&str]) -> ModelMeta {
    ModelMeta {
        id: id.into(),
        context_length: Some(500_000),
        prices: seed_prices(id).unwrap(),
        efforts: Some(efforts.iter().map(|e| e.to_string()).collect()),
        tool_calling: Some(true),
        api_shape: Some("responses".into()),
        ..ModelMeta::default()
    }
}

/// grok-4.3 and grok-4.7 at their seed prices, on a plan.
fn fleet() -> Registry {
    let mut reg = Registry::default();
    reg.entitlement.credential = Credential::Plan;
    reg.apply_refresh(
        &[Listing::new(
            SourceKind::XaiApi,
            vec![
                meta("grok-4.3", &["none", "low", "medium", "high"]),
                meta("grok-4.7", &["low", "medium", "high", "xhigh"]),
            ],
        )],
        &[],
        1,
    );
    reg
}

fn chat<'a>(episode: &'a str, text: &'a str) -> RouteCall<'a> {
    RouteCall {
        provider: "xai",
        class: "chat:default",
        model: "grok-4.7",
        episode,
        session: episode,
        text,
        needs_tools: true,
        ..RouteCall::default()
    }
}

fn done(ticks: i64) -> RouteDone {
    let mut d = RouteDone {
        ok: true,
        http: 200,
        ..RouteDone::default()
    };
    d.usage.cost_in_usd_ticks = ticks;
    d
}

fn turn(dir: &Path, call: &RouteCall<'_>, at: u64, ticks: i64) -> live::Decision {
    let d = live::decide(dir, call, at);
    live::route_log(dir, call, &d, &done(ticks));
    d
}

#[test]
fn arms_come_from_the_registry_inside_the_lane_band() {
    let reg = fleet();
    let row = crate::route::policy::class_row("chat:default").unwrap();
    let input = RouteInput {
        class: "chat:default",
        needs_tools: true,
        ..RouteInput::default()
    };
    let profiles = BTreeMap::new();
    let t = arms(
        &reg,
        &profiles,
        &RoutingTable::default(),
        &input,
        "grok-4.3",
        rung("medium").unwrap(),
        Band::of(row, 0),
        80,
        1,
    );
    let ids: Vec<String> = t.arms.iter().map(ArmSpec::id).collect();
    assert_eq!(
        ids,
        [
            "grok-4.3@low",
            "grok-4.3@medium",
            "grok-4.7@low",
            "grok-4.7@medium",
            "grok-4.7@high",
            "grok-4.7@xhigh"
        ]
    );
    assert_eq!(t.arms[t.base].id(), "grok-4.3@medium");
    assert!(t.arms[2].strength > t.arms[0].strength);
    // A cheaper rung costs less; the other model pays for a cold cache.
    assert!(t.arms[0].cost_usd < t.arms[1].cost_usd);
    assert!(t.arms[3].cost_usd > t.arms[1].cost_usd);
    // A pinned long chat keeps its model unless the switch pays.
    let pinned = RouteInput {
        ctx_tokens: 120_000,
        pin_turns: Some(3),
        ..input.clone()
    };
    let t = arms(
        &reg,
        &profiles,
        &RoutingTable::default(),
        &pinned,
        "grok-4.3",
        rung("medium").unwrap(),
        Band::of(row, 0),
        80,
        1,
    );
    assert!(t.arms.iter().all(|a| a.model == "grok-4.3"), "{t:?}");
}

#[test]
fn a_settled_turn_logs_ids_and_probabilities_and_every_signal_moves_its_quality() {
    let dir = crate::harness::test_dir("bandit-log");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet()).unwrap();
    let ep = "bandit-ep-log";
    assert!(
        !in_holdout(ep),
        "{ep} sits in the holdout; pick another name"
    );
    let text = "how does ray tracing work?";
    let first = turn(&dir, &chat(ep, text), 1_000, 2_000_000_000);
    assert!(
        read_log(&dir).is_empty(),
        "nothing settles before the next turn"
    );
    // You copied a code block from the answer, then moved on.
    note_copy(ep);
    turn(&dir, &chat(ep, "thanks, what about shadows?"), 2_000, 0);
    let log = read_log(&dir);
    assert_eq!(log.len(), 1);
    let r = read(text, &AskContext::default());
    let base = arm_id(&first.route.model, first.route.effort.as_deref());
    assert_eq!(
        log[0],
        Line {
            at: 2_000,
            ctx: context(r.lane(), r.kind.id(), r.d),
            arm: base.clone(),
            base: base.clone(),
            p: 1.0,
            explore: false,
            guard: None,
            hold: false,
            q: 1.0,
            cost: 0.2,
            r: 0.8,
            shadow: BTreeMap::new(),
        }
    );
    let raw = std::fs::read_to_string(log_path(&dir)).unwrap();
    assert!(
        !raw.contains("ray tracing") && !raw.contains("shadows"),
        "{raw}"
    );
    // A redo says the last answer was wrong; a tool error read late is trouble.
    turn(&dir, &chat(ep, "that's wrong, try again"), 3_000, 0);
    crate::route::ladder::note_tool_error();
    turn(&dir, &chat(ep, "ok and now the bounce light?"), 4_000, 0);
    let qs: Vec<f64> = read_log(&dir).iter().map(|l| l.q).collect();
    assert_eq!(qs[1..], [Q_REDO, Q_TROUBLE]);
    // A cancelled turn teaches nothing.
    let call = chat(ep, "and caustics?");
    let d = live::decide(&dir, &call, 5_000);
    live::route_log(
        &dir,
        &call,
        &d,
        &RouteDone {
            cancelled: true,
            ..RouteDone::default()
        },
    );
    turn(&dir, &chat(ep, "never mind"), 6_000, 0);
    let qs: Vec<f64> = read_log(&dir).iter().map(|l| l.q).collect();
    assert_eq!(qs, [1.0, Q_REDO, Q_TROUBLE, Q_MOVED_ON]);
    let state = load_state(&dir);
    assert_eq!(state.contexts.values().map(|c| c.settled).sum::<u32>(), 4);
    // The redo turn climbed to its own context; with no reported cost a turn
    // pays the arm's estimate ($0.00778 for medium here).
    let sum = state.contexts[&log[0].ctx].arms[&base].sum;
    assert!(
        (sum - (0.8 + 0.0 - 0.00778 + 0.8 - 0.00778)).abs() < 1e-9,
        "{sum}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_adult_turn_and_the_holdout_never_steer_and_only_the_holdout_is_logged() {
    let dir = crate::harness::test_dir("bandit-skip");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet()).unwrap();
    let ep = "bandit-ep-adult";
    assert!(!in_holdout(ep));
    turn(&dir, &chat(ep, "write me an nsfw sex scene"), 1_000, 0);
    turn(&dir, &chat(ep, "thanks!"), 2_000, 0);
    assert!(read_log(&dir).is_empty());
    let held = (0..)
        .map(|i| format!("bandit-hold-{i}"))
        .find(|s| in_holdout(s))
        .unwrap();
    turn(&dir, &chat(&held, "how does ray tracing work?"), 3_000, 0);
    turn(&dir, &chat(&held, "thanks!"), 4_000, 0);
    let log = read_log(&dir);
    assert_eq!(log.len(), 1);
    assert!(log[0].hold && log[0].arm.ends_with("@high") && log[0].p == 1.0);
    assert!(
        load_state(&dir).contexts.is_empty(),
        "the holdout teaches nothing"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_learned_arm_steers_the_route_through_the_router_and_logs_its_propensity() {
    let dir = crate::harness::test_dir("bandit-steer");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet()).unwrap();
    let ep = "bandit-ep-steer";
    assert!(!in_holdout(ep));
    let text = "how does ray tracing work?";
    let r = read(text, &AskContext::default());
    let ctx = context(r.lane(), r.kind.id(), r.d);
    // Without history the read's route stands.
    let base = live::decide(&dir, &chat("bandit-ep-plain", text), 1_000).route;
    assert!(!base.rule_ids.iter().any(|x| x.starts_with("bandit:")));
    // Forty turns on the stronger model, one rung up, earned clearly more
    // than the read's arm.
    assert_eq!(
        arm_id(&base.model, base.effort.as_deref()),
        "grok-4.3@medium"
    );
    let up = "grok-4.7@high".to_string();
    let mut state = State::default();
    for _ in 0..40 {
        state.learn(&ctx, &up, 1.0);
        state.learn(&ctx, &arm_id(&base.model, base.effort.as_deref()), 0.2);
    }
    write_json(&state_path(&dir), &state).unwrap();
    let call = chat(ep, text);
    let d = live::decide(&dir, &call, 2_000);
    assert_eq!(arm_id(&d.route.model, d.route.effort.as_deref()), up);
    assert!(
        d.route.rule_ids.iter().any(|x| x == LEARNED_RULE),
        "{:?}",
        d.route.rule_ids
    );
    live::route_log(&dir, &call, &d, &done(0));
    turn(&dir, &chat(ep, "thanks!"), 3_000, 0);
    let log = read_log(&dir);
    assert_eq!(log.len(), 1);
    assert_eq!((log[0].arm.as_str(), log[0].explore), (up.as_str(), false));
    assert!(log[0].p > 0.9 && log[0].p <= 1.0, "{}", log[0].p);
    let _ = std::fs::remove_dir_all(dir);
}
