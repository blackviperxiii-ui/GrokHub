//! Router R3a acceptance on real files: DE2 from outcome records, one
//! fixture run of shadow → canary → promote → rollback through the
//! ChangeLedger, a cost-raising candidate that takes the same silent guarded
//! path, Undo byte-identical, the stub refusal, and the local model: off by
//! default, and with it forced on (fake runtime) only for background work.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{json, Value};

use grokhub_core::learn::decide;
use grokhub_core::model_registry::cost_class::CostClass;
use grokhub_core::model_registry::profile::ModelProfile;
use grokhub_core::model_registry::{
    CatalogSource, Credential, Listing, ModelMeta, Prices, Registry, SourceKind,
};
use grokhub_core::outcome::{append_outcome, OutcomeResult, TaskOutcome};

use super::difficulty::{difficulty, estimate, DifficultyInput};
use super::ladder::{routine_rung, rung, Band};
use super::learn::{self, load_state, tuning_path, write_tuning, TuningFile, NEEDS_DATA};
use super::local::{self, Device, ForceOn, Installed, LocalGate, LocalRuntime, LocalSource};
use super::log::{Chosen, RouteOutcome, RouteRecord, RouteSignals, ROUTE_TRACE};
use super::policy::class_row;
use super::signals::{LedgerOutcomeSource, SpanVerifySource, StubSource};
use super::table::RoutingTable;
use super::trust::{self, Trust};
use super::tune::{Candidate, Change, Stage, TuneState, Tuning, DAY_MS};
use super::{RouteInput, Router};
use crate::harness::{
    append_span, read_egress, test_dir, undo_change, AccessMode, ChangeKind, ChangeLedger, Span,
    UndoAsk,
};

const T0: u64 = 2_000 * DAY_MS;

fn rec(
    model: &str,
    effort: &str,
    episode: &str,
    verify: &str,
    cost: f64,
    tune: Option<String>,
) -> RouteRecord {
    RouteRecord {
        episode: episode.into(),
        class: "chat:default".into(),
        used: Chosen {
            provider: "xai".into(),
            model: model.into(),
            effort: Some(effort.into()),
        },
        outcome: Some(RouteOutcome {
            ok: true,
            verify: Some(verify.into()),
            cost_usd: cost,
            ..RouteOutcome::default()
        }),
        signals: RouteSignals {
            latency: Some(2_000),
            ..RouteSignals::default()
        },
        tune,
        ..RouteRecord::default()
    }
}

fn log(dir: &std::path::Path, at: u64, r: RouteRecord) {
    let mut span = Span::soft_allow(
        ROUTE_TRACE,
        "model.call",
        "{}",
        "ok",
        "",
        AccessMode::Supervised,
        "xai",
    );
    span.ts_ms = at;
    span.route = Some(Box::new(r));
    append_span(dir, &span).unwrap();
}

fn tick(dir: &std::path::Path, now: u64) -> learn::TuneRun {
    let verify = SpanVerifySource {
        config_dir: dir.to_path_buf(),
        session: String::new(),
    };
    learn::tick(dir, &verify, &LedgerOutcomeSource::new(dir), now).unwrap()
}

fn seed(dir: &std::path::Path, c: Candidate) {
    let state = TuneState {
        candidates: vec![c],
        built_at: T0,
        inputs_hash: format!(
            "{}:{}",
            Registry::default().hash,
            super::table::profiles_hash(&BTreeMap::new())
        ),
        seq: 1,
    };
    grokhub_core::model_registry::store::write_json(&learn::state_path(dir), &state).unwrap();
}

/// grok-4.7 is live for everyday chat; grok-4.6 is the candidate.
fn order_candidate() -> Candidate {
    Candidate {
        id: "chat:default#1".into(),
        class: "chat:default".into(),
        change: Some(Change::Order {
            model: "grok-4.6".into(),
        }),
        stage: Stage::Shadow,
        created_at: T0,
        ..Candidate::default()
    }
}

#[test]
fn shadow_then_canary_then_promote_then_rollback_with_both_ledger_lines_and_undo_byte_identical() {
    let dir = test_dir("r3a-lifecycle");
    seed(&dir, order_candidate());
    // Shadow: 199 eligible steps are not enough, 200 are.
    for i in 0..200u64 {
        let ep = format!("sh-{i}");
        log(
            &dir,
            T0 + 1 + i,
            rec(
                "grok-4.7",
                "medium",
                &ep,
                "ok",
                0.0100,
                Some("shadow:chat:default#1".into()),
            ),
        );
        if i == 198 {
            assert!(tick(&dir, T0 + 300).canaries.is_empty());
        }
    }
    let t1 = T0 + DAY_MS;
    assert_eq!(tick(&dir, t1).canaries, vec!["chat:default#1".to_string()]);
    // Canary: 50 steps at 90% passed and $0.0088 a step vs live 90% at $0.0100 (12% cheaper).
    for i in 0..400u64 {
        let pass = if i % 10 == 9 { "reject" } else { "ok" };
        log(
            &dir,
            t1 + 1 + i,
            rec(
                "grok-4.7",
                "medium",
                &format!("live-{i}"),
                pass,
                0.0100,
                None,
            ),
        );
    }
    for i in 0..50u64 {
        let pass = if i % 10 == 9 { "reject" } else { "ok" };
        log(
            &dir,
            t1 + 500 + i,
            rec(
                "grok-4.6",
                "medium",
                &format!("can-{i}"),
                pass,
                0.0088,
                Some("canary:chat:default#1".into()),
            ),
        );
    }
    let t2 = t1 + DAY_MS;
    let run = tick(&dir, t2);
    assert_eq!(run.promoted, vec!["chat:default#1".to_string()]);
    let promoted = std::fs::read_to_string(tuning_path(&dir)).unwrap();
    assert!(
        promoted.contains("\"chat:default\": \"grok-4.6\""),
        "{promoted}"
    );
    let promoted = &load_state(&dir).candidates[0];
    assert_eq!(
        promoted
            .promoted_snap
            .unwrap()
            .cost_gain_pct(&promoted.baseline.unwrap())
            .round(),
        12.0
    );
    assert_eq!(
        (promoted.id.as_str(), promoted.stage),
        ("chat:default#1", Stage::Watch)
    );
    assert_eq!(
        learn::load_tuning(&dir)
            .orders
            .get("chat:default")
            .map(String::as_str),
        Some("grok-4.6")
    );
    // After promotion: 30 steps at 80% passed (10 points down) roll it back at once.
    for i in 0..30u64 {
        let pass = if i % 5 == 4 { "reject" } else { "ok" };
        log(
            &dir,
            t2 + 1 + i,
            rec(
                "grok-4.6",
                "medium",
                &format!("post-{i}"),
                pass,
                0.0088,
                None,
            ),
        );
    }
    let t3 = t2 + DAY_MS;
    let run = tick(&dir, t3);
    assert_eq!(run.rolled_back, vec!["chat:default#1".to_string()]);
    assert!(
        !tuning_path(&dir).exists(),
        "back to the state that ran before: no tuning file"
    );
    let lines: Vec<_> = ChangeLedger::load_kind(&dir, ChangeKind::Model)
        .all()
        .iter()
        .filter(|c| c.id == learn::TUNING_ID)
        .cloned()
        .collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(
        (lines[0].op.as_str(), lines[0].origin.as_str()),
        ("create", "self_manage")
    );
    assert!(
        lines[0]
            .reason
            .starts_with("router tuned everyday chat: grok-4.6 first (12% cheaper"),
        "{}",
        lines[0].reason
    );
    assert!(
        lines[1].reason.starts_with(
            "router rolled back everyday chat: quality fell after it went live (80% vs 90% passed)"
        ),
        "{}",
        lines[1].reason
    );
    let c = &load_state(&dir).candidates[0];
    assert_eq!(
        (c.stage, c.until),
        (Stage::Rejected, Some(t3 + 14 * DAY_MS))
    );
    assert!(c.promoted_at.is_some(), "rolled back after it went live");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn undo_of_a_promotion_restores_the_tuning_file_byte_identical_and_the_tuner_backs_off() {
    let dir = test_dir("r3a-undo");
    let mut before = Tuning::default();
    before.starts.insert("plan".into(), "medium".into());
    write_tuning(&dir, &before, "an earlier change").unwrap();
    let bytes = std::fs::read(tuning_path(&dir)).unwrap();
    let mut c = order_candidate();
    c.stage = Stage::Canary;
    c.canary_at = Some(T0);
    seed(&dir, c);
    for i in 0..60u64 {
        log(
            &dir,
            T0 + 1 + i,
            rec("grok-4.7", "medium", &format!("l-{i}"), "ok", 0.0100, None),
        );
        log(
            &dir,
            T0 + 100 + i,
            rec(
                "grok-4.6",
                "medium",
                &format!("c-{i}"),
                "ok",
                0.0080,
                Some("canary:chat:default#1".into()),
            ),
        );
    }
    assert_eq!(tick(&dir, T0 + DAY_MS).promoted.len(), 1);
    assert_ne!(std::fs::read(tuning_path(&dir)).unwrap(), bytes);
    undo_change(
        &dir,
        &TuningFile {
            path: &tuning_path(&dir),
        },
        UndoAsk::from_click(),
    )
    .unwrap();
    assert_eq!(
        std::fs::read(tuning_path(&dir)).unwrap(),
        bytes,
        "byte-identical"
    );
    tick(&dir, T0 + 2 * DAY_MS);
    let c = &load_state(&dir).candidates[0];
    assert_eq!((c.stage, c.reason.as_str()), (Stage::Rejected, "undone"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_cost_raising_candidate_promotes_silently_on_its_canary_and_undo_puts_the_file_back() {
    let dir = test_dir("r3a-pricier");
    let pricier = Candidate {
        id: "chat:default#2".into(),
        class: "chat:default".into(),
        change: Some(Change::Start {
            effort: "high".into(),
        }),
        stage: Stage::Canary,
        created_at: T0,
        canary_at: Some(T0),
        cost_rise_pct: Some(30.0),
        ..Candidate::default()
    };
    seed(&dir, pricier);
    // Live 80% passed at $0.0100 a step; the canary 95% at $0.0130.
    for i in 0..400u64 {
        let pass = if i % 5 == 4 { "reject" } else { "ok" };
        log(
            &dir,
            T0 + 1 + i,
            rec(
                "grok-4.7",
                "medium",
                &format!("live-{i}"),
                pass,
                0.0100,
                None,
            ),
        );
    }
    for i in 0..60u64 {
        let pass = if i % 20 == 19 { "reject" } else { "ok" };
        log(
            &dir,
            T0 + 500 + i,
            rec(
                "grok-4.7",
                "high",
                &format!("can-{i}"),
                pass,
                0.0130,
                Some("canary:chat:default#2".into()),
            ),
        );
    }
    let run = tick(&dir, T0 + DAY_MS);
    assert_eq!(run.promoted, vec!["chat:default#2".to_string()]);
    assert_eq!(
        learn::load_tuning(&dir)
            .starts
            .get("chat:default")
            .map(String::as_str),
        Some("high")
    );
    assert_eq!(load_state(&dir).candidates[0].stage, Stage::Watch);
    let line = ChangeLedger::load_kind(&dir, ChangeKind::Model)
        .all()
        .last()
        .cloned()
        .unwrap();
    assert_eq!(
        (line.id.as_str(), line.reason.as_str()),
        (
            learn::TUNING_ID,
            "router tuned everyday chat: start at High (quality +15 pts for about 30% more cost)"
        )
    );
    undo_change(
        &dir,
        &TuningFile {
            path: &tuning_path(&dir),
        },
        UndoAsk::from_click(),
    )
    .unwrap();
    assert!(!tuning_path(&dir).exists());
    tick(&dir, T0 + 2 * DAY_MS);
    let c = &load_state(&dir).candidates[0];
    assert_eq!((c.stage, c.reason.as_str()), (Stage::Rejected, "undone"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn stubbed_outcome_or_verify_hooks_make_the_tuner_refuse_to_run() {
    let dir = test_dir("r3a-stub");
    seed(&dir, order_candidate());
    let real_v = SpanVerifySource {
        config_dir: dir.clone(),
        session: String::new(),
    };
    let real_o = LedgerOutcomeSource::new(&dir);
    assert_eq!(
        learn::tick(&dir, &StubSource, &real_o, T0),
        Err(NEEDS_DATA.to_string())
    );
    assert_eq!(
        learn::tick(&dir, &real_v, &StubSource, T0),
        Err(NEEDS_DATA.to_string())
    );
    assert!(!learn::scorecards_path(&dir).exists(), "nothing ran");
    assert!(learn::tick(&dir, &real_v, &real_o, T0).is_ok());
    assert!(learn::scorecards_path(&dir).exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn de2_reads_outcome_records_and_the_rung_each_run_used() {
    let dir = test_dir("r3a-de2");
    let now = T0;
    let sig = grokhub_core::outcome::topic_signature("summarize the march invoices");
    let mut records = Vec::new();
    for (i, days) in [1u64, 4, 9].iter().enumerate() {
        let ep = format!("de2-{i}");
        records.push((
            now - days * DAY_MS,
            rec("grok-4.7", "low", &ep, "ok", 0.0, None),
        ));
        let o = TaskOutcome {
            task: format!("{ep}:1"),
            signature: sig.clone(),
            result: OutcomeResult::Success,
            episode: ep.clone(),
            finished_at: now - days * DAY_MS,
            ..TaskOutcome::default()
        };
        append_outcome(&dir, &o).unwrap();
    }
    let outcomes = grokhub_core::outcome::read_outcomes(&dir);
    let band = Band::of(class_row("chat:default").unwrap(), 0);
    let r = learn::routine_from(&records, &outcomes, &sig, "chat:turn", now);
    assert_eq!(
        routine_rung("chat:default", &r, band, now),
        Some(rung("low").unwrap())
    );
    // Only two of them inside 14 days: no routine.
    let late = learn::routine_from(&records, &outcomes, &sig, "chat:default", now + 6 * DAY_MS);
    assert_eq!(
        routine_rung("chat:default", &late, band, now + 6 * DAY_MS),
        None
    );
    // Another signature: nothing.
    assert_eq!(
        learn::routine_from(&records, &outcomes, "topic:other", "chat:default", now),
        Default::default()
    );
    let _ = std::fs::remove_dir_all(dir);
}

// local model

fn meta(id: &str) -> ModelMeta {
    ModelMeta {
        id: id.into(),
        context_length: Some(256_000),
        prices: Prices {
            prompt: Some(20_000),
            cached: Some(5_000),
            completion: Some(100_000),
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

/// grok-4.7 from xAI plus both local tiers, as if the flag had been on once.
fn fleet() -> Registry {
    let mut reg = Registry::default();
    reg.entitlement.credential = Credential::Plan;
    let local = Listing::new(
        SourceKind::Local,
        local::TIERS.iter().map(|t| local::tier_meta(t)).collect(),
    );
    reg.apply_refresh(
        &[
            Listing::new(SourceKind::XaiApi, vec![meta("grok-4.7")]),
            local,
        ],
        &[],
        1,
    );
    reg
}

fn ask<'a>(class: &'a str, gate: LocalGate) -> RouteInput<'a> {
    RouteInput {
        provider: "xai",
        class,
        current_model: "grok-4.7",
        d: 0.5,
        local: gate,
        ..RouteInput::default()
    }
}

struct FakeRt;

impl LocalRuntime for FakeRt {
    fn classify(&self, text: &str, labels: &[&str]) -> Result<Value, String> {
        let label = if labels.contains(&"pii") {
            if text.contains("Alice") {
                "pii"
            } else {
                "clean"
            }
        } else {
            labels[0]
        };
        Ok(json!({ "label": label, "score": 0.9 }))
    }

    fn summarize(&self, _text: &str, _max_words: u32) -> Result<Value, String> {
        Ok(json!({ "summary": "two lines" }))
    }

    fn difficulty(&self, _text: &str) -> Result<Value, String> {
        Ok(json!({ "d": 0.42 }))
    }
}

#[test]
fn the_local_flag_is_off_by_default_and_no_route_picks_local_in_1000_random_inputs() {
    assert!(!local::available());
    assert!(
        LocalSource.fetch().is_err(),
        "the source lists nothing while off"
    );
    let mut fresh = Registry::default();
    fresh.apply_refresh(
        &[Listing::new(SourceKind::XaiApi, vec![meta("grok-4.7")])],
        &[],
        1,
    );
    assert!(
        !fresh.models.keys().any(|id| local::is_local(id)),
        "no local:* rows"
    );
    // Even with stale local rows in the registry, nothing routes there while the flag is off.
    let reg = fleet();
    assert_eq!(
        reg.models.keys().filter(|id| local::is_local(id)).count(),
        2
    );
    let classes: Vec<&str> = super::policy::CLASS_TABLE.iter().map(|r| r.class).collect();
    let mut x = 7u64;
    for _ in 0..1000 {
        x = x
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let class = classes[(x >> 33) as usize % classes.len()];
        let gate = LocalGate {
            enabled: local::available(),
            tier: local::live_tier().into(),
            sensitive: x & 1 == 1,
            cloud_grant: x & 2 == 2,
            hard_tool: x & 4 == 4,
            offline: false,
            offline_switch: false,
            local_verify_failed: false,
            capacity: local::LocalCapacity::now(),
        };
        let mut input = ask(class, gate);
        input.ctx_tokens = (x >> 40) % 8_000;
        let r = Router::choose(&input, &reg, &BTreeMap::new(), &RoutingTable::default(), 10);
        assert!(
            !local::is_local(&r.model) && r.provider != local::PROVIDER_LOCAL,
            "{class}: {:?}",
            r.rule_ids
        );
    }
    assert!(local::runtime().is_none(), "no runtime in this build");
    let input = DifficultyInput {
        text: "Debug why this deadlocks",
        ..DifficultyInput::default()
    };
    assert_eq!(
        estimate(&input, Some(&FakeRt)),
        difficulty(&input),
        "off: R1's rules unchanged"
    );
}

#[test]
fn with_the_flag_on_only_background_work_goes_local_and_sensitive_data_stays_on_the_device() {
    let reg = fleet();
    let _on = ForceOn::with(Some(Arc::new(FakeRt)));
    let profiles: BTreeMap<String, ModelProfile> = BTreeMap::new();
    let table = RoutingTable::default();
    let gate = || LocalGate::now(false, false, false);
    // Online with the cloud answering, background work stays in the cloud.
    assert_eq!(
        Router::choose(
            &ask("background:summarize", gate()),
            &reg,
            &profiles,
            &table,
            10
        )
        .model,
        "grok-4.7"
    );
    // With the cloud down, the local route takes it.
    let mut down = fleet();
    down.models.get_mut("grok-4.7").unwrap().state =
        grokhub_core::model_registry::ModelState::Quarantined;
    let r = Router::choose(
        &ask("background:summarize", gate()),
        &down,
        &profiles,
        &table,
        10,
    );
    assert_eq!(
        (
            r.provider.as_str(),
            r.model.as_str(),
            r.effort.clone(),
            r.cost_class
        ),
        ("local", "local:small", None, CostClass::Included)
    );
    // Never prepare:hard, never a user-facing class, never a step with a send/delete/credential tool, never past the context cap.
    for class in ["prepare:hard", "chat:default", "code:multi-file"] {
        assert_eq!(
            Router::choose(&ask(class, gate()), &reg, &profiles, &table, 10).model,
            "grok-4.7",
            "{class}"
        );
    }
    assert_eq!(
        Router::choose(
            &ask("background:summarize", LocalGate::now(false, false, true)),
            &reg,
            &profiles,
            &table,
            10
        )
        .model,
        "grok-4.7"
    );
    let mut big = ask("background:classify", gate());
    big.ctx_tokens = local::LOCAL_CTX_CAP + 1;
    assert_eq!(
        Router::choose(&big, &reg, &profiles, &table, 10).model,
        "grok-4.7"
    );
    // Sensitive with no cloud grant: local only; on a class local can't take, it pauses.
    let r = Router::choose(
        &ask("background:triage", LocalGate::now(true, false, false)),
        &reg,
        &profiles,
        &table,
        10,
    );
    assert_eq!(
        (r.model.as_str(), r.rule_ids[0].as_str()),
        ("local:small", "privacy:local_only")
    );
    let r = Router::choose(
        &ask("chat:default", LocalGate::now(true, false, false)),
        &reg,
        &profiles,
        &table,
        10,
    );
    assert!(
        r.no_route && r.rule_ids.contains(&"privacy:no_local".to_string()),
        "{:?}",
        r.rule_ids
    );
    let r = Router::choose(
        &ask("chat:default", LocalGate::now(true, true, false)),
        &reg,
        &profiles,
        &table,
        10,
    );
    assert_eq!(r.model, "grok-4.7", "a cloud grant lets it go out");
    // The downshift ladder only switches the live tier.
    local::set_device(Device {
        battery_low: true,
        low_end: false,
    });
    assert_eq!(
        Router::choose(
            &ask("background:summarize", gate()),
            &down,
            &profiles,
            &table,
            10
        )
        .model,
        "local:tiny"
    );
    local::set_device(Device::default());
    // The estimator hook reads the fake; PII is regex first, the model second.
    assert_eq!(
        estimate(
            &DifficultyInput {
                text: "x",
                ..DifficultyInput::default()
            },
            local::runtime().as_deref()
        ),
        0.42
    );
    let scan = local::scan_pii(
        "mail alice@example.com or call +1 555 010 9999, Alice said",
        local::runtime().as_deref(),
    );
    assert_eq!(
        (scan.redacted.as_str(), scan.regex_hits, scan.model_flag),
        (
            "mail [redacted] or call [redacted], Alice said",
            2,
            Some(true)
        )
    );
}

#[test]
fn the_settings_toggle_routes_local_only_while_a_runtime_is_installed() {
    let reg = fleet();
    let profiles: BTreeMap<String, ModelProfile> = BTreeMap::new();
    let table = RoutingTable::default();
    let pick = |class: &str| {
        Router::choose(
            &ask(class, LocalGate::now(false, false, false)),
            &reg,
            &profiles,
            &table,
            10,
        )
        .model
    };
    // Toggle on with nothing installed (this build): the route stays off and background work stays in the cloud.
    local::set_enabled(true);
    assert!(!local::installed() && !local::available());
    assert!(LocalSource.fetch().is_err());
    assert_eq!(pick("background:summarize"), "grok-4.7");
    // A runtime installed: online, the cloud still wins while it answers; local is the last resort.
    let rt = Installed::with(Arc::new(FakeRt));
    assert!(local::installed() && local::available());
    assert_eq!(LocalSource.fetch().map(|l| l.models.len()), Ok(2));
    assert_eq!(pick("background:summarize"), "grok-4.7");
    assert_eq!(pick("background:triage"), "grok-4.7");
    assert_eq!(pick("chat:default"), "grok-4.7");
    // Toggle off: back to the cloud on the next call.
    local::set_enabled(false);
    assert!(local::installed() && !local::available());
    assert_eq!(pick("background:summarize"), "grok-4.7");
    drop(rt);
    assert!(!local::installed());
}

#[test]
fn a_local_call_writes_no_egress_line() {
    let dir = test_dir("r3a-egress");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    // Online, local takes the step only with the cloud down.
    let mut reg = fleet();
    reg.models.get_mut("grok-4.7").unwrap().state =
        grokhub_core::model_registry::ModelState::Quarantined;
    grokhub_core::model_registry::store::save_registry(&dir, &reg).unwrap();
    let _on = ForceOn::with(Some(Arc::new(FakeRt)));
    let input = vec![crate::client::InputItem::Message {
        role: "user".into(),
        content: vec![crate::client::ContentPart::InputText("sort this".into())],
    }];
    let call = super::ModelCall::xai(
        "grok-4.7",
        Some("low"),
        "background:summarize",
        "ep-local",
        input,
    );
    struct Never;
    impl crate::client::ModelClient for Never {
        fn stream(
            &self,
            _r: &crate::client::ResponsesRequest,
            _c: &crate::CancelToken,
            _s: &mut dyn FnMut(crate::client::StreamEvent),
        ) -> Result<crate::client::TurnOutput, crate::client::ClientError> {
            panic!("a local route never reaches the cloud client")
        }
    }
    let routed = super::call_model(&Never, &call, &crate::CancelToken::new()).unwrap();
    assert_eq!(routed.out.text, r#"{"summary":"two lines"}"#);
    assert!(
        local::fallback_active(),
        "the cabin shows the local fallback"
    );
    assert!(
        read_egress(&dir).is_empty(),
        "0 egress lines from a local route"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cargo_lock_has_no_local_runtime_crate() {
    let lock = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("Cargo.lock");
    let text = std::fs::read_to_string(lock).unwrap();
    let names: Vec<&str> = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("name = \""))
        .map(|l| l.trim_end_matches('"'))
        .collect();
    assert!(names.len() > 100, "read the lock file");
    for banned in [
        "llama-cpp-2",
        "llama-cpp-sys-2",
        "ort",
        "ort-sys",
        "candle-core",
        "candle-nn",
        "candle-transformers",
    ] {
        assert!(!names.contains(&banned), "{banned} is in Cargo.lock");
    }
}

/// A gate at one governor level (the runtime is there; Unload pauses it).
fn gate_at(level: grokhub_core::local_governor::Level, sensitive: bool) -> LocalGate {
    let capacity = local::LocalCapacity { ready: true, level };
    LocalGate {
        enabled: capacity.available(),
        tier: "local:small".into(),
        sensitive,
        offline: false,
        capacity,
        ..LocalGate::default()
    }
}

#[test]
fn online_background_work_goes_local_only_when_the_cloud_is_down() {
    use grokhub_core::local_governor::Level;
    use grokhub_core::model_registry::ModelState;
    let profiles: BTreeMap<String, ModelProfile> = BTreeMap::new();
    let table = RoutingTable::default();
    let up = fleet();
    let pick = |reg: &Registry, class: &str, gate: LocalGate, ctx: u64| {
        let mut input = ask(class, gate);
        input.ctx_tokens = ctx;
        let r = Router::choose(&input, reg, &profiles, &table, 10);
        let rule = r
            .rule_ids
            .iter()
            .rev()
            .find(|x| x.starts_with("local:"))
            .cloned()
            .unwrap_or_default();
        (r.model, r.no_route, rule)
    };
    let s = |m: &str, no: bool, rule: &str| (m.to_string(), no, rule.to_string());
    let small = 500;
    let heavy = local::ctx_cap() / 2 + 1;
    // (level, light step, heavy step, light step with the cloud down): online the
    // cloud takes every step while it answers, at any governor level.
    let mut down = fleet();
    down.models.get_mut("grok-4.7").unwrap().state = ModelState::Quarantined;
    for (level, light, big, outage) in [
        (
            Level::RampUp,
            s("grok-4.7", false, ""),
            s("grok-4.7", false, ""),
            s("local:small", false, "local:cloud_down"),
        ),
        (
            Level::Normal,
            s("grok-4.7", false, ""),
            s("grok-4.7", false, ""),
            s("local:small", false, "local:cloud_down"),
        ),
        (
            Level::Throttle,
            s("grok-4.7", false, ""),
            s("grok-4.7", false, ""),
            s("local:small", false, "local:cloud_down"),
        ),
        (
            Level::Unload,
            s("grok-4.7", false, ""),
            s("grok-4.7", false, ""),
            s("grok-4.7", true, "local:paused"),
        ),
    ] {
        let g = || gate_at(level, false);
        assert_eq!(
            pick(&up, "background:summarize", g(), small),
            light,
            "{level:?}"
        );
        assert_eq!(
            pick(&up, "background:summarize", g(), heavy),
            big,
            "{level:?}"
        );
        assert_eq!(
            pick(&down, "background:summarize", g(), small),
            outage,
            "{level:?}"
        );
        // Grok stays primary for user-facing work at every level.
        assert_eq!(
            pick(&up, "chat:default", g(), small),
            s("grok-4.7", false, "")
        );
    }
    // Sensitive data with no cloud grant: the device, or a named pause.
    assert_eq!(
        pick(
            &up,
            "background:triage",
            gate_at(Level::Throttle, true),
            small
        )
        .0,
        "local:small"
    );
    let mut input = ask("background:triage", gate_at(Level::Unload, true));
    input.ctx_tokens = small;
    let r = Router::choose(&input, &up, &profiles, &table, 10);
    assert!(
        r.no_route && r.rule_ids.contains(&"local:paused".to_string()),
        "{:?}",
        r.rule_ids
    );
    assert_eq!(r.reason, "Local paused: game or low power");
}

#[test]
fn a_heavy_background_step_waits_for_ramp_up_and_a_pause_refuses_it() {
    use crate::client::{CancelToken, ClientError};
    use grokhub_core::local_governor::Level;
    use std::time::Duration;
    let _on = ForceOn::with(Some(Arc::new(FakeRt)));
    super::local_gov::reset_for_test();
    let go = CancelToken::new();
    let long = "word ".repeat(2_000);
    // Light work never waits.
    assert!(local::serve_within("background:summarize", "short", Duration::ZERO, &go).is_ok());
    // Heavy work at Normal waits for RampUp; a level change wakes it.
    let waiter = std::thread::spawn({
        let long = long.clone();
        move || {
            let _on = ForceOn::with(Some(Arc::new(FakeRt)));
            let go = CancelToken::new();
            local::serve_within("background:summarize", &long, Duration::from_secs(30), &go)
                .map(|o| o.text)
        }
    });
    while super::local_gov::queued() == 0 {
        std::thread::yield_now();
    }
    assert_eq!(super::local_gov::queued(), 1);
    super::local_gov::apply(Level::RampUp, "idle on AC power");
    assert_eq!(
        waiter.join().unwrap(),
        Ok(r#"{"summary":"two lines"}"#.to_string())
    );
    assert_eq!(super::local_gov::queued(), 0);
    // Stop ends the wait at once, with no level change.
    super::local_gov::apply(Level::Normal, "normal use");
    let stop = CancelToken::new();
    let stopped = std::thread::spawn({
        let long = long.clone();
        let stop = stop.clone();
        move || {
            let _on = ForceOn::with(Some(Arc::new(FakeRt)));
            local::serve_within(
                "background:summarize",
                &long,
                Duration::from_secs(600),
                &stop,
            )
            .map(|o| o.text)
        }
    });
    while super::local_gov::queued() == 0 {
        std::thread::yield_now();
    }
    let asked = std::time::Instant::now();
    stop.cancel();
    assert!(matches!(
        stopped.join().unwrap(),
        Err(ClientError::Cancelled)
    ));
    assert!(
        asked.elapsed() < Duration::from_secs(5),
        "{:?}",
        asked.elapsed()
    );
    assert_eq!(super::local_gov::queued(), 0);
    assert_eq!(super::local_gov::level(), Level::Normal);
    // Out of time at Normal it runs anyway; at Unload it's refused.
    assert!(local::serve_within("background:summarize", &long, Duration::ZERO, &go).is_ok());
    super::local_gov::apply(Level::Unload, "a game is running");
    assert_eq!(
        local::serve_within("background:summarize", &long, Duration::ZERO, &go)
            .err()
            .map(|e| e.to_string()),
        Some(ClientError::Protocol(local::LOCAL_PAUSED_MSG.into()).to_string())
    );
    super::local_gov::reset_for_test();
}

#[test]
fn a_tick_reads_the_chat_metrics_as_an_input() {
    use super::session_metrics::{note_call, CallFacts, SessionSummary};
    let dir = test_dir("r3a-sessions");
    let up = vec!["model:stronger".to_string()];
    let keep = vec!["episode:keep".to_string()];
    let facts = |model, rule_ids, cached, ok| CallFacts {
        model,
        rule_ids,
        prompt_tokens: 1_000,
        cached_tokens: cached,
        ok,
    };
    note_call(&dir, "ep-1", &facts("grok-4.3", &keep, 0, true), T0);
    note_call(&dir, "ep-1", &facts("grok-4.7", &up, 500, true), T0 + 1);
    note_call(&dir, "ep-2", &facts("grok-4.7", &up, 1_000, false), T0 + 2);
    note_call(&dir, "ep-2", &facts("grok-4.7", &keep, 500, true), T0 + 3);
    let run = tick(&dir, T0 + DAY_MS);
    assert_eq!(
        run.sessions,
        SessionSummary {
            chats: 2,
            calls: 4,
            cache_hit_pct: 50.0,
            switches: 1,
            switches_per_chat: 0.5,
            escalations: 2,
            escalation_pct: 50.0,
            precision_pct: 50.0,
        }
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A real decision state behind the tuner's gate, with nothing going on.
struct Engine(std::cell::RefCell<decide::State>);

impl Trust for Engine {
    fn weigh(&self, p: &decide::Proposal, now_ms: u64) -> Option<decide::Decision> {
        Some(
            self.0
                .borrow_mut()
                .decide(p, decide::Situation::default(), now_ms),
        )
    }

    fn outcome(
        &self,
        rule: &grokhub_core::learn::Subject,
        outcome: decide::Outcome,
        now_ms: u64,
    ) -> Option<decide::Alarm> {
        self.0.borrow_mut().outcome(rule, outcome, now_ms)
    }
}

fn tick_under(dir: &std::path::Path, now: u64, engine: &Engine) -> learn::TuneRun {
    let verify = SpanVerifySource {
        config_dir: dir.to_path_buf(),
        session: String::new(),
    };
    learn::tick_under(dir, &verify, &LedgerOutcomeSource::new(dir), now, engine).unwrap()
}

/// Candidate `n` in its canary since `T0`, with 60 canary steps 20% cheaper
/// than live and every step passing.
fn canary_ready(dir: &std::path::Path, n: u32) {
    let mut c = order_candidate();
    c.id = format!("chat:default#{n}");
    c.stage = Stage::Canary;
    c.canary_at = Some(T0);
    seed(dir, c);
    for i in 0..60u64 {
        log(
            dir,
            T0 + 1 + i,
            rec(
                "grok-4.7",
                "medium",
                &format!("l{n}-{i}"),
                "ok",
                0.0100,
                None,
            ),
        );
        log(
            dir,
            T0 + 100 + i,
            rec(
                "grok-4.6",
                "medium",
                &format!("c{n}-{i}"),
                "ok",
                0.0080,
                Some(format!("canary:chat:default#{n}")),
            ),
        );
    }
}

#[test]
fn the_trust_dial_holds_a_promotion_and_two_undos_turn_that_kind_of_change_off() {
    let dir = test_dir("r3a-trust");
    canary_ready(&dir, 1);
    let engine = Engine(Default::default());
    let rule = grokhub_core::learn::Subject::parse("rule:router/chat:default/order").unwrap();
    let last = |e: &Engine| {
        let s = e.0.borrow();
        let d = s.log().last().unwrap();
        (d.rule.clone(), d.area, d.verdict, d.reason)
    };

    // Offer only: the canary passed, and the change is not made.
    engine
        .0
        .borrow_mut()
        .set_dial(decide::Area::Router, decide::Dial::Offer);
    let t1 = T0 + DAY_MS;
    let run = tick_under(&dir, t1, &engine);
    assert_eq!(
        (run.held, run.promoted),
        (vec!["chat:default#1".to_string()], vec![])
    );
    assert!(!tuning_path(&dir).exists());
    let c = &load_state(&dir).candidates[0];
    assert_eq!(
        (c.stage, c.reason.as_str()),
        (Stage::Canary, "waiting: This area is set to offer only.")
    );
    assert_eq!(
        last(&engine),
        (
            rule.clone(),
            decide::Area::Router,
            decide::Verdict::Offer,
            decide::Reason::DialOffer
        )
    );

    // Back to act: the same candidate goes live with no yes answers, since
    // the canary was its trial. It spends one of the day's five changes.
    engine
        .0
        .borrow_mut()
        .set_dial(decide::Area::Router, decide::Dial::Act);
    let run = tick_under(&dir, t1 + 1, &engine);
    assert_eq!(
        (run.held, run.promoted),
        (vec![], vec!["chat:default#1".to_string()])
    );
    let c = &load_state(&dir).candidates[0];
    assert_eq!((c.stage, c.reason.as_str()), (Stage::Watch, ""));
    assert_eq!(
        last(&engine),
        (
            rule.clone(),
            decide::Area::Router,
            decide::Verdict::Act,
            decide::Reason::Sure
        )
    );
    assert_eq!(engine.0.borrow().budget_left(t1 + 1), 4);

    // The user undoes it: the rule hears one undo, and the area's score
    // has one outcome, a bad one.
    let undo = || {
        undo_change(
            &dir,
            &TuningFile {
                path: &tuning_path(&dir),
            },
            UndoAsk::from_click(),
        )
        .unwrap();
    };
    undo();
    let t2 = t1 + 2 * DAY_MS;
    tick_under(&dir, t2, &engine);
    assert_eq!(
        engine
            .0
            .borrow()
            .score()
            .score(decide::Area::Router, t2)
            .now,
        Some(0)
    );

    // A second candidate of the same kind for the same class is promoted
    // and undone too. A third is refused before it changes anything.
    canary_ready(&dir, 2);
    let t3 = t2 + 2 * DAY_MS;
    assert_eq!(
        tick_under(&dir, t3, &engine).promoted,
        vec!["chat:default#2".to_string()]
    );
    undo();
    let t4 = t3 + 2 * DAY_MS;
    tick_under(&dir, t4, &engine);
    // The undo of the first change to the file takes the file away again.
    assert!(!tuning_path(&dir).exists());
    canary_ready(&dir, 3);
    let run = tick_under(&dir, t4 + 2 * DAY_MS, &engine);
    assert_eq!(
        (run.rejected, run.promoted),
        (vec!["chat:default#3".to_string()], vec![])
    );
    let c = &load_state(&dir).candidates[0];
    assert_eq!(
        (c.stage, c.reason.as_str()),
        (Stage::Rejected, "You undid this twice, so it stopped.")
    );
    assert!(!tuning_path(&dir).exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_held_promotion_goes_live_on_the_users_yes_and_ends_on_a_no() {
    let dir = test_dir("r3a-say-yes");
    canary_ready(&dir, 1);
    let engine = Engine(Default::default());
    let rule = grokhub_core::learn::Subject::parse("rule:router/chat:default/order").unwrap();
    engine
        .0
        .borrow_mut()
        .set_dial(decide::Area::Router, decide::Dial::Offer);
    let t1 = T0 + DAY_MS;
    assert_eq!(
        tick_under(&dir, t1, &engine).held,
        vec!["chat:default#1".to_string()]
    );
    assert_eq!(
        engine.0.borrow().asks(t1),
        vec![(rule.clone(), decide::Reason::DialOffer)]
    );

    // Yes: the next pass makes the change, with the dial still on offer.
    assert!(engine.0.borrow_mut().answer(&rule, true, t1 + 1));
    let run = tick_under(&dir, t1 + 2, &engine);
    assert_eq!(
        (run.held, run.promoted),
        (vec![], vec!["chat:default#1".to_string()])
    );
    assert!(tuning_path(&dir).exists());
    assert_eq!(load_state(&dir).candidates[0].stage, Stage::Watch);
    {
        let s = engine.0.borrow();
        let d = s.log().last().unwrap();
        assert_eq!(
            (d.verdict, d.reason),
            (decide::Verdict::Act, decide::Reason::YouSaidYes)
        );
        assert_eq!(s.budget_left(t1 + 2), decide::ACTS_PER_DAY);
    }

    // A second one of the same kind waits again. No: it ends, and the
    // tuning file is as the first change left it.
    let live = std::fs::read(tuning_path(&dir)).unwrap();
    canary_ready(&dir, 2);
    let t2 = t1 + DAY_MS;
    assert_eq!(
        tick_under(&dir, t2, &engine).held,
        vec!["chat:default#2".to_string()]
    );
    assert!(engine.0.borrow_mut().answer(&rule, false, t2 + 1));
    let run = tick_under(&dir, t2 + 2, &engine);
    assert_eq!(
        (run.rejected, run.promoted, run.held),
        (vec!["chat:default#2".to_string()], vec![], vec![])
    );
    let state = load_state(&dir);
    let c = state
        .candidates
        .iter()
        .find(|c| c.id == "chat:default#2")
        .unwrap();
    assert_eq!(
        (c.stage, c.reason.as_str()),
        (Stage::Rejected, "You said no to this, so it waits a month.")
    );
    assert_eq!(std::fs::read(tuning_path(&dir)).unwrap(), live);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_promotion_is_weighed_by_what_it_gains_and_which_goal_it_serves() {
    use super::tune::{Evidence, Snap};
    let ev = Evidence {
        canary: Snap {
            n: 50,
            pass_pct: 92.0,
            ..Snap::default()
        },
        live: Snap {
            n: 400,
            pass_pct: 82.0,
            ..Snap::default()
        },
        ..Evidence::default()
    };
    let cheaper = trust::promotion(&order_candidate(), &ev, 20.0, 12.0).unwrap();
    assert_eq!(
        cheaper,
        decide::Proposal {
            area: decide::Area::Router,
            rule: grokhub_core::learn::Subject::parse("rule:router/chat:default/order").unwrap(),
            confidence: 1.0,
            evidence: 50.0,
            undo: grokhub_core::learn::Undo::OneClick,
            value: 0.2,
            hard: false,
            helps: Some(decide::Goal::Cheapest),
            hurts: None,
            tested: true,
        }
    );
    let faster = trust::promotion(&order_candidate(), &ev, 5.0, 30.0).unwrap();
    assert_eq!(
        (faster.value, faster.helps, faster.hurts),
        (0.3, Some(decide::Goal::Fastest), None)
    );
    // A pricier model is worth the quality it adds, and works against cost.
    let pricier = Candidate {
        cost_rise_pct: Some(40.0),
        change: Some(Change::Start {
            effort: "high".into(),
        }),
        ..order_candidate()
    };
    let p = trust::promotion(&pricier, &ev, -40.0, 0.0).unwrap();
    assert_eq!(
        (p.rule.as_str(), p.value, p.helps, p.hurts),
        (
            "rule:router/chat:default/start",
            0.1,
            Some(decide::Goal::BestQuality),
            Some(decide::Goal::Cheapest)
        )
    );
    // A candidate with no change has nothing to weigh.
    let empty = Candidate {
        change: None,
        ..order_candidate()
    };
    assert_eq!(trust::promotion(&empty, &ev, 20.0, 12.0), None);
}

#[test]
fn the_scoreboard_hears_a_rollback_and_a_change_that_was_kept() {
    let dir = test_dir("r3a-trust-score");
    let engine = Engine(Default::default());
    let score = |at: u64| {
        engine
            .0
            .borrow()
            .score()
            .score(decide::Area::Router, at)
            .now
    };
    // Promoted, then one step in five fails where every step passed before:
    // the tuner takes it back itself. That is a bad outcome for the area.
    canary_ready(&dir, 1);
    let t1 = T0 + DAY_MS;
    assert_eq!(tick_under(&dir, t1, &engine).promoted.len(), 1);
    assert_eq!(score(t1), None);
    for i in 0..30u64 {
        let pass = if i % 5 == 4 { "reject" } else { "ok" };
        log(
            &dir,
            t1 + 1 + i,
            rec("grok-4.6", "medium", &format!("p1-{i}"), pass, 0.0080, None),
        );
    }
    let t2 = t1 + DAY_MS;
    assert_eq!(
        tick_under(&dir, t2, &engine).rolled_back,
        vec!["chat:default#1".to_string()]
    );
    assert_eq!(score(t2), Some(0));
    // It is not the user's undo: nothing is paused and the rule still acts.
    assert!(!engine.0.borrow().paused(t2));
    canary_ready(&dir, 2);
    let t3 = t2 + DAY_MS;
    assert_eq!(
        tick_under(&dir, t3, &engine).promoted,
        vec!["chat:default#2".to_string()]
    );
    // A week on with nothing wrong, the change is kept: one good outcome.
    let t4 = t3 + 7 * DAY_MS;
    tick_under(&dir, t4, &engine);
    assert_eq!(load_state(&dir).candidates[0].stage, Stage::Kept);
    assert_eq!(score(t4), Some(50));
    // Kept is told once, not on every later pass.
    tick_under(&dir, t4 + DAY_MS, &engine);
    assert_eq!(score(t4 + DAY_MS), Some(50));
    let _ = std::fs::remove_dir_all(dir);
}
