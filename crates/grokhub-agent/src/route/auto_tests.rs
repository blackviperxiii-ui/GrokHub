//! Auto router acceptance: live plan discovery, tier/effort rules, and the
//! redirected-flag fix. Pure unit tests, Windows-safe (no network).

use std::collections::BTreeMap;

use grokhub_core::model_registry::model_id_stem;
use grokhub_core::model_registry::profile::{EffortTiming, ModelProfile, ProbeResult};
use grokhub_core::model_registry::{
    Credential, Listing, ModelMeta, ModelState, Prices, Registry, SourceKind,
};

use super::policy::{self, CLASS_TABLE};
use super::table::{self, build_table, EvalCache, SpeedQuality, TableInputs};
use super::{RouteInput, Router};

const NOW: u64 = 2_000_000;

fn meta(id: &str, reasoning: bool) -> ModelMeta {
    let v: u64 = id
        .rsplit('.')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(1);
    ModelMeta {
        id: id.into(),
        context_length: Some(256_000),
        max_output: Some(32_000),
        prices: Prices {
            prompt: Some(10_000 * v),
            cached: Some(2_500 * v),
            completion: Some(50_000 * v),
            long_context_threshold: Some(200_000),
            ..Prices::default()
        },
        efforts: reasoning
            .then(|| vec!["low".into(), "medium".into(), "high".into(), "xhigh".into()]),
        tool_calling: Some(true),
        structured_output: Some(true),
        input_modalities: Some(vec!["text".into(), "image".into()]),
        api_shape: Some("responses".into()),
        ..ModelMeta::default()
    }
}

fn ok_probe() -> ProbeResult {
    ProbeResult {
        status: "ok".into(),
        tool_call: Some(true),
        json_mode: Some(true),
        caching: Some(true),
        cached_tokens: Some(768),
        efforts: vec![EffortTiming {
            effort: "medium".into(),
            measured: "probe".into(),
            reply_ms: Some(900),
            tokens_per_sec: Some(40),
        }],
        ..ProbeResult::default()
    }
}

fn plan_with(ids: &[&str]) -> (Registry, BTreeMap<String, ModelProfile>) {
    let mut reg = Registry::default();
    reg.entitlement.credential = Credential::Plan;
    let metas: Vec<ModelMeta> = ids
        .iter()
        .map(|id| {
            let reasoning = !id.contains("non-reasoning");
            meta(id, reasoning)
        })
        .collect();
    let mut listing = Listing::new(SourceKind::XaiApi, metas);
    listing.tier = Some("SuperGrok Heavy".into());
    reg.apply_refresh(&[listing], &[], 1);
    let profiles = ids
        .iter()
        .map(|id| {
            let reasoning = !id.contains("non-reasoning");
            (
                id.to_string(),
                ModelProfile::build(&meta(id, reasoning), ok_probe(), 1),
            )
        })
        .collect();
    (reg, profiles)
}

fn ask<'a>(class: &'a str, model: &'a str) -> RouteInput<'a> {
    RouteInput {
        provider: "xai",
        class,
        current_model: model,
        current_effort: None,
        pinned: false,
        d: 0.5,
        needs_tools: !class.starts_with("background:") && class != "chat:quick",
        ..RouteInput::default()
    }
}

#[test]
fn each_class_maps_to_its_tier_and_start_effort() {
    let want: &[(&str, SpeedQuality, &str)] = &[
        ("background:classify", SpeedQuality::Speed, "low"),
        ("background:triage", SpeedQuality::Speed, "low"),
        ("background:redact", SpeedQuality::Speed, "low"),
        ("background:summarize", SpeedQuality::Speed, "low"),
        ("background:compact", SpeedQuality::Speed, "low"),
        ("background:memory", SpeedQuality::Speed, "low"),
        ("chat:quick", SpeedQuality::Speed, "low"),
        ("background:judge", SpeedQuality::Quality, "low"),
        ("background:dream", SpeedQuality::Quality, "low"),
        ("chat:default", SpeedQuality::Quality, "medium"),
        ("code:edit-small", SpeedQuality::Quality, "low"),
        ("plan", SpeedQuality::Quality, "high"),
        ("design", SpeedQuality::Quality, "high"),
        ("debug", SpeedQuality::Quality, "high"),
        ("desktop:soft", SpeedQuality::Quality, "medium"),
        ("repair:diagnose", SpeedQuality::Quality, "high"),
        ("code:multi-file", SpeedQuality::Quality, "high"),
        ("prepare:hard", SpeedQuality::Quality, "high"),
    ];
    assert_eq!(want.len(), CLASS_TABLE.len());
    for (class, tier, start) in want {
        assert_eq!(SpeedQuality::of(class), *tier, "{class}");
        assert_eq!(policy::class_row(class).unwrap().start, *start, "{class}");
    }
    // Speed never pays for high: every speed class ceilings at medium or below.
    for row in CLASS_TABLE {
        if SpeedQuality::of(row.class) == SpeedQuality::Speed {
            let ceil = grokhub_core::model_registry::EFFORT_LADDER
                .iter()
                .position(|e| *e == row.ceiling)
                .unwrap();
            let medium = grokhub_core::model_registry::EFFORT_LADDER
                .iter()
                .position(|e| *e == "medium")
                .unwrap();
            assert!(ceil <= medium, "{} ceiling {}", row.class, row.ceiling);
        }
    }
}

#[test]
fn quality_never_picks_non_reasoning_and_speed_never_gets_high_effort() {
    let ids = ["grok-4.7", "grok-4.6", "grok-4.20-0309-non-reasoning"];
    let (reg, profiles) = plan_with(&ids);
    let table = build_table(
        &TableInputs {
            reg: &reg,
            profiles: &profiles,
        },
        &EvalCache::default(),
        NOW,
    );
    for row in CLASS_TABLE {
        let r = Router::choose(
            &ask(row.class, "grok-4.20-0309-non-reasoning"),
            &reg,
            &profiles,
            &table,
            NOW,
        );
        if SpeedQuality::of(row.class).needs_reasoning() {
            assert!(
                !r.model.contains("non-reasoning"),
                "{} → {}",
                row.class,
                r.model
            );
            assert!(
                table::is_reasoning_model(&reg, &profiles, &r.model),
                "{} → {}",
                row.class,
                r.model
            );
        }
        if SpeedQuality::of(row.class) == SpeedQuality::Speed {
            if let Some(e) = r.effort.as_deref() {
                let rung = grokhub_core::model_registry::EFFORT_LADDER
                    .iter()
                    .position(|x| *x == e)
                    .unwrap();
                let medium = grokhub_core::model_registry::EFFORT_LADDER
                    .iter()
                    .position(|x| *x == "medium")
                    .unwrap();
                assert!(rung <= medium, "{} effort {}", row.class, e);
            }
        }
        assert!(
            r.reason.contains(SpeedQuality::of(row.class).word()),
            "{} reason {}",
            row.class,
            r.reason
        );
    }
    // Non-reasoning never ranks in a quality class.
    for row in CLASS_TABLE {
        if SpeedQuality::of(row.class).needs_reasoning() {
            assert!(
                table
                    .rows(row.class)
                    .iter()
                    .all(|r| !r.model.contains("non-reasoning")),
                "{}",
                row.class
            );
        }
    }
}

#[test]
fn a_newer_plan_model_is_selected_and_adding_one_moves_quality_picks() {
    let (reg, profiles) = plan_with(&["grok-4.3", "grok-4.5", "grok-4.6", "grok-4.7"]);
    let table = build_table(
        &TableInputs {
            reg: &reg,
            profiles: &profiles,
        },
        &EvalCache::default(),
        NOW,
    );
    let mut quality_tops = Vec::new();
    for row in CLASS_TABLE {
        if SpeedQuality::of(row.class).needs_reasoning() {
            let top = &table.rows(row.class)[0].model;
            assert_eq!(top, "grok-4.7", "{} top {top}", row.class);
            quality_tops.push(top.clone());
            let r = Router::choose(&ask(row.class, "grok-4.3"), &reg, &profiles, &table, NOW);
            assert_eq!(r.model, "grok-4.7", "{}", row.class);
        }
    }
    assert!(!quality_tops.is_empty());
    // Adding grok-4.8 on the plan moves every quality top with no code change.
    let (reg8, profiles8) =
        plan_with(&["grok-4.3", "grok-4.5", "grok-4.6", "grok-4.7", "grok-4.8"]);
    let table8 = build_table(
        &TableInputs {
            reg: &reg8,
            profiles: &profiles8,
        },
        &EvalCache::default(),
        NOW,
    );
    for row in CLASS_TABLE {
        if SpeedQuality::of(row.class).needs_reasoning() {
            assert_eq!(table8.rows(row.class)[0].model, "grok-4.8", "{}", row.class);
            let r = Router::choose(&ask(row.class, "grok-4.7"), &reg8, &profiles8, &table8, NOW);
            assert_eq!(r.model, "grok-4.8", "{}", row.class);
        }
    }
}

#[test]
fn a_model_outside_the_plan_is_never_picked_even_when_plan_models_are_down() {
    let (mut reg, profiles) = plan_with(&["grok-4.7", "grok-4.6"]);
    // Premium / out-of-plan row that still looks healthy.
    let outside = meta("grok-4.8", true);
    reg.models.insert(
        "grok-4.8".into(),
        grokhub_core::model_registry::ModelRecord {
            meta: outside.clone(),
            state: ModelState::NotInPlan,
            reason: "Not in your plan.".into(),
            ..grokhub_core::model_registry::ModelRecord::default()
        },
    );
    let mut profiles = profiles;
    profiles.insert(
        "grok-4.8".into(),
        ModelProfile::build(&outside, ok_probe(), 1),
    );
    // Quarantine every plan model.
    for id in ["grok-4.7", "grok-4.6"] {
        reg.models.get_mut(id).unwrap().state = ModelState::Quarantined;
    }
    let table = build_table(
        &TableInputs {
            reg: &reg,
            profiles: &profiles,
        },
        &EvalCache::default(),
        NOW,
    );
    assert!(table
        .classes
        .values()
        .all(|c| c.ranked.iter().all(|r| r.model != "grok-4.8")));
    let r = Router::choose(&ask("plan", "grok-4.7"), &reg, &profiles, &table, NOW);
    assert_ne!(r.model, "grok-4.8");
    assert!(
        r.no_route || r.model == "grok-4.7" || r.model == "grok-4.6",
        "{}",
        r.model
    );
}

#[test]
fn dated_and_alias_response_ids_are_not_redirected() {
    assert_eq!(model_id_stem("grok-4.7"), "grok-4.7");
    assert_eq!(model_id_stem("grok-4.7-latest"), "grok-4.7");
    assert_eq!(model_id_stem("grok-4.7-0414"), "grok-4.7");
    assert_eq!(model_id_stem("grok-4.7-20260415"), "grok-4.7");
    let rec = grokhub_core::model_registry::ModelRecord {
        meta: ModelMeta {
            id: "grok-4.7".into(),
            aliases: vec!["grok-4.7-latest".into()],
            ..ModelMeta::default()
        },
        state: ModelState::Live,
        ..grokhub_core::model_registry::ModelRecord::default()
    };
    assert!(rec.answers_as("grok-4.7"));
    assert!(rec.answers_as("grok-4.7-latest"));
    assert!(rec.answers_as("grok-4.7-0414"));
    assert!(rec.answers_as("grok-4.7-20260415"));
    assert!(!rec.answers_as("grok-4.5"));
    let obs = grokhub_core::model_registry::Observation {
        model: "grok-4.7".into(),
        ts_ms: 1,
        status: grokhub_core::model_registry::CallStatus::Ok,
        latency_ms: 100,
        served_model: Some("grok-4.7-0414".into()),
        endpoint_ok: true,
        reasoning_tokens: 0,
        cost_ticks: 0,
        http: 200,
    };
    let mut reg = Registry::default();
    reg.models.insert("grok-4.7".into(), rec);
    let (events, _) = reg.fold(&[obs]);
    assert!(events.is_empty(), "{events:?}");
    assert_eq!(reg.get("grok-4.7").unwrap().state, ModelState::Live);
}
