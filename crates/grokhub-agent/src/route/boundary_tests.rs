use grokhub_core::model_registry::store::save_registry;
use grokhub_core::model_registry::{Credential, Listing, ModelMeta, Registry, SourceKind};

use super::*;
use crate::client::{ContentPart, InputItem};
use crate::route::guard::in_holdout;
use crate::route::ladder::note_tool_error;
use crate::route::live::{self, RouteCall, RouteDone};

const NOW: u64 = 1_000_000;

#[test]
fn it_is_off_unless_the_file_turns_it_on() {
    assert!(!parse_enabled(""));
    assert!(!parse_enabled("enabled = false"));
    assert!(parse_enabled("enabled = true"));
    let dir = crate::harness::test_dir("boundary-flag");
    assert!(!enabled(&dir));
    std::fs::write(dir.join(CONFIG), "enabled = true\n").unwrap();
    assert!(enabled(&dir));
    let _ = std::fs::remove_dir_all(dir);
}

fn e(s: &str) -> Option<String> {
    Some(s.to_string())
}

#[test]
fn a_segment_keeps_its_first_effort_until_a_boundary() {
    let (slot, key) = ("bh-slot", "bh-key");
    let (first, held) = hold(slot, key, 1, "grok-4.3", e("medium"), None);
    assert_eq!((first, held), (e("medium"), false));
    // The ladder climbs mid-run: the segment's effort goes out instead.
    assert_eq!(
        hold(slot, key, 1, "grok-4.3", e("high"), None),
        (e("medium"), true)
    );
    assert_eq!(
        hold(slot, key, 1, "grok-4.3", e("medium"), None),
        (e("medium"), false)
    );
    // A new turn is a boundary.
    assert_eq!(
        hold(slot, key, 2, "grok-4.3", e("high"), None),
        (e("high"), false)
    );
    // So is a compaction.
    assert_eq!(
        hold(slot, key, 2, "grok-4.3", e("low"), None),
        (e("high"), true)
    );
    note_compaction(key);
    assert_eq!(
        hold(slot, key, 2, "grok-4.3", e("low"), None),
        (e("low"), false)
    );
    // A model change (a fallback) starts a new hold.
    assert_eq!(
        hold(slot, key, 2, "grok-4.1", e("xhigh"), None),
        (e("xhigh"), false)
    );
    // The class floor always wins over a held effort under it.
    let (slot, key) = ("bh-floor", "bh-floor-key");
    hold(slot, key, 1, "grok-4.3", e("low"), None);
    let high = crate::route::ladder::high();
    assert_eq!(
        hold(slot, key, 1, "grok-4.3", e("high"), Some(high)),
        (e("high"), false)
    );
}

#[test]
fn a_helper_brief_needs_every_labeled_line() {
    let full = "Goal: port the parser\nConstraints: no new deps\n- Edge cases: empty input\n\
                Done-when: tests pass\nFiles: src/parse.rs";
    assert!(missing_brief(full).is_empty(), "{:?}", missing_brief(full));
    assert_eq!(
        missing_brief("port the parser to the new API"),
        BRIEF.to_vec()
    );
    let partial = "GOAL: port it\nfiles: src/parse.rs";
    assert_eq!(
        missing_brief(partial),
        vec!["constraints", "edge cases", "done when"]
    );
    let refusal = brief_refusal(&missing_brief(partial));
    assert!(refusal.contains("Missing: constraints, edge cases, done when."));
}

fn user(text: &str) -> InputItem {
    InputItem::Message {
        role: "user".into(),
        content: vec![ContentPart::InputText(text.into())],
    }
}

#[test]
fn notes_the_app_writes_mid_run_are_not_read_as_the_ask() {
    let input = vec![
        user("refactor the config loader"),
        user(&crate::compact::cabin_note_text("Checkpoint: verify it")),
        user("<background-notice>\ntask done\n</background-notice>"),
    ];
    assert_eq!(live::last_user_text(&input).0, "refactor the config loader");
    // The person's own steer mid-run is still their words.
    let mut steered = input.clone();
    steered.push(user("actually skip the tests"));
    assert_eq!(live::last_user_text(&steered).0, "actually skip the tests");
    assert_eq!(live::last_user_text(&input[1..]).0, "");
}

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

#[test]
fn the_router_climbs_at_the_next_boundary_not_mid_run() {
    let dir = crate::harness::test_dir("boundary-decide");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet()).unwrap();
    std::fs::write(dir.join(CONFIG), "enabled = true\n").unwrap();
    let (s, ep) = ("bd-s", "bd-ep-climb");
    assert!(!in_holdout(ep));
    let c = RouteCall {
        provider: "xai",
        class: "chat:default",
        model: "grok-4.3",
        episode: ep,
        session: s,
        text: "explain how the config loader works",
        needs_tools: true,
        ..RouteCall::default()
    };
    let ok = RouteDone {
        ok: true,
        http: 200,
        ..RouteDone::default()
    };
    let step = || {
        let d = live::decide(&dir, &c, NOW);
        live::route_log(&dir, &c, &d, &ok);
        (
            d.route.effort.clone().unwrap_or_default(),
            d.route.rule_ids.clone(),
        )
    };
    let (start, _) = step();
    note_tool_error();
    // The ladder climbs (E1), but the run keeps sending its first effort.
    let (mid, rules) = step();
    assert_eq!(mid, start, "{rules:?}");
    assert!(rules.iter().any(|r| r == HELD_RULE), "{rules:?}");
    assert!(rules.iter().any(|r| r == "E1"), "{rules:?}");
    // At the compaction the climbed rung goes out.
    note_compaction(ep);
    let (after, rules) = step();
    assert_ne!(after, start, "{rules:?}");
    assert!(!rules.iter().any(|r| r == HELD_RULE), "{rules:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_passed_eval_turns_boundary_routing_on_unless_the_user_already_chose() {
    let dir = std::env::temp_dir().join(format!("grokhub-boundary-adopt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!super::enabled(&dir));
    assert_eq!(super::adopt_after_pass(&dir), Ok(true));
    assert!(super::enabled(&dir));
    // A file already there, even one turned off by hand, stays as it is.
    std::fs::write(dir.join(super::CONFIG), "enabled = false\n").unwrap();
    assert_eq!(super::adopt_after_pass(&dir), Ok(false));
    assert!(!super::enabled(&dir));
    let _ = std::fs::remove_dir_all(dir);
}
