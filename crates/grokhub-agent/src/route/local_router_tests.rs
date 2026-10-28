//! Offline card PR E: router awareness — caps in tier_meta, escalate after
//! verify fail, cloud-down → local.

use std::collections::BTreeMap;
use std::sync::Arc;

use grokhub_core::model_registry::profile::ModelProfile;
use grokhub_core::model_registry::{
    Credential, Listing, ModelMeta, ModelState, Prices, Registry, SourceKind,
};

use serde_json::{json, Value};

use super::local::{self, ForceOn, LocalGate, LocalRuntime, TierCaps};
use super::table::RoutingTable;
use super::{RouteInput, Router};

struct FakeRt;

impl LocalRuntime for FakeRt {
    fn classify(&self, _text: &str, labels: &[&str]) -> Result<Value, String> {
        Ok(json!({ "label": labels[0], "score": 0.9 }))
    }
    fn summarize(&self, _text: &str, _max_words: u32) -> Result<Value, String> {
        Ok(json!({ "summary": "two lines" }))
    }
    fn difficulty(&self, _text: &str) -> Result<Value, String> {
        Ok(json!({ "d": 0.4 }))
    }
    fn chat_stream(
        &self,
        _messages: &[local::ChatMessage],
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        on_delta("hello");
        Ok("hello".into())
    }
}

fn meta(id: &str) -> ModelMeta {
    ModelMeta {
        id: id.into(),
        context_length: Some(128_000),
        efforts: Some(vec!["low".into(), "medium".into(), "high".into()]),
        tool_calling: Some(true),
        prices: Prices {
            prompt: Some(1),
            cached: Some(1),
            completion: Some(1),
            ..Prices::default()
        },
        ..ModelMeta::default()
    }
}

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

#[test]
fn tier_meta_carries_bench_caps_without_claiming_tools() {
    local::set_caps(TierCaps {
        context: 8_192,
        eval_pass_rate: 1.0,
        tokens_per_sec: Some(42.0),
        first_token_ms: Some(120),
    });
    let m = local::tier_meta("local:small");
    assert_eq!(m.context_length, Some(8_192));
    // Chat tools (cabin web_search) are offered; eval pass rate is unrelated.
    assert_eq!(m.tool_calling, Some(true));
    let n = m.notice.expect("notice");
    assert!(
        n.text.contains("42.0") && n.text.contains("120") && n.text.contains("eval"),
        "{}",
        n.text
    );
    assert!(!n.text.contains("tools "), "{}", n.text);
    local::set_caps(TierCaps::default());
}

#[test]
fn online_escalates_to_cloud_after_local_verify_fail() {
    let reg = fleet();
    let _on = ForceOn::with(Some(Arc::new(FakeRt)));
    local::note_local_verify_failed(true);
    let mut gate = LocalGate::now(false, false, false);
    gate.offline = false;
    gate.local_verify_failed = true;
    let profiles: BTreeMap<String, ModelProfile> = BTreeMap::new();
    let table = RoutingTable::default();
    let r = Router::choose(&ask("chat:default", gate), &reg, &profiles, &table, 10);
    assert_eq!(r.model, "grok-4.7");
    assert!(
        r.rule_ids.contains(&"local:escalate_verify".into()),
        "{:?}",
        r.rule_ids
    );
    assert_eq!(r.replaced.as_deref(), Some("local:small"));
    assert!(
        r.reason.contains("local") && r.reason.contains("grok-4.7"),
        "{}",
        r.reason
    );
    assert!(!local::local_verify_failed(), "flag cleared after escalate");
}

#[test]
fn cloud_down_falls_back_to_local_when_eligible() {
    let mut reg = fleet();
    // Quarantine the only cloud model so pick_model has nowhere to go.
    if let Some(row) = reg.models.get_mut("grok-4.7") {
        row.state = ModelState::Quarantined;
    }
    let _on = ForceOn::with(Some(Arc::new(FakeRt)));
    local::note_local_verify_failed(false);
    let mut gate = LocalGate::now(false, false, false);
    gate.offline = false;
    // Background summarize is eligible for local while online.
    let profiles: BTreeMap<String, ModelProfile> = BTreeMap::new();
    let table = RoutingTable::default();
    let r = Router::choose(
        &ask("background:summarize", gate),
        &reg,
        &profiles,
        &table,
        10,
    );
    assert_eq!(r.model, "local:small");
    assert!(
        r.rule_ids.contains(&"local:cloud_down".into()),
        "{:?}",
        r.rule_ids
    );
    assert_eq!(r.provider, local::PROVIDER_LOCAL);
}

#[test]
fn escalate_verify_falls_back_to_local_when_cloud_down() {
    let mut reg = fleet();
    if let Some(row) = reg.models.get_mut("grok-4.7") {
        row.state = ModelState::Quarantined;
    }
    let _on = ForceOn::with(Some(Arc::new(FakeRt)));
    local::note_local_verify_failed(true);
    let mut gate = LocalGate::now(false, false, false);
    gate.offline = false;
    gate.local_verify_failed = true;
    let profiles: BTreeMap<String, ModelProfile> = BTreeMap::new();
    let table = RoutingTable::default();
    // Chat is not local-eligible online — use background so cloud_down can fire.
    let r = Router::choose(
        &ask("background:summarize", gate),
        &reg,
        &profiles,
        &table,
        10,
    );
    assert_eq!(r.model, "local:small");
    assert!(
        r.rule_ids.contains(&"local:cloud_down".into()),
        "{:?}",
        r.rule_ids
    );
    assert!(
        !r.rule_ids.contains(&"local:escalate_verify".into()),
        "{:?}",
        r.rule_ids
    );
    assert!(
        !local::local_verify_failed(),
        "flag cleared after cloud_down fallback"
    );
}

#[test]
fn escalate_note_names_both_models() {
    let n = local::escalate_note("local:small", "grok-4.7");
    assert!(
        n.contains("local:small") && n.contains("grok-4.7") && n.contains("failed"),
        "{n}"
    );
}
