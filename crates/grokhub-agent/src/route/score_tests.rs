//! Failover PR 6 acceptance: the scorer's hard filters and route ladder
//! at the pick itself. An explicit turn runs on the device; a route the
//! quota ledger has closed (OpenRouter spent, xAI blocked, Claude
//! benched) is never picked while another route can answer; and a long
//! task's paid routes are weighed against each other — the plan's own
//! models and a metered frontier model both outrank a cheap provider
//! the table happens to prefer.

use std::collections::BTreeMap;

use grokhub_core::local_governor::Level;
use grokhub_core::model_registry::profile::ModelProfile;
use grokhub_core::model_registry::quota::{
    AnthropicBench, OpenRouterQuota, OpenRouterState, QuotaLedger,
};
use grokhub_core::model_registry::{Credential, Listing, ModelMeta, Prices, Registry, SourceKind};

use super::local::{self, LocalCapacity, LocalGate};
use super::sizing::{SizeEstimate, TaskSize};
use super::spend::Spend;
use super::table::{ClassTable, RoutingTable, SpeedQuality, TableRow};
use super::{RouteInput, Router};

const FREE_MODEL: &str = "openrouter/nvidia/nemotron-test:free";
const PAID_MODEL: &str = "openrouter/vendor/paid";
const SONNET: &str = "anthropic/claude-sonnet-5.5";
const NOW: u64 = 1_000_000;

fn meta(id: &str, prompt: u64, completion: u64) -> ModelMeta {
    ModelMeta {
        id: id.into(),
        context_length: Some(256_000),
        prices: Prices {
            prompt: Some(prompt),
            completion: Some(completion),
            ..Prices::default()
        },
        efforts: Some(vec!["low".into(), "medium".into(), "high".into()]),
        tool_calling: Some(true),
        ..ModelMeta::default()
    }
}

/// grok-4.7 and grok-4.3 on xAI; a free and a paid OpenRouter model;
/// Claude Sonnet on its own key; the local tiers.
fn fleet() -> Registry {
    let mut reg = Registry::default();
    reg.entitlement.credential = Credential::Plan;
    reg.apply_refresh(
        &[
            Listing::new(
                SourceKind::XaiApi,
                vec![
                    meta("grok-4.7", 70_000, 350_000),
                    meta("grok-4.3", 30_000, 150_000),
                ],
            ),
            Listing::new(
                SourceKind::OpenAiCompatible,
                vec![meta(FREE_MODEL, 0, 0), meta(PAID_MODEL, 1_000, 5_000)],
            ),
            Listing::new(SourceKind::Anthropic, vec![meta(SONNET, 20_000, 100_000)]),
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
    let mut spend = Spend {
        providers: ids.iter().map(|s| s.to_string()).collect(),
        ..Spend::default()
    };
    spend.providers.push("anthropic".into());
    spend
}

fn ask<'a>(class: &'a str, size: Option<TaskSize>) -> RouteInput<'a> {
    RouteInput {
        provider: "xai",
        class,
        current_model: "grok-4.7",
        d: 0.5,
        needs_tools: true,
        spend: granted(&["openrouter"]),
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
fn an_explicit_turn_runs_on_the_device() {
    let reg = fleet();
    let mut input = ask("background:classify", Some(TaskSize::Short));
    input.local = LocalGate {
        enabled: true,
        tier: "local:small".into(),
        capacity: LocalCapacity {
            ready: true,
            level: Level::Normal,
        },
        ..LocalGate::default()
    };
    input.adult = true;
    let r = Router::choose(&input, &reg, &none(), &RoutingTable::default(), NOW);
    assert_eq!(r.model, "local:small");
    assert!(r.rule_ids.contains(&"adult:local_only".to_string()));
}

#[test]
fn a_spent_openrouter_account_loses_the_pick_to_the_plan() {
    let reg = fleet();
    let mut input = ask("chat:default", Some(TaskSize::Short));
    input.quota = QuotaLedger {
        openrouter: OpenRouterQuota {
            state: OpenRouterState::Negative,
            ..OpenRouterQuota::default()
        },
        ..QuotaLedger::default()
    };
    let r = Router::choose(
        &input,
        &reg,
        &none(),
        &table_with("chat:default", FREE_MODEL),
        NOW,
    );
    assert!(
        r.model == "grok-4.7" || r.model == "grok-4.3",
        "a closed account's models are filtered, got {}",
        r.model
    );
    assert!(r.rule_ids.contains(&"score:quota:openrouter".to_string()));
}

#[test]
fn a_blocked_xai_route_falls_to_the_next_paid_route() {
    let reg = fleet();
    let mut input = ask("chat:default", Some(TaskSize::Medium));
    input.quota.xai.checked_at_ms = 1;
    input.quota.xai.api_key_blocked = Some(true);
    // No table: the medium ladder runs xAI, then cheap paid, then a
    // free model only when it is a proven tool model — this one is not.
    let r = Router::choose(&input, &reg, &none(), &RoutingTable::default(), NOW);
    assert_eq!(r.model, PAID_MODEL, "got {}", r.model);
    assert!(r.rule_ids.contains(&"score:quota:xai".to_string()));
}

#[test]
fn a_benched_claude_is_not_picked() {
    let reg = fleet();
    let mut input = ask("chat:default", Some(TaskSize::Medium));
    input.quota.anthropic.bench = AnthropicBench::SpendLimit;
    input.quota.anthropic.benched_until_ms = NOW + 60_000;
    let r = Router::choose(
        &input,
        &reg,
        &none(),
        &table_with("chat:default", SONNET),
        NOW,
    );
    assert!(
        r.model == "grok-4.7" || r.model == "grok-4.3",
        "a benched Claude is filtered, got {}",
        r.model
    );
    assert!(r.rule_ids.contains(&"score:quota:anthropic".to_string()));
}

#[test]
fn a_long_task_weighs_the_paid_routes_against_each_other() {
    let reg = fleet();
    // The table prefers the cheap provider; a long task is one paid
    // tier scored as a whole, so a frontier route carries it instead.
    let r = Router::choose(
        &ask("chat:default", Some(TaskSize::Long)),
        &reg,
        &none(),
        &table_with("chat:default", PAID_MODEL),
        NOW,
    );
    assert!(
        r.model == "grok-4.7" || r.model == "grok-4.3" || r.model == SONNET,
        "a long task goes to a frontier paid route, got {}",
        r.model
    );
}

#[test]
fn gaming_throttle_keeps_medium_work_off_the_device() {
    let reg = fleet();
    let mut input = ask("background:classify", Some(TaskSize::Medium));
    input.local = LocalGate {
        enabled: true,
        tier: "local:small".into(),
        capacity: LocalCapacity {
            ready: true,
            level: Level::Throttle,
        },
        ..LocalGate::default()
    };
    input.local_warm = true;
    let r = Router::choose(&input, &reg, &none(), &RoutingTable::default(), NOW);
    assert!(
        r.model.starts_with("grok-"),
        "throttled, medium work stays online, got {}",
        r.model
    );
    assert!(!r.rule_ids.contains(&"size:warm_local".to_string()));
}
