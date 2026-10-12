//! Auto router acceptance: capability-aware picks, tier/effort rules, and the
//! redirected-flag fix. Pure unit tests, Windows-safe (no network).

use std::collections::BTreeMap;

use grokhub_core::model_registry::model_id_stem;
use grokhub_core::model_registry::profile::{EffortTiming, ModelProfile, ProbeResult};
use grokhub_core::model_registry::{
    Credential, Listing, ModelMeta, ModelState, Prices, Registry, SourceKind,
};

use super::capabilities::{self, class_need};
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
    // Non-reasoning / mid / flagship price ladder so cost_efficiency differs.
    let prompt = if id.contains("non-reasoning") {
        8_000
    } else if id.contains("4.20") {
        20_000
    } else {
        10_000 * v.max(1)
    };
    ModelMeta {
        id: id.into(),
        context_length: Some(256_000),
        max_output: Some(32_000),
        prices: Prices {
            prompt: Some(prompt),
            cached: Some(prompt / 4),
            completion: Some(prompt * 5),
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

fn top_of(table: &table::RoutingTable, class: &str) -> String {
    table.rows(class)[0].model.clone()
}

#[test]
fn each_class_maps_to_its_tier_and_start_effort() {
    let want: &[(&str, SpeedQuality, &str)] = &[
        ("background:classify", SpeedQuality::Speed, "low"),
        ("background:triage", SpeedQuality::Speed, "low"),
        ("background:redact", SpeedQuality::Speed, "low"),
        ("background:summarize", SpeedQuality::Speed, "low"),
        ("background:compact", SpeedQuality::Speed, "low"),
        ("background:memory", SpeedQuality::Balanced, "low"),
        ("chat:quick", SpeedQuality::Speed, "low"),
        ("background:judge", SpeedQuality::Balanced, "low"),
        ("background:dream", SpeedQuality::Balanced, "low"),
        ("chat:default", SpeedQuality::Balanced, "medium"),
        ("code:edit-small", SpeedQuality::Balanced, "low"),
        ("plan", SpeedQuality::Quality, "high"),
        ("design", SpeedQuality::Quality, "high"),
        ("debug", SpeedQuality::Quality, "high"),
        ("desktop:soft", SpeedQuality::Balanced, "medium"),
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
fn quality_and_balanced_never_pick_non_reasoning_and_speed_never_gets_high_effort() {
    let ids = [
        "grok-4.7",
        "grok-4.3",
        "grok-4.20-0309-reasoning",
        "grok-4.20-0309-non-reasoning",
    ];
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
        let need = class_need(row.class);
        if SpeedQuality::of(row.class).needs_reasoning() || need.require_reasoning {
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
    for row in CLASS_TABLE {
        if SpeedQuality::of(row.class).needs_reasoning() || class_need(row.class).require_reasoning
        {
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

/// Literal pick diversity: given a mixed plan catalog, classes must not all
/// collapse to the same two model ids (the v11 failure mode).
#[test]
fn capability_fit_diversifies_picks_across_task_classes() {
    let ids = [
        "grok-4.7",
        "grok-4.5",
        "grok-4.3",
        "grok-4.20-0309-reasoning",
        "grok-4.20-0309-non-reasoning",
    ];
    let (reg, profiles) = plan_with(&ids);
    let table = build_table(
        &TableInputs {
            reg: &reg,
            profiles: &profiles,
        },
        &EvalCache::default(),
        NOW,
    );

    // Speed / light work → cheapest fast non-reasoning.
    for class in [
        "background:triage",
        "background:redact",
        "background:summarize",
        "background:compact",
        "chat:quick",
    ] {
        assert_eq!(
            top_of(&table, class),
            "grok-4.20-0309-non-reasoning",
            "{class}"
        );
    }

    // Memory needs light reasoning, not the non-reasoning speed slug.
    assert_eq!(
        top_of(&table, "background:memory"),
        "grok-4.20-0309-reasoning"
    );

    // Everyday chat / small edits: mid reasoning workhorse, not flagship.
    assert_eq!(top_of(&table, "chat:default"), "grok-4.20-0309-reasoning");
    assert_eq!(
        top_of(&table, "code:edit-small"),
        "grok-4.20-0309-reasoning"
    );

    // Multi-file prefers the coding-strong family; debug/prepare/repair prefer flagship reasoning.
    assert_eq!(top_of(&table, "code:multi-file"), "grok-4.3");
    assert_eq!(top_of(&table, "debug"), "grok-4.7");
    assert_eq!(top_of(&table, "prepare:hard"), "grok-4.7");
    assert_eq!(top_of(&table, "repair:diagnose"), "grok-4.7");
    assert_eq!(top_of(&table, "plan"), "grok-4.7");
    assert_eq!(
        table.classes["repair:diagnose"].speed_quality,
        SpeedQuality::Quality
    );

    let mut tops: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for row in CLASS_TABLE {
        tops.entry(top_of(&table, row.class))
            .or_default()
            .push(row.class);
    }
    // At least three distinct model ids across the table (was two in v11).
    assert!(
        tops.len() >= 3,
        "only {} distinct tops: {tops:?}",
        tops.len()
    );

    // Router::choose matches the table tops for these classes.
    for (class, want) in [
        ("background:triage", "grok-4.20-0309-non-reasoning"),
        ("background:memory", "grok-4.20-0309-reasoning"),
        ("chat:default", "grok-4.20-0309-reasoning"),
        ("code:multi-file", "grok-4.3"),
        ("debug", "grok-4.7"),
        ("plan", "grok-4.7"),
    ] {
        let r = Router::choose(&ask(class, "grok-4.5"), &reg, &profiles, &table, NOW);
        assert_eq!(r.model, want, "{class}");
    }
}

#[test]
fn adding_a_stronger_flagship_moves_reasoning_heavy_classes_not_speed() {
    let base = [
        "grok-4.3",
        "grok-4.5",
        "grok-4.7",
        "grok-4.20-0309-non-reasoning",
    ];
    let (reg, profiles) = plan_with(&base);
    let table = build_table(
        &TableInputs {
            reg: &reg,
            profiles: &profiles,
        },
        &EvalCache::default(),
        NOW,
    );
    assert_eq!(top_of(&table, "debug"), "grok-4.7");
    assert_eq!(
        top_of(&table, "background:triage"),
        "grok-4.20-0309-non-reasoning"
    );

    let with8 = [
        "grok-4.3",
        "grok-4.5",
        "grok-4.7",
        "grok-4.8",
        "grok-4.20-0309-non-reasoning",
    ];
    let (reg8, profiles8) = plan_with(&with8);
    let table8 = build_table(
        &TableInputs {
            reg: &reg8,
            profiles: &profiles8,
        },
        &EvalCache::default(),
        NOW,
    );
    // Flagship reasoning classes (including repair) move to 4.8; speed and code-specialist stay put.
    assert_eq!(top_of(&table8, "debug"), "grok-4.8");
    assert_eq!(top_of(&table8, "prepare:hard"), "grok-4.8");
    assert_eq!(top_of(&table8, "repair:diagnose"), "grok-4.8");
    assert_eq!(
        top_of(&table8, "background:triage"),
        "grok-4.20-0309-non-reasoning"
    );
    assert_eq!(top_of(&table8, "code:multi-file"), "grok-4.3");
    let r = Router::choose(&ask("debug", "grok-4.7"), &reg8, &profiles8, &table8, NOW);
    assert_eq!(r.model, "grok-4.8");
}

#[test]
fn a_model_outside_the_plan_is_never_picked_even_when_plan_models_are_down() {
    let (mut reg, profiles) = plan_with(&["grok-4.7", "grok-4.6"]);
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

/// Hard rule: host repair must never land on a balanced/cheap/mid model when a
/// stronger reasoning model is on the plan.
#[test]
fn repair_diagnose_always_picks_the_strongest_reasoning_model() {
    let ids = [
        "grok-4.7",
        "grok-4.5",
        "grok-4.3",
        "grok-4.20-0309-reasoning",
        "grok-4.20-0309-non-reasoning",
    ];
    let (reg, profiles) = plan_with(&ids);
    let table = build_table(
        &TableInputs {
            reg: &reg,
            profiles: &profiles,
        },
        &EvalCache::default(),
        NOW,
    );
    assert_eq!(SpeedQuality::of("repair:diagnose"), SpeedQuality::Quality);
    assert!(class_need("repair:diagnose").require_reasoning);
    assert_eq!(class_need("repair:diagnose").cost, 0);
    assert_eq!(class_need("repair:diagnose").speed, 0);
    assert_eq!(top_of(&table, "repair:diagnose"), "grok-4.7");
    assert_ne!(top_of(&table, "repair:diagnose"), "grok-4.3");
    assert_ne!(
        top_of(&table, "repair:diagnose"),
        "grok-4.20-0309-reasoning"
    );
    let r = Router::choose(
        &ask("repair:diagnose", "grok-4.20-0309-non-reasoning"),
        &reg,
        &profiles,
        &table,
        NOW,
    );
    assert_eq!(r.model, "grok-4.7");
    assert!(r.reason.contains("quality"), "{}", r.reason);
    // Stronger flagship on the plan moves repair with it.
    let with8 = [
        "grok-4.7",
        "grok-4.8",
        "grok-4.3",
        "grok-4.20-0309-non-reasoning",
    ];
    let (reg8, profiles8) = plan_with(&with8);
    let table8 = build_table(
        &TableInputs {
            reg: &reg8,
            profiles: &profiles8,
        },
        &EvalCache::default(),
        NOW,
    );
    assert_eq!(top_of(&table8, "repair:diagnose"), "grok-4.8");
    let r8 = Router::choose(
        &ask("repair:diagnose", "grok-4.7"),
        &reg8,
        &profiles8,
        &table8,
        NOW,
    );
    assert_eq!(r8.model, "grok-4.8");
}

#[test]
fn catalog_build_covers_every_live_plan_model() {
    let ids = ["grok-4.7", "grok-4.3", "grok-4.20-0309-non-reasoning"];
    let (reg, _) = plan_with(&ids);
    let cat = capabilities::build_catalog(&reg, NOW);
    assert_eq!(cat.seed_version, capabilities::SEED_VERSION);
    for id in ids {
        assert!(cat.entries.contains_key(id), "missing {id}");
        assert!(cat.entries[id].caps.speed > 0);
    }
    assert!(cat.entries["grok-4.3"].caps.code > cat.entries["grok-4.7"].caps.code);
}
