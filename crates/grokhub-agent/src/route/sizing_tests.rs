//! Failover PR 5 acceptance: a long task is sized before it runs and kept
//! off free provider models while a route that can carry it exists — paid
//! routes carry it (the plan, xAI, ahead of even a table-preferred paid
//! provider), local only after paid. Free models stay when they are the
//! only cloud there is (a working free route beats a dead stop), a turn
//! upgraded to long mid-flight leaves the free model it was riding, and
//! less challenging work defaults to a local model that is already loaded.

use std::collections::BTreeMap;

use grokhub_core::model_registry::profile::ModelProfile;
use grokhub_core::model_registry::{Credential, Listing, ModelMeta, Prices, Registry, SourceKind};

use grokhub_core::local_governor::Level;

use super::local::{self, LocalCapacity, LocalGate};
use super::sizing::{SizeEstimate, TaskSize};
use super::spend::Spend;
use super::table::{ClassTable, RoutingTable, SpeedQuality, TableRow};
use super::{RouteInput, Router};

const FREE_MODEL: &str = "openrouter/nvidia/nemotron-test:free";
const PAID_MODEL: &str = "openrouter/vendor/paid";
const NOW: u64 = 1_000_000;

fn xai(id: &str, completion: u64) -> ModelMeta {
    ModelMeta {
        id: id.into(),
        context_length: Some(256_000),
        prices: Prices {
            prompt: Some(completion / 5),
            completion: Some(completion),
            ..Prices::default()
        },
        efforts: Some(vec![
            "low".into(),
            "medium".into(),
            "high".into(),
            "xhigh".into(),
        ]),
        tool_calling: Some(true),
        api_shape: Some("responses".into()),
        ..ModelMeta::default()
    }
}

fn provider(id: &str, completion: u64) -> ModelMeta {
    ModelMeta {
        id: id.into(),
        context_length: Some(128_000),
        prices: Prices {
            prompt: Some(completion / 5),
            completion: Some(completion),
            ..Prices::default()
        },
        efforts: Some(vec!["low".into(), "high".into()]),
        tool_calling: Some(true),
        ..ModelMeta::default()
    }
}

/// grok-4.7 and grok-4.3 on xAI; a free and a paid OpenRouter model.
fn fleet() -> Registry {
    let mut reg = Registry::default();
    reg.entitlement.credential = Credential::Plan;
    reg.apply_refresh(
        &[
            Listing::new(
                SourceKind::XaiApi,
                vec![xai("grok-4.7", 350_000), xai("grok-4.3", 150_000)],
            ),
            Listing::new(
                SourceKind::OpenAiCompatible,
                vec![provider(FREE_MODEL, 0), provider(PAID_MODEL, 5_000)],
            ),
            Listing::new(
                SourceKind::Local,
                local::TIERS.iter().map(|t| local::tier_meta(t)).collect(),
            ),
        ],
        &[],
        1,
    );
    reg
}

fn table_with(class: &str, first: &str) -> RoutingTable {
    let row = |m: &str| TableRow {
        model: m.into(),
        effort: Some("medium".into()),
        quality: Some(1.0),
        est_cost_usd: None,
        p50_latency_ms: None,
        why: "the class preference".into(),
    };
    let mut t = RoutingTable {
        schema: 1,
        version: 1,
        ..RoutingTable::default()
    };
    t.classes.insert(
        class.into(),
        ClassTable {
            speed_quality: SpeedQuality::Balanced,
            ranked: vec![row(first), row("grok-4.7")],
        },
    );
    t
}

fn granted(ids: &[&str]) -> Spend {
    Spend {
        providers: ids.iter().map(|s| s.to_string()).collect(),
        ..Spend::default()
    }
}

fn ask<'a>(class: &'a str, model: &'a str, spend: Spend, size: Option<TaskSize>) -> RouteInput<'a> {
    RouteInput {
        provider: "xai",
        class,
        current_model: model,
        d: 0.5,
        needs_tools: true,
        spend,
        size: size.map(|size| SizeEstimate {
            size,
            ..SizeEstimate::default()
        }),
        ..RouteInput::default()
    }
}

fn none() -> BTreeMap<String, ModelProfile> {
    BTreeMap::new()
}

#[test]
fn without_a_size_the_table_preference_takes_the_free_model() {
    let reg = fleet();
    let r = Router::choose(
        &ask("chat:default", "grok-4.7", granted(&["openrouter"]), None),
        &reg,
        &none(),
        &table_with("chat:default", FREE_MODEL),
        NOW,
    );
    assert_eq!(r.model, FREE_MODEL);
    assert!(!r.rule_ids.iter().any(|x| x.starts_with("size:")));
}

#[test]
fn a_long_task_never_lands_on_the_free_model() {
    let reg = fleet();
    let r = Router::choose(
        &ask(
            "chat:default",
            "grok-4.7",
            granted(&["openrouter"]),
            Some(TaskSize::Long),
        ),
        &reg,
        &none(),
        &table_with("chat:default", FREE_MODEL),
        NOW,
    );
    assert!(!r.no_route);
    assert_ne!(r.model, FREE_MODEL);
    assert!(
        r.model == "grok-4.7" || r.model == "grok-4.3",
        "a long task goes to Grok, got {}",
        r.model
    );
    assert!(r.rule_ids.contains(&"size:long".to_string()));
    assert!(r.rule_ids.contains(&"size:long:no_free".to_string()));
}

#[test]
fn a_short_task_keeps_the_free_model() {
    let reg = fleet();
    let r = Router::choose(
        &ask(
            "chat:default",
            "grok-4.7",
            granted(&["openrouter"]),
            Some(TaskSize::Short),
        ),
        &reg,
        &none(),
        &table_with("chat:default", FREE_MODEL),
        NOW,
    );
    assert_eq!(r.model, FREE_MODEL);
    assert!(!r.rule_ids.iter().any(|x| x.starts_with("size:")));
}

#[test]
fn a_long_task_ranks_grok_ahead_of_a_table_preferred_paid_provider() {
    let reg = fleet();
    let table = table_with("chat:default", PAID_MODEL);
    // Control: unsized, the class preference takes the paid provider.
    let r = Router::choose(
        &ask("chat:default", "grok-4.7", granted(&["openrouter"]), None),
        &reg,
        &none(),
        &table,
        NOW,
    );
    assert_eq!(r.model, PAID_MODEL);
    // Long: the plan (xAI) first within the paid routes, the table's
    // provider preference notwithstanding.
    let r = Router::choose(
        &ask(
            "chat:default",
            "grok-4.7",
            granted(&["openrouter"]),
            Some(TaskSize::Long),
        ),
        &reg,
        &none(),
        &table,
        NOW,
    );
    assert!(
        r.model == "grok-4.7" || r.model == "grok-4.3",
        "a long task goes to Grok, got {}",
        r.model
    );
}

#[test]
fn a_long_task_keeps_free_models_when_they_are_the_only_cloud() {
    let mut reg = Registry::default();
    reg.entitlement.credential = Credential::None;
    reg.apply_refresh(
        &[Listing::new(
            SourceKind::OpenAiCompatible,
            vec![provider(FREE_MODEL, 0)],
        )],
        &[],
        1,
    );
    let r = Router::choose(
        &ask(
            "chat:default",
            FREE_MODEL,
            granted(&["openrouter"]),
            Some(TaskSize::Long),
        ),
        &reg,
        &none(),
        &RoutingTable::default(),
        NOW,
    );
    assert!(!r.no_route, "a working free route beats a dead stop");
    assert_eq!(r.model, FREE_MODEL);
    assert!(!r.rule_ids.contains(&"size:long:no_free".to_string()));
}

/// An online gate that admits the small local tier at the given governor
/// level. Online, only the background classes are locally eligible, so the
/// warm tests ask as one of those.
fn warm_gate(level: Level) -> LocalGate {
    LocalGate {
        enabled: true,
        tier: "local:small".into(),
        capacity: LocalCapacity { ready: true, level },
        ..LocalGate::default()
    }
}

fn warm_ask<'a>(size: TaskSize, level: Level, warm: bool) -> RouteInput<'a> {
    let mut input = ask(
        "background:classify",
        "grok-4.7",
        Spend::default(),
        Some(size),
    );
    input.local = warm_gate(level);
    input.local_warm = warm;
    input
}

#[test]
fn a_warm_local_model_takes_less_challenging_work_by_default() {
    let reg = fleet();
    for size in [TaskSize::Tiny, TaskSize::Short] {
        let r = Router::choose(
            &warm_ask(size, Level::Normal, true),
            &reg,
            &none(),
            &RoutingTable::default(),
            NOW,
        );
        assert_eq!(r.model, "local:small", "size {size:?}");
        assert!(r.rule_ids.contains(&"size:warm_local".to_string()));
    }
    // Nothing loaded: the same tiny task takes the cloud pick as before.
    let r = Router::choose(
        &warm_ask(TaskSize::Tiny, Level::Normal, false),
        &reg,
        &none(),
        &RoutingTable::default(),
        NOW,
    );
    assert!(r.model.starts_with("grok-"), "got {}", r.model);
    assert!(!r.rule_ids.contains(&"size:warm_local".to_string()));
}

#[test]
fn a_warm_local_model_stays_off_long_work() {
    let reg = fleet();
    let r = Router::choose(
        &warm_ask(TaskSize::Long, Level::Normal, true),
        &reg,
        &none(),
        &RoutingTable::default(),
        NOW,
    );
    assert!(
        r.model.starts_with("grok-"),
        "a long task goes to a paid route, got {}",
        r.model
    );
    assert!(!r.rule_ids.contains(&"size:warm_local".to_string()));
}

#[test]
fn governor_throttle_limits_the_warm_default_to_tiny_work() {
    let reg = fleet();
    // Throttled (a game, a busy GPU): short work stays on the cloud rather
    // than loading new local work; tiny work still defaults to the warm model.
    let r = Router::choose(
        &warm_ask(TaskSize::Short, Level::Throttle, true),
        &reg,
        &none(),
        &RoutingTable::default(),
        NOW,
    );
    assert!(r.model.starts_with("grok-"), "got {}", r.model);
    assert!(!r.rule_ids.contains(&"size:warm_local".to_string()));
    let r = Router::choose(
        &warm_ask(TaskSize::Tiny, Level::Throttle, true),
        &reg,
        &none(),
        &RoutingTable::default(),
        NOW,
    );
    assert_eq!(r.model, "local:small");
    assert!(r.rule_ids.contains(&"size:warm_local".to_string()));
}

#[test]
fn with_no_cloud_route_the_warm_model_is_a_fallback_not_a_default() {
    // Every cloud model down: the device takes the step through the
    // fallback path (its rules, its cabin note), never the warm default.
    let mut reg = fleet();
    for id in ["grok-4.7", "grok-4.3"] {
        reg.models.get_mut(id).unwrap().state =
            grokhub_core::model_registry::ModelState::Quarantined;
    }
    let r = Router::choose(
        &warm_ask(TaskSize::Tiny, Level::Normal, true),
        &reg,
        &none(),
        &RoutingTable::default(),
        NOW,
    );
    assert_eq!(r.model, "local:small");
    assert!(!r.rule_ids.contains(&"size:warm_local".to_string()));
    assert!(
        r.rule_ids.iter().any(|x| x.starts_with("local:")),
        "the fallback path owns the pick: {:?}",
        r.rule_ids
    );
}

#[test]
fn the_warm_default_yields_to_an_episode_pin() {
    let reg = fleet();
    let mut input = warm_ask(TaskSize::Tiny, Level::Normal, true);
    input.episode_model = Some("grok-4.7");
    let r = Router::choose(&input, &reg, &none(), &RoutingTable::default(), NOW);
    assert_eq!(r.model, "grok-4.7");
    assert!(!r.rule_ids.contains(&"size:warm_local".to_string()));
}

#[test]
fn an_upgraded_turn_leaves_its_free_episode_model() {
    let reg = fleet();
    let mut input = ask(
        "chat:default",
        FREE_MODEL,
        granted(&["openrouter"]),
        Some(TaskSize::Long),
    );
    input.episode_model = Some(FREE_MODEL);
    let r = Router::choose(
        &input,
        &reg,
        &none(),
        &table_with("chat:default", FREE_MODEL),
        NOW,
    );
    assert!(!r.no_route);
    assert_ne!(r.model, FREE_MODEL);
    assert!(
        r.model == "grok-4.7" || r.model == "grok-4.3",
        "the upgraded turn moves to Grok, got {}",
        r.model
    );
}
