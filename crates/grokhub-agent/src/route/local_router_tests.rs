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
use super::spend::Spend;
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

#[test]
fn online_chat_uses_the_frontier_and_falls_back_to_local_only_when_no_cloud_model_can_answer() {
    let _on = ForceOn::with(Some(Arc::new(FakeRt)));
    local::note_local_verify_failed(false);
    let profiles: BTreeMap<String, ModelProfile> = BTreeMap::new();
    let table = RoutingTable::default();
    let online = || {
        let mut gate = LocalGate::now(false, false, false);
        gate.offline = false;
        gate
    };
    // The cloud answers: the frontier model takes chat and background work.
    let up = fleet();
    for class in ["chat:default", "background:summarize"] {
        let r = Router::choose(&ask(class, online()), &up, &profiles, &table, 10);
        assert_eq!(r.model, "grok-4.7", "{class}");
    }
    // Every cloud model down: the device takes chat too, and says so.
    let mut down = fleet();
    down.models.get_mut("grok-4.7").unwrap().state = ModelState::Quarantined;
    let r = Router::choose(&ask("chat:default", online()), &down, &profiles, &table, 10);
    assert_eq!(r.model, "local:small");
    assert!(
        r.rule_ids.contains(&"local:cloud_down".into()),
        "{:?}",
        r.rule_ids
    );
    assert!(r.reason.contains("using local"), "{}", r.reason);
    // No sign-in or key: the device answers, and the note names why.
    let mut no_keys = fleet();
    no_keys.entitlement.credential = Credential::None;
    for class in ["chat:default", "background:summarize"] {
        let r = Router::choose(&ask(class, online()), &no_keys, &profiles, &table, 10);
        assert_eq!(r.model, "local:small", "{class}");
        assert!(
            r.rule_ids.contains(&"local:no_keys".into()),
            "{class}: {:?}",
            r.rule_ids
        );
        assert!(r.reason.contains("sign-in or key"), "{}", r.reason);
    }
    // Hard work never falls back to the device.
    let r = Router::choose(&ask("prepare:hard", online()), &down, &profiles, &table, 10);
    assert_ne!(r.model, "local:small");
}

fn online() -> LocalGate {
    let mut gate = LocalGate::now(false, false, false);
    gate.offline = false;
    gate
}

#[test]
fn hard_chat_never_falls_back_to_local_with_the_cloud_down() {
    let _on = ForceOn::with(Some(Arc::new(FakeRt)));
    local::note_local_verify_failed(false);
    let profiles: BTreeMap<String, ModelProfile> = BTreeMap::new();
    let table = RoutingTable::default();
    let mut down = fleet();
    down.models.get_mut("grok-4.7").unwrap().state = ModelState::Quarantined;
    let mut hard = ask("chat:default", online());
    hard.d = 0.8;
    let r = Router::choose(&hard, &down, &profiles, &table, 10);
    assert_ne!(r.model, "local:small", "{:?}", r.rule_ids);
    assert!(
        !r.rule_ids.iter().any(|x| x.starts_with("local:")),
        "{:?}",
        r.rule_ids
    );
    // Nothing answers: the turn pauses until the cloud is back.
    assert!(r.no_route, "{:?}", r.rule_ids);
}

#[test]
fn with_no_sign_in_hard_chat_goes_to_the_device_but_prepare_hard_does_not() {
    let _on = ForceOn::with(Some(Arc::new(FakeRt)));
    local::note_local_verify_failed(false);
    let profiles: BTreeMap<String, ModelProfile> = BTreeMap::new();
    let table = RoutingTable::default();
    let mut no_keys = fleet();
    no_keys.entitlement.credential = Credential::None;
    // No sign-in or key: the device is the only model there is, so a hard
    // chat goes there instead of failing at xAI.
    let mut hard = ask("chat:default", online());
    hard.d = 0.8;
    let r = Router::choose(&hard, &no_keys, &profiles, &table, 10);
    assert_eq!(
        (r.model.as_str(), r.no_route, r.provider.as_str()),
        ("local:small", false, local::PROVIDER_LOCAL),
        "{:?}",
        r.rule_ids
    );
    assert!(
        r.rule_ids.contains(&"local:no_keys".into()),
        "{:?}",
        r.rule_ids
    );
    // prepare:hard still never runs on the device.
    let r = Router::choose(
        &ask("prepare:hard", online()),
        &no_keys,
        &profiles,
        &table,
        10,
    );
    assert_ne!(r.model, "local:small", "{:?}", r.rule_ids);
    assert!(
        r.rule_ids.contains(&"cost:not_included".into()),
        "{:?}",
        r.rule_ids
    );
    // A step that offers a send, delete or credential tool stays off it too.
    let mut gate = online();
    gate.hard_tool = true;
    let r = Router::choose(&ask("chat:default", gate), &no_keys, &profiles, &table, 10);
    assert_ne!(r.model, "local:small", "{:?}", r.rule_ids);
}

#[test]
fn never_signed_in_and_online_chat_goes_to_the_installed_local_model() {
    // A fresh install: online, no sign-in or key ever, so the model list holds
    // only the device's tiers.
    let mut reg = Registry::default();
    reg.entitlement.credential = Credential::None;
    reg.apply_refresh(
        &[Listing::new(
            SourceKind::Local,
            local::TIERS.iter().map(|t| local::tier_meta(t)).collect(),
        )],
        &[],
        1,
    );
    let _on = ForceOn::with(Some(Arc::new(FakeRt)));
    local::note_local_verify_failed(false);
    let profiles: BTreeMap<String, ModelProfile> = BTreeMap::new();
    let table = RoutingTable::default();
    for class in ["chat:default", "chat:quick", "background:summarize"] {
        let gate = online();
        assert!(!gate.offline, "{class}: online");
        let r = Router::choose(&ask(class, gate), &reg, &profiles, &table, 10);
        assert_eq!(
            (r.model.as_str(), r.no_route, r.provider.as_str()),
            ("local:small", false, local::PROVIDER_LOCAL),
            "{class}: {:?}",
            r.rule_ids
        );
        assert!(
            r.rule_ids.contains(&"local:no_keys".into()),
            "{class}: {:?}",
            r.rule_ids
        );
    }
}

#[test]
fn a_hard_step_that_failed_verify_never_goes_back_to_the_device_with_the_cloud_down() {
    let _on = ForceOn::with(Some(Arc::new(FakeRt)));
    local::note_local_verify_failed(true);
    let mut gate = online();
    gate.local_verify_failed = true;
    let mut down = fleet();
    down.models.get_mut("grok-4.7").unwrap().state = ModelState::Quarantined;
    let profiles: BTreeMap<String, ModelProfile> = BTreeMap::new();
    let table = RoutingTable::default();
    let mut hard = ask("background:summarize", gate);
    hard.d = 0.8;
    let r = Router::choose(&hard, &down, &profiles, &table, 10);
    assert_ne!(r.model, "local:small", "{:?}", r.rule_ids);
    assert!(
        !r.rule_ids.contains(&"local:cloud_down".into()),
        "{:?}",
        r.rule_ids
    );
    local::note_local_verify_failed(false);
}

#[test]
fn verify_escalation_with_no_keys_keeps_the_device() {
    let mut reg = fleet();
    reg.entitlement.credential = Credential::None;
    let _on = ForceOn::with(Some(Arc::new(FakeRt)));
    local::note_local_verify_failed(true);
    let mut gate = online();
    gate.local_verify_failed = true;
    let profiles: BTreeMap<String, ModelProfile> = BTreeMap::new();
    let table = RoutingTable::default();
    let r = Router::choose(&ask("chat:default", gate), &reg, &profiles, &table, 10);
    assert_eq!(r.model, "local:small", "{:?}", r.rule_ids);
    assert!(
        r.rule_ids.contains(&"local:no_keys".into()),
        "{:?}",
        r.rule_ids
    );
    assert!(
        !r.rule_ids.contains(&"local:escalate_verify".into()),
        "{:?}",
        r.rule_ids
    );
    assert_eq!(r.provider, local::PROVIDER_LOCAL);
    assert!(!local::local_verify_failed(), "flag cleared");
}

#[test]
fn no_keys_with_a_granted_provider_routes_to_that_provider() {
    const OR: &str = "openrouter/vendor/model";
    let mut reg = fleet();
    reg.entitlement.credential = Credential::None;
    reg.apply_refresh(
        &[
            Listing::new(SourceKind::XaiApi, vec![meta("grok-4.7")]),
            Listing::new(
                SourceKind::Local,
                local::TIERS.iter().map(|t| local::tier_meta(t)).collect(),
            ),
            Listing::new(SourceKind::OpenAiCompatible, vec![meta(OR)]),
        ],
        &[],
        2,
    );
    let _on = ForceOn::with(Some(Arc::new(FakeRt)));
    local::note_local_verify_failed(false);
    let profiles: BTreeMap<String, ModelProfile> = BTreeMap::new();
    let table = RoutingTable::default();
    let granted = Spend {
        providers: vec!["openrouter".into()],
        ..Spend::default()
    };
    // Pinned to the provider: it answers on its own key.
    let mut pinned = ask("chat:default", online());
    pinned.current_model = OR;
    pinned.pinned = true;
    pinned.spend = granted.clone();
    let r = Router::choose(&pinned, &reg, &profiles, &table, 10);
    assert_eq!(
        (r.provider.as_str(), r.model.as_str(), r.no_route),
        ("openrouter", OR, false),
        "{:?}",
        r.rule_ids
    );
    assert!(
        !r.rule_ids.iter().any(|x| x.starts_with("local:")),
        "{:?}",
        r.rule_ids
    );
    // Picked in Settings: the same.
    let mut picked = ask("chat:default", online());
    picked.prefer = Some(OR);
    picked.spend = granted.clone();
    let r = Router::choose(&picked, &reg, &profiles, &table, 10);
    assert_eq!(
        (r.provider.as_str(), r.model.as_str()),
        ("openrouter", OR),
        "{:?}",
        r.rule_ids
    );
    assert!(
        r.rule_ids.contains(&"provider:picked".into()),
        "{:?}",
        r.rule_ids
    );
    // With the verify flag set, it escalates there too.
    local::note_local_verify_failed(true);
    let mut verify = ask("chat:default", online());
    verify.local.local_verify_failed = true;
    verify.prefer = Some(OR);
    verify.spend = granted;
    let r = Router::choose(&verify, &reg, &profiles, &table, 10);
    assert_eq!(r.model, OR, "{:?}", r.rule_ids);
    assert!(
        r.rule_ids.contains(&"local:escalate_verify".into()),
        "{:?}",
        r.rule_ids
    );
    // No grant for it: no cloud route, so the device answers.
    local::note_local_verify_failed(false);
    let mut ungranted = ask("chat:default", online());
    ungranted.prefer = Some(OR);
    let r = Router::choose(&ungranted, &reg, &profiles, &table, 10);
    assert_eq!(r.model, "local:small", "{:?}", r.rule_ids);
    assert!(
        r.rule_ids.contains(&"local:no_keys".into()),
        "{:?}",
        r.rule_ids
    );
}

#[test]
fn a_premium_model_over_the_ceiling_asks_for_an_ok_instead_of_going_local() {
    let mut reg = Registry::default();
    reg.entitlement.credential = Credential::ApiKey;
    let mut pricey = meta("grok-4.7");
    // $40/M output, over the default $15/M ceiling.
    pricey.prices.completion = Some(400_000);
    reg.apply_refresh(
        &[
            Listing::new(SourceKind::XaiApi, vec![pricey]),
            Listing::new(
                SourceKind::Local,
                local::TIERS.iter().map(|t| local::tier_meta(t)).collect(),
            ),
        ],
        &[],
        1,
    );
    let _on = ForceOn::with(Some(Arc::new(FakeRt)));
    local::note_local_verify_failed(false);
    let profiles: BTreeMap<String, ModelProfile> = BTreeMap::new();
    let table = RoutingTable::default();
    for class in ["chat:default", "background:summarize"] {
        let r = Router::choose(&ask(class, online()), &reg, &profiles, &table, 10);
        assert_ne!(r.model, "local:small", "{class}: {:?}", r.rule_ids);
        assert!(
            !r.rule_ids.contains(&"local:cloud_down".into()),
            "{class}: {:?}",
            r.rule_ids
        );
        assert!(r.premium_ask.is_some(), "{class}: {:?}", r.rule_ids);
    }
    // The same model actually down: the device is the last resort.
    reg.models.get_mut("grok-4.7").unwrap().state = ModelState::Quarantined;
    let r = Router::choose(&ask("chat:default", online()), &reg, &profiles, &table, 10);
    assert_eq!(r.model, "local:small", "{:?}", r.rule_ids);
    assert!(
        r.rule_ids.contains(&"local:cloud_down".into()),
        "{:?}",
        r.rule_ids
    );
}
