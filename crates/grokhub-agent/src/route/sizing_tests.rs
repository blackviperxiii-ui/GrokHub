//! Failover PR 5 acceptance: a long task is sized before it runs and sent
//! to Grok — never to a free provider model while a route that can carry
//! it exists, and Grok ahead of even a table-preferred paid provider. Free
//! models stay when they are the only cloud there is (a working free route
//! beats a dead stop), and a turn upgraded to long mid-flight leaves the
//! free model it was riding.

use std::collections::BTreeMap;

use grokhub_core::model_registry::profile::ModelProfile;
use grokhub_core::model_registry::{Credential, Listing, ModelMeta, Prices, Registry, SourceKind};

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
    // Long: Grok first, the table's provider preference notwithstanding.
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
