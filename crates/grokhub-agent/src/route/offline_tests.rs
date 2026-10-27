//! Offline card PR B: connectivity forces local for chat + background;
//! cloud-only classes queue with a named note.

use std::collections::BTreeMap;
use std::sync::Arc;

use grokhub_core::model_registry::cost_class::CostClass;
use grokhub_core::model_registry::profile::ModelProfile;
use grokhub_core::model_registry::{Credential, Listing, ModelMeta, Prices, Registry, SourceKind};

use serde_json::{json, Value};

use super::connectivity::{self, ForceOffline};
use super::local::{self, ForceOn, LocalGate, LocalRuntime};
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
        on_delta("hello offline");
        Ok("hello offline".into())
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
fn offline_routes_chat_and_background_to_local_and_queues_image() {
    let reg = fleet();
    let _on = ForceOn::with(Some(Arc::new(FakeRt)));
    let _off = ForceOffline::on();
    let profiles: BTreeMap<String, ModelProfile> = BTreeMap::new();
    let table = RoutingTable::default();
    let gate = LocalGate::now(false, false, false);
    assert!(gate.offline && gate.enabled);

    let r = Router::choose(
        &ask("chat:default", gate.clone()),
        &reg,
        &profiles,
        &table,
        10,
    );
    assert_eq!(
        (r.provider.as_str(), r.model.as_str()),
        ("local", "local:small")
    );
    assert!(
        r.rule_ids.contains(&"offline:local".into()),
        "{:?}",
        r.rule_ids
    );
    assert_eq!(r.cost_class, CostClass::Included);

    let r = Router::choose(
        &ask("background:summarize", gate.clone()),
        &reg,
        &profiles,
        &table,
        10,
    );
    assert_eq!(r.model, "local:small");

    let r = Router::choose(&ask("media:image", gate), &reg, &profiles, &table, 10);
    assert!(r.no_route, "{:?}", r);
    assert!(
        r.rule_ids.contains(&"offline:cloud_queued".into()),
        "{:?}",
        r.rule_ids
    );
    assert_eq!(
        connectivity::cloud_queue_note("media:image"),
        "Waiting for the network: image generation needs the cloud."
    );
    assert_eq!(
        connectivity::offline_chip("Gemma 4 E2B"),
        "Offline · on Gemma 4 E2B (local)"
    );
}

#[test]
fn online_keeps_chat_on_cloud_even_with_local_runtime() {
    let reg = fleet();
    let _on = ForceOn::with(Some(Arc::new(FakeRt)));
    connectivity::set_state(connectivity::NetState::Online);
    let profiles: BTreeMap<String, ModelProfile> = BTreeMap::new();
    let table = RoutingTable::default();
    let gate = LocalGate::now(false, false, false);
    assert!(!gate.offline);
    let r = Router::choose(&ask("chat:default", gate), &reg, &profiles, &table, 10);
    assert_eq!(r.model, "grok-4.7");
}

#[test]
fn offline_serve_chat_uses_chat_stream() {
    let _on = ForceOn::with(Some(Arc::new(FakeRt)));
    let out =
        local::serve("chat:default", "hi", &crate::client::CancelToken::new()).expect("serve");
    assert_eq!(out.text, "hello offline");
}

#[test]
fn offline_work_goes_local_at_every_level_but_unload_which_pauses_with_a_chip() {
    use grokhub_core::local_governor::Level;
    let reg = fleet();
    let profiles: BTreeMap<String, ModelProfile> = BTreeMap::new();
    let table = RoutingTable::default();
    for level in [Level::RampUp, Level::Normal, Level::Throttle, Level::Unload] {
        let capacity = local::LocalCapacity { ready: true, level };
        let gate = LocalGate {
            enabled: capacity.available(),
            tier: "local:small".into(),
            offline: true,
            capacity,
            ..LocalGate::default()
        };
        for class in ["chat:default", "background:summarize"] {
            let r = Router::choose(&ask(class, gate.clone()), &reg, &profiles, &table, 10);
            if level == Level::Unload {
                assert!(r.no_route, "{class}: {r:?}");
                assert!(
                    r.rule_ids.contains(&"local:paused".into()),
                    "{:?}",
                    r.rule_ids
                );
                assert_eq!(r.reason, local::LOCAL_PAUSED_MSG);
            } else {
                assert_eq!(
                    (r.model.as_str(), r.no_route),
                    ("local:small", false),
                    "{level:?} {class}"
                );
                assert!(
                    r.rule_ids.contains(&"offline:local".into()),
                    "{:?}",
                    r.rule_ids
                );
            }
        }
        // Cloud-only work still queues for the network, paused or not.
        let r = Router::choose(&ask("media:image", gate), &reg, &profiles, &table, 10);
        assert!(
            r.rule_ids.contains(&"offline:cloud_queued".into()),
            "{:?}",
            r.rule_ids
        );
    }
}

#[test]
fn offline_with_no_local_runtime_waits_and_says_why_instead_of_trying_the_cloud() {
    let reg = fleet();
    let _off = ForceOffline::on();
    let profiles: BTreeMap<String, ModelProfile> = BTreeMap::new();
    let table = RoutingTable::default();
    // The toggle is on but no runtime is installed (no fake, no files).
    let gate = LocalGate {
        offline: true,
        ..LocalGate::default()
    };
    let r = Router::choose(
        &ask("chat:default", gate.clone()),
        &reg,
        &profiles,
        &table,
        10,
    );
    assert!(r.no_route, "{r:?}");
    assert!(
        r.rule_ids.contains(&"offline:local_unavailable".into()),
        "{:?}",
        r.rule_ids
    );
    // Cloud-only work keeps its own named queue note.
    let r = Router::choose(
        &ask("media:image", gate.clone()),
        &reg,
        &profiles,
        &table,
        10,
    );
    assert!(r.rule_ids.contains(&"offline:cloud_queued".into()));
    // Online, the same gate goes to the cloud as before.
    let online = LocalGate {
        offline: false,
        ..gate
    };
    let r = Router::choose(&ask("chat:default", online), &reg, &profiles, &table, 10);
    assert_eq!((r.no_route, r.model.as_str()), (false, "grok-4.7"));
    assert_eq!(
        local::offline_pause_text("runtime missing"),
        "Offline, local model unavailable: runtime missing. GrokHub keeps your message and answers it once you're back online."
    );
    local::set_enabled(false);
    assert_eq!(local::unavailable_reason(), "Local AI is off in Settings");
}
