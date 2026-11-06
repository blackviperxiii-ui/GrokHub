//! The router's model and price facts: the 200k gate, grok-4.20-multi-agent's
//! agent count, request params a reasoning model refuses, per-model request
//! limits in failover, and Grok 4.7 Fast only on a Grok Build sign-in.

use std::collections::BTreeMap;

use grokhub_core::model_registry::cost_class::CostClass;
use grokhub_core::model_registry::profile::ModelProfile;
use grokhub_core::model_registry::seed::seed_prices;
use grokhub_core::model_registry::{
    Credential, Listing, ModelMeta, ModelState, Prices, Registry, SourceKind,
};

use super::spend::{Latency, Spend};
use super::table::{ClassTable, RoutingTable, SpeedQuality, TableRow};
use super::{live, RouteInput, Router};
use crate::client::{
    compact_body, responses_body, CompactRequest, ContentPart, InputItem, ResponsesRequest,
    TextFormat,
};

const NOW: u64 = 1_000_000;

fn meta(id: &str, context: u64, prices: Prices) -> ModelMeta {
    ModelMeta {
        id: id.into(),
        context_length: Some(context),
        prices,
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

fn fleet(cred: Credential, rows: Vec<ModelMeta>) -> Registry {
    let mut reg = Registry::default();
    reg.entitlement.credential = cred;
    reg.apply_refresh(&[Listing::new(SourceKind::XaiApi, rows)], &[], 1);
    reg
}

/// grok-4.7 (500k) and grok-4.3 (1M) at their seed prices.
fn grok_fleet() -> Registry {
    fleet(
        Credential::Plan,
        vec![
            meta("grok-4.7", 500_000, seed_prices("grok-4.7").unwrap()),
            meta("grok-4.3", 1_000_000, seed_prices("grok-4.3").unwrap()),
        ],
    )
}

/// A table that ranks grok-4.7 first for everyday chat.
fn table_47_first() -> RoutingTable {
    let row = |m: &str| TableRow {
        model: m.into(),
        effort: Some("medium".into()),
        quality: Some(1.0),
        est_cost_usd: None,
        p50_latency_ms: None,
        why: "the best balance in your plan".into(),
    };
    let mut t = RoutingTable {
        schema: 1,
        version: 1,
        ..RoutingTable::default()
    };
    t.classes.insert(
        "chat:default".into(),
        ClassTable {
            speed_quality: SpeedQuality::Balanced,
            ranked: vec![row("grok-4.7"), row("grok-4.3")],
        },
    );
    t
}

fn none() -> BTreeMap<String, ModelProfile> {
    BTreeMap::new()
}

fn ask<'a>(class: &'a str, model: &'a str, ctx_tokens: u64) -> RouteInput<'a> {
    RouteInput {
        provider: "xai",
        class,
        current_model: model,
        d: 0.5,
        ctx_tokens,
        needs_tools: true,
        ..RouteInput::default()
    }
}

#[test]
fn at_200k_a_non_flagship_lane_moves_to_a_1m_model_and_nothing_takes_a_prompt_it_cant_hold() {
    let reg = grok_fleet();
    let t = table_47_first();
    let pick = |class: &str, ctx| {
        let r = Router::choose(&ask(class, "grok-4.7", ctx), &reg, &none(), &t, NOW);
        (r.model, r.rule_ids.contains(&"ctx:long_1m".to_string()))
    };
    // Under the step the table's pick stands.
    assert_eq!(
        pick("chat:default", 199_999),
        ("grok-4.7".to_string(), false)
    );
    // From 200k grok-4.7 doubles: the 1M grok-4.3 takes everyday chat.
    assert_eq!(
        pick("chat:default", 200_000),
        ("grok-4.3".to_string(), true)
    );
    // The Flagship lane keeps the strongest reasoner while the prompt fits it.
    assert_eq!(pick("plan", 250_000), ("grok-4.7".to_string(), false));
    // Past grok-4.7's 500k window only grok-4.3 can hold the prompt, whatever the lane.
    assert_eq!(pick("plan", 600_000), ("grok-4.3".to_string(), false));
    // Doubled, grok-4.3 still costs less than grok-4.7 doubled.
    let rates = |id| seed_prices(id).unwrap().rates_at(200_000).unwrap();
    assert_eq!(rates("grok-4.3"), (25_000, 4_000, 50_000));
    assert_eq!(rates("grok-4.7"), (40_000, 10_000, 120_000));
}

#[test]
fn multi_agent_effort_is_four_agents_unless_a_granted_flagship_turn_climbs_to_high() {
    let ma = "grok-4.20-multi-agent";
    let reg = fleet(
        Credential::Plan,
        vec![meta(
            ma,
            2_000_000,
            Prices {
                prompt: Some(20_000),
                cached: Some(5_000),
                completion: Some(60_000),
                ..Prices::default()
            },
        )],
    );
    let effort = |class: &str, spend: Spend| {
        let i = RouteInput {
            spend,
            ..ask(class, ma, 0)
        };
        let r = Router::choose(&i, &reg, &none(), &RoutingTable::default(), NOW);
        assert_eq!(
            (r.model.as_str(), r.no_route),
            (ma, false),
            "{:?}",
            r.rule_ids
        );
        let agents = r
            .rule_ids
            .iter()
            .find(|x| x.starts_with("multi_agent:"))
            .cloned();
        (r.effort, agents)
    };
    let four = (Some("low".to_string()), Some("multi_agent:4".to_string()));
    // A cheap turn never asks for 16 agents.
    assert_eq!(effort("chat:quick", Spend::default()), four);
    assert_eq!(effort("chat:default", Spend::default()), four);
    // plan starts at High, but 16 agents is a premium route: no grant, 4 agents.
    assert_eq!(effort("plan", Spend::default()), four);
    let granted = Spend {
        grants: vec!["premium:grok-4.20-multi-agent+agents16".into()],
        ..Spend::default()
    };
    assert_eq!(
        effort("plan", granted.clone()),
        (Some("high".to_string()), Some("multi_agent:16".to_string()))
    );
    // The grant alone doesn't lift an everyday turn.
    assert_eq!(effort("chat:default", granted), four);
    // Any other model keeps the ladder's effort.
    let r = Router::choose(
        &ask("plan", "grok-4.7", 0),
        &grok_fleet(),
        &none(),
        &RoutingTable::default(),
        NOW,
    );
    assert_eq!(
        (r.model.as_str(), r.effort.as_deref()),
        ("grok-4.7", Some("high"))
    );
}

#[test]
fn request_bodies_for_a_reasoning_model_carry_no_penalty_or_stop_keys() {
    let req = ResponsesRequest {
        model: "grok-4.7".into(),
        effort: Some("high".into()),
        input: vec![InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText("hi".into())],
        }],
        conversation_id: "s".into(),
        tools: vec![serde_json::json!({"type":"function","name":"read_file"})],
        hosted_search: true,
        call_timeout: None,
        text_format: Some(TextFormat {
            name: "reply",
            schema: r#"{"type":"object","properties":{},"required":[],"additionalProperties":false}"#,
        }),
    };
    let keys =
        |v: &serde_json::Value| -> Vec<String> { v.as_object().unwrap().keys().cloned().collect() };
    assert_eq!(
        keys(&responses_body(&req)),
        [
            "include",
            "input",
            "max_turns",
            "model",
            "parallel_tool_calls",
            "prompt_cache_key",
            "reasoning",
            "store",
            "stream",
            "text",
            "tools"
        ]
    );
    assert_eq!(
        keys(&crate::batch::batch_body(&req)),
        [
            "include",
            "input",
            "max_turns",
            "model",
            "parallel_tool_calls",
            "prompt_cache_key",
            "reasoning",
            "store",
            "text",
            "tools"
        ]
    );
    let compact = CompactRequest {
        model: "grok-4.7".into(),
        input: req.input.clone(),
        conversation_id: "s".into(),
        timeout: std::time::Duration::from_secs(1),
    };
    assert_eq!(keys(&compact_body(&compact)), ["input", "model"]);
    for model in ["grok-4.3", "grok-4.20-multi-agent", "grok-4.5"] {
        let body = responses_body(&ResponsesRequest {
            model: model.into(),
            ..req.clone()
        })
        .to_string();
        for bad in [
            "presence_penalty",
            "presencePenalty",
            "frequency_penalty",
            "frequencyPenalty",
            "\"stop\"",
        ] {
            assert!(!body.contains(bad), "{model}: {bad} in {body}");
        }
    }
}

#[test]
fn a_model_at_its_request_limit_is_busy_and_failover_takes_the_next() {
    let dir = crate::harness::test_dir("facts-pace");
    let other = crate::harness::test_dir("facts-pace-other");
    let reg = std::sync::Arc::new(grok_fleet());
    // Everyday code work ranks grok-4.3 first.
    let code = |reg: &Registry| {
        Router::choose(
            &ask("code:multi-file", "grok-4.3", 0),
            reg,
            &none(),
            &RoutingTable::default(),
            NOW,
        )
        .model
    };
    assert_eq!(code(&reg), "grok-4.3");
    for i in 0..37 {
        live::note_send(&dir, "grok-4.3", NOW - 36 + i);
    }
    let view = live::paced(&dir, reg.clone(), NOW);
    let rec = view.get("grok-4.3").unwrap();
    assert_eq!(
        (rec.state, rec.reason.as_str()),
        (
            ModelState::Degraded,
            "It is at its request limit this second."
        )
    );
    assert_eq!(view.get("grok-4.7").unwrap().state, ModelState::Live);
    assert_eq!(code(&view), "grok-4.7");
    // Another config dir has its own count, and a second later the limit has passed.
    assert_eq!(
        live::paced(&other, reg.clone(), NOW)
            .get("grok-4.3")
            .unwrap()
            .state,
        ModelState::Live
    );
    assert_eq!(
        live::paced(&dir, reg.clone(), NOW + 1_000)
            .get("grok-4.3")
            .unwrap()
            .state,
        ModelState::Live
    );
    // The saved registry itself never changed.
    assert_eq!(reg.get("grok-4.3").unwrap().state, ModelState::Live);
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(other);
}

#[test]
fn a_model_that_was_at_capacity_is_routed_around_for_a_minute() {
    let dir = crate::harness::test_dir("facts-cool");
    let reg = std::sync::Arc::new(grok_fleet());
    let code = |reg: &Registry| {
        Router::choose(
            &ask("code:multi-file", "grok-4.3", 0),
            reg,
            &none(),
            &RoutingTable::default(),
            NOW,
        )
        .model
    };
    assert_eq!(code(&reg), "grok-4.3");
    live::cool_down(&dir, "grok-4.3", NOW);
    let view = live::paced(&dir, reg.clone(), NOW + 1_000);
    let rec = view.get("grok-4.3").unwrap();
    assert_eq!(
        (rec.state, rec.reason.as_str()),
        (
            ModelState::Degraded,
            "It was too busy to answer a moment ago."
        )
    );
    // The resumed step goes to the next model, not the same wall.
    assert_eq!(code(&view), "grok-4.7");
    // A minute on it is back in the running.
    assert_eq!(
        live::paced(&dir, reg.clone(), NOW + live::OUTAGE_COOL_MS)
            .get("grok-4.3")
            .unwrap()
            .state,
        ModelState::Live
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn grok_4_7_fast_swaps_in_only_on_a_grok_build_sign_in() {
    let fleet_with_fast = |cred| {
        let mut reg = Registry::default();
        reg.entitlement.credential = cred;
        let api = Listing::new(
            SourceKind::XaiApi,
            vec![meta("grok-4.7", 500_000, seed_prices("grok-4.7").unwrap())],
        );
        let gb = Listing::new(
            SourceKind::GrokBuild,
            vec![
                ModelMeta::bare("grok-4.7"),
                ModelMeta::bare("grok-4.7-fast"),
            ],
        );
        reg.apply_refresh(&[api, gb], &[], 1);
        reg
    };
    let waiting = Spend {
        latency: Latency {
            waiting_live: true,
            ..Latency::default()
        },
        ..Spend::default()
    };
    let pick = |cred, provider| {
        let i = RouteInput {
            provider,
            spend: waiting.clone(),
            ..ask("chat:default", "grok-4.7", 0)
        };
        let r = Router::choose(&i, &fleet_with_fast(cred), &none(), &table_47_first(), NOW);
        (r.model, r.base, r.cost_class)
    };
    assert_eq!(
        pick(Credential::Plan, "xai"),
        (
            "grok-4.7-fast".to_string(),
            Some("grok-4.7".to_string()),
            CostClass::AutonomousPremium
        )
    );
    assert_eq!(
        pick(Credential::None, live::PROVIDER_GROK_BUILD).0,
        "grok-4.7-fast"
    );
    // A plain API key: the public API has no Fast variant.
    assert_eq!(
        pick(Credential::ApiKey, "xai"),
        ("grok-4.7".to_string(), None, CostClass::Included)
    );
}
