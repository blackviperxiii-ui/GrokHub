//! Router R3b acceptance: a provider you add is off until its key and your
//! destination grant exist. 0 egress without a grant (a hard send card;
//! approving once is one egress line; revoking blocks the next call), no key
//! means never a candidate, sensitive data never goes there without a grant
//! that names it, it is never a fallback or a cost saving, effort maps per
//! provider (golden bodies, xAI byte-identical), the key never lands in a
//! file or the model's input, and tests never touch the network.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::json;

use grokhub_core::model_registry::cost_class::CostClass;
use grokhub_core::model_registry::profile::{read_profile, ModelProfile};
use grokhub_core::model_registry::store::save_registry;
use grokhub_core::model_registry::{CatalogSource, Credential, Listing, ModelMeta, ModelState, Prices, Registry, SourceKind};

use super::learn::RegistryFilters;
use super::log::last_routes;
use super::policy::{class_row, CLASS_TABLE};
use super::providers::{
    self, add_provider, call_data, parse_base_url, request_body, usable, HttpReply, MemoryVault, ProviderKind, ProviderSource, ProviderTransport,
    CALL_DATA, CALL_DATA_SENSITIVE,
};
use super::refresh::{run_refresh, RefreshJob};
use super::spend::Spend;
use super::table::{ClassTable, RoutingTable, SpeedQuality, TableRow};
use super::tune::Filters;
use super::{live, Router, RouteInput};
use crate::client::{responses_body, ClientError, ContentPart, InputItem, ModelClient, ResponsesRequest, StreamEvent, TurnOutput};
use crate::harness::{answer_next_park, grant_destination, read_egress, revoke_grant, test_dir, DataClass, UserClick};
use crate::CancelToken;

/// An obvious fake, never a real key.
const KEY: &str = "sk-abcdefghijklmnopqrstuv";
const OR_URL: &str = "https://openrouter.ai/api/v1";
const OR_DEST: &str = "openrouter.ai";
const OR_MODEL: &str = "openrouter/vendor/cheap";
const AN_MODEL: &str = "anthropic/model-x";
const NOW: u64 = 1_000_000;

/// The counting fake wire: every request is recorded, none leaves the box.
/// One recorded request: URL, headers, body.
type Sent = (String, Vec<(String, String)>, String);

#[derive(Default)]
struct Wire(Mutex<Vec<Sent>>);

impl Wire {
    fn n(&self) -> usize {
        self.0.lock().unwrap().len()
    }
    fn bodies(&self) -> Vec<String> {
        self.0.lock().unwrap().iter().map(|(_, _, b)| b.clone()).collect()
    }
}

const OR_LIST: &str = r#"{"data":[{"id":"vendor/cheap","context_length":128000,"pricing":{"prompt":"0.0000001","completion":"0.0000005"},"supported_parameters":["tools"],"reasoning_efforts":["low","high"]}]}"#;
const OR_REPLY: &str = r#"{"choices":[{"message":{"role":"assistant","content":"hello from the provider"}}],"usage":{"prompt_tokens":12,"completion_tokens":4,"cost":0.0000025}}"#;

impl ProviderTransport for Wire {
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<HttpReply, String> {
        self.0.lock().unwrap().push((url.into(), headers.to_vec(), String::new()));
        Ok(HttpReply { status: 200, body: OR_LIST.into() })
    }
    fn post(&self, url: &str, headers: &[(String, String)], body: &str) -> Result<HttpReply, String> {
        self.0.lock().unwrap().push((url.into(), headers.to_vec(), body.into()));
        Ok(HttpReply { status: 200, body: OR_REPLY.into() })
    }
}

/// A config dir with a memory vault and the counting wire.
fn setup(label: &str) -> (PathBuf, Arc<Wire>) {
    let dir = test_dir(label);
    providers::use_vault_for(&dir, Arc::new(MemoryVault::default()));
    let wire = Arc::new(Wire::default());
    providers::use_transport_for(&dir, wire.clone());
    (dir, wire)
}

fn hello() -> ResponsesRequest {
    ResponsesRequest {
        model: OR_MODEL.into(),
        effort: Some("high".into()),
        input: vec![InputItem::Message { role: "user".into(), content: vec![ContentPart::InputText("say hello".into())] }],
        conversation_id: "conv-1".into(),
        tools: Vec::new(),
        hosted_search: false,
        call_timeout: None,
    }
}

fn send(dir: &Path) -> Result<TurnOutput, ClientError> {
    providers::call_model(dir, OR_MODEL, Some("high"), &hello(), CALL_DATA, "conv-1:7", &CancelToken::new())
}

#[test]
fn zero_egress_without_a_grant_approve_once_is_one_line_and_revoke_blocks_the_next_call() {
    let (dir, wire) = setup("r3b-egress");
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    // Key, no grant: blocked, with a hard send card. Deny sends nothing.
    let card = answer_next_park(dir.clone(), false);
    let err = send(&dir).unwrap_err().to_string();
    let park = card.join().unwrap().expect("a hard send card");
    assert_eq!((park.class.as_str(), park.tool.as_str()), ("send", providers::PROVIDER_TOOL));
    assert_eq!(park.action, "model.provider → openrouter.ai (chats, memory)");
    assert!(err.contains("hard-class send") && err.contains("Denied: nothing was sent."), "{err}");
    assert_eq!((wire.n(), read_egress(&dir).len()), (0, 0), "0 egress without a grant");
    // Approve once (a click): exactly one egress line, and the call goes.
    let card = answer_next_park(dir.clone(), true);
    assert_eq!(send(&dir).unwrap().text, "hello from the provider");
    card.join().unwrap().expect("asked again");
    let log = read_egress(&dir);
    assert_eq!(wire.n(), 1);
    assert_eq!(log.len(), 1);
    assert_eq!((log[0].dest.as_str(), log[0].basis.as_str(), log[0].grant_id.as_str(), log[0].span_id.as_str()), (OR_DEST, "approved_once", "approved-once", "conv-1:7"));
    assert_eq!(log[0].data_classes, vec![DataClass::Chat, DataClass::Personal]);
    // A standing grant (the Settings card's click): no card, one line naming it.
    let g = grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    send(&dir).unwrap();
    let log = read_egress(&dir);
    assert_eq!((wire.n(), log.len()), (2, 2));
    assert_eq!((log[1].basis.as_str(), log[1].grant_id.as_str()), ("grant", g.id.as_str()));
    // Revoke: the next call is blocked again.
    assert_eq!(revoke_grant(&dir, &g.id), Ok(true));
    let card = answer_next_park(dir.clone(), false);
    assert!(send(&dir).is_err());
    card.join().unwrap().expect("the card is back");
    assert_eq!((wire.n(), read_egress(&dir).len()), (2, 2), "nothing left after the revoke");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_model_list_needs_the_key_and_the_grant_and_gets_a_metadata_only_profile() {
    let (dir, wire) = setup("r3b-listing");
    let p = add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    assert_eq!((p.id.as_str(), p.kind, p.models_url()), ("openrouter", ProviderKind::OpenAiCompatible, "https://openrouter.ai/api/v1/models".to_string()));
    let src = ProviderSource { config_dir: dir.clone(), provider: p.clone() };
    let err = src.fetch().unwrap_err();
    assert!(err.starts_with("openrouter: hard-class send"), "{err}");
    assert_eq!((wire.n(), read_egress(&dir).len()), (0, 0), "no grant, no request");
    grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    let srcs = providers::sources(&dir);
    let sources: Vec<&dyn CatalogSource> = srcs.iter().map(|s| s as &dyn CatalogSource).collect();
    let done = run_refresh(&RefreshJob { config_dir: dir.clone(), sources, credential: Credential::Plan, named: Vec::new(), default_model: "grok-4.7".into(), now_ms: NOW });
    assert_eq!(done.errors, Vec::<String>::new());
    assert_eq!(wire.n(), 1);
    let line = &read_egress(&dir)[0];
    assert_eq!((line.dest.as_str(), line.basis.as_str(), line.data_classes.len()), (OR_DEST, "grant", 0), "the list carries no user data");
    let profile = read_profile(&dir, OR_MODEL).expect("a profile");
    assert_eq!(profile.probe.status, "not_run: cost_class", "probes stay on the plan pool");
    assert_eq!(profile.metadata.prices.completion, Some(5_000));
    // No key: never listed again.
    providers::remove_provider(&dir, "openrouter").unwrap();
    assert!(providers::sources(&dir).is_empty());
    let src = ProviderSource { config_dir: dir.clone(), provider: p };
    assert_eq!(src.fetch().unwrap_err(), "openrouter: no key");
    assert_eq!(wire.n(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_test_build_never_reaches_the_network_without_a_fake() {
    let dir = test_dir("r3b-nonet");
    providers::use_vault_for(&dir, Arc::new(MemoryVault::default()));
    let p = add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    let src = ProviderSource { config_dir: dir.clone(), provider: p };
    assert_eq!(src.fetch().unwrap_err(), "no network in tests");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn usable_needs_a_key_and_a_grant_that_covers_the_data() {
    let (dir, _wire) = setup("r3b-usable");
    assert!(usable(&dir, CALL_DATA).is_empty(), "fresh config: none");
    grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    assert!(usable(&dir, CALL_DATA).is_empty(), "a grant with no provider and no key: none");
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    assert_eq!(usable(&dir, CALL_DATA), vec!["openrouter".to_string()]);
    assert!(usable(&dir, CALL_DATA_SENSITIVE).is_empty(), "sensitive needs a grant that names it");
    grant_destination(&dir, OR_DEST, CALL_DATA_SENSITIVE, UserClick::from_click()).unwrap();
    assert_eq!(usable(&dir, call_data(true)), vec!["openrouter".to_string()]);
    providers::remove_provider(&dir, "openrouter").unwrap();
    assert!(usable(&dir, CALL_DATA).is_empty(), "no key: none");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn base_urls_are_https_hosts_and_xai_is_built_in() {
    assert_eq!(parse_base_url(" https://openrouter.ai/api/v1/ ").unwrap(), ("https://openrouter.ai/api/v1".to_string(), ProviderKind::OpenAiCompatible, "openrouter".to_string()));
    assert_eq!(parse_base_url("https://api.anthropic.com").unwrap(), ("https://api.anthropic.com".to_string(), ProviderKind::Anthropic, "anthropic".to_string()));
    assert_eq!(parse_base_url("http://openrouter.ai/api/v1").unwrap_err(), "Use an https:// address.");
    assert_eq!(parse_base_url("https://api.x.ai/v1").unwrap_err(), "xAI is already built in.");
    assert_eq!(parse_base_url("https://me:pw@openrouter.ai").unwrap_err(), "That address isn't a plain https URL.");
    assert_eq!(parse_base_url("https://localhost").unwrap_err(), "That address has no host.");
    let a = providers::Provider { id: "anthropic".into(), kind: ProviderKind::Anthropic, base_url: "https://api.anthropic.com".into(), added_at: 0 };
    assert_eq!((a.models_url(), a.call_url()), ("https://api.anthropic.com/v1/models".to_string(), "https://api.anthropic.com/v1/messages".to_string()));
    let (dir, _w) = setup("r3b-add");
    assert_eq!(add_provider(&dir, OR_URL, "short", NOW).unwrap_err(), "Paste the whole key.");
    let no_vault = test_dir("r3b-add-novault");
    assert_eq!(add_provider(&no_vault, OR_URL, KEY, NOW).unwrap_err(), "The keyring isn't available, so the key wasn't saved.");
    assert!(providers::load_providers(&no_vault).is_empty(), "no key saved, no provider saved");
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(no_vault);
}

// ---- Routing (pure) ----------------------------------------------------------

fn xai(id: &str, completion: u64) -> ModelMeta {
    ModelMeta {
        id: id.into(),
        context_length: Some(256_000),
        prices: Prices { prompt: Some(completion / 5), completion: Some(completion), ..Prices::default() },
        efforts: Some(vec!["low".into(), "medium".into(), "high".into(), "xhigh".into()]),
        tool_calling: Some(true),
        api_shape: Some("responses".into()),
        ..ModelMeta::default()
    }
}

/// grok-4.7 and grok-4.3 on xAI; a far cheaper OpenRouter model (efforts low/high)
/// and an Anthropic model with no prices and no effort list.
fn fleet() -> Registry {
    let mut reg = Registry::default();
    reg.entitlement.credential = Credential::Plan;
    let or = ModelMeta {
        id: OR_MODEL.into(),
        context_length: Some(128_000),
        prices: Prices { prompt: Some(1_000), completion: Some(5_000), ..Prices::default() },
        efforts: Some(vec!["low".into(), "high".into()]),
        tool_calling: Some(true),
        ..ModelMeta::default()
    };
    let an = ModelMeta { id: AN_MODEL.into(), context_length: Some(200_000), ..ModelMeta::default() };
    reg.apply_refresh(
        &[
            Listing::new(SourceKind::XaiApi, vec![xai("grok-4.7", 350_000), xai("grok-4.3", 150_000)]),
            Listing::new(SourceKind::OpenAiCompatible, vec![or]),
            Listing::new(SourceKind::Anthropic, vec![an]),
        ],
        &[],
        1,
    );
    reg
}

fn table_with(class: &str, first: &str) -> RoutingTable {
    let row = |m: &str| TableRow { model: m.into(), effort: Some("medium".into()), quality: Some(1.0), est_cost_usd: None, p50_latency_ms: None, why: "the class preference".into() };
    let mut t = RoutingTable { schema: 1, version: 1, ..RoutingTable::default() };
    t.classes.insert(class.into(), ClassTable { speed_quality: SpeedQuality::Balanced, ranked: vec![row(first), row("grok-4.7")] });
    t
}

fn granted(ids: &[&str]) -> Spend {
    Spend { providers: ids.iter().map(|s| s.to_string()).collect(), ..Spend::default() }
}

fn ask<'a>(class: &'a str, model: &'a str, pinned: bool, spend: Spend) -> RouteInput<'a> {
    RouteInput { provider: "xai", class, current_model: model, pinned, d: 0.5, needs_tools: true, spend, ..RouteInput::default() }
}

fn none() -> BTreeMap<String, ModelProfile> {
    BTreeMap::new()
}

#[test]
fn a_provider_is_never_picked_to_save_money_and_a_pin_or_the_class_preference_takes_it() {
    let reg = fleet();
    let empty = RoutingTable::default();
    assert_eq!(reg.get(OR_MODEL).map(|r| r.state), Some(ModelState::Live));
    // Granted and twenty times cheaper, but Auto never takes it for cost.
    let r = Router::choose(&ask("chat:default", "grok-4.7", false, granted(&["openrouter", "anthropic"])), &reg, &none(), &empty, NOW);
    assert_eq!((r.provider.as_str(), r.model.as_str(), r.cost_class), ("xai", "grok-4.3", CostClass::Included));
    // Your pin with its key and grant: it goes there, at an effort from its own list.
    let r = Router::choose(&ask("chat:default", OR_MODEL, true, granted(&["openrouter"])), &reg, &none(), &empty, NOW);
    assert_eq!((r.provider.as_str(), r.model.as_str(), r.cost_class), ("openrouter", OR_MODEL, CostClass::NewProvider));
    assert!(matches!(r.effort.as_deref(), Some("low" | "high")), "{:?}", r.effort);
    // No effort list in its listing: no effort is sent.
    let r = Router::choose(&ask("chat:default", AN_MODEL, true, granted(&["anthropic"])), &reg, &none(), &empty, NOW);
    assert_eq!((r.provider.as_str(), r.effort.as_deref()), ("anthropic", None));
    // Your pick in Settings: taken while it has its key and grant, else Auto as usual.
    let mut picked = ask("chat:default", "grok-4.7", false, granted(&["openrouter"]));
    picked.prefer = Some(OR_MODEL);
    let r = Router::choose(&picked, &reg, &none(), &empty, NOW);
    assert_eq!((r.model.as_str(), r.reason.as_str()), (OR_MODEL, "Using openrouter/vendor/cheap at Low: you picked it in Settings."));
    picked.spend = Spend::default();
    let r = Router::choose(&picked, &reg, &none(), &empty, NOW);
    assert_eq!((r.provider.as_str(), r.model.as_str()), ("xai", "grok-4.3"));
    // The class preference ranks it first: taken while granted, never without.
    let pref = table_with("chat:default", OR_MODEL);
    let r = Router::choose(&ask("chat:default", "grok-4.7", false, granted(&["openrouter"])), &reg, &none(), &pref, NOW);
    assert_eq!(r.model, OR_MODEL);
    let r = Router::choose(&ask("chat:default", "grok-4.7", false, Spend::default()), &reg, &none(), &pref, NOW);
    assert_eq!((r.provider.as_str(), r.model.as_str()), ("xai", "grok-4.7"));
}

#[test]
fn no_key_or_no_grant_means_never_a_candidate_and_xai_stands_in_for_a_pin() {
    let reg = fleet();
    let r = Router::choose(&ask("chat:default", OR_MODEL, true, Spend::default()), &reg, &none(), &RoutingTable::default(), NOW);
    assert_eq!((r.provider.as_str(), r.cost_class), ("xai", CostClass::Included));
    assert!(r.rule_ids.contains(&"provider:ungranted".to_string()), "{:?}", r.rule_ids);
    assert_eq!(r.replaced.as_deref(), Some(OR_MODEL));
    assert!(r.reason.ends_with(&format!("{OR_MODEL} needs its key and your OK in Settings first.")), "{}", r.reason);
    assert_eq!(r.candidates_n, 2, "only the two xAI models are candidates");
}

#[test]
fn fallback_chains_never_include_a_provider() {
    let mut reg = fleet();
    // Your pinned grok-4.7 rests; the granted provider is cheaper, but xAI stands in.
    reg.models.get_mut("grok-4.7").unwrap().state = ModelState::Quarantined;
    let all = granted(&["openrouter", "anthropic"]);
    let r = Router::choose(&ask("chat:default", "grok-4.7", true, all.clone()), &reg, &none(), &RoutingTable::default(), NOW);
    assert_eq!((r.model.as_str(), r.provider.as_str()), ("grok-4.3", "xai"));
    assert!(r.rule_ids.contains(&"pin:fallback".to_string()), "{:?}", r.rule_ids);
    // Nothing on xAI answers: the step pauses rather than leaving for a provider.
    reg.models.get_mut("grok-4.3").unwrap().state = ModelState::Quarantined;
    let r = Router::choose(&ask("chat:default", "grok-4.7", true, all), &reg, &none(), &RoutingTable::default(), NOW);
    assert!(r.no_route, "{:?}", r.rule_ids);
    assert_eq!(r.provider, "xai");
}

/// A tiny deterministic generator, so the property test needs no new crate.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[(self.next() as usize) % xs.len()]
    }
    fn coin(&mut self) -> bool {
        self.next().is_multiple_of(2)
    }
}

#[test]
fn sensitive_data_never_routes_to_a_provider_without_a_grant_that_names_it_property() {
    // Real grants and keys: what `live::decide` reads, per data and grant shape.
    let (plain, _w1) = setup("r3b-prop-plain");
    add_provider(&plain, OR_URL, KEY, NOW).unwrap();
    grant_destination(&plain, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    let (named, _w2) = setup("r3b-prop-named");
    add_provider(&named, OR_URL, KEY, NOW).unwrap();
    grant_destination(&named, OR_DEST, CALL_DATA_SENSITIVE, UserClick::from_click()).unwrap();
    let (keyless, _w3) = setup("r3b-prop-keyless");
    grant_destination(&keyless, OR_DEST, CALL_DATA_SENSITIVE, UserClick::from_click()).unwrap();
    let gate = |dir: &Path, sensitive: bool| usable(dir, call_data(sensitive));
    assert!(gate(&plain, true).is_empty() && !gate(&plain, false).is_empty() && !gate(&named, true).is_empty() && gate(&keyless, false).is_empty());
    let reg = fleet();
    let classes: Vec<&str> = CLASS_TABLE.iter().map(|r| r.class).collect();
    let models = ["grok-4.7", "grok-4.3", OR_MODEL, AN_MODEL];
    let mut rng = Lcg(0x5eed_003b);
    let mut sent_to_provider = 0;
    for _ in 0..1_500 {
        let class = *rng.pick(&classes);
        let model = *rng.pick(&models);
        let pinned = rng.coin();
        let sensitive = rng.coin();
        let shape = rng.next() % 3;
        let dir = [&plain, &named, &keyless][shape as usize];
        let table = if rng.coin() { table_with(class, OR_MODEL) } else { RoutingTable::default() };
        let mut input = ask(class, model, pinned, Spend { providers: gate(dir, sensitive), ..Spend::default() });
        input.d = (rng.next() % 100) as f64 / 100.0;
        input.prefer = rng.coin().then_some(*rng.pick(&[OR_MODEL, AN_MODEL]));
        let r = Router::choose(&input, &reg, &none(), &table, NOW);
        if r.cost_class != CostClass::NewProvider {
            continue;
        }
        sent_to_provider += 1;
        assert!(!(sensitive && shape == 0), "sensitive data left for a provider without a grant naming it: {class} {model}");
        assert_ne!(shape, 2, "no key, yet routed to a provider");
        let chosen_by_you = (pinned && r.model == model) || input.prefer == Some(r.model.as_str());
        assert!(chosen_by_you || table.row(class, &r.model).is_some(), "only your pick or the class preference: {class} {model} {:?}", r.rule_ids);
        assert!(!r.rule_ids.iter().any(|x| x.ends_with(":fallback") || x == "model:cheapest"), "{:?}", r.rule_ids);
    }
    assert!(sent_to_provider > 100, "the property ran on real provider picks: {sent_to_provider}");
    for d in [plain, named, keyless] {
        let _ = std::fs::remove_dir_all(d);
    }
}

#[test]
fn the_self_tuner_never_promotes_a_provider() {
    let reg = fleet();
    let profiles = none();
    let f = RegistryFilters { reg: &reg, profiles: &profiles, spend: granted(&["openrouter", "anthropic"]), now_ms: NOW };
    assert!(f.allowed("chat:default", "grok-4.3"));
    assert!(!f.allowed("chat:default", OR_MODEL), "never for cost or latency, even granted");
    assert!(!f.allowed("background:summarize", AN_MODEL));
    assert!(class_row("chat:default").is_some());
}

// ---- Effort mapping and the wire ----------------------------------------------

/// System, user, a tool call and its output, and one function tool plus a hosted one.
fn turn(effort: &str) -> ResponsesRequest {
    ResponsesRequest {
        model: "m".into(),
        effort: Some(effort.into()),
        input: vec![
            InputItem::Message { role: "system".into(), content: vec![ContentPart::InputText("be brief".into())] },
            InputItem::Message { role: "user".into(), content: vec![ContentPart::InputText("weather?".into())] },
            InputItem::FunctionCall { call_id: "c1".into(), name: "weather".into(), arguments: r#"{"city":"Oslo"}"#.into() },
            InputItem::FunctionCallOutput { call_id: "c1".into(), output: "4C".into() },
        ],
        conversation_id: "conv".into(),
        tools: vec![
            json!({"type":"function","name":"weather","description":"Look it up","parameters":{"type":"object","properties":{"city":{"type":"string"}}}}),
            json!({"type":"web_search"}),
        ],
        hosted_search: false,
        call_timeout: None,
    }
}

#[test]
fn effort_maps_per_provider_golden_bodies() {
    // xAI: byte-identical to the body the native client already sends.
    let xai = turn("high");
    assert_eq!(
        responses_body(&ResponsesRequest { model: "grok-4.7".into(), ..xai.clone() }).to_string(),
        r#"{"input":[{"content":[{"text":"be brief","type":"input_text"}],"role":"system","type":"message"},{"content":[{"text":"weather?","type":"input_text"}],"role":"user","type":"message"},{"arguments":"{\"city\":\"Oslo\"}","call_id":"c1","name":"weather","type":"function_call"},{"call_id":"c1","output":"4C","type":"function_call_output"}],"model":"grok-4.7","reasoning":{"effort":"high"},"stream":true,"tools":[{"description":"Look it up","name":"weather","parameters":{"properties":{"city":{"type":"string"}},"type":"object"},"type":"function"},{"type":"web_search"}]}"#
    );
    // Anthropic: `output_config.effort`, tool_use / tool_result blocks, the system prompt apart.
    assert_eq!(
        request_body(ProviderKind::Anthropic, "model-x", Some("high"), &xai, Some(32_000)).to_string(),
        r#"{"max_tokens":8192,"messages":[{"content":[{"text":"weather?","type":"text"}],"role":"user"},{"content":[{"id":"c1","input":{"city":"Oslo"},"name":"weather","type":"tool_use"}],"role":"assistant"},{"content":[{"content":"4C","tool_use_id":"c1","type":"tool_result"}],"role":"user"}],"model":"model-x","output_config":{"effort":"high"},"system":"be brief","tools":[{"description":"Look it up","input_schema":{"properties":{"city":{"type":"string"}},"type":"object"},"name":"weather"}]}"#
    );
    // OpenAI-compatible: `reasoning.effort`, chat messages, nested function tools; hosted tools dropped.
    assert_eq!(
        request_body(ProviderKind::OpenAiCompatible, "vendor/cheap", Some("low"), &xai, None).to_string(),
        r#"{"messages":[{"content":"be brief","role":"system"},{"content":"weather?","role":"user"},{"content":null,"role":"assistant","tool_calls":[{"function":{"arguments":"{\"city\":\"Oslo\"}","name":"weather"},"id":"c1","type":"function"}]},{"content":"4C","role":"tool","tool_call_id":"c1"}],"model":"vendor/cheap","reasoning":{"effort":"low"},"stream":false,"tools":[{"function":{"description":"Look it up","name":"weather","parameters":{"properties":{"city":{"type":"string"}},"type":"object"}},"type":"function"}]}"#
    );
    // No effort: no field at all.
    let none = request_body(ProviderKind::Anthropic, "model-x", None, &xai, None);
    let open = request_body(ProviderKind::OpenAiCompatible, "vendor/cheap", None, &xai, None);
    assert!(none.get("output_config").is_none() && open.get("reasoning").is_none());
    assert_eq!((ProviderKind::Anthropic.effort_field(), ProviderKind::OpenAiCompatible.effort_field()), ("output_config.effort", "reasoning.effort"));
}

#[test]
fn replies_map_back_to_turns() {
    let t = providers::parse_openai_reply(r#"{"choices":[{"message":{"content":null,"tool_calls":[{"id":"c9","type":"function","function":{"name":"weather","arguments":"{}"}}]}}],"usage":{"prompt_tokens":10,"completion_tokens":3,"completion_tokens_details":{"reasoning_tokens":1},"prompt_tokens_details":{"cached_tokens":4},"cost":0.001}}"#).unwrap();
    assert_eq!((t.calls[0].call_id.as_str(), t.calls[0].name.as_str(), t.text.as_str()), ("c9", "weather", ""));
    assert_eq!((t.usage.input_tokens, t.usage.output_tokens, t.usage.reasoning_tokens, t.usage.cached_tokens, t.usage.cost_in_usd_ticks), (10, 3, 1, 4, 10_000_000));
    let a = providers::parse_anthropic_reply(r#"{"content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"Hi"},{"type":"tool_use","id":"t1","name":"weather","input":{"city":"Oslo"}}],"usage":{"input_tokens":7,"output_tokens":2,"cache_read_input_tokens":5}}"#).unwrap();
    assert_eq!((a.text.as_str(), a.reasoning.as_str(), a.calls[0].arguments.as_str()), ("Hi", "hmm", r#"{"city":"Oslo"}"#));
    assert_eq!((a.usage.input_tokens, a.usage.output_tokens, a.usage.cached_tokens), (7, 2, 5));
    assert!(providers::parse_anthropic_reply("{}").is_err());
}

// ---- End to end: the route record, and the key is nowhere ---------------------

struct Never;

impl ModelClient for Never {
    fn stream(&self, _r: &ResponsesRequest, _c: &CancelToken, _s: &mut dyn FnMut(StreamEvent)) -> Result<TurnOutput, ClientError> {
        panic!("a provider route never reaches the xAI client")
    }
}

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            files_under(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn a_pinned_provider_call_routes_logs_its_provider_and_the_key_is_in_no_file_or_input() {
    let (dir, wire) = setup("r3b-e2e");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet()).unwrap();
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    let g = grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    live::set_pin(OR_MODEL);
    let input = vec![InputItem::Message { role: "user".into(), content: vec![ContentPart::InputText("say hello".into())] }];
    let call = super::ModelCall::xai(OR_MODEL, Some("medium"), "chat:default", "ep-r3b", input).in_session("chat-r3b");
    let routed = super::call_model(&Never, &call, &CancelToken::new());
    live::set_pin("");
    let routed = routed.unwrap();
    assert_eq!(routed.out.text, "hello from the provider");
    assert_eq!(routed.tokens.cost_ticks, 25_000);
    // One request, one egress line under the grant, with the span id.
    assert_eq!(wire.n(), 1);
    let log = read_egress(&dir);
    assert_eq!(log.len(), 1);
    assert_eq!((log[0].grant_id.as_str(), log[0].span_id.starts_with("chat-r3b:")), (g.id.as_str(), true));
    // The route record carries the provider and the cost class.
    let (_, rec) = last_routes(&dir).pop().expect("a route record");
    assert_eq!((rec.chosen.provider.as_str(), rec.used.provider.as_str(), rec.cost_class.as_str()), ("openrouter", "openrouter", "new_provider"));
    assert_eq!(rec.used.model, OR_MODEL);
    // The key: only in the auth header, never in the body (model input), a span, the ledger, a log or an egress line.
    let (_, headers, body) = wire.0.lock().unwrap()[0].clone();
    assert!(headers.iter().any(|(k, v)| k == "Authorization" && v == &format!("Bearer {KEY}")));
    assert!(!body.contains(KEY) && wire.bodies().iter().all(|b| !b.contains(KEY)));
    assert_eq!(serde_json::from_str::<serde_json::Value>(&body).unwrap()["model"], "vendor/cheap");
    let mut files = Vec::new();
    files_under(&dir, &mut files);
    assert!(files.len() >= 4, "{files:?}");
    for f in files {
        let bytes = std::fs::read(&f).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains(KEY) && !text.contains("abcdefghijklmnopqrstuv"), "the key is in {}", f.display());
    }
    let _ = std::fs::remove_dir_all(dir);
}
