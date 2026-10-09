//! Router R3a acceptance on real files: DE2 from outcome records, one
//! fixture run of shadow → canary → promote → rollback through the
//! ChangeLedger, a cost-raising card that changes nothing until you accept
//! it, Undo byte-identical, the stub refusal, and the local model: off by
//! default, and with it forced on (fake runtime) only for background work.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{json, Value};

use grokhub_core::model_registry::cost_class::CostClass;
use grokhub_core::model_registry::profile::ModelProfile;
use grokhub_core::model_registry::{CatalogSource, Credential, Listing, ModelMeta, Prices, Registry, SourceKind};
use grokhub_core::outcome::{append_outcome, OutcomeResult, TaskOutcome};

use super::difficulty::{difficulty, estimate, DifficultyInput};
use super::ladder::{routine_rung, rung, Band};
use super::learn::{self, load_state, tuning_path, write_tuning, TuningFile, NEEDS_DATA};
use super::local::{self, Device, ForceOn, Installed, LocalGate, LocalRuntime, LocalSource};
use super::log::{Chosen, RouteOutcome, RouteRecord, RouteSignals, ROUTE_TRACE};
use super::policy::class_row;
use super::signals::{LedgerOutcomeSource, SpanVerifySource, StubSource};
use super::table::RoutingTable;
use super::tune::{Candidate, Change, Stage, TuneState, Tuning, DAY_MS};
use super::{RouteInput, Router};
use crate::harness::{append_span, read_egress, test_dir, undo_change, AccessMode, ChangeKind, ChangeLedger, Span, UndoAsk};

const T0: u64 = 2_000 * DAY_MS;

fn rec(model: &str, effort: &str, episode: &str, verify: &str, cost: f64, tune: Option<String>) -> RouteRecord {
    RouteRecord {
        episode: episode.into(),
        class: "chat:default".into(),
        used: Chosen { provider: "xai".into(), model: model.into(), effort: Some(effort.into()) },
        outcome: Some(RouteOutcome { ok: true, verify: Some(verify.into()), cost_usd: cost, ..RouteOutcome::default() }),
        signals: RouteSignals { latency: Some(2_000), ..RouteSignals::default() },
        tune,
        ..RouteRecord::default()
    }
}

fn log(dir: &std::path::Path, at: u64, r: RouteRecord) {
    let mut span = Span::soft_allow(ROUTE_TRACE, "model.call", "{}", "ok", "", AccessMode::Supervised, "xai");
    span.ts_ms = at;
    span.route = Some(Box::new(r));
    append_span(dir, &span).unwrap();
}

fn tick(dir: &std::path::Path, now: u64) -> learn::TuneRun {
    let verify = SpanVerifySource { config_dir: dir.to_path_buf(), session: String::new() };
    learn::tick(dir, &verify, &LedgerOutcomeSource::new(dir), now).unwrap()
}

fn seed(dir: &std::path::Path, c: Candidate) {
    let state = TuneState { candidates: vec![c], built_at: T0, inputs_hash: format!("{}:{}", Registry::default().hash, super::table::profiles_hash(&BTreeMap::new())), seq: 1, ..TuneState::default() };
    grokhub_core::model_registry::store::write_json(&learn::state_path(dir), &state).unwrap();
}

/// grok-4.7 is live for everyday chat; grok-4.6 is the candidate.
fn order_candidate() -> Candidate {
    Candidate { id: "chat:default#1".into(), class: "chat:default".into(), change: Some(Change::Order { model: "grok-4.6".into() }), stage: Stage::Shadow, created_at: T0, ..Candidate::default() }
}

#[test]
fn shadow_then_canary_then_promote_then_rollback_with_both_ledger_lines_and_undo_byte_identical() {
    let dir = test_dir("r3a-lifecycle");
    seed(&dir, order_candidate());
    // Shadow: 199 eligible steps are not enough, 200 are.
    for i in 0..200u64 {
        let ep = format!("sh-{i}");
        log(&dir, T0 + 1 + i, rec("grok-4.7", "medium", &ep, "ok", 0.0100, Some("shadow:chat:default#1".into())));
        if i == 198 {
            assert!(tick(&dir, T0 + 300).canaries.is_empty());
        }
    }
    let t1 = T0 + DAY_MS;
    assert_eq!(tick(&dir, t1).canaries, vec!["chat:default#1".to_string()]);
    // Canary: 50 steps at 90% passed and $0.0088 a step vs live 90% at $0.0100 (12% cheaper).
    for i in 0..400u64 {
        let pass = if i % 10 == 9 { "reject" } else { "ok" };
        log(&dir, t1 + 1 + i, rec("grok-4.7", "medium", &format!("live-{i}"), pass, 0.0100, None));
    }
    for i in 0..50u64 {
        let pass = if i % 10 == 9 { "reject" } else { "ok" };
        log(&dir, t1 + 500 + i, rec("grok-4.6", "medium", &format!("can-{i}"), pass, 0.0088, Some("canary:chat:default#1".into())));
    }
    let t2 = t1 + DAY_MS;
    let run = tick(&dir, t2);
    assert_eq!(run.promoted, vec!["chat:default#1".to_string()]);
    let promoted = std::fs::read_to_string(tuning_path(&dir)).unwrap();
    assert!(promoted.contains("\"chat:default\": \"grok-4.6\""), "{promoted}");
    assert_eq!(learn::review_line(&dir, t2), "Router: 1 change kept (everyday chat: \u{2212}12% cost, quality same), 0 rolled back.");
    let why = learn::why_table_lines(&dir, t2).join("\n");
    assert!(why.contains("chat:default → grok-4.6 first") && why.contains("candidate chat:default: grok-4.6 first (live, watched 7 days)"), "{why}");
    // After promotion: 30 steps at 80% passed (10 points down) roll it back at once.
    for i in 0..30u64 {
        let pass = if i % 5 == 4 { "reject" } else { "ok" };
        log(&dir, t2 + 1 + i, rec("grok-4.6", "medium", &format!("post-{i}"), pass, 0.0088, None));
    }
    let t3 = t2 + DAY_MS;
    let run = tick(&dir, t3);
    assert_eq!(run.rolled_back, vec!["chat:default#1".to_string()]);
    assert!(!tuning_path(&dir).exists(), "back to the state that ran before: no tuning file");
    let lines: Vec<_> = ChangeLedger::load_kind(&dir, ChangeKind::Model).all().iter().filter(|c| c.id == learn::TUNING_ID).cloned().collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!((lines[0].op.as_str(), lines[0].origin.as_str()), ("create", "self_manage"));
    assert!(lines[0].reason.starts_with("router tuned everyday chat: grok-4.6 first (12% cheaper"), "{}", lines[0].reason);
    assert!(lines[1].reason.starts_with("router rolled back everyday chat: quality fell after it went live (80% vs 90% passed)"), "{}", lines[1].reason);
    let c = &load_state(&dir).candidates[0];
    assert_eq!((c.stage, c.until), (Stage::Rejected, Some(t3 + 14 * DAY_MS)));
    assert_eq!(learn::review_line(&dir, t3), "Router: 0 changes kept, 1 rolled back.");
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
        log(&dir, T0 + 1 + i, rec("grok-4.7", "medium", &format!("l-{i}"), "ok", 0.0100, None));
        log(&dir, T0 + 100 + i, rec("grok-4.6", "medium", &format!("c-{i}"), "ok", 0.0080, Some("canary:chat:default#1".into())));
    }
    assert_eq!(tick(&dir, T0 + DAY_MS).promoted.len(), 1);
    assert_ne!(std::fs::read(tuning_path(&dir)).unwrap(), bytes);
    undo_change(&dir, &TuningFile { path: &tuning_path(&dir) }, UndoAsk::from_click()).unwrap();
    assert_eq!(std::fs::read(tuning_path(&dir)).unwrap(), bytes, "byte-identical");
    tick(&dir, T0 + 2 * DAY_MS);
    let c = &load_state(&dir).candidates[0];
    assert_eq!((c.stage, c.reason.as_str()), (Stage::Rejected, "undone"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_cost_raising_card_changes_nothing_until_accepted_and_undo_puts_the_file_back() {
    let dir = test_dir("r3a-card");
    let card = Candidate {
        id: "chat:default#2".into(),
        class: "chat:default".into(),
        change: Some(Change::Start { effort: "high".into() }),
        stage: Stage::Card,
        created_at: T0,
        cost_rise_pct: Some(30.0),
        ..Candidate::default()
    };
    seed(&dir, card);
    let due = learn::cards_due(&dir, T0);
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].0, "self-review:router:chat:default#2");
    assert_eq!(due[0].2, "everyday chat would do better at High, which costs about 30% more. Start it at High?");
    // No click: a tick never applies it.
    tick(&dir, T0 + DAY_MS);
    assert!(!tuning_path(&dir).exists());
    assert_eq!(load_state(&dir).candidates[0].stage, Stage::Card);
    // Two cards a week at most.
    learn::cards_posted(&dir, 2, T0);
    assert!(learn::cards_due(&dir, T0 + DAY_MS).is_empty());
    assert_eq!(learn::cards_due(&dir, T0 + 8 * DAY_MS).len(), 1);
    // Accept writes through the ledger; Undo removes the file again (it was not there).
    assert_eq!(learn::accept_card(&dir, "chat:default#2", T0).unwrap(), "Router changed (everyday chat: start at High). Undo is on the change list.");
    assert_eq!(learn::load_tuning(&dir).starts.get("chat:default").map(String::as_str), Some("high"));
    let line = ChangeLedger::load_kind(&dir, ChangeKind::Model).all().last().cloned().unwrap();
    assert_eq!((line.id.as_str(), line.reason.as_str()), (learn::TUNING_ID, "you accepted: everyday chat: start at High"));
    undo_change(&dir, &TuningFile { path: &tuning_path(&dir) }, UndoAsk::from_click()).unwrap();
    assert!(!tuning_path(&dir).exists());
    assert!(learn::accept_card(&dir, "chat:default#2", T0).is_err(), "a used card can't apply twice");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn stubbed_outcome_or_verify_hooks_make_the_tuner_refuse_to_run() {
    let dir = test_dir("r3a-stub");
    seed(&dir, order_candidate());
    let real_v = SpanVerifySource { config_dir: dir.clone(), session: String::new() };
    let real_o = LedgerOutcomeSource::new(&dir);
    assert_eq!(learn::tick(&dir, &StubSource, &real_o, T0), Err(NEEDS_DATA.to_string()));
    assert_eq!(learn::tick(&dir, &real_v, &StubSource, T0), Err(NEEDS_DATA.to_string()));
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
        records.push((now - days * DAY_MS, rec("grok-4.7", "low", &ep, "ok", 0.0, None)));
        let o = TaskOutcome { task: format!("{ep}:1"), signature: sig.clone(), result: OutcomeResult::Success, episode: ep.clone(), finished_at: now - days * DAY_MS, ..TaskOutcome::default() };
        append_outcome(&dir, &o).unwrap();
    }
    let outcomes = grokhub_core::outcome::read_outcomes(&dir);
    let band = Band::of(class_row("chat:default").unwrap(), 0);
    let r = learn::routine_from(&records, &outcomes, &sig, "chat:turn", now);
    assert_eq!(routine_rung("chat:default", &r, band, now), Some(rung("low").unwrap()));
    // Only two of them inside 14 days: no routine.
    let late = learn::routine_from(&records, &outcomes, &sig, "chat:default", now + 6 * DAY_MS);
    assert_eq!(routine_rung("chat:default", &late, band, now + 6 * DAY_MS), None);
    // Another signature: nothing.
    assert_eq!(learn::routine_from(&records, &outcomes, "topic:other", "chat:default", now), Default::default());
    let _ = std::fs::remove_dir_all(dir);
}

// local model

fn meta(id: &str) -> ModelMeta {
    ModelMeta {
        id: id.into(),
        context_length: Some(256_000),
        prices: Prices { prompt: Some(20_000), cached: Some(5_000), completion: Some(100_000), ..Prices::default() },
        efforts: Some(vec!["low".into(), "medium".into(), "high".into(), "xhigh".into()]),
        tool_calling: Some(true),
        api_shape: Some("responses".into()),
        ..ModelMeta::default()
    }
}

/// grok-4.7 from xAI plus both local tiers, as if the flag had been on once.
fn fleet() -> Registry {
    let mut reg = Registry::default();
    reg.entitlement.credential = Credential::Plan;
    let local = Listing::new(SourceKind::Local, local::TIERS.iter().map(|t| local::tier_meta(t)).collect());
    reg.apply_refresh(&[Listing::new(SourceKind::XaiApi, vec![meta("grok-4.7")]), local], &[], 1);
    reg
}

fn ask<'a>(class: &'a str, gate: LocalGate) -> RouteInput<'a> {
    RouteInput { provider: "xai", class, current_model: "grok-4.7", d: 0.5, local: gate, ..RouteInput::default() }
}

struct FakeRt;

impl LocalRuntime for FakeRt {
    fn classify(&self, text: &str, labels: &[&str]) -> Result<Value, String> {
        let label = if labels.contains(&"pii") { if text.contains("Alice") { "pii" } else { "clean" } } else { labels[0] };
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
    assert!(!local::enabled());
    assert!(LocalSource.fetch().is_err(), "the source lists nothing while off");
    let mut fresh = Registry::default();
    fresh.apply_refresh(&[Listing::new(SourceKind::XaiApi, vec![meta("grok-4.7")])], &[], 1);
    assert!(!fresh.models.keys().any(|id| local::is_local(id)), "no local:* rows");
    // Even with stale local rows in the registry, nothing routes there while the flag is off.
    let reg = fleet();
    assert_eq!(reg.models.keys().filter(|id| local::is_local(id)).count(), 2);
    let classes: Vec<&str> = super::policy::CLASS_TABLE.iter().map(|r| r.class).collect();
    let mut x = 7u64;
    for _ in 0..1000 {
        x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let class = classes[(x >> 33) as usize % classes.len()];
        let gate = LocalGate { enabled: local::enabled(), tier: local::live_tier().into(), sensitive: x & 1 == 1, cloud_grant: x & 2 == 2, hard_tool: x & 4 == 4 };
        let mut input = ask(class, gate);
        input.ctx_tokens = (x >> 40) % 8_000;
        let r = Router::choose(&input, &reg, &BTreeMap::new(), &RoutingTable::default(), 10);
        assert!(!local::is_local(&r.model) && r.provider != local::PROVIDER_LOCAL, "{class}: {:?}", r.rule_ids);
    }
    assert!(local::runtime().is_none(), "no runtime in this build");
    let input = DifficultyInput { text: "Debug why this deadlocks", ..DifficultyInput::default() };
    assert_eq!(estimate(&input, Some(&FakeRt)), difficulty(&input), "off: R1's rules unchanged");
}

#[test]
fn with_the_flag_on_only_background_work_goes_local_and_sensitive_data_stays_on_the_device() {
    let reg = fleet();
    let _on = ForceOn::with(Some(Arc::new(FakeRt)));
    let profiles: BTreeMap<String, ModelProfile> = BTreeMap::new();
    let table = RoutingTable::default();
    let gate = || LocalGate::now(false, false, false);
    let r = Router::choose(&ask("background:summarize", gate()), &reg, &profiles, &table, 10);
    assert_eq!((r.provider.as_str(), r.model.as_str(), r.effort.clone(), r.cost_class), ("local", "local:small", None, CostClass::Included));
    // Never prepare:hard, never a user-facing class, never a step with a send/delete/credential tool, never past the context cap.
    for class in ["prepare:hard", "chat:default", "code:multi-file"] {
        assert_eq!(Router::choose(&ask(class, gate()), &reg, &profiles, &table, 10).model, "grok-4.7", "{class}");
    }
    assert_eq!(Router::choose(&ask("background:summarize", LocalGate::now(false, false, true)), &reg, &profiles, &table, 10).model, "grok-4.7");
    let mut big = ask("background:classify", gate());
    big.ctx_tokens = local::LOCAL_CTX_CAP + 1;
    assert_eq!(Router::choose(&big, &reg, &profiles, &table, 10).model, "grok-4.7");
    // Sensitive with no cloud grant: local only; on a class local can't take, it pauses.
    let r = Router::choose(&ask("background:triage", LocalGate::now(true, false, false)), &reg, &profiles, &table, 10);
    assert_eq!((r.model.as_str(), r.rule_ids[0].as_str()), ("local:small", "privacy:local_only"));
    let r = Router::choose(&ask("chat:default", LocalGate::now(true, false, false)), &reg, &profiles, &table, 10);
    assert!(r.no_route && r.rule_ids.contains(&"privacy:no_local".to_string()), "{:?}", r.rule_ids);
    let r = Router::choose(&ask("chat:default", LocalGate::now(true, true, false)), &reg, &profiles, &table, 10);
    assert_eq!(r.model, "grok-4.7", "a cloud grant lets it go out");
    // The downshift ladder only switches the live tier.
    local::set_device(Device { battery_low: true, low_end: false });
    assert_eq!(Router::choose(&ask("background:summarize", gate()), &reg, &profiles, &table, 10).model, "local:tiny");
    local::set_device(Device::default());
    // The estimator hook reads the fake; PII is regex first, the model second.
    assert_eq!(estimate(&DifficultyInput { text: "x", ..DifficultyInput::default() }, local::runtime().as_deref()), 0.42);
    let scan = local::scan_pii("mail alice@example.com or call +1 555 010 9999, Alice said", local::runtime().as_deref());
    assert_eq!((scan.redacted.as_str(), scan.regex_hits, scan.model_flag), ("mail [redacted] or call [redacted], Alice said", 2, Some(true)));
}

#[test]
fn the_settings_toggle_routes_local_only_while_a_runtime_is_installed() {
    let reg = fleet();
    let profiles: BTreeMap<String, ModelProfile> = BTreeMap::new();
    let table = RoutingTable::default();
    let pick = |class: &str| Router::choose(&ask(class, LocalGate::now(false, false, false)), &reg, &profiles, &table, 10).model;
    // Toggle on with nothing installed (this build): the route stays off and background work stays in the cloud.
    local::set_enabled(true);
    assert!(!local::installed() && !local::enabled());
    assert!(LocalSource.fetch().is_err());
    assert_eq!(pick("background:summarize"), "grok-4.7");
    // A runtime installed: the toggle goes live for background work only, never for chat.
    let rt = Installed::with(Arc::new(FakeRt));
    assert!(local::installed() && local::enabled());
    assert_eq!(LocalSource.fetch().map(|l| l.models.len()), Ok(2));
    assert_eq!(pick("background:summarize"), "local:small");
    assert_eq!(pick("background:triage"), "local:small");
    assert_eq!(pick("chat:default"), "grok-4.7");
    // Toggle off: back to the cloud on the next call.
    local::set_enabled(false);
    assert!(local::installed() && !local::enabled());
    assert_eq!(pick("background:summarize"), "grok-4.7");
    drop(rt);
    assert!(!local::installed());
}

#[test]
fn a_local_call_writes_no_egress_line() {
    let dir = test_dir("r3a-egress");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    grokhub_core::model_registry::store::save_registry(&dir, &fleet()).unwrap();
    let _on = ForceOn::with(Some(Arc::new(FakeRt)));
    let input = vec![crate::client::InputItem::Message { role: "user".into(), content: vec![crate::client::ContentPart::InputText("sort this".into())] }];
    let call = super::ModelCall::xai("grok-4.7", Some("low"), "background:summarize", "ep-local", input);
    struct Never;
    impl crate::client::ModelClient for Never {
        fn stream(&self, _r: &crate::client::ResponsesRequest, _c: &crate::CancelToken, _s: &mut dyn FnMut(crate::client::StreamEvent)) -> Result<crate::client::TurnOutput, crate::client::ClientError> {
            panic!("a local route never reaches the cloud client")
        }
    }
    let routed = super::call_model(&Never, &call, &crate::CancelToken::new()).unwrap();
    assert_eq!(routed.out.text, r#"{"summary":"two lines"}"#);
    assert!(read_egress(&dir).is_empty(), "0 egress lines from a local route");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cargo_lock_has_no_local_runtime_crate() {
    let lock = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("Cargo.lock");
    let text = std::fs::read_to_string(lock).unwrap();
    let names: Vec<&str> = text.lines().filter_map(|l| l.trim().strip_prefix("name = \"")).map(|l| l.trim_end_matches('"')).collect();
    assert!(names.len() > 100, "read the lock file");
    for banned in ["llama-cpp-2", "llama-cpp-sys-2", "ort", "ort-sys", "candle-core", "candle-nn", "candle-transformers"] {
        assert!(!names.contains(&banned), "{banned} is in Cargo.lock");
    }
}
