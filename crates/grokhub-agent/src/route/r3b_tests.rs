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
use grokhub_core::model_registry::{
    CatalogSource, Credential, Listing, ModelMeta, ModelState, Prices, Registry, SourceKind,
};

use super::learn::RegistryFilters;
use super::log::{route_records, ROUTE_TAIL_LINES};
use super::policy::{class_row, CLASS_TABLE};
use super::providers::{
    self, add_provider, call_data, parse_base_url, request_body, usable, HttpReply, HttpStream,
    MemoryVault, ProviderKind, ProviderSource, ProviderTransport, CALL_DATA, CALL_DATA_SENSITIVE,
};
use super::refresh::{run_refresh, RefreshJob};
use super::spend::Spend;
use super::table::{ClassTable, RoutingTable, SpeedQuality, TableRow};
use super::tune::Filters;
use super::{live, RouteInput, Router};
use crate::client::{
    responses_body, ClientError, ContentPart, InputItem, ModelClient, ResponsesRequest,
    StreamEvent, TurnOutput,
};
use crate::harness::{
    answer_next_park, grant_destination, read_egress, revoke_grant, test_dir, DataClass, UserClick,
};
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
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|(_, _, b)| b.clone())
            .collect()
    }
}

const OR_LIST: &str = r#"{"data":[{"id":"vendor/cheap","context_length":128000,"pricing":{"prompt":"0.0000001","completion":"0.0000005"},"supported_parameters":["tools"],"reasoning_efforts":["low","high"]}]}"#;
const OR_REPLY: &str = r#"{"choices":[{"message":{"role":"assistant","content":"hello from the provider"}}],"usage":{"prompt_tokens":12,"completion_tokens":4,"cost":0.0000025}}"#;

impl ProviderTransport for Wire {
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<HttpReply, String> {
        self.0
            .lock()
            .unwrap()
            .push((url.into(), headers.to_vec(), String::new()));
        Ok(HttpReply {
            status: 200,
            body: OR_LIST.into(),
            headers: Vec::new(),
        })
    }
    fn post(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &str,
    ) -> Result<HttpReply, String> {
        self.0
            .lock()
            .unwrap()
            .push((url.into(), headers.to_vec(), body.into()));
        Ok(HttpReply {
            status: 200,
            body: OR_REPLY.into(),
            headers: Vec::new(),
        })
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
        text_format: None,
        model: OR_MODEL.into(),
        effort: Some("high".into()),
        input: vec![InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText("say hello".into())],
        }],
        conversation_id: "conv-1".into(),
        tools: Vec::new(),
        hosted_search: false,
        call_timeout: None,
    }
}

fn send(dir: &Path) -> Result<TurnOutput, ClientError> {
    providers::call_model(
        dir,
        "chat:default",
        OR_MODEL,
        Some("high"),
        &hello(),
        CALL_DATA,
        "conv-1:7",
        &CancelToken::new(),
    )
}

#[test]
fn zero_egress_without_a_grant_approve_once_is_one_line_and_revoke_blocks_the_next_call() {
    let (dir, wire) = setup("r3b-egress");
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    // Key, no grant: blocked, with a hard send card. Deny sends nothing.
    let card = answer_next_park(dir.clone(), false);
    let err = send(&dir).unwrap_err().to_string();
    let park = card.join().unwrap().expect("a hard send card");
    assert_eq!(
        (park.class.as_str(), park.tool.as_str()),
        ("send", providers::PROVIDER_TOOL)
    );
    assert_eq!(
        park.action,
        "model.provider → openrouter.ai (chats, memory)"
    );
    assert!(
        err.contains("hard-class send") && err.contains("Denied: nothing was sent."),
        "{err}"
    );
    assert_eq!(
        (wire.n(), read_egress(&dir).len()),
        (0, 0),
        "0 egress without a grant"
    );
    // Approve once (a click): exactly one egress line, and the call goes.
    let card = answer_next_park(dir.clone(), true);
    assert_eq!(send(&dir).unwrap().text, "hello from the provider");
    card.join().unwrap().expect("asked again");
    let log = read_egress(&dir);
    assert_eq!(wire.n(), 1);
    assert_eq!(log.len(), 1);
    assert_eq!(
        (
            log[0].dest.as_str(),
            log[0].basis.as_str(),
            log[0].grant_id.as_str(),
            log[0].span_id.as_str()
        ),
        (OR_DEST, "approved_once", "approved-once", "conv-1:7")
    );
    assert_eq!(
        log[0].data_classes,
        vec![DataClass::Chat, DataClass::Personal]
    );
    // A standing grant (the Settings card's click): no card, one line naming it.
    let g = grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    send(&dir).unwrap();
    let log = read_egress(&dir);
    assert_eq!((wire.n(), log.len()), (2, 2));
    assert_eq!(
        (log[1].basis.as_str(), log[1].grant_id.as_str()),
        ("grant", g.id.as_str())
    );
    // Revoke: the next call is blocked again.
    assert_eq!(revoke_grant(&dir, &g.id), Ok(true));
    let card = answer_next_park(dir.clone(), false);
    assert!(send(&dir).is_err());
    card.join().unwrap().expect("the card is back");
    assert_eq!(
        (wire.n(), read_egress(&dir).len()),
        (2, 2),
        "nothing left after the revoke"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_model_list_needs_the_key_and_the_grant_and_gets_a_metadata_only_profile() {
    let (dir, wire) = setup("r3b-listing");
    let p = add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    assert_eq!(
        (p.id.as_str(), p.kind, p.models_url()),
        (
            "openrouter",
            ProviderKind::OpenAiCompatible,
            "https://openrouter.ai/api/v1/models".to_string()
        )
    );
    let src = ProviderSource {
        config_dir: dir.clone(),
        provider: p.clone(),
    };
    let err = src.fetch().unwrap_err();
    assert!(err.starts_with("openrouter: hard-class send"), "{err}");
    assert_eq!(
        (wire.n(), read_egress(&dir).len()),
        (0, 0),
        "no grant, no request"
    );
    grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    let srcs = providers::sources(&dir);
    let sources: Vec<&dyn CatalogSource> = srcs.iter().map(|s| s as &dyn CatalogSource).collect();
    let done = run_refresh(&RefreshJob {
        config_dir: dir.clone(),
        sources,
        credential: Credential::Plan,
        named: Vec::new(),
        default_model: "grok-4.7".into(),
        now_ms: NOW,
        xai_bearer: None,
    });
    assert_eq!(done.errors, Vec::<String>::new());
    // Two reads: the model list, and the refresh's quota check reading
    // the account's `/key` state under the same grant.
    assert_eq!(wire.n(), 2);
    let line = &read_egress(&dir)[0];
    assert_eq!(
        (
            line.dest.as_str(),
            line.basis.as_str(),
            line.data_classes.len()
        ),
        (OR_DEST, "grant", 0),
        "the list carries no user data"
    );
    let profile = read_profile(&dir, OR_MODEL).expect("a profile");
    assert_eq!(
        profile.probe.status, "not_run: cost_class",
        "probes stay on the plan pool"
    );
    assert_eq!(profile.metadata.prices.completion, Some(5_000));
    // No key: never listed again.
    providers::remove_provider(&dir, "openrouter").unwrap();
    assert!(providers::sources(&dir).is_empty());
    let src = ProviderSource {
        config_dir: dir.clone(),
        provider: p,
    };
    assert_eq!(src.fetch().unwrap_err(), "openrouter: no key");
    assert_eq!(wire.n(), 2);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_test_build_never_reaches_the_network_without_a_fake() {
    let dir = test_dir("r3b-nonet");
    providers::use_vault_for(&dir, Arc::new(MemoryVault::default()));
    let p = add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    let src = ProviderSource {
        config_dir: dir.clone(),
        provider: p,
    };
    assert_eq!(src.fetch().unwrap_err(), "no network in tests");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn usable_needs_a_key_and_a_grant_that_covers_the_data() {
    let (dir, _wire) = setup("r3b-usable");
    assert!(usable(&dir, CALL_DATA).is_empty(), "fresh config: none");
    grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    assert!(
        usable(&dir, CALL_DATA).is_empty(),
        "a grant with no provider and no key: none"
    );
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    assert_eq!(usable(&dir, CALL_DATA), vec!["openrouter".to_string()]);
    assert!(
        usable(&dir, CALL_DATA_SENSITIVE).is_empty(),
        "sensitive needs a grant that names it"
    );
    grant_destination(&dir, OR_DEST, CALL_DATA_SENSITIVE, UserClick::from_click()).unwrap();
    assert_eq!(
        usable(&dir, call_data(true)),
        vec!["openrouter".to_string()]
    );
    providers::remove_provider(&dir, "openrouter").unwrap();
    assert!(usable(&dir, CALL_DATA).is_empty(), "no key: none");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn provider_agent_dials_through_the_public_only_resolver() {
    let arm = include_str!("providers.rs")
        .split("fn agent() -> grokhub_core::http::Agent {")
        .nth(1)
        .and_then(|s| s.split("fn reply(").next())
        .expect("provider agent");
    // `public_only` dials through public_only_resolve (grokhub_core::http tests).
    assert!(
        arm.contains("public_only: true,"),
        "provider dials must use public_only_resolve: {arm}"
    );
    assert!(
        arm.contains("redirects: 0,"),
        "provider dials must not follow redirects blindly: {arm}"
    );
    assert!(
        !arm.contains("env_proxy")
            && !arm.contains("proxy_from_env")
            && !arm.to_ascii_lowercase().contains("http_proxy"),
        "env proxy would skip public_only_resolve on the target: {arm}"
    );
}

#[test]
fn base_urls_are_https_hosts_and_xai_is_built_in() {
    assert_eq!(
        parse_base_url(" https://openrouter.ai/api/v1/ ").unwrap(),
        (
            "https://openrouter.ai/api/v1".to_string(),
            ProviderKind::OpenAiCompatible,
            "openrouter".to_string()
        )
    );
    assert_eq!(
        parse_base_url("https://api.anthropic.com").unwrap(),
        (
            "https://api.anthropic.com".to_string(),
            ProviderKind::Anthropic,
            "anthropic".to_string()
        )
    );
    assert_eq!(
        parse_base_url("http://openrouter.ai/api/v1").unwrap_err(),
        "Use an https:// address."
    );
    assert_eq!(
        parse_base_url("https://api.x.ai/v1").unwrap_err(),
        "xAI is already built in."
    );
    assert_eq!(
        parse_base_url("https://me:pw@openrouter.ai").unwrap_err(),
        "That address isn't a plain https URL."
    );
    assert_eq!(
        parse_base_url("https://localhost").unwrap_err(),
        "That address isn't a public https URL."
    );
    assert_eq!(
        parse_base_url("https://127.0.0.1/v1").unwrap_err(),
        "That address isn't a public https URL."
    );
    assert_eq!(
        parse_base_url("https://169.254.169.254/latest").unwrap_err(),
        "That address isn't a public https URL."
    );
    assert_eq!(
        parse_base_url("https://10.0.0.5/v1").unwrap_err(),
        "That address isn't a public https URL."
    );
    let a = providers::Provider {
        id: "anthropic".into(),
        kind: ProviderKind::Anthropic,
        base_url: "https://api.anthropic.com".into(),
        added_at: 0,
    };
    assert_eq!(
        (a.models_url(), a.call_url()),
        (
            "https://api.anthropic.com/v1/models".to_string(),
            "https://api.anthropic.com/v1/messages".to_string()
        )
    );
    let (dir, _w) = setup("r3b-add");
    assert_eq!(
        add_provider(&dir, OR_URL, "short", NOW).unwrap_err(),
        "Paste the whole key."
    );
    let no_vault = test_dir("r3b-add-novault");
    assert_eq!(
        add_provider(&no_vault, OR_URL, KEY, NOW).unwrap_err(),
        "The keyring isn't available, so the key wasn't saved."
    );
    assert!(
        providers::load_providers(&no_vault).is_empty(),
        "no key saved, no provider saved"
    );
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(no_vault);
}

// ---- Routing (pure) ----------------------------------------------------------

fn xai(id: &str, completion: u64) -> ModelMeta {
    ModelMeta {
        id: id.into(),
        context_length: Some(256_000),
        prices: Prices {
            prompt: Some(completion / 5),
            completion: Some(completion),
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

/// grok-4.7 and grok-4.3 on xAI; a far cheaper OpenRouter model (efforts low/high)
/// and an Anthropic model with no prices and no effort list.
fn fleet() -> Registry {
    let mut reg = Registry::default();
    reg.entitlement.credential = Credential::Plan;
    let or = ModelMeta {
        id: OR_MODEL.into(),
        context_length: Some(128_000),
        prices: Prices {
            prompt: Some(1_000),
            completion: Some(5_000),
            ..Prices::default()
        },
        efforts: Some(vec!["low".into(), "high".into()]),
        tool_calling: Some(true),
        ..ModelMeta::default()
    };
    let an = ModelMeta {
        id: AN_MODEL.into(),
        context_length: Some(200_000),
        ..ModelMeta::default()
    };
    reg.apply_refresh(
        &[
            Listing::new(
                SourceKind::XaiApi,
                vec![xai("grok-4.7", 350_000), xai("grok-4.3", 150_000)],
            ),
            Listing::new(SourceKind::OpenAiCompatible, vec![or]),
            Listing::new(SourceKind::Anthropic, vec![an]),
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
    Spend {
        providers: ids.iter().map(|s| s.to_string()).collect(),
        ..Spend::default()
    }
}

fn ask<'a>(class: &'a str, model: &'a str, spend: Spend) -> RouteInput<'a> {
    RouteInput {
        provider: "xai",
        class,
        current_model: model,
        d: 0.5,
        needs_tools: true,
        spend,
        ..RouteInput::default()
    }
}

fn none() -> BTreeMap<String, ModelProfile> {
    BTreeMap::new()
}

#[test]
fn a_provider_is_never_picked_to_save_money_and_the_class_preference_takes_it() {
    let reg = fleet();
    let empty = RoutingTable::default();
    assert_eq!(reg.get(OR_MODEL).map(|r| r.state), Some(ModelState::Live));
    // Granted and twenty times cheaper, but Auto never takes a provider to save money.
    let r = Router::choose(
        &ask(
            "chat:default",
            "grok-4.7",
            granted(&["openrouter", "anthropic"]),
        ),
        &reg,
        &none(),
        &empty,
        NOW,
    );
    assert_eq!(r.provider.as_str(), "xai");
    assert_eq!(r.cost_class, CostClass::Included);
    assert!(
        r.model == "grok-4.7" || r.model == "grok-4.3",
        "expected an included xAI model, got {}",
        r.model
    );
    assert!(!r.model.contains('/'), "must not pick a provider model");
    // The class preference ranks it first: taken while granted, at an effort
    // from its own list.
    let pref = table_with("chat:default", OR_MODEL);
    let r = Router::choose(
        &ask("chat:default", "grok-4.7", granted(&["openrouter"])),
        &reg,
        &none(),
        &pref,
        NOW,
    );
    assert_eq!(
        (r.provider.as_str(), r.model.as_str(), r.cost_class),
        ("openrouter", OR_MODEL, CostClass::NewProvider)
    );
    assert!(
        matches!(r.effort.as_deref(), Some("low" | "high")),
        "{:?}",
        r.effort
    );
    // Never without its grant.
    let r = Router::choose(
        &ask("chat:default", "grok-4.7", Spend::default()),
        &reg,
        &none(),
        &pref,
        NOW,
    );
    assert_eq!(r.provider.as_str(), "xai");
    assert!(
        r.model == "grok-4.7" || r.model == "grok-4.3",
        "ungranted table preference falls back to included xAI, got {}",
        r.model
    );
    // No effort list in its listing: no effort is sent.
    let r = Router::choose(
        &ask("chat:default", "grok-4.7", granted(&["anthropic"])),
        &reg,
        &none(),
        &table_with("chat:default", AN_MODEL),
        NOW,
    );
    assert_eq!(
        (r.provider.as_str(), r.model.as_str(), r.effort.as_deref()),
        ("anthropic", AN_MODEL, None)
    );
}

#[test]
fn no_key_or_no_grant_means_never_a_candidate_and_xai_stands_in_for_the_episode() {
    let reg = fleet();
    let mut input = ask("chat:default", OR_MODEL, Spend::default());
    input.episode_model = Some(OR_MODEL);
    let r = Router::choose(&input, &reg, &none(), &RoutingTable::default(), NOW);
    assert_eq!(
        (r.provider.as_str(), r.cost_class),
        ("xai", CostClass::Included)
    );
    assert!(
        r.rule_ids.contains(&"provider:ungranted".to_string()),
        "{:?}",
        r.rule_ids
    );
    assert_eq!(r.replaced.as_deref(), Some(OR_MODEL));
    assert!(
        r.reason.ends_with(&format!(
            "{OR_MODEL} needs its key and your OK in Settings first."
        )),
        "{}",
        r.reason
    );
    assert_eq!(r.candidates_n, 2, "only the two xAI models are candidates");
}

/// E15: after two failures at one rung the episode moves to the strongest
/// xAI model above its own, then to the strongest provider model you set up;
/// with nothing stronger it keeps its model.
#[test]
fn e15_a_failing_episode_moves_to_a_stronger_model_one_step_at_a_time() {
    let reg = fleet();
    let next = super::ladder::Pick {
        rung: super::ladder::rung("xhigh").unwrap(),
        rules: vec!["E1:ceiling".into(), super::ladder::NEXT_MODEL.into()],
        ..super::ladder::Pick::default()
    };
    let step = |episode: &str, spend: Spend| {
        let mut input = ask("chat:default", episode, spend);
        input.episode_model = Some(episode);
        input.pick = Some(next.clone());
        let r = Router::choose(&input, &reg, &none(), &RoutingTable::default(), NOW);
        let stronger = ["model:stronger", "model:strongest_kept", "provider:auto"]
            .iter()
            .filter(|id| r.rule_ids.contains(&id.to_string()))
            .copied()
            .collect::<Vec<_>>();
        (r.model, r.provider, r.effort, stronger)
    };
    let or = granted(&["openrouter"]);
    assert_eq!(
        step("grok-4.3", or.clone()),
        (
            "grok-4.7".to_string(),
            "xai".to_string(),
            Some("xhigh".to_string()),
            vec!["model:stronger"]
        )
    );
    assert_eq!(
        step("grok-4.7", or),
        (
            OR_MODEL.to_string(),
            "openrouter".to_string(),
            Some("high".to_string()),
            vec!["model:stronger", "provider:auto"]
        )
    );
    // No provider set up for this data: the strongest xAI model keeps the step.
    assert_eq!(
        step("grok-4.7", Spend::default()),
        (
            "grok-4.7".to_string(),
            "xai".to_string(),
            Some("xhigh".to_string()),
            vec!["model:strongest_kept"]
        )
    );
    // A plain step (no E15 rule) keeps the episode's model: no cache bust.
    let mut input = ask("chat:default", "grok-4.3", granted(&["openrouter"]));
    input.episode_model = Some("grok-4.3");
    let r = Router::choose(&input, &reg, &none(), &RoutingTable::default(), NOW);
    assert_eq!(r.model, "grok-4.3");
    assert!(
        r.rule_ids.contains(&"episode:keep".to_string()),
        "{:?}",
        r.rule_ids
    );
}

#[test]
fn fallback_chains_never_include_a_provider_and_one_answers_only_when_xai_cannot() {
    let mut reg = fleet();
    // The episode's grok-4.7 rests; the granted provider is cheaper, but xAI stands in.
    reg.models.get_mut("grok-4.7").unwrap().state = ModelState::Quarantined;
    let all = granted(&["openrouter", "anthropic"]);
    let mut input = ask("chat:default", "grok-4.7", all);
    input.episode_model = Some("grok-4.7");
    let r = Router::choose(&input, &reg, &none(), &RoutingTable::default(), NOW);
    assert_eq!((r.model.as_str(), r.provider.as_str()), ("grok-4.3", "xai"));
    assert!(
        r.rule_ids.contains(&"episode:fallback".to_string()),
        "{:?}",
        r.rule_ids
    );
    // Nothing on xAI answers: the priced provider model you set up takes the
    // step (the Anthropic one lists no price, so it stays out).
    reg.models.get_mut("grok-4.3").unwrap().state = ModelState::Quarantined;
    let r = Router::choose(&input, &reg, &none(), &RoutingTable::default(), NOW);
    assert_eq!(
        (r.model.as_str(), r.provider.as_str(), r.no_route),
        (OR_MODEL, "openrouter", false),
        "{:?}",
        r.rule_ids
    );
    assert!(
        r.rule_ids.contains(&"provider:auto".to_string()),
        "{:?}",
        r.rule_ids
    );
}

/// A tiny deterministic generator, so the property test needs no new crate.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
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
    grant_destination(
        &named,
        OR_DEST,
        CALL_DATA_SENSITIVE,
        UserClick::from_click(),
    )
    .unwrap();
    let (keyless, _w3) = setup("r3b-prop-keyless");
    grant_destination(
        &keyless,
        OR_DEST,
        CALL_DATA_SENSITIVE,
        UserClick::from_click(),
    )
    .unwrap();
    let gate = |dir: &Path, sensitive: bool| usable(dir, call_data(sensitive));
    assert!(
        gate(&plain, true).is_empty()
            && !gate(&plain, false).is_empty()
            && !gate(&named, true).is_empty()
            && gate(&keyless, false).is_empty()
    );
    let reg = fleet();
    let classes: Vec<&str> = CLASS_TABLE.iter().map(|r| r.class).collect();
    let models = ["grok-4.7", "grok-4.3", OR_MODEL, AN_MODEL];
    let mut rng = Lcg(0x5eed_003b);
    let mut sent_to_provider = 0;
    for _ in 0..1_500 {
        let class = *rng.pick(&classes);
        let model = *rng.pick(&models);
        let sensitive = rng.coin();
        let shape = rng.next() % 3;
        let dir = [&plain, &named, &keyless][shape as usize];
        let table = if rng.coin() {
            table_with(class, OR_MODEL)
        } else {
            RoutingTable::default()
        };
        let mut input = ask(
            class,
            model,
            Spend {
                providers: gate(dir, sensitive),
                ..Spend::default()
            },
        );
        input.d = (rng.next() % 100) as f64 / 100.0;
        let r = Router::choose(&input, &reg, &none(), &table, NOW);
        if r.cost_class != CostClass::NewProvider {
            continue;
        }
        sent_to_provider += 1;
        assert!(
            !(sensitive && shape == 0),
            "sensitive data left for a provider without a grant naming it: {class} {model}"
        );
        assert_ne!(shape, 2, "no key, yet routed to a provider");
        // xAI answers in this fleet, so a provider is reached only where the
        // routing table ranks it for the step.
        assert!(
            table.row(class, &r.model).is_some(),
            "only the class preference: {class} {model} {:?}",
            r.rule_ids
        );
        assert!(
            !r.rule_ids
                .iter()
                .any(|x| x.ends_with(":fallback") || x == "model:cheapest"),
            "{:?}",
            r.rule_ids
        );
    }
    assert!(
        sent_to_provider > 100,
        "the property ran on real provider picks: {sent_to_provider}"
    );
    for d in [plain, named, keyless] {
        let _ = std::fs::remove_dir_all(d);
    }
}

#[test]
fn the_self_tuner_never_promotes_a_provider() {
    let reg = fleet();
    let profiles = none();
    let f = RegistryFilters {
        reg: &reg,
        profiles: &profiles,
        spend: granted(&["openrouter", "anthropic"]),
        now_ms: NOW,
    };
    assert!(f.allowed("chat:default", "grok-4.3"));
    assert!(
        !f.allowed("chat:default", OR_MODEL),
        "never for cost or latency, even granted"
    );
    assert!(!f.allowed("background:summarize", AN_MODEL));
    assert!(class_row("chat:default").is_some());
}

// ---- Effort mapping and the wire ----------------------------------------------

/// System, user, a tool call and its output, and one function tool plus a hosted one.
fn turn(effort: &str) -> ResponsesRequest {
    ResponsesRequest {
        text_format: None,
        model: "m".into(),
        effort: Some(effort.into()),
        input: vec![
            InputItem::Message {
                role: "system".into(),
                content: vec![ContentPart::InputText("be brief".into())],
            },
            InputItem::Message {
                role: "user".into(),
                content: vec![ContentPart::InputText("weather?".into())],
            },
            InputItem::FunctionCall {
                call_id: "c1".into(),
                name: "weather".into(),
                arguments: r#"{"city":"Oslo"}"#.into(),
            },
            InputItem::FunctionCallOutput {
                call_id: "c1".into(),
                output: "4C".into(),
            },
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
        responses_body(&ResponsesRequest {
            model: "grok-4.7".into(),
            ..xai.clone()
        })
        .to_string(),
        r#"{"include":["reasoning.encrypted_content"],"input":[{"content":[{"text":"be brief","type":"input_text"}],"role":"system","type":"message"},{"content":[{"text":"weather?","type":"input_text"}],"role":"user","type":"message"},{"arguments":"{\"city\":\"Oslo\"}","call_id":"c1","name":"weather","type":"function_call"},{"call_id":"c1","output":"4C","type":"function_call_output"}],"model":"grok-4.7","parallel_tool_calls":true,"prompt_cache_key":"conv","reasoning":{"effort":"high"},"store":false,"stream":true,"tools":[{"description":"Look it up","name":"weather","parameters":{"properties":{"city":{"type":"string"}},"type":"object"},"type":"function"},{"type":"web_search"}]}"#
    );
    // Anthropic: `output_config.effort`, tool_use / tool_result blocks, the
    // system prompt apart — and prompt-cache marks on the system text, the
    // last tool and the last message block (failover PR 8).
    assert_eq!(
        request_body(
            ProviderKind::Anthropic,
            "model-x",
            Some("high"),
            &xai,
            Some(32_000)
        )
        .to_string(),
        r#"{"max_tokens":8192,"messages":[{"content":[{"text":"weather?","type":"text"}],"role":"user"},{"content":[{"id":"c1","input":{"city":"Oslo"},"name":"weather","type":"tool_use"}],"role":"assistant"},{"content":[{"cache_control":{"type":"ephemeral"},"content":"4C","tool_use_id":"c1","type":"tool_result"}],"role":"user"}],"model":"model-x","output_config":{"effort":"high"},"system":[{"cache_control":{"type":"ephemeral"},"text":"be brief","type":"text"}],"tools":[{"cache_control":{"type":"ephemeral"},"description":"Look it up","input_schema":{"properties":{"city":{"type":"string"}},"type":"object"},"name":"weather"}]}"#
    );
    // OpenAI-compatible: `reasoning.effort`, chat messages, nested function tools; hosted tools dropped.
    assert_eq!(
        request_body(
            ProviderKind::OpenAiCompatible,
            "vendor/cheap",
            Some("low"),
            &xai,
            None
        )
        .to_string(),
        r#"{"messages":[{"content":"be brief","role":"system"},{"content":"weather?","role":"user"},{"content":null,"role":"assistant","tool_calls":[{"function":{"arguments":"{\"city\":\"Oslo\"}","name":"weather"},"id":"c1","type":"function"}]},{"content":"4C","role":"tool","tool_call_id":"c1"}],"model":"vendor/cheap","reasoning":{"effort":"low"},"stream":false,"tools":[{"function":{"description":"Look it up","name":"weather","parameters":{"properties":{"city":{"type":"string"}},"type":"object"}},"type":"function"}]}"#
    );
    // No effort: no field at all.
    let none = request_body(ProviderKind::Anthropic, "model-x", None, &xai, None);
    let open = request_body(
        ProviderKind::OpenAiCompatible,
        "vendor/cheap",
        None,
        &xai,
        None,
    );
    assert!(none.get("output_config").is_none() && open.get("reasoning").is_none());
    assert_eq!(
        (
            ProviderKind::Anthropic.effort_field(),
            ProviderKind::OpenAiCompatible.effort_field()
        ),
        ("output_config.effort", "reasoning.effort")
    );
}

#[test]
fn replies_map_back_to_turns() {
    let t = providers::parse_openai_reply(r#"{"choices":[{"message":{"content":null,"tool_calls":[{"id":"c9","type":"function","function":{"name":"weather","arguments":"{}"}}]}}],"usage":{"prompt_tokens":10,"completion_tokens":3,"completion_tokens_details":{"reasoning_tokens":1},"prompt_tokens_details":{"cached_tokens":4},"cost":0.001}}"#).unwrap();
    assert_eq!(
        (
            t.calls[0].call_id.as_str(),
            t.calls[0].name.as_str(),
            t.text.as_str()
        ),
        ("c9", "weather", "")
    );
    assert_eq!(
        (
            t.usage.input_tokens,
            t.usage.output_tokens,
            t.usage.reasoning_tokens,
            t.usage.cached_tokens,
            t.usage.cost_in_usd_ticks
        ),
        (10, 3, 1, 4, 10_000_000)
    );
    let a = providers::parse_anthropic_reply(r#"{"content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"Hi"},{"type":"tool_use","id":"t1","name":"weather","input":{"city":"Oslo"}}],"usage":{"input_tokens":7,"output_tokens":2,"cache_read_input_tokens":5}}"#).unwrap();
    assert_eq!(
        (
            a.text.as_str(),
            a.reasoning.as_str(),
            a.calls[0].arguments.as_str()
        ),
        ("Hi", "hmm", r#"{"city":"Oslo"}"#)
    );
    assert_eq!(
        (
            a.usage.input_tokens,
            a.usage.output_tokens,
            a.usage.cached_tokens
        ),
        (7, 2, 5)
    );
    assert!(providers::parse_anthropic_reply("{}").is_err());
}

/// E14: a chat-completions or messages reply cut at the output limit is a
/// truncated turn; with no answer text it reads as a spent reasoning budget.
#[test]
fn e14_a_length_cut_with_no_answer_reads_as_a_spent_budget() {
    let empty = providers::parse_openai_reply(
        r#"{"choices":[{"finish_reason":"length","message":{"content":"","reasoning_content":"thinking and thinking"}}],"usage":{"prompt_tokens":9,"completion_tokens":4096}}"#,
    )
    .unwrap();
    assert_eq!(
        (
            empty.truncated,
            empty.truncate_reason.as_deref(),
            empty.out_of_budget()
        ),
        (true, Some("max_output_tokens"), true)
    );
    let partial = providers::parse_openai_reply(
        r#"{"choices":[{"finish_reason":"length","message":{"content":"Here is the first half"}}]}"#,
    )
    .unwrap();
    assert_eq!((partial.truncated, partial.out_of_budget()), (true, false));
    let done = providers::parse_openai_reply(
        r#"{"choices":[{"finish_reason":"stop","message":{"content":""}}]}"#,
    )
    .unwrap();
    assert_eq!((done.truncated, done.out_of_budget()), (false, false));
    let cut = providers::parse_anthropic_reply(
        r#"{"content":[{"type":"thinking","thinking":"hmm"}],"stop_reason":"max_tokens","usage":{"input_tokens":3,"output_tokens":2048}}"#,
    )
    .unwrap();
    assert_eq!(
        (
            cut.truncated,
            cut.truncate_reason.as_deref(),
            cut.out_of_budget()
        ),
        (true, Some("max_output_tokens"), true)
    );
    let ended = providers::parse_anthropic_reply(
        r#"{"content":[{"type":"text","text":"Hi"}],"stop_reason":"end_turn"}"#,
    )
    .unwrap();
    assert!(!ended.truncated);
    // The Responses API: status incomplete for max_output_tokens, no text.
    let xai = TurnOutput {
        truncated: true,
        truncate_reason: Some("max_output_tokens".into()),
        reasoning: "long".into(),
        ..TurnOutput::default()
    };
    assert!(xai.out_of_budget());
    // Cut for another reason (a content filter) is not a spent budget.
    let filtered = TurnOutput {
        truncate_reason: Some("content_filter".into()),
        ..xai.clone()
    };
    assert!(!filtered.out_of_budget());
}

/// Failover PR 8: a refusal is its own error class, never a silent
/// empty answer, and a server-tool pause is an ordinary ending.
#[test]
fn an_anthropic_refusal_is_its_own_error_and_a_pause_is_an_ordinary_end() {
    let refused = providers::parse_anthropic_reply(
        r#"{"content":[],"stop_reason":"refusal","stop_details":{"type":"refusal","category":"bio"},"usage":{"input_tokens":3,"output_tokens":0}}"#,
    );
    match refused {
        Err(ClientError::Refused { message }) => assert!(message.contains("bio"), "{message}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    let paused = providers::parse_anthropic_reply(
        r#"{"content":[{"type":"text","text":"half the search is in"}],"stop_reason":"pause_turn","usage":{"input_tokens":3,"output_tokens":9}}"#,
    )
    .unwrap();
    assert_eq!(
        (paused.text.as_str(), paused.truncated),
        ("half the search is in", false)
    );
    let tools = providers::parse_anthropic_reply(
        r#"{"content":[{"type":"tool_use","id":"t1","name":"weather","input":{}}],"stop_reason":"tool_use"}"#,
    )
    .unwrap();
    assert_eq!((tools.calls.len(), tools.truncated), (1, false));
}

/// A fake wire that answers a POST as an event stream: the canned SSE
/// body is handed over as a reader, the way the real wire streams it,
/// with one rate-limit header riding along.
struct SseWire {
    posts: Mutex<Vec<String>>,
    sse: String,
    status: u16,
}

impl ProviderTransport for SseWire {
    fn get(&self, _url: &str, _headers: &[(String, String)]) -> Result<HttpReply, String> {
        Ok(HttpReply {
            status: 200,
            body: OR_LIST.into(),
            headers: Vec::new(),
        })
    }

    fn post(
        &self,
        _url: &str,
        _headers: &[(String, String)],
        body: &str,
    ) -> Result<HttpReply, String> {
        self.posts.lock().unwrap().push(body.into());
        Ok(HttpReply {
            status: self.status,
            body: self.sse.clone(),
            headers: Vec::new(),
        })
    }

    fn post_streamed(
        &self,
        _url: &str,
        _headers: &[(String, String)],
        body: &str,
        _limit: Option<std::time::Duration>,
    ) -> Result<HttpStream, String> {
        self.posts.lock().unwrap().push(body.into());
        Ok(HttpStream {
            status: self.status,
            headers: vec![("anthropic-ratelimit-requests-remaining".into(), "9".into())],
            reader: Box::new(std::io::Cursor::new(self.sse.clone().into_bytes())),
        })
    }
}

/// A config dir whose Anthropic provider answers with the canned stream.
fn sse_setup(label: &str, sse: &str) -> (PathBuf, Arc<SseWire>) {
    let dir = test_dir(label);
    providers::use_vault_for(&dir, Arc::new(MemoryVault::default()));
    let wire = Arc::new(SseWire {
        posts: Mutex::new(Vec::new()),
        sse: sse.into(),
        status: 200,
    });
    providers::use_transport_for(&dir, wire.clone());
    add_provider(&dir, "https://api.anthropic.com", KEY, NOW).unwrap();
    grant_destination(
        &dir,
        "api.anthropic.com",
        CALL_DATA,
        UserClick::from_click(),
    )
    .unwrap();
    (dir, wire)
}

fn stream_call(dir: &Path, shown: &mut Vec<String>) -> Result<TurnOutput, ClientError> {
    providers::stream_model(
        dir,
        "chat:default",
        AN_MODEL,
        Some("high"),
        &hello(),
        CALL_DATA,
        "conv-1:11",
        &CancelToken::new(),
        &mut |ev| {
            if let StreamEvent::TextDelta(t) = ev {
                shown.push(t);
            }
        },
    )
}

/// Failover PR 8: a streamed Claude call reaches the chat delta by
/// delta, assembles its tool call, counts cache reads and writes into
/// the spend ledger, and asks for the stream with cache marks on.
#[test]
fn a_streamed_claude_call_arrives_in_deltas_and_notes_its_spend() {
    let sse = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"usage\":{\"input_tokens\":100,\"cache_creation_input_tokens\":25,\"cache_read_input_tokens\":40,\"output_tokens\":1}}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello \"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"there\"}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"weather\",\"input\":{}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"city\\\":\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"Oslo\\\"}\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":7}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );
    let (dir, wire) = sse_setup("anthropic-stream", sse);
    save_registry(&dir, &fleet()).unwrap();
    let mut shown: Vec<String> = Vec::new();
    let out = stream_call(&dir, &mut shown).unwrap();
    // The chat saw the answer arrive in pieces, not as one block.
    assert_eq!(shown, ["Hello ", "there"]);
    assert_eq!(out.text, "Hello there");
    assert_eq!(
        (
            out.calls[0].call_id.as_str(),
            out.calls[0].name.as_str(),
            out.calls[0].arguments.as_str()
        ),
        ("t1", "weather", r#"{"city":"Oslo"}"#)
    );
    assert_eq!(
        (
            out.usage.input_tokens,
            out.usage.output_tokens,
            out.usage.cached_tokens
        ),
        (100, 7, 40)
    );
    // The request asked for the stream, with a cache mark on the last block.
    let body: serde_json::Value = serde_json::from_str(&wire.posts.lock().unwrap()[0]).unwrap();
    assert_eq!(body["stream"], json!(true));
    assert_eq!(
        body["messages"][0]["content"][0]["cache_control"],
        json!({"type": "ephemeral"})
    );
    // The spend ledger counted fresh + cache read + cache write input,
    // and kept the rate-limit header the reply carried.
    let ledger = grokhub_core::model_registry::store::load_quota(&dir);
    assert_eq!(ledger.anthropic.spend.day_input_tokens, 100 + 40 + 25);
    assert_eq!(ledger.anthropic.spend.day_output_tokens, 7);
    assert_eq!(ledger.anthropic.rate.requests.remaining, Some(9));
    let _ = std::fs::remove_dir_all(dir);
}

/// Failover PR 8: the streamed path keeps the native stream's failure
/// rules — a failure after text showed is a cut-off, one before is the
/// error itself (a disconnect the router can fail over from), and a
/// streamed refusal is the refusal class.
#[test]
fn a_streamed_claude_failure_follows_the_shown_rules() {
    // Text showed, then the stream errored: what showed is kept.
    let (dir, _w) = sse_setup(
        "anthropic-stream-cutoff",
        concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":5,\"output_tokens\":1}}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"partial\"}}\n\n",
            "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"overloaded\"}}\n\n",
        ),
    );
    let mut shown: Vec<String> = Vec::new();
    let err = stream_call(&dir, &mut shown).unwrap_err();
    assert_eq!(shown, ["partial"]);
    assert!(matches!(err, ClientError::CutOff(_)), "{err:?}");
    let _ = std::fs::remove_dir_all(dir);

    // Nothing showed and the stream just ended: a disconnect.
    let (dir, _w) = sse_setup(
        "anthropic-stream-dropped",
        "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":5,\"output_tokens\":1}}}\n\n",
    );
    let mut shown: Vec<String> = Vec::new();
    let err = stream_call(&dir, &mut shown).unwrap_err();
    assert!(matches!(err, ClientError::Disconnect), "{err:?}");
    let _ = std::fs::remove_dir_all(dir);

    // A refusal riding the stream is the refusal class, not an empty turn.
    let (dir, _w) = sse_setup(
        "anthropic-stream-refusal",
        concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":5,\"output_tokens\":1}}}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"refusal\",\"stop_details\":{\"type\":\"refusal\",\"category\":\"sexual\"}},\"usage\":{\"output_tokens\":2}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        ),
    );
    let mut shown: Vec<String> = Vec::new();
    let err = stream_call(&dir, &mut shown).unwrap_err();
    assert!(matches!(err, ClientError::Refused { .. }), "{err:?}");
    let _ = std::fs::remove_dir_all(dir);
}

/// A config dir whose OpenRouter provider answers with the canned stream.
fn or_sse_setup(label: &str, sse: &str) -> (PathBuf, Arc<SseWire>) {
    let dir = test_dir(label);
    providers::use_vault_for(&dir, Arc::new(MemoryVault::default()));
    let wire = Arc::new(SseWire {
        posts: Mutex::new(Vec::new()),
        sse: sse.into(),
        status: 200,
    });
    providers::use_transport_for(&dir, wire.clone());
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    (dir, wire)
}

fn or_stream_call(
    dir: &Path,
    shown: &mut Vec<String>,
    thought: &mut Vec<String>,
) -> Result<TurnOutput, ClientError> {
    providers::stream_model(
        dir,
        "chat:default",
        OR_MODEL,
        Some("high"),
        &hello(),
        CALL_DATA,
        "conv-1:12",
        &CancelToken::new(),
        &mut |ev| match ev {
            StreamEvent::TextDelta(t) => shown.push(t),
            StreamEvent::ReasoningDelta(t) => thought.push(t),
            _ => {}
        },
    )
}

/// Failover PR 10: a streamed OpenRouter call reaches the chat delta by
/// delta (keep-alive comments ignored), assembles its tool call from
/// indexed pieces, merges its reasoning details, reads the usage chunk
/// that comes after the finish reason (cost included), gives the
/// reasoning back to the model the chunks name, and asks for the
/// stream with its usage on.
#[test]
fn a_streamed_openrouter_call_arrives_in_deltas_with_its_tools_and_cost() {
    let sse = concat!(
        ": OPENROUTER PROCESSING\n\n",
        "data: {\"model\":\"vendor/cheap\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"reasoning\":\"Think\",\"reasoning_details\":[{\"type\":\"reasoning.text\",\"text\":\"Think\",\"index\":0}]}}]}\n\n",
        "data: {\"model\":\"vendor/cheap\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning\":\"ing\",\"reasoning_details\":[{\"type\":\"reasoning.text\",\"text\":\"ing\",\"signature\":\"sig\",\"index\":0}]}}]}\n\n",
        "data: {\"model\":\"vendor/cheap\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello \"}}]}\n\n",
        "data: {\"model\":\"vendor/cheap\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"there\"}}]}\n\n",
        "data: {\"model\":\"vendor/cheap\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"type\":\"function\",\"function\":{\"name\":\"weather\",\"arguments\":\"\"}}]}}]}\n\n",
        "data: {\"model\":\"vendor/cheap\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"city\\\":\"}}]}}]}\n\n",
        "data: {\"model\":\"vendor/cheap\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"Oslo\\\"}\"}}]}}]}\n\n",
        "data: {\"model\":\"vendor/cheap\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: {\"model\":\"vendor/cheap\",\"choices\":[],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":4,\"completion_tokens_details\":{\"reasoning_tokens\":2},\"prompt_tokens_details\":{\"cached_tokens\":3},\"cost\":0.0000025}}\n\n",
        "data: [DONE]\n\n",
    );
    let (dir, wire) = or_sse_setup("or-stream", sse);
    let (mut shown, mut thought) = (Vec::new(), Vec::new());
    let out = or_stream_call(&dir, &mut shown, &mut thought).unwrap();
    assert_eq!(shown, ["Hello ", "there"]);
    assert_eq!(thought, ["Think", "ing"]);
    assert_eq!(
        (out.text.as_str(), out.reasoning.as_str()),
        ("Hello there", "Thinking")
    );
    assert_eq!(
        (
            out.calls[0].call_id.as_str(),
            out.calls[0].name.as_str(),
            out.calls[0].arguments.as_str()
        ),
        ("c1", "weather", r#"{"city":"Oslo"}"#)
    );
    assert_eq!(
        (
            out.usage.input_tokens,
            out.usage.output_tokens,
            out.usage.reasoning_tokens,
            out.usage.cached_tokens,
            out.usage.cost_in_usd_ticks
        ),
        (12, 4, 2, 3, 25_000)
    );
    // The reasoning details merged into one piece, and they go back to
    // the model that wrote them.
    assert_eq!(out.reasoning_items.len(), 1);
    let details = out.reasoning_items[0]
        .reasoning_details("vendor/cheap")
        .expect("tagged with the answering model");
    assert_eq!(
        (
            details[0]["text"].as_str(),
            details[0]["signature"].as_str()
        ),
        (Some("Thinking"), Some("sig"))
    );
    assert!(!out.truncated);
    let body: serde_json::Value = serde_json::from_str(&wire.posts.lock().unwrap()[0]).unwrap();
    assert_eq!(
        (&body["stream"], &body["stream_options"]),
        (&json!(true), &json!({"include_usage": true}))
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// Failover PR 10: the streamed OpenRouter path keeps the native
/// stream's failure rules — an error chunk before any text is the error
/// itself (a rate limit the router fails over from, stamped on the
/// quota ledger), one after text showed is a cut-off, a stream with no
/// finish reason is a disconnect, a cut at the output limit is a
/// truncation, and a host that answered with the whole reply is read
/// whole.
#[test]
fn a_streamed_openrouter_failure_follows_the_shown_rules() {
    // An error chunk before anything showed: the error class itself.
    let (dir, _w) = or_sse_setup(
        "or-stream-limit",
        concat!(
            ": OPENROUTER PROCESSING\n\n",
            "data: {\"error\":{\"code\":429,\"message\":\"Rate limit exceeded: free-models-per-day\"},\"choices\":[{\"index\":0,\"delta\":{\"content\":\"\"},\"finish_reason\":\"error\"}]}\n\n",
        ),
    );
    let (mut shown, mut thought) = (Vec::new(), Vec::new());
    let err = or_stream_call(&dir, &mut shown, &mut thought).unwrap_err();
    assert!(shown.is_empty());
    assert!(matches!(err, ClientError::RateLimited { .. }), "{err:?}");
    // The limit is account news: the ledger asks for a re-read at once.
    let ledger = grokhub_core::model_registry::store::load_quota(&dir);
    assert!(ledger.openrouter.recheck_at_ms > 0);
    let _ = std::fs::remove_dir_all(dir);

    // Text showed, then the provider failed mid-stream: what showed is kept.
    let (dir, _w) = or_sse_setup(
        "or-stream-cutoff",
        concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"}}]}\n\n",
            "data: {\"error\":{\"code\":502,\"message\":\"Provider returned error\"},\"choices\":[{\"index\":0,\"delta\":{\"content\":\"\"},\"finish_reason\":\"error\"}]}\n\n",
        ),
    );
    let (mut shown, mut thought) = (Vec::new(), Vec::new());
    let err = or_stream_call(&dir, &mut shown, &mut thought).unwrap_err();
    assert_eq!(shown, ["partial"]);
    assert!(matches!(err, ClientError::CutOff(_)), "{err:?}");
    let _ = std::fs::remove_dir_all(dir);

    // Nothing showed and the stream just ended: a disconnect.
    let (dir, _w) = or_sse_setup("or-stream-dropped", ": OPENROUTER PROCESSING\n\n");
    let (mut shown, mut thought) = (Vec::new(), Vec::new());
    let err = or_stream_call(&dir, &mut shown, &mut thought).unwrap_err();
    assert!(matches!(err, ClientError::Disconnect), "{err:?}");
    let _ = std::fs::remove_dir_all(dir);

    // Cut at the output limit: a truncation the run loop carries on from.
    let (dir, _w) = or_sse_setup(
        "or-stream-length",
        concat!(
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"long\"}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"length\"}]}\n\n",
            "data: [DONE]\n\n",
        ),
    );
    let (mut shown, mut thought) = (Vec::new(), Vec::new());
    let out = or_stream_call(&dir, &mut shown, &mut thought).unwrap();
    assert_eq!(
        (
            out.text.as_str(),
            out.truncated,
            out.truncate_reason.as_deref()
        ),
        ("long", true, Some("max_output_tokens"))
    );
    let _ = std::fs::remove_dir_all(dir);

    // The whole reply instead of a stream: read whole, sent as one delta.
    let (dir, _w) = or_sse_setup("or-stream-whole", OR_REPLY);
    let (mut shown, mut thought) = (Vec::new(), Vec::new());
    let out = or_stream_call(&dir, &mut shown, &mut thought).unwrap();
    assert_eq!(shown, ["hello from the provider"]);
    assert_eq!(out.usage.cost_in_usd_ticks, 25_000);
    let _ = std::fs::remove_dir_all(dir);
}

/// Failover PR 10: an OpenAI-compatible host other than OpenRouter keeps
/// the whole-reply call, its answer one delta, with no stream asked for.
#[test]
fn a_custom_openai_compatible_host_keeps_the_whole_reply() {
    let dir = test_dir("custom-host-whole");
    providers::use_vault_for(&dir, Arc::new(MemoryVault::default()));
    let wire = Arc::new(SseWire {
        posts: Mutex::new(Vec::new()),
        sse: OR_REPLY.into(),
        status: 200,
    });
    providers::use_transport_for(&dir, wire.clone());
    let p = add_provider(&dir, "https://api.example.com/v1", KEY, NOW).unwrap();
    grant_destination(&dir, "api.example.com", CALL_DATA, UserClick::from_click()).unwrap();
    let model = format!("{}/vendor/cheap", p.id);
    let mut shown: Vec<String> = Vec::new();
    let out = providers::stream_model(
        &dir,
        "chat:default",
        &model,
        None,
        &hello(),
        CALL_DATA,
        "conv-1:13",
        &CancelToken::new(),
        &mut |ev| {
            if let StreamEvent::TextDelta(t) = ev {
                shown.push(t);
            }
        },
    )
    .unwrap();
    assert_eq!(
        (out.text.as_str(), shown.len()),
        ("hello from the provider", 1)
    );
    let body: serde_json::Value = serde_json::from_str(&wire.posts.lock().unwrap()[0]).unwrap();
    assert_eq!(
        (&body["stream"], body.get("stream_options")),
        (&json!(false), None)
    );
    let _ = std::fs::remove_dir_all(dir);
}

// ---- End to end: the route record, and the key is nowhere ---------------------

struct Never;

impl ModelClient for Never {
    fn stream(
        &self,
        _r: &ResponsesRequest,
        _c: &CancelToken,
        _s: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
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
fn a_provider_call_the_table_ranks_first_routes_logs_its_provider_and_the_key_is_in_no_file_or_input(
) {
    let (dir, wire) = setup("r3b-e2e");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet()).unwrap();
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    let g = grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    rank_first(&dir, OR_MODEL);
    let input = vec![InputItem::Message {
        role: "user".into(),
        content: vec![ContentPart::InputText("say hello".into())],
    }];
    let call = super::ModelCall::xai(OR_MODEL, Some("medium"), "chat:default", "ep-r3b", input)
        .in_session("chat-r3b");
    let routed = super::call_model(&Never, &call, &CancelToken::new()).unwrap();
    assert_eq!(routed.out.text, "hello from the provider");
    assert_eq!(routed.tokens.cost_ticks, 25_000);
    // One request, one egress line under the grant, with the span id.
    assert_eq!(wire.n(), 1);
    let log = read_egress(&dir);
    assert_eq!(log.len(), 1);
    assert_eq!(
        (
            log[0].grant_id.as_str(),
            log[0].span_id.starts_with("chat-r3b:")
        ),
        (g.id.as_str(), true)
    );
    // The route record carries the provider and the cost class.
    let (_, rec) = route_records(&dir, ROUTE_TAIL_LINES)
        .pop()
        .expect("a route record");
    assert_eq!(
        (
            rec.chosen.provider.as_str(),
            rec.used.provider.as_str(),
            rec.cost_class.as_str()
        ),
        ("openrouter", "openrouter", "new_provider")
    );
    assert_eq!(rec.used.model, OR_MODEL);
    // The key: only in the auth header, never in the body (model input), a span, the ledger, a log or an egress line.
    let (_, headers, body) = wire.0.lock().unwrap()[0].clone();
    assert!(headers
        .iter()
        .any(|(k, v)| k == "Authorization" && v == &format!("Bearer {KEY}")));
    assert!(!body.contains(KEY) && wire.bodies().iter().all(|b| !b.contains(KEY)));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["model"],
        "vendor/cheap"
    );
    let mut files = Vec::new();
    files_under(&dir, &mut files);
    assert!(files.len() >= 4, "{files:?}");
    for f in files {
        let bytes = std::fs::read(&f).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains(KEY) && !text.contains("abcdefghijklmnopqrstuv"),
            "the key is in {}",
            f.display()
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// OpenRouter's key exchange, faked: a 200 with a key for `good-code`, else a 403.
#[derive(Default)]
struct KeyWire(Mutex<Vec<Sent>>);

impl ProviderTransport for KeyWire {
    fn get(&self, url: &str, _: &[(String, String)]) -> Result<HttpReply, String> {
        Err(format!("no GET expected: {url}"))
    }
    fn post(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &str,
    ) -> Result<HttpReply, String> {
        self.0
            .lock()
            .unwrap()
            .push((url.into(), headers.to_vec(), body.into()));
        if body.contains(r#""code":"good-code""#) {
            Ok(HttpReply {
                status: 200,
                body: json!({"key": KEY, "user_id": "u-1"}).to_string(),
                headers: Vec::new(),
            })
        } else {
            Ok(HttpReply {
                status: 403,
                body: "{}".into(),
                headers: Vec::new(),
            })
        }
    }
}

/// The browser: approve at once by calling the `callback_url` with `code`,
/// echoing the `state` the link carried, as OpenRouter does.
fn approving_browser(
    code: &'static str,
    seen: Arc<Mutex<String>>,
) -> impl Fn(&str) -> Result<(), String> {
    move |link: &str| {
        *seen.lock().unwrap() = link.to_string();
        let query = link.split_once('?').map(|(_, q)| q).unwrap_or("");
        let callback = query
            .split('&')
            .find_map(|p| p.strip_prefix("callback_url="))
            .unwrap_or("")
            .replace("%3A", ":")
            .replace("%2F", "/");
        let state = query
            .split('&')
            .find_map(|p| p.strip_prefix("state="))
            .unwrap_or("")
            .to_string();
        let target = callback.trim_start_matches("http://").to_string();
        let (host, path) = target
            .split_once('/')
            .map(|(h, p)| (h.to_string(), p.to_string()))
            .unwrap();
        std::thread::spawn(move || {
            use std::io::{Read, Write};
            let mut sock = std::net::TcpStream::connect(&host).unwrap();
            let _ = write!(
                sock,
                "GET /{path}?code={code}&state={state} HTTP/1.1\r\nHost: {host}\r\n\r\n"
            );
            let mut sink = String::new();
            let _ = sock.read_to_string(&mut sink);
        });
        Ok(())
    }
}

#[test]
fn openrouter_sign_in_buys_a_key_with_pkce_and_saves_nothing_itself() {
    let dir = test_dir("r3b-openrouter-signin");
    providers::use_vault_for(&dir, Arc::new(MemoryVault::default()));
    let wire = Arc::new(KeyWire::default());
    providers::use_transport_for(&dir, wire.clone());
    let seen = Arc::new(Mutex::new(String::new()));
    let key = providers::openrouter_key(
        &dir,
        &approving_browser("good-code", seen.clone()),
        std::time::Duration::from_secs(10),
    )
    .unwrap();
    assert_eq!(key.as_str(), KEY);
    let link = seen.lock().unwrap().clone();
    assert!(
        link.starts_with("https://openrouter.ai/auth?callback_url=http%3A%2F%2F127.0.0.1%3A"),
        "{link}"
    );
    assert!(
        link.contains("%2Fcallback&code_challenge=")
            && link.contains("&code_challenge_method=S256&state=")
            && link.ends_with("&key_label=Loadout"),
        "{link}"
    );
    let sent = wire.0.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "https://openrouter.ai/api/v1/auth/keys");
    let body: serde_json::Value = serde_json::from_str(&sent[0].2).unwrap();
    assert_eq!(
        (
            body["code"].as_str(),
            body["code_challenge_method"].as_str()
        ),
        (Some("good-code"), Some("S256"))
    );
    // The verifier sent is the one the challenge in the link was made from.
    let challenge = link
        .split("code_challenge=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap();
    assert_eq!(
        grokhub_core::pkce::pkce_challenge(body["code_verifier"].as_str().unwrap()),
        challenge
    );
    // One handshake line, no user data; nothing saved, no provider added.
    let log = read_egress(&dir);
    assert_eq!(log.len(), 1, "{log:?}");
    assert_eq!(
        (log[0].dest.as_str(), log[0].data_classes.len()),
        ("openrouter.ai", 0)
    );
    assert!(!providers::has_key(&dir, "openrouter") && providers::load_providers(&dir).is_empty());
    // A code OpenRouter refuses buys nothing.
    let err = providers::openrouter_key(
        &dir,
        &approving_browser("bad-code", seen),
        std::time::Duration::from_secs(10),
    )
    .unwrap_err();
    assert_eq!(err, "OpenRouter didn't hand over a key (HTTP 403)");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn key_pages_and_the_api_shape_pick_follow_the_address() {
    assert_eq!(
        providers::key_page("https://api.openai.com/v1"),
        Some("https://platform.openai.com/api-keys")
    );
    assert_eq!(
        providers::key_page("https://api.anthropic.com"),
        Some("https://console.anthropic.com/settings/keys")
    );
    assert_eq!(
        providers::key_page(OR_URL),
        Some("https://openrouter.ai/settings/keys")
    );
    assert_eq!(
        providers::key_page("https://api.groq.com/openai/v1"),
        Some("https://console.groq.com/keys")
    );
    assert_eq!(providers::key_page("https://llm.example.com/v1"), None);
    assert_eq!(
        providers::key_page("https://notopenai.com/v1"),
        None,
        "a lookalike host is not OpenAI"
    );
    let (dir, _wire) = setup("r3b-kind-pick");
    let detected =
        providers::add_provider_as(&dir, "https://llm.example.com/v1", KEY, None, NOW).unwrap();
    assert_eq!(
        (detected.id.as_str(), detected.kind),
        ("llm", ProviderKind::OpenAiCompatible)
    );
    let proxy = providers::add_provider_as(
        &dir,
        "https://claude-proxy.example.org",
        KEY,
        Some(ProviderKind::Anthropic),
        NOW,
    )
    .unwrap();
    assert_eq!(
        (proxy.kind, proxy.models_url().as_str()),
        (
            ProviderKind::Anthropic,
            "https://claude-proxy.example.org/v1/models"
        )
    );
    assert_eq!(
        providers::check_key("short"),
        Err("Paste the whole key.".to_string())
    );
    assert_eq!(providers::check_key(&format!("  {KEY} ")), Ok(KEY));
    let _ = std::fs::remove_dir_all(dir);
}

// ---- Playbooks 8: the pre-send check ------------------------------------------

/// The xAI client, recording what reached it.
#[derive(Default)]
struct XaiRec(Mutex<Vec<ResponsesRequest>>);

impl ModelClient for XaiRec {
    fn stream(
        &self,
        r: &ResponsesRequest,
        _c: &CancelToken,
        s: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        self.0.lock().unwrap().push(r.clone());
        s(StreamEvent::TextDelta("hey you".into()));
        Ok(TurnOutput {
            text: "hey you".into(),
            ..TurnOutput::default()
        })
    }
}

fn chat(session: &str, lines: &[&str]) -> ResponsesRequest {
    ResponsesRequest {
        text_format: None,
        model: "grok-4.7".into(),
        effort: None,
        input: lines
            .iter()
            .map(|t| InputItem::Message {
                role: "user".into(),
                content: vec![ContentPart::InputText((*t).into())],
            })
            .collect(),
        conversation_id: session.into(),
        tools: Vec::new(),
        hosted_search: false,
        call_timeout: None,
    }
}

#[test]
fn an_explicit_turn_with_no_local_model_gets_the_one_line_offer_and_goes_nowhere() {
    let (dir, wire) = setup("adult-anthropic");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet()).unwrap();
    add_provider(&dir, "https://api.anthropic.com", KEY, NOW).unwrap();
    grant_destination(
        &dir,
        "api.anthropic.com",
        CALL_DATA,
        UserClick::from_click(),
    )
    .unwrap();
    rank_first(&dir, AN_MODEL);
    let xai = XaiRec::default();
    let mut shown = String::new();
    let send = |req: &ResponsesRequest, shown: &mut String| {
        live::stream_routed(
            &xai,
            req,
            &CancelToken::new(),
            &mut |ev| {
                if let StreamEvent::TextDelta(t) = ev {
                    shown.push_str(&t);
                }
            },
            "chat:default",
        )
    };
    let to_anthropic = |lines: &[&str]| ResponsesRequest {
        model: AN_MODEL.into(),
        ..chat("adult-an-1", lines)
    };
    let out = send(&to_anthropic(&["talk dirty to me, I'm horny"]), &mut shown).unwrap();
    assert_eq!(out.text, grokhub_core::adult_guard::LOCAL_ONLY_MISSING);
    assert_eq!(shown, out.text, "what streams is what is kept");
    // Routed to xAI it stays off xAI too.
    let on_xai = send(&chat("adult-an-2", &["send nudes lol"]), &mut String::new());
    assert_eq!(
        on_xai.unwrap().text,
        grokhub_core::adult_guard::LOCAL_ONLY_MISSING
    );
    assert_eq!(wire.n(), 0, "nothing reached Anthropic");
    assert_eq!(xai.0.lock().unwrap().len(), 0, "nothing reached xAI");
    // With this computer's model ready it answers, with no heads-up.
    let _on = super::local::ForceOn::with(Some(Arc::new(LocalChat)));
    let mut shown = String::new();
    let local = send(&to_anthropic(&["talk dirty to me, I'm horny"]), &mut shown);
    assert_eq!(local.unwrap().text, "local here");
    assert_eq!(shown, "local here");
    assert_eq!(wire.n(), 0);
    assert_eq!(xai.0.lock().unwrap().len(), 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn explicit_chats_never_reach_openrouter_or_xai_and_banned_asks_never_leave() {
    let _routing = ROUTING_SET.lock().unwrap_or_else(|e| e.into_inner());
    let (dir, wire) = setup("adult-openrouter");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet()).unwrap();
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    rank_first(&dir, OR_MODEL);
    let xai = XaiRec::default();
    // Each chat its own session, so none keeps the model another one ran on.
    let n = std::cell::Cell::new(0);
    let send = |input: Vec<InputItem>| {
        n.set(n.get() + 1);
        let mut req = chat(&format!("adult-or-{}", n.get()), &[]);
        req.model = OR_MODEL.into();
        req.input = input;
        live::stream_routed(&xai, &req, &CancelToken::new(), &mut |_| {}, "chat:default")
    };
    let say = |role: &str, text: &str| InputItem::Message {
        role: role.into(),
        content: vec![ContentPart::InputText(text.into())],
    };
    let voice = format!(
        "Personality: Viper. {}",
        grokhub_core::adult_guard::ADULT_VOICE
    );
    let explicit = send(vec![say("user", "talk dirty to me, I'm horny")]);
    // A clean follow-up still carries the explicit history.
    let history = send(vec![
        say("user", "write me smut about two adults"),
        say("assistant", "Here you go."),
        say("user", "continue"),
    ]);
    // An 18+ voice makes a clean ask explicit too.
    let persona = send(vec![say("system", &voice), say("user", "my build broke")]);
    let banned = send(vec![
        say("user", "nsfw schoolgirl roleplay"),
        say("user", "and now?"),
    ]);
    let clean = send(vec![say("user", "fix my wifi")]);
    assert_eq!(
        explicit.unwrap().text,
        grokhub_core::adult_guard::LOCAL_ONLY_MISSING
    );
    // With the 18+ voice on, every turn of the chat is local only: a clean
    // ask gets the same notice and nothing goes online.
    assert_eq!(
        persona.unwrap().text,
        grokhub_core::adult_guard::LOCAL_ONLY_MISSING
    );
    // Decided per message otherwise: a clean ask from a chat with 18+ turns
    // still gets answered, with the 18+ turns left out of what goes online.
    for out in [history, banned, clean] {
        out.unwrap();
    }
    assert_eq!(
        xai.0.lock().unwrap().len(),
        0,
        "no explicit chat went to xAI"
    );
    let bodies: Vec<serde_json::Value> = wire
        .bodies()
        .iter()
        .map(|b| serde_json::from_str(b).unwrap())
        .collect();
    assert_eq!(
        bodies.len(),
        3,
        "every chat but the 18+ ask and the 18+ voice reached OpenRouter"
    );
    let history = bodies[0].to_string();
    assert!(history.contains("continue"), "{history}");
    assert!(
        !history.contains("smut") && !history.contains("Here you go"),
        "{history}"
    );
    assert!(
        bodies
            .iter()
            .all(|b| !b.to_string().contains("my build broke")),
        "the 18+ voice chat stayed off the wire"
    );
    let held = bodies[1].to_string();
    assert!(!held.contains("schoolgirl"), "{held}");
    assert!(held.contains("Loadout held this message back"), "{held}");
    for body in &bodies {
        assert_eq!(
            body["provider"],
            json!({"require_parameters": true, "data_collection": "deny"})
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Records the time limit of each post; can cancel the call while "on the wire".
#[derive(Default)]
struct LimitWire {
    limits: Mutex<Vec<Option<std::time::Duration>>>,
    cancel: Mutex<Option<CancelToken>>,
}

impl ProviderTransport for LimitWire {
    fn get(&self, _: &str, _: &[(String, String)]) -> Result<HttpReply, String> {
        Ok(HttpReply {
            status: 200,
            body: OR_LIST.into(),
            headers: Vec::new(),
        })
    }
    fn post(&self, _: &str, _: &[(String, String)], _: &str) -> Result<HttpReply, String> {
        Err("call_model posts through post_within".into())
    }
    fn post_within(
        &self,
        _: &str,
        _: &[(String, String)],
        _: &str,
        limit: Option<std::time::Duration>,
    ) -> Result<HttpReply, String> {
        self.limits.lock().unwrap().push(limit);
        if let Some(token) = self.cancel.lock().unwrap().take() {
            token.cancel();
        }
        Ok(HttpReply {
            status: 200,
            body: OR_REPLY.into(),
            headers: Vec::new(),
        })
    }
}

#[test]
fn a_provider_call_keeps_its_time_limit_and_a_cancel_on_the_wire_drops_the_reply() {
    let dir = test_dir("r3b-limit");
    providers::use_vault_for(&dir, Arc::new(MemoryVault::default()));
    let wire = Arc::new(LimitWire::default());
    providers::use_transport_for(&dir, wire.clone());
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    let mut timed = hello();
    timed.call_timeout = Some(std::time::Duration::from_secs(45));
    let call = |req: &ResponsesRequest, cancel: &CancelToken| {
        providers::call_model(
            &dir,
            "chat:default",
            OR_MODEL,
            None,
            req,
            CALL_DATA,
            "try-it:1",
            cancel,
        )
    };
    assert_eq!(
        call(&timed, &CancelToken::new()).unwrap().text,
        "hello from the provider"
    );
    assert_eq!(send(&dir).unwrap().text, "hello from the provider");
    let cancel = CancelToken::new();
    *wire.cancel.lock().unwrap() = Some(cancel.clone());
    assert!(matches!(call(&timed, &cancel), Err(ClientError::Cancelled)));
    assert_eq!(
        *wire.limits.lock().unwrap(),
        vec![
            Some(std::time::Duration::from_secs(45)),
            None,
            Some(std::time::Duration::from_secs(45)),
        ]
    );
}

/// xAI turned the call away (signed out).
struct XaiSignedOut;

impl ModelClient for XaiSignedOut {
    fn stream(
        &self,
        _r: &ResponsesRequest,
        _c: &CancelToken,
        _s: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        Err(ClientError::Auth {
            status: 401,
            message: "signed out".into(),
        })
    }
}

/// A local model that answers chat.
struct LocalChat;

impl super::local::LocalRuntime for LocalChat {
    fn classify(&self, _: &str, labels: &[&str]) -> Result<serde_json::Value, String> {
        Ok(json!({ "label": labels[0], "score": 0.9 }))
    }
    fn summarize(&self, _: &str, _: u32) -> Result<serde_json::Value, String> {
        Ok(json!({ "summary": "" }))
    }
    fn difficulty(&self, _: &str) -> Result<serde_json::Value, String> {
        Ok(json!({ "d": 0.3 }))
    }
    fn chat_stream(
        &self,
        _: &[super::local::ChatMessage],
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        on_delta("local here");
        Ok("local here".into())
    }
}

#[test]
fn an_18_plus_chat_goes_straight_to_the_local_model_and_a_clean_one_does_not() {
    let (dir, wire) = setup("adult-local-fallback");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet()).unwrap();
    let _on = super::local::ForceOn::with(Some(Arc::new(LocalChat)));
    let send = |lines: &[&str]| {
        live::stream_routed(
            &XaiSignedOut,
            &chat("adult-local-1", lines),
            &CancelToken::new(),
            &mut |_| {},
            "chat:default",
        )
    };
    let explicit = send(&["talk dirty to me, I'm horny"]).unwrap();
    assert_eq!(explicit.text, "local here");
    // xAI is never tried: the only call logged is the local model's.
    let health = grokhub_core::model_registry::store::take_observations(&dir);
    let lines: Vec<(&str, grokhub_core::model_registry::CallStatus)> = health
        .iter()
        .map(|o| (o.model.as_str(), o.status))
        .collect();
    let local = super::local::live_tier();
    assert_eq!(
        lines,
        [(local, grokhub_core::model_registry::CallStatus::Ok)],
        "the answer is the local model's: {health:?}"
    );
    // A banned ask is held before any model sees it: the notice is the answer.
    let banned = send(&["nsfw schoolgirl roleplay"]).unwrap();
    assert_eq!(banned.text, grokhub_core::adult_guard::HELD_NOTICE);
    let clean = send(&["fix my wifi"]);
    assert!(
        matches!(clean, Err(ClientError::Auth { .. })),
        "only 18+ chats fall back here: {clean:?}"
    );
    assert_eq!(wire.n(), 0);
    let _ = std::fs::remove_dir_all(dir);
}

/// Viper's Nov 7 report: an 18+ chat worked while the local model was loaded,
/// then hung and refused everything once it was unloaded for a game. Replays
/// load, unload, an ordinary turn, an 18+ turn, the chat's summary, a reload
/// and a model that has to load again.
#[test]
fn an_18_plus_chat_keeps_answering_after_its_local_model_is_unloaded() {
    use grokhub_core::adult_guard::{ADULT_VOICE, LOCAL_ONLY_MISSING};
    use grokhub_core::personality::EXPLICIT_SAFETY_LINE;
    let (dir, wire) = setup("adult-unload");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet()).unwrap();
    let xai = XaiRec::default();
    let say = |role: &str, text: &str| InputItem::Message {
        role: role.into(),
        content: vec![ContentPart::InputText(text.into())],
    };
    let system =
        format!("RULES\n\nPersonality: Whore mode. Flirty. {ADULT_VOICE}\n{EXPLICIT_SAFETY_LINE}");
    let history = vec![
        say("system", &system),
        say("user", "wanna have some sexy time"),
        say("assistant", "Mmm, get naked and talk dirty to me."),
    ];
    let send = |ask: &str, class: &str| {
        let mut req = chat("adult-unload-1", &[]);
        req.input = history.clone();
        req.input.push(say("user", ask));
        let (mut shown, mut thought) = (String::new(), String::new());
        let out = live::stream_routed(
            &xai,
            &req,
            &CancelToken::new(),
            &mut |ev| match ev {
                StreamEvent::TextDelta(t) => shown.push_str(&t),
                StreamEvent::ReasoningDelta(t) => thought.push_str(&t),
                StreamEvent::RateWait(_) => {}
            },
            class,
        );
        (out, shown, thought)
    };
    let sent_to_xai = || xai.0.lock().unwrap().len();

    // Loaded: the 18+ turn runs on this computer's model.
    let loaded = super::local::ForceOn::with(Some(Arc::new(LocalChat)));
    let (out, shown, _) = send("lets do this, I'm horny", "chat:default");
    assert_eq!(out.unwrap().text, "local here");
    assert_eq!(shown, "local here");
    assert_eq!(sent_to_xai(), 0);

    // Unloaded for a game.
    drop(loaded);
    // The 18+ voice is on, so even an ordinary turn stays on this computer:
    // it gets the one-line notice and nothing goes online (Nov 9 bug card:
    // turns of an 18+ chat must never fall to a cloud model).
    let (out, shown, _) = send("whats a good budget gaming mouse", "chat:default");
    assert_eq!(out.unwrap().text, LOCAL_ONLY_MISSING);
    assert_eq!(shown, LOCAL_ONLY_MISSING);
    assert_eq!(sent_to_xai(), 0);
    // An 18+ turn says in one line why it can't run, and sends nothing.
    let (out, shown, _) = send("lets get naked", "chat:default");
    assert_eq!(out.unwrap().text, LOCAL_ONLY_MISSING);
    assert_eq!(shown, LOCAL_ONLY_MISSING);
    assert_eq!(sent_to_xai(), 0);
    // The chat's summary never becomes that notice, and never goes online.
    let (out, shown, _) = send("Summarize the conversation so far.", super::CLASS_COMPACT);
    assert!(out.is_err(), "{out:?}");
    assert!(shown.is_empty(), "{shown}");
    assert_eq!(sent_to_xai(), 0);

    // Loaded again: 18+ turns are back on this computer's model.
    let again = super::local::ForceOn::with(Some(Arc::new(LocalChat)));
    let (out, _, thought) = send("lets get naked", "chat:default");
    assert_eq!(out.unwrap().text, "local here");
    assert!(
        thought.is_empty(),
        "a loaded model needs no note: {thought}"
    );
    drop(again);

    // Ready but not loaded yet: the chat says it's loading, never a silent wait.
    let _cold = super::local::ForceOn::with(None);
    let (out, _, thought) = send("lets get naked", "chat:default");
    assert_eq!(thought, super::local::LOCAL_LOADING_NOTE);
    assert!(out.is_err(), "no engine here, so it can't start: {out:?}");
    assert_eq!(sent_to_xai(), 0);
    assert_eq!(wire.n(), 0);
    let _ = std::fs::remove_dir_all(dir);
}

/// xAI streamed part of an answer, then the connection dropped.
struct XaiCutOff;

impl ModelClient for XaiCutOff {
    fn stream(
        &self,
        _r: &ResponsesRequest,
        _c: &CancelToken,
        s: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        s(StreamEvent::TextDelta("half an ans".into()));
        Err(ClientError::Server {
            status: 502,
            message: "bad gateway".into(),
            retry_after_secs: None,
        })
    }
}

#[test]
fn a_half_streamed_xai_answer_is_never_finished_by_the_local_model() {
    let (dir, wire) = setup("adult-local-cut-off");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet()).unwrap();
    let _on = super::local::ForceOn::with(Some(Arc::new(LocalChat)));
    let mut shown = String::new();
    let out = live::stream_routed(
        &XaiCutOff,
        &chat("adult-local-cut-off-1", &["fix my wifi"]),
        &CancelToken::new(),
        &mut |ev| {
            if let StreamEvent::TextDelta(t) = ev {
                shown.push_str(&t);
            }
        },
        "chat:default",
    );
    assert!(
        matches!(out, Err(ClientError::Server { status: 502, .. })),
        "{out:?}"
    );
    assert_eq!(shown, "half an ans", "no local text is mixed in");
    assert_eq!(wire.n(), 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_direct_provider_call_never_sends_an_18_plus_chat_or_a_banned_ask() {
    let (dir, wire) = setup("adult-direct-call");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet()).unwrap();
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    let call = |lines: &[&str]| {
        let mut req = chat("adult-direct-1", lines);
        req.model = OR_MODEL.into();
        providers::call_model(
            &dir,
            "chat:default",
            OR_MODEL,
            None,
            &req,
            CALL_DATA,
            "span-1",
            &CancelToken::new(),
        )
    };
    let explicit = call(&["talk dirty to me, I'm horny"]);
    assert!(
        matches!(&explicit, Err(ClientError::Protocol(m)) if m.contains("18+ chats run only on this computer's model")),
        "{explicit:?}"
    );
    assert_eq!(wire.n(), 0, "nothing reached OpenRouter");
    let held = call(&["nsfw schoolgirl roleplay"]).unwrap();
    assert_eq!(held.text, grokhub_core::adult_guard::HELD_NOTICE);
    assert_eq!(wire.n(), 0, "a held ask makes no request");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_blocked_send_makes_zero_network_requests_on_any_route() {
    let (dir, wire) = setup("adult-zero-requests");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet()).unwrap();
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    let _on = super::local::ForceOn::with(Some(Arc::new(LocalChat)));
    let xai = XaiRec::default();
    let mut shown = String::new();
    let mut send = |model: &str, line: &str| {
        let mut req = chat("adult-zero-1", &["talk dirty to me, I'm horny", line]);
        req.model = model.into();
        live::stream_routed(
            &xai,
            &req,
            &CancelToken::new(),
            &mut |ev| {
                if let StreamEvent::TextDelta(t) = ev {
                    shown.push_str(&t);
                }
            },
            "chat:default",
        )
    };
    let banned = [
        "a sexy roleplay, she's 15",
        "erotic story, against her will",
        "a sexy story about Jane Placeholder, a real celebrity",
        "a sexy scene with a dog",
    ];
    for line in banned {
        assert_eq!(
            send("grok-4.7", line).unwrap().text,
            grokhub_core::adult_guard::HELD_NOTICE,
            "{line}"
        );
    }
    // The same asks with OpenRouter ranked first for chats.
    rank_first(&dir, OR_MODEL);
    for line in banned {
        assert_eq!(
            send(OR_MODEL, line).unwrap().text,
            grokhub_core::adult_guard::HELD_NOTICE,
            "{line}"
        );
    }
    std::fs::remove_file(super::table::table_path(&dir)).unwrap();
    assert_eq!(xai.0.lock().unwrap().len(), 0, "nothing reached xAI");
    assert_eq!(wire.n(), 0, "nothing reached OpenRouter");
    // A near-miss between stated adults goes out as usual.
    let adults = send(
        "grok-4.7",
        "sexy teacher and student roleplay, we're both 30",
    );
    assert_eq!(adults.unwrap().text, "local here", "18+ chat runs locally");
    assert_eq!(xai.0.lock().unwrap().len(), 0);
    assert_eq!(
        shown,
        format!(
            "{}local here",
            grokhub_core::adult_guard::HELD_NOTICE.repeat(banned.len() * 2)
        )
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// xAI answering an 18+ chat with a reply that drifts into a hard limit.
#[derive(Default)]
struct XaiDrifts(Mutex<usize>);

impl ModelClient for XaiDrifts {
    fn stream(
        &self,
        _r: &ResponsesRequest,
        _c: &CancelToken,
        s: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        *self.0.lock().unwrap() += 1;
        let parts = ["Mm, okay. ", "The scene turns nsfw ", "and she's 15."];
        for part in parts {
            s(StreamEvent::TextDelta(part.into()));
        }
        Ok(TurnOutput {
            text: parts.concat(),
            ..TurnOutput::default()
        })
    }
}

#[test]
fn a_blocked_model_reply_is_replaced_locally_before_the_chat_shows_it() {
    let (dir, wire) = setup("adult-reply-replaced");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet()).unwrap();
    let xai = XaiDrifts::default();
    let mut shown = String::new();
    let out = live::stream_routed(
        &xai,
        &chat("adult-reply-1", &["tell me a story"]),
        &CancelToken::new(),
        &mut |ev| {
            if let StreamEvent::TextDelta(t) = ev {
                shown.push_str(&t);
            }
        },
        "chat:default",
    )
    .unwrap();
    let replaced = format!("Mm, okay. {}", grokhub_core::adult_guard::REPLY_REPLACED);
    assert_eq!(shown, replaced, "the drifted part never reached the chat");
    assert_eq!(out.text, replaced, "and the history keeps what showed");
    assert!(!out.text.contains("15"), "{}", out.text);
    assert_eq!(*xai.0.lock().unwrap(), 1);
    assert_eq!(wire.n(), 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_openrouter_privacy_refusal_reads_as_what_to_do_not_a_raw_404() {
    let body = "0 endpoints out of 1 requested are available matching your guardrail restrictions and data policy. We removed them for the following reasons: Provider not allowed by guardrail: 1 endpoint excluded; ZDR violation (account settings): 1 endpoint excluded";
    assert!(providers::policy_refusal(404, body));
    assert!(providers::policy_refusal(
        404,
        "ZDR violation (account settings)"
    ));
    // A plain missing model, a key problem or a server fault is not a policy refusal.
    assert!(!providers::policy_refusal(
        404,
        "No endpoints found for vendor/model."
    ));
    assert!(!providers::policy_refusal(401, "guardrail"));
    assert!(!providers::policy_refusal(503, "data policy"));
    assert!(providers::POLICY_REFUSED.contains("OpenRouter data policy"));
    assert!(providers::POLICY_REFUSED.contains("https://openrouter.ai/settings/privacy"));
}

#[test]
fn set_routing_with_zdr_sends_provider_zdr_and_never_a_sort_or_a_models_list() {
    use super::providers::{apply_openrouter_routing, is_openrouter_router, Routing};
    let _routing = ROUTING_SET.lock().unwrap_or_else(|e| e.into_inner());
    let base = json!({"model": "google/gemma-4-31b-it:free", "stream": false, "messages": []});
    // The default denies endpoints that keep prompts, and every endpoint must
    // take every field sent. Only OpenRouter gets this: the golden bodies
    // ([`request_body`]) stay byte-identical.
    let mut b = base.clone();
    apply_openrouter_routing(&mut b, &Routing::default());
    assert_eq!(
        b,
        json!({"model": "google/gemma-4-31b-it:free", "stream": false, "messages": [], "provider": {"require_parameters": true, "data_collection": "deny"}})
    );
    // "allow" is data_collection.
    let mut b = base.clone();
    apply_openrouter_routing(
        &mut b,
        &Routing {
            data: "allow".into(),
        },
    );
    assert_eq!(
        b,
        json!({"model": "google/gemma-4-31b-it:free", "stream": false, "messages": [], "provider": {"require_parameters": true, "data_collection": "allow"}})
    );
    // An old saved setting with a sort and a fallback keeps only its data part.
    let old: Routing =
        serde_json::from_value(json!({"sort": "price", "fallback": true, "data": "zdr"})).unwrap();
    assert_eq!(old, Routing { data: "zdr".into() });
    // Set as the cabin sets it, a real call to openrouter.ai carries zdr and
    // nothing else of OpenRouter's routing.
    let (dir, wire) = setup("r3b-routing-zdr");
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    providers::set_routing(old);
    let out = send(&dir);
    providers::set_routing(Routing::default());
    assert_eq!(out.unwrap().text, "hello from the provider");
    let body: serde_json::Value = serde_json::from_str(&wire.bodies()[0]).unwrap();
    assert_eq!(
        body["provider"],
        json!({"require_parameters": true, "data_collection": "deny", "zdr": true}),
        "{body}"
    );
    assert_eq!(body.get("models"), None, "{body}");
    assert_eq!(body["model"], "vendor/cheap");
    let _ = std::fs::remove_dir_all(dir);
    assert!(is_openrouter_router("openrouter/auto"));
    assert!(is_openrouter_router("openrouter/openrouter/free"));
    assert!(!is_openrouter_router(
        "openrouter/google/gemma-4-31b-it:free"
    ));
    assert!(!is_openrouter_router("google/gemma-4-31b-it:free"));
    assert!(providers::is_free_model("openrouter/openrouter/free"));
    assert!(providers::is_free_model("google/gemma-4-31b-it:free"));
    assert!(!providers::is_free_model("openrouter/openrouter/auto"));
}

#[test]
fn auto_takes_a_set_up_provider_model_only_priced_under_the_ceiling_and_in_text() {
    let profiles: BTreeMap<String, ModelProfile> = BTreeMap::new();
    let table = RoutingTable::default();
    // No sign-in, so nothing on xAI is included; no preferences of any kind.
    let mut reg = fleet();
    reg.entitlement.credential = Credential::None;
    let input = || {
        ask(
            "chat:default",
            "grok-4.7",
            granted(&["openrouter", "anthropic"]),
        )
    };
    // OpenRouter's model lists $0.50/M, under the $15/M ceiling: Auto takes it.
    // The Anthropic model lists no price, so it is no candidate.
    let r = Router::choose(&input(), &reg, &profiles, &table, NOW);
    assert_eq!(
        (r.provider.as_str(), r.model.as_str()),
        ("openrouter", OR_MODEL),
        "{:?}",
        r.rule_ids
    );
    assert!(
        r.rule_ids.contains(&"provider:auto".into()),
        "{:?}",
        r.rule_ids
    );
    // Without its key and grant it is never a candidate.
    let r = Router::choose(
        &ask("chat:default", "grok-4.7", Spend::default()),
        &reg,
        &profiles,
        &table,
        NOW,
    );
    assert_eq!(
        (r.model.as_str(), r.provider.as_str()),
        ("grok-4.7", "xai"),
        "{:?}",
        r.rule_ids
    );
    // Priced at $20/M, over the ceiling: out, and the call keeps its own model.
    reg.models.get_mut(OR_MODEL).unwrap().meta.prices.completion = Some(200_000);
    let r = Router::choose(&input(), &reg, &profiles, &table, NOW);
    assert_eq!(r.model, "grok-4.7", "{:?}", r.rule_ids);
    assert!(
        r.rule_ids.contains(&"cost:not_included".into()),
        "{:?}",
        r.rule_ids
    );
    // The Anthropic model with a listed price under the ceiling: Auto takes it.
    reg.models.get_mut(AN_MODEL).unwrap().meta.prices = Prices {
        prompt: Some(2_000),
        completion: Some(10_000),
        ..Prices::default()
    };
    let r = Router::choose(&input(), &reg, &profiles, &table, NOW);
    assert_eq!(
        (r.provider.as_str(), r.model.as_str()),
        ("anthropic", AN_MODEL),
        "{:?}",
        r.rule_ids
    );
    // One that answers in something other than text stays out.
    reg.models.get_mut(AN_MODEL).unwrap().meta.output_modalities = Some(vec!["embeddings".into()]);
    let r = Router::choose(&input(), &reg, &profiles, &table, NOW);
    assert_eq!(r.model, "grok-4.7", "{:?}", r.rule_ids);
    // An OpenRouter router lists no price (it may pick any model at any price): out.
    let router = "openrouter/openrouter/auto";
    let mut rec = reg.models.get(OR_MODEL).unwrap().clone();
    rec.meta.id = router.into();
    rec.meta.prices = Prices::default();
    reg.models.insert(router.into(), rec);
    let r = Router::choose(&input(), &reg, &profiles, &table, NOW);
    assert_ne!(r.model, router, "{:?}", r.rule_ids);
    // Signed in, xAI answers first; the mid-episode provider model keeps its step.
    let mut plan = fleet();
    plan.entitlement.credential = Credential::Plan;
    let r = Router::choose(&input(), &plan, &profiles, &table, NOW);
    assert_eq!(r.provider.as_str(), "xai", "{:?}", r.rule_ids);
    let mut mid = input();
    mid.episode_model = Some(OR_MODEL);
    let r = Router::choose(&mid, &plan, &profiles, &table, NOW);
    assert_eq!(r.model, OR_MODEL, "{:?}", r.rule_ids);
    assert!(
        r.rule_ids.contains(&"episode:keep".into()),
        "{:?}",
        r.rule_ids
    );
}

/// Saves a routing table that ranks `model` first for a chat's lanes (the
/// default, quick and debug ones), as the evals would.
fn rank_first(dir: &Path, model: &str) {
    let mut t = table_with("chat:default", model);
    let row = t.classes["chat:default"].clone();
    for lane in ["chat:quick", "debug"] {
        t.classes.insert(lane.into(), row.clone());
    }
    let path = super::table::table_path(dir);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_string(&t).unwrap()).unwrap();
}

/// Held by every test that sets or reads OpenRouter's process-wide routing setting.
static ROUTING_SET: Mutex<()> = Mutex::new(());

#[test]
fn every_provider_call_is_told_which_model_answers() {
    let (dir, wire) = setup("r3b-identity");
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    // Straight through the transport (Settings' Try it, an episode step, a
    // chat): the identity rides the request without the caller adding it.
    assert_eq!(send(&dir).unwrap().text, "hello from the provider");
    let body: serde_json::Value = serde_json::from_str(&wire.bodies()[0]).unwrap();
    let system = body["messages"][0].clone();
    assert_eq!(system["role"], "system", "{body}");
    let text = system["content"].to_string();
    assert!(
        text.contains(
            "[Model identity] This reply comes from `vendor/cheap`, served through OpenRouter"
        ),
        "{text}"
    );
    assert_eq!(body["messages"][1]["role"], "user", "{body}");
    let _ = std::fs::remove_dir_all(dir);
}

/// [`XaiRec`] behind xAI's Batch API: the answer took minutes in a queue.
struct BatchedXai(XaiRec);

impl ModelClient for BatchedXai {
    fn stream(
        &self,
        r: &ResponsesRequest,
        c: &CancelToken,
        s: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        self.0.stream(r, c, s)
    }

    fn batches(&self) -> bool {
        true
    }
}

#[test]
fn a_batched_call_teaches_no_speed() {
    let (dir, _wire) = setup("batched-latency");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet()).unwrap();
    let live_xai = XaiRec::default();
    let queued = BatchedXai(XaiRec::default());
    let req = chat("batched-1", &["sum up the week"]);
    live::stream_routed(
        &live_xai,
        &req,
        &CancelToken::new(),
        &mut |_| {},
        "chat:default",
    )
    .unwrap();
    live::stream_routed(
        &queued,
        &req,
        &CancelToken::new(),
        &mut |_| {},
        "chat:default",
    )
    .unwrap();
    let latencies: Vec<Option<u64>> = route_records(&dir, ROUTE_TAIL_LINES)
        .into_iter()
        .map(|(_, r)| r.signals.latency)
        .collect();
    assert_eq!(latencies.len(), 2, "{latencies:?}");
    assert!(latencies[0].is_some(), "a live call is timed");
    assert_eq!(latencies[1], None, "a queued call is not");
    let health = std::fs::read_to_string(
        dir.join("models")
            .join(grokhub_core::model_registry::store::HEALTH_FILE),
    )
    .unwrap();
    let lines: Vec<serde_json::Value> = health
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2, "{health}");
    assert_eq!(lines[1]["latency_ms"], json!(0), "health takes no sample");
    let _ = std::fs::remove_dir_all(dir);
}

/// xAI is down: the client's retries ran out on a 503.
struct XaiDown;

impl ModelClient for XaiDown {
    fn stream(
        &self,
        _r: &ResponsesRequest,
        _c: &CancelToken,
        _s: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        Err(ClientError::Server {
            status: 503,
            message: "overloaded".into(),
            retry_after_secs: None,
        })
    }
}

#[test]
fn xai_down_hands_an_ordinary_turn_to_a_provider_then_to_this_computer() {
    let (dir, wire) = setup("outage-fallback");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet()).unwrap();
    let send = |client: &dyn ModelClient, n: u32, lines: &[&str]| {
        let mut shown = String::new();
        let out = live::stream_routed(
            client,
            &chat(&format!("outage-{n}"), lines),
            &CancelToken::new(),
            &mut |ev| {
                if let StreamEvent::TextDelta(t) = ev {
                    shown.push_str(&t);
                }
            },
            "chat:default",
        );
        (out, shown)
    };
    // Nothing else is set up: the outage is the answer.
    let (out, _) = send(&XaiDown, 1, &["fix my wifi"]);
    assert!(
        matches!(out, Err(ClientError::Server { status: 503, .. })),
        "{out:?}"
    );
    // This computer's model takes it.
    {
        let _on = super::local::ForceOn::with(Some(Arc::new(LocalChat)));
        let (out, shown) = send(&XaiDown, 2, &["fix my wifi"]);
        assert_eq!(out.unwrap().text, "local here");
        assert_eq!(shown, "local here");
    }
    assert_eq!(wire.n(), 0);
    // A provider you set up goes first, ahead of this computer's model.
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    let _on = super::local::ForceOn::with(Some(Arc::new(LocalChat)));
    let (out, shown) = send(&XaiDown, 3, &["fix my wifi"]);
    assert_eq!(out.unwrap().text, "hello from the provider");
    assert_eq!(shown, "hello from the provider");
    assert_eq!(wire.n(), 1);
    assert!(wire.bodies()[0].contains("\"model\":\"vendor/cheap\""));
    let (_, rec) = route_records(&dir, ROUTE_TAIL_LINES)
        .pop()
        .expect("a route record");
    assert_eq!(
        (rec.used.provider.as_str(), rec.used.model.as_str()),
        ("openrouter", OR_MODEL)
    );
    // A signed-out xAI is no outage: nothing stands in for it.
    let (out, _) = send(&XaiSignedOut, 4, &["fix my wifi"]);
    assert!(matches!(out, Err(ClientError::Auth { .. })), "{out:?}");
    // An 18+ chat never goes to the provider: this computer's model answers.
    let (out, _) = send(&XaiDown, 5, &["talk dirty to me, I'm horny"]);
    assert_eq!(out.unwrap().text, "local here");
    assert_eq!(wire.n(), 1, "nothing more reached the provider");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_providers_thinking_is_read_and_goes_back_to_it_alone() {
    // llama.cpp with `--reasoning-format deepseek` says `reasoning_content`.
    let local = providers::parse_openai_reply(
        r#"{"choices":[{"message":{"content":"Hi.","reasoning_content":"They said hello."}}]}"#,
    )
    .unwrap();
    assert_eq!(
        (local.text.as_str(), local.reasoning.as_str()),
        ("Hi.", "They said hello.")
    );
    assert!(local.reasoning_items.is_empty());

    // OpenRouter adds `reasoning_details`, which the next tool step sends back.
    let or = providers::parse_openai_reply(
        r#"{"choices":[{"message":{"content":null,"reasoning":"Look it up.","reasoning_details":[{"type":"reasoning.encrypted","data":"abc","id":"r1","format":"openai-responses-v1","index":0}],"tool_calls":[{"id":"c1","type":"function","function":{"name":"weather","arguments":"{}"}}]}}]}"#,
    )
    .unwrap();
    assert_eq!(or.reasoning, "Look it up.");
    assert_eq!(or.reasoning_items.len(), 1);
    let mut item = or.reasoning_items[0].clone();
    if let InputItem::Reasoning { model, .. } = &mut item {
        *model = "x-ai/grok-4.3".into();
    }
    let history = vec![
        InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText("weather?".into())],
        },
        item,
        InputItem::FunctionCall {
            call_id: "c1".into(),
            name: "weather".into(),
            arguments: "{}".into(),
        },
        InputItem::FunctionCallOutput {
            call_id: "c1".into(),
            output: "sunny".into(),
        },
    ];
    let req = ResponsesRequest {
        input: history,
        ..hello()
    };
    let back = providers::openai_body("x-ai/grok-4.3", None, &req);
    let msgs = back["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 3, "{back}");
    assert_eq!(msgs[1]["role"], "assistant");
    assert_eq!(msgs[1]["tool_calls"][0]["id"], "c1");
    assert_eq!(msgs[1]["reasoning_details"][0]["data"], "abc");
    assert_eq!(msgs[1]["reasoning_details"][0]["id"], "r1");
    // Another model never gets them, and neither does xAI.
    let other = providers::openai_body("google/gemini-3-pro", None, &req);
    assert!(
        other["messages"][1].get("reasoning_details").is_none(),
        "{other}"
    );
    let xai = responses_body(&ResponsesRequest {
        model: "x-ai/grok-4.3".into(),
        ..req.clone()
    });
    let wire = xai["input"].to_string();
    assert!(!wire.contains("reasoning_details"), "{wire}");
    assert!(!wire.contains("abc"), "{wire}");
}

// ---- OpenRouter picks: its Models API, the request fields, provider health ----

/// Shaped like OpenRouter's `GET /api/v1/models`.
const OR_FIXTURE: &str =
    include_str!("../../../grokhub-core/tests/fixtures/openrouter_models.json");

/// The fixture listing as a refresh reads it: no xAI, no sign-in.
fn or_fleet() -> Registry {
    let mut rows =
        grokhub_core::model_registry::parse_openai_compatible_catalog("openrouter", OR_FIXTURE)
            .unwrap();
    providers::openrouter_efforts(&mut rows);
    let mut reg = Registry::default();
    reg.apply_refresh(&[Listing::new(SourceKind::OpenAiCompatible, rows)], &[], 1);
    reg
}

fn pickable(reg: &Registry, need: &providers::PickNeed) -> Vec<String> {
    reg.models
        .iter()
        .filter(|(id, r)| r.state.routable() && providers::auto_pickable(id, &r.meta, need))
        .map(|(id, _)| id.clone())
        .collect()
}

#[test]
fn auto_picks_openrouter_models_from_its_listing_never_its_router() {
    let reg = or_fleet();
    assert_eq!(
        reg.get("openrouter/vendor/retiring").map(|r| r.state),
        Some(ModelState::Retired),
        "a dated expiration_date takes it out of routing"
    );
    // OpenRouter's unified levels for a model that names `reasoning` and no
    // list of its own; a list it names stays; none for one that doesn't reason.
    let efforts = |id: &str| reg.get(id).and_then(|r| r.meta.efforts.clone());
    assert_eq!(
        efforts("openrouter/vendor/agent-lite"),
        Some(
            ["none", "minimal", "low", "medium", "high", "xhigh"]
                .map(String::from)
                .to_vec()
        )
    );
    assert_eq!(
        efforts("openrouter/vendor/agent-pro"),
        Some(["low", "medium", "high"].map(String::from).to_vec())
    );
    assert_eq!(efforts("openrouter/vendor/tools-only"), None);
    let need = |tools, reasoning, ctx_tokens| providers::PickNeed {
        tools,
        reasoning,
        ctx_tokens,
        ceiling_usd_per_m: 15.0,
    };
    // An agent lane that reasons: tools and reasoning, under $15/M, text out.
    assert_eq!(
        pickable(&reg, &need(true, true, 50_000)),
        vec![
            "openrouter/vendor/agent-lite",
            "openrouter/vendor/agent-pro",
            "openrouter/x-ai/grok-test"
        ]
    );
    // A short prompt fits the 8k window too.
    assert_eq!(
        pickable(&reg, &need(true, true, 4_000)),
        vec![
            "openrouter/vendor/agent-lite",
            "openrouter/vendor/agent-pro",
            "openrouter/vendor/small-window",
            "openrouter/x-ai/grok-test"
        ]
    );
    // No tools, no reasoning: the chat-only and tools-only models join. The
    // router, the image model, the $150/M one and the retiring one never do.
    assert_eq!(
        pickable(&reg, &need(false, false, 50_000)),
        vec![
            "openrouter/vendor/agent-lite",
            "openrouter/vendor/agent-pro",
            "openrouter/vendor/chat-only",
            "openrouter/vendor/tools-only",
            "openrouter/x-ai/grok-test"
        ]
    );
    // Through the router: no sign-in, OpenRouter set up, an agent lane that
    // reasons hard. The ranking takes the 400k model with its own effort list.
    let table = RoutingTable::default();
    let r = Router::choose(
        &ask("debug", "grok-4.7", granted(&["openrouter"])),
        &reg,
        &none(),
        &table,
        NOW,
    );
    assert_eq!(
        (r.provider.as_str(), r.model.as_str()),
        ("openrouter", "openrouter/vendor/agent-pro"),
        "{:?}",
        r.rule_ids
    );
    assert!(
        r.rule_ids.contains(&"provider:auto".into()),
        "{:?}",
        r.rule_ids
    );
    // Its router is never the pick, even ranked first by the table.
    let r = Router::choose(
        &ask("debug", "grok-4.7", granted(&["openrouter"])),
        &reg,
        &none(),
        &table_with("debug", "openrouter/openrouter/auto"),
        NOW,
    );
    assert_ne!(r.model, "openrouter/openrouter/auto", "{:?}", r.rule_ids);
}

/// Serves the fixture listing and a reply that names the model that answered.
#[derive(Default)]
struct FixtureWire {
    reply: Mutex<String>,
    gets: Mutex<usize>,
    posts: Mutex<Vec<String>>,
}

impl ProviderTransport for FixtureWire {
    fn get(&self, _url: &str, _headers: &[(String, String)]) -> Result<HttpReply, String> {
        *self.gets.lock().unwrap() += 1;
        Ok(HttpReply {
            status: 200,
            body: OR_FIXTURE.into(),
            headers: Vec::new(),
        })
    }
    fn post(
        &self,
        _url: &str,
        _headers: &[(String, String)],
        body: &str,
    ) -> Result<HttpReply, String> {
        self.posts.lock().unwrap().push(body.into());
        Ok(HttpReply {
            status: 200,
            body: self.reply.lock().unwrap().clone(),
            headers: Vec::new(),
        })
    }
}

fn fixture_setup(label: &str) -> (PathBuf, Arc<FixtureWire>) {
    let dir = test_dir(label);
    providers::use_vault_for(&dir, Arc::new(MemoryVault::default()));
    let wire = Arc::new(FixtureWire::default());
    providers::use_transport_for(&dir, wire.clone());
    (dir, wire)
}

#[test]
fn openrouters_model_list_is_read_at_most_once_a_day() {
    use grokhub_core::model_registry::probe::DAY_MS;
    let (dir, wire) = fixture_setup("or-daily-list");
    let p = add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    let g = grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    let src = ProviderSource {
        config_dir: dir.clone(),
        provider: p.clone(),
    };
    let first = src.fetch_at(NOW).unwrap();
    assert_eq!(first.models.len(), 10);
    assert_eq!(
        (*wire.gets.lock().unwrap(), read_egress(&dir).len()),
        (1, 1)
    );
    // Within the day: the same list, and nothing sent (no request, no egress line).
    let again = src.fetch_at(NOW + DAY_MS - 1).unwrap();
    assert_eq!(again, first);
    assert_eq!(
        (*wire.gets.lock().unwrap(), read_egress(&dir).len()),
        (1, 1)
    );
    // A day on: read again.
    src.fetch_at(NOW + DAY_MS).unwrap();
    assert_eq!(*wire.gets.lock().unwrap(), 2);
    // Adding the provider again starts over: the next refresh reads it.
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    src.fetch_at(NOW + DAY_MS + 1).unwrap();
    assert_eq!(*wire.gets.lock().unwrap(), 3);
    // Revoked: the day's copy is no way around the grant.
    assert_eq!(revoke_grant(&dir, &g.id), Ok(true));
    let err = src.fetch_at(NOW + DAY_MS + 2).unwrap_err();
    assert!(err.starts_with("openrouter: hard-class send"), "{err}");
    assert_eq!(*wire.gets.lock().unwrap(), 3);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_backup_is_the_next_pick_from_the_same_filter() {
    let reg = or_fleet();
    let table = RoutingTable::default();
    let need = |tools, reasoning, ctx_tokens| providers::PickNeed {
        tools,
        reasoning,
        ctx_tokens,
        ceiling_usd_per_m: 15.0,
    };
    let fit = |ctx_tokens| super::policy::Fit {
        needs_tools: true,
        ctx_tokens,
        ..super::policy::Fit::default()
    };
    let backup = |primary: &str, n: &providers::PickNeed, ctx| {
        providers::openrouter_backup(&reg, &none(), &table, "debug", primary, n, fit(ctx), NOW)
    };
    assert_eq!(
        backup(
            "openrouter/vendor/agent-lite",
            &need(true, true, 50_000),
            50_000
        )
        .as_deref(),
        Some("openrouter/vendor/agent-pro")
    );
    // Only one model holds a 300k prompt: no backup.
    assert_eq!(
        backup(
            "openrouter/vendor/agent-pro",
            &need(true, true, 300_000),
            300_000
        ),
        None
    );
}

#[test]
fn an_openrouter_call_sends_its_backup_session_and_deny_and_keeps_the_answering_model_out_of_the_chat(
) {
    let _routing = ROUTING_SET.lock().unwrap_or_else(|e| e.into_inner());
    let (dir, wire) = fixture_setup("or-call-picks");
    save_registry(&dir, &or_fleet()).unwrap();
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    // The backup answered: the reply names it.
    *wire.reply.lock().unwrap() = r#"{"model":"vendor/agent-pro","choices":[{"message":{"content":"done","reasoning_details":[{"type":"reasoning.encrypted","data":"abc","id":"r1","format":"openai-responses-v1","index":0}]}}]}"#.into();
    let tool = json!({"type": "function", "name": "read_file", "description": "Read a file", "parameters": {"type": "object", "properties": {}}});
    let req = ResponsesRequest {
        tools: vec![tool],
        ..hello()
    };
    let out = providers::call_model(
        &dir,
        "debug",
        "openrouter/vendor/agent-lite",
        Some("high"),
        &req,
        CALL_DATA,
        "conv-1:9",
        &CancelToken::new(),
    )
    .unwrap();
    let body: serde_json::Value = serde_json::from_str(&wire.posts.lock().unwrap()[0]).unwrap();
    assert_eq!(body["model"], "vendor/agent-lite");
    assert_eq!(
        body["models"],
        json!(["vendor/agent-lite", "vendor/agent-pro"])
    );
    assert_eq!(
        body["provider"],
        json!({"require_parameters": true, "data_collection": "deny"})
    );
    assert_eq!(body["session_id"], "36524fd8f6747fc2712506d01fee0e18");
    assert_eq!(body["reasoning"], json!({"effort": "high"}));
    // Which model answered never reaches the chat; its thinking goes back to it.
    assert_eq!((out.text.as_str(), out.reasoning.as_str()), ("done", ""));
    assert!(!out.text.contains("agent-pro"));
    match &out.reasoning_items[..] {
        [InputItem::Reasoning { model, .. }] => assert_eq!(model, "vendor/agent-pro"),
        other => panic!("{other:?}"),
    }
    // A reply naming a model that wasn't sent is not believed.
    *wire.reply.lock().unwrap() = r#"{"model":"other/model","choices":[{"message":{"content":"ok","reasoning_details":[{"type":"reasoning.text","text":"t"}]}}]}"#.into();
    let out = providers::call_model(
        &dir,
        "debug",
        "openrouter/vendor/agent-lite",
        Some("high"),
        &req,
        CALL_DATA,
        "conv-1:10",
        &CancelToken::new(),
    )
    .unwrap();
    match &out.reasoning_items[..] {
        [InputItem::Reasoning { model, .. }] => assert_eq!(model, "vendor/agent-lite"),
        other => panic!("{other:?}"),
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_chats_session_id_is_a_hash_of_its_id_never_its_text() {
    assert_eq!(
        providers::openrouter_session_id("conv-1").as_deref(),
        Some("36524fd8f6747fc2712506d01fee0e18")
    );
    assert_eq!(
        providers::openrouter_session_id(" or-chat-7 ").as_deref(),
        Some("bed5f8094605e99e4ec7d0fd7a4e7dc4")
    );
    assert_eq!(providers::openrouter_session_id("  "), None);
    // With no backup and no conversation, only the provider field is added.
    let mut b = json!({"model": "vendor/a"});
    providers::apply_openrouter_picks(&mut b, None, "");
    assert_eq!(b, json!({"model": "vendor/a"}));
}

/// A clock late enough for marks hours old.
const HNOW: u64 = 10 * 60 * 60 * 1000;

/// Two OpenAI-compatible providers set up, each with one reasoning model
/// under the ceiling; `marks` are OpenRouter's model's recent calls.
fn two_providers(label: &str, or_marks: &[(u64, bool, bool, u64)]) -> PathBuf {
    use grokhub_core::model_registry::record::CallMark;
    let (dir, _wire) = setup(label);
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    add_provider(&dir, "https://api.groq.com/openai/v1", KEY, NOW).unwrap();
    grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    grant_destination(&dir, "api.groq.com", CALL_DATA, UserClick::from_click()).unwrap();
    let row = |id: &str| ModelMeta {
        id: id.into(),
        context_length: Some(128_000),
        prices: Prices {
            prompt: Some(1_000),
            completion: Some(5_000),
            ..Prices::default()
        },
        tool_calling: Some(true),
        reasoning: Some(true),
        efforts: Some(vec!["low".into(), "high".into()]),
        output_modalities: Some(vec!["text".into()]),
        ..ModelMeta::default()
    };
    let mut reg = Registry::default();
    reg.apply_refresh(
        &[Listing::new(
            SourceKind::OpenAiCompatible,
            vec![row("openrouter/x-ai/grok-test"), row("groq/vendor/fast")],
        )],
        &[],
        1,
    );
    let rec = reg.models.get_mut("openrouter/x-ai/grok-test").unwrap();
    for &(ago, ok, pressure, latency_ms) in or_marks {
        rec.health.recent.push_back(CallMark {
            ts_ms: HNOW - ago,
            ok,
            pressure,
            latency_ms,
        });
    }
    let groq = reg.models.get_mut("groq/vendor/fast").unwrap();
    groq.health.recent.push_back(CallMark {
        ts_ms: HNOW - 100,
        ok: true,
        pressure: false,
        latency_ms: 1_000,
    });
    save_registry(&dir, &reg).unwrap();
    dir
}

fn stand_in(dir: &Path) -> Option<String> {
    let call = live::RouteCall {
        provider: "xai",
        class: "chat:default",
        model: "grok-4.7",
        ..live::RouteCall::default()
    };
    live::outage_stand_in(dir, &call, HNOW)
}

#[test]
fn a_busy_or_slow_provider_stands_in_after_a_healthy_one() {
    // Healthy: Grok on OpenRouter goes first, so the reply keeps its voice.
    let dir = two_providers("health-ok", &[(200, true, false, 1_500)]);
    assert_eq!(stand_in(&dir).as_deref(), Some("openrouter/x-ai/grok-test"));
    let _ = std::fs::remove_dir_all(dir);
    // Three of its last five calls answered 429: the other provider goes first.
    let busy = [
        (500, false, true, 0),
        (400, false, true, 0),
        (300, false, true, 0),
        (200, true, false, 1_000),
        (100, true, false, 1_000),
    ];
    let dir = two_providers("health-busy", &busy);
    assert_eq!(stand_in(&dir).as_deref(), Some("groq/vendor/fast"));
    let _ = std::fs::remove_dir_all(dir);
    // Over twice as slow as the other provider lately: after it too.
    let dir = two_providers(
        "health-slow",
        &[(300, true, false, 2_500), (200, true, false, 2_500)],
    );
    assert_eq!(stand_in(&dir).as_deref(), Some("groq/vendor/fast"));
    let _ = std::fs::remove_dir_all(dir);
    // A batched call's queueing (0, no sample) teaches nothing, and an hour-old
    // busy spell is over.
    let old_and_queued = [
        (2 * 60 * 60 * 1000, false, true, 0),
        (2 * 60 * 60 * 1000 - 1, false, true, 0),
        (2 * 60 * 60 * 1000 - 2, false, true, 0),
        (2 * 60 * 60 * 1000 - 3, false, true, 0),
        (2 * 60 * 60 * 1000 - 4, false, true, 0),
        (100, true, false, 0),
    ];
    let dir = two_providers("health-queued", &old_and_queued);
    assert_eq!(stand_in(&dir).as_deref(), Some("openrouter/x-ai/grok-test"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_openrouter_long_context_step_reaches_the_ceiling_and_the_cost() {
    // Base $10/M out, $15/M past 200,000 prompt tokens (OpenRouter's
    // `pricing.overrides`), read the way a refresh reads the listing.
    let body = r#"{"data":[{"id":"vendor/long-ctx","context_length":1000000,
        "supported_parameters":["tools","reasoning"],
        "architecture":{"output_modalities":["text"]},
        "pricing":{"prompt":"0.00000125","completion":"0.00001",
          "overrides":[{"min_prompt_tokens":200000,"prompt":"0.0000025","completion":"0.000015"}]}}]}"#;
    let rows =
        grokhub_core::model_registry::parse_openai_compatible_catalog("openrouter", body).unwrap();
    let meta = &rows[0];
    let need = |ctx_tokens| providers::PickNeed {
        tools: true,
        reasoning: true,
        ctx_tokens,
        ceiling_usd_per_m: 12.0,
    };
    let id = "openrouter/vendor/long-ctx";
    assert!(providers::auto_pickable(id, meta, &need(200_000)));
    assert!(
        !providers::auto_pickable(id, meta, &need(200_001)),
        "past the step its $15/M output is over a $12/M ceiling"
    );
    // chat:default predicts 600 tokens out; no cache price is listed, so
    // cached tokens pay the prompt rate.
    let cost = |ctx| super::policy::expected_cost_usd(&meta.prices, "chat:default", ctx).unwrap();
    assert_eq!(
        (cost(200_000) * 1e8).round() / 1e8,
        0.256,
        "200,000 in at $1.25/M and 600 out at $10/M"
    );
    assert_eq!(
        (cost(200_001) * 1e8).round() / 1e8,
        0.5090025,
        "every token at the long rates: $2.50/M in, $15/M out"
    );
}

#[test]
fn a_claude_row_becomes_auto_pickable_once_the_bundled_table_prices_it() {
    // Anthropic's own listing names no prices, so before the model card
    // table the row could never enter Auto's pool.
    let body = r#"{"data":[{"id":"claude-sonnet-5.5","type":"model","display_name":"Sonnet",
        "max_input_tokens":1000000,"max_tokens":128000,
        "capabilities":{"effort":{"levels":["low","medium","high"]}}}]}"#;
    let rows = grokhub_core::model_registry::parse_anthropic_catalog("anthropic", body).unwrap();
    let need = providers::PickNeed {
        tools: true,
        reasoning: true,
        ctx_tokens: 10_000,
        ceiling_usd_per_m: 15.0,
    };
    assert!(
        !providers::auto_pickable("anthropic/claude-sonnet-5.5", &rows[0], &need),
        "no prices in the listing: never pickable"
    );
    let mut reg = Registry::default();
    reg.apply_refresh(&[Listing::new(SourceKind::Anthropic, rows)], &[], 1);
    let rec = reg.get("anthropic/claude-sonnet-5.5").unwrap();
    assert_eq!(rec.route(), grokhub_core::model_registry::Route::Anthropic);
    // The bundled table priced it ($10/M out, under the $15 ceiling) and
    // filled its window, so the same filter now lets it in.
    assert!(providers::auto_pickable(
        "anthropic/claude-sonnet-5.5",
        &rec.meta,
        &need
    ));
    assert_eq!(rec.meta.prices.cache_write, Some(25_000));
}

/// A wire that answers every call 402: the account is out of credit.
#[derive(Default)]
struct BrokeWire(Mutex<Vec<Sent>>);

impl BrokeWire {
    fn calls(&self) -> usize {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|(url, _, _)| url.contains("chat/completions"))
            .count()
    }
}

impl ProviderTransport for BrokeWire {
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<HttpReply, String> {
        self.0
            .lock()
            .unwrap()
            .push((url.into(), headers.to_vec(), String::new()));
        Ok(HttpReply {
            status: 200,
            body: OR_LIST.into(),
            headers: Vec::new(),
        })
    }
    fn post(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &str,
    ) -> Result<HttpReply, String> {
        self.0
            .lock()
            .unwrap()
            .push((url.into(), headers.to_vec(), body.into()));
        Ok(HttpReply {
            status: 402,
            body: r#"{"error":{"message":"Insufficient credits"}}"#.into(),
            headers: Vec::new(),
        })
    }
}

/// Grok answers, and remembers the model it was asked for.
struct GrokOk(Mutex<String>);

impl ModelClient for GrokOk {
    fn stream(
        &self,
        r: &ResponsesRequest,
        _c: &CancelToken,
        s: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        *self.0.lock().unwrap() = r.model.clone();
        s(StreamEvent::TextDelta("hello from grok".into()));
        Ok(TurnOutput {
            text: "hello from grok".into(),
            ..TurnOutput::default()
        })
    }
}

#[test]
fn a_provider_out_of_credit_cools_it_and_grok_takes_the_turn() {
    let dir = test_dir("r3b-402-fallback");
    providers::use_vault_for(&dir, Arc::new(MemoryVault::default()));
    let wire = Arc::new(BrokeWire::default());
    providers::use_transport_for(&dir, wire.clone());
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet()).unwrap();
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    rank_first(&dir, OR_MODEL);
    let grok = GrokOk(Mutex::new(String::new()));
    let mut shown = String::new();
    let out = live::stream_routed(
        &grok,
        &chat("chat-402", &["say hello"]),
        &CancelToken::new(),
        &mut |ev| {
            if let StreamEvent::TextDelta(t) = ev {
                shown.push_str(&t);
            }
        },
        "chat:default",
    )
    .expect("Grok takes the turn the provider couldn't pay for");
    assert_eq!(out.text, "hello from grok");
    assert_eq!(shown, "hello from grok");
    assert_eq!(grok.0.lock().unwrap().as_str(), "grok-4.7");
    // The provider was tried once, then cooled: the next pick skips it.
    assert_eq!(wire.calls(), 1);
    assert!(live::cooling(&dir, OR_MODEL, grokhub_core::now_ms()));
    // The route record names Grok as the route that answered.
    let (_, rec) = route_records(&dir, ROUTE_TAIL_LINES)
        .pop()
        .expect("a route record");
    assert_eq!(
        (rec.used.provider.as_str(), rec.used.model.as_str()),
        ("xai", "grok-4.7")
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A provider out of credit on small talk: Grok answers with no word about
/// the switch. On a work turn the one plain line still shows.
#[test]
fn the_switch_line_shows_on_work_turns_and_never_on_small_talk() {
    let thought_for = |label: &str, ask: &str| {
        let dir = test_dir(label);
        providers::use_vault_for(&dir, Arc::new(MemoryVault::default()));
        providers::use_transport_for(&dir, Arc::new(BrokeWire::default()));
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        save_registry(&dir, &fleet()).unwrap();
        add_provider(&dir, OR_URL, KEY, NOW).unwrap();
        grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
        rank_first(&dir, OR_MODEL);
        let grok = GrokOk(Mutex::new(String::new()));
        let (mut shown, mut thought) = (String::new(), String::new());
        live::stream_routed(
            &grok,
            &chat(label, &[ask]),
            &CancelToken::new(),
            &mut |ev| match ev {
                StreamEvent::TextDelta(t) => shown.push_str(&t),
                StreamEvent::ReasoningDelta(t) => thought.push_str(&t),
                StreamEvent::RateWait(_) => {}
            },
            "chat:default",
        )
        .expect("Grok takes the turn");
        let _ = std::fs::remove_dir_all(dir);
        assert_eq!(shown, "hello from grok");
        thought
    };
    assert_eq!(
        thought_for("r3b-switch-work", "fix my wifi"),
        "The model I was using ran out of credit, so I'm carrying on with another one — the work so far is kept."
    );
    assert_eq!(
        thought_for("r3b-switch-casual", "haha how was your day?"),
        ""
    );
}

#[test]
fn provider_fallible_covers_limits_credit_and_outages_only() {
    let fallible = [
        ClientError::RateLimited {
            retry_after_secs: None,
            message: "slow down".into(),
        },
        ClientError::PaymentRequired {
            status: 402,
            message: "Insufficient credits".into(),
        },
        ClientError::Server {
            status: 502,
            message: "bad gateway".into(),
            retry_after_secs: None,
        },
        ClientError::Auth {
            status: 401,
            message: "bad key".into(),
        },
        ClientError::Transport("connection reset".into()),
        ClientError::Disconnect,
        ClientError::IdleTimeout,
        // OpenRouter's privacy settings left the model no endpoint.
        ClientError::Protocol(providers::POLICY_REFUSED.into()),
    ];
    for err in &fallible {
        assert!(live::provider_fallible(err), "{err:?}");
    }
    let not_fallible = [
        ClientError::Cancelled,
        // A refusal about the prompt: the next route would refuse it too.
        ClientError::Protocol("Blocked by content safety".into()),
        // Too long for this model is the run loop's compaction retry, and
        // the app's own offline mode blocks every cloud route alike.
        ClientError::Protocol("HTTP 400: maximum context length exceeded".into()),
        ClientError::Transport(
            "Offline only is on, so Loadout can't reach the internet. Turn it off in Settings → AI models to use this."
                .into(),
        ),
    ];
    for err in &not_fallible {
        assert!(!live::provider_fallible(err), "{err:?}");
    }
}

/// Failover step 9: a wire that answers every POST with one canned
/// status and body, and keeps what it was sent.
struct ProbeWire {
    status: u16,
    body: String,
    sent: Mutex<Vec<String>>,
}

impl ProviderTransport for ProbeWire {
    fn get(&self, _url: &str, _headers: &[(String, String)]) -> Result<HttpReply, String> {
        Ok(HttpReply {
            status: 200,
            body: OR_LIST.into(),
            headers: Vec::new(),
        })
    }
    fn post(
        &self,
        _url: &str,
        _headers: &[(String, String)],
        body: &str,
    ) -> Result<HttpReply, String> {
        self.sent.lock().unwrap().push(body.into());
        Ok(HttpReply {
            status: self.status,
            body: self.body.clone(),
            headers: Vec::new(),
        })
    }
}

const TRIAL_MODEL: &str = "openrouter/vendor/new:free";

/// A granted OpenRouter key, a registry with one free model on tryout
/// (saved, as the send path reads it) and the canned wire.
fn probe_setup(label: &str, status: u16, body: &str) -> (PathBuf, Arc<ProbeWire>, Registry) {
    let dir = test_dir(label);
    providers::use_vault_for(&dir, Arc::new(MemoryVault::default()));
    let wire = Arc::new(ProbeWire {
        status,
        body: body.into(),
        sent: Mutex::new(Vec::new()),
    });
    providers::use_transport_for(&dir, wire.clone());
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    let or = |ids: &[&str]| {
        Listing::new(
            SourceKind::OpenAiCompatible,
            ids.iter().map(|id| ModelMeta::bare(id)).collect(),
        )
    };
    let mut reg = Registry::default();
    reg.apply_refresh(&[or(&["openrouter/vendor/old:free"])], &[], NOW);
    reg.apply_refresh(
        &[or(&["openrouter/vendor/old:free", TRIAL_MODEL])],
        &[],
        NOW,
    );
    assert!(reg.get(TRIAL_MODEL).is_some_and(|r| r.probation.is_some()));
    save_registry(&dir, &reg).unwrap();
    (dir, wire, reg)
}

fn tally(reg: &Registry) -> (u32, u32, u32) {
    let p = reg.get(TRIAL_MODEL).unwrap().probation.clone().unwrap();
    (p.calls, p.ok, p.probes)
}

#[test]
fn a_tryout_probe_counts_a_tool_call_that_answers_and_one_that_does_not() {
    let call = r#"{"choices":[{"finish_reason":"tool_calls","message":{"role":"assistant","content":null,"tool_calls":[{"id":"c1","type":"function","function":{"name":"record_number","arguments":"{\"n\":7}"}}]}}]}"#;
    let (dir, wire, mut reg) = probe_setup("trial-probe-pass", 200, call);
    assert_eq!(super::trial::run_probes(&dir, &mut reg, NOW), 1);
    assert_eq!(tally(&reg), (1, 1, 1));
    // The probe goes as the model's own answer: no backup, a small cap, the tool.
    let sent: serde_json::Value = serde_json::from_str(&wire.sent.lock().unwrap()[0]).unwrap();
    assert!(sent.get("models").is_none(), "{sent}");
    assert_eq!(sent["max_tokens"], json!(512));
    assert!(sent.to_string().contains("record_number"));
    // The second of the day is the text probe; a third waits for tomorrow.
    super::trial::run_probes(&dir, &mut reg, NOW + 1);
    assert_eq!(tally(&reg), (2, 1, 2));
    assert_eq!(super::trial::run_probes(&dir, &mut reg, NOW + 2), 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_limit_on_a_probe_counts_against_the_account_not_the_model() {
    let (dir, wire, mut reg) = probe_setup(
        "trial-probe-limit",
        429,
        r#"{"error":{"code":429,"message":"Rate limit exceeded: free-models-per-day"}}"#,
    );
    assert_eq!(super::trial::run_probes(&dir, &mut reg, NOW), 1);
    assert_eq!(tally(&reg), (0, 0, 0));
    assert_eq!(wire.sent.lock().unwrap().len(), 1);
    // A server error is the model's.
    let (dir2, _wire, mut reg2) = probe_setup("trial-probe-502", 502, r#"{"error":{"code":502}}"#);
    super::trial::run_probes(&dir2, &mut reg2, NOW);
    assert_eq!(tally(&reg2), (1, 0, 1));
    for d in [dir, dir2] {
        let _ = std::fs::remove_dir_all(d);
    }
}

#[test]
fn no_grant_means_no_probe() {
    let dir = test_dir("trial-probe-nogrant");
    providers::use_vault_for(&dir, Arc::new(MemoryVault::default()));
    let wire = Arc::new(ProbeWire {
        status: 200,
        body: OR_REPLY.into(),
        sent: Mutex::new(Vec::new()),
    });
    providers::use_transport_for(&dir, wire.clone());
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    let or = |ids: &[&str]| {
        Listing::new(
            SourceKind::OpenAiCompatible,
            ids.iter().map(|id| ModelMeta::bare(id)).collect(),
        )
    };
    let mut reg = Registry::default();
    reg.apply_refresh(&[or(&["openrouter/vendor/old:free"])], &[], NOW);
    reg.apply_refresh(
        &[or(&["openrouter/vendor/old:free", TRIAL_MODEL])],
        &[],
        NOW,
    );
    assert_eq!(super::trial::run_probes(&dir, &mut reg, NOW), 0);
    assert!(wire.sent.lock().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// The connection check's wire: a listing answer and a reply answer.
struct CheckWire {
    list_status: u16,
    reply_body: String,
    posts: Mutex<u32>,
}

impl ProviderTransport for CheckWire {
    fn get(&self, _url: &str, _headers: &[(String, String)]) -> Result<HttpReply, String> {
        Ok(HttpReply {
            status: self.list_status,
            body: if self.list_status == 200 {
                OR_LIST.into()
            } else {
                r#"{"error":{"code":401,"message":"No auth credentials found"}}"#.into()
            },
            headers: Vec::new(),
        })
    }
    fn post(
        &self,
        _url: &str,
        _headers: &[(String, String)],
        _body: &str,
    ) -> Result<HttpReply, String> {
        *self.posts.lock().unwrap() += 1;
        Ok(HttpReply {
            status: 200,
            body: self.reply_body.clone(),
            headers: Vec::new(),
        })
    }
}

fn check_setup(label: &str, list_status: u16, grant: bool) -> (PathBuf, Arc<CheckWire>) {
    let dir = test_dir(label);
    providers::use_vault_for(&dir, Arc::new(MemoryVault::default()));
    let wire = Arc::new(CheckWire {
        list_status,
        reply_body: r#"{"choices":[{"finish_reason":"stop","message":{"role":"assistant","content":"Ready"}}]}"#.into(),
        posts: Mutex::new(0),
    });
    providers::use_transport_for(&dir, wire.clone());
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    if grant {
        grant_destination(&dir, OR_DEST, CALL_DATA, UserClick::from_click()).unwrap();
    }
    let mut reg = Registry::default();
    reg.apply_refresh(
        &[Listing::new(
            SourceKind::OpenAiCompatible,
            vec![ModelMeta::bare("openrouter/vendor/cheap")],
        )],
        &[],
        NOW,
    );
    save_registry(&dir, &reg).unwrap();
    (dir, wire)
}

#[test]
fn the_connection_check_asks_the_wire_and_reports_what_it_said() {
    use super::check::{self, Wire};
    // Allowed: the key lists models and the cheapest one says the word.
    let (dir, wire) = check_setup("check-ready", 200, true);
    let c = check::run(&dir, true, NOW);
    let p = &c.providers[0];
    assert_eq!(p.name, "OpenRouter");
    assert_eq!(p.listing, Some(Wire::Listed(1)));
    assert!(matches!(p.reply, Some(Wire::Replied(_))), "{:?}", p.reply);
    assert_eq!(*wire.posts.lock().unwrap(), 1);
    assert!(check::report(&c, false).contains("Auto can use it."));
    // Saved settings only: nothing goes out.
    let quiet = check::run(&dir, false, NOW);
    assert_eq!(
        (
            quiet.providers[0].listing.clone(),
            quiet.providers[0].reply.clone()
        ),
        (None, None)
    );
    assert_eq!(*wire.posts.lock().unwrap(), 1);
    // Not allowed yet: nothing goes out, and the report says why.
    let (dir2, wire2) = check_setup("check-ungranted", 200, false);
    let c2 = check::run(&dir2, true, NOW);
    assert_eq!(
        (
            c2.providers[0].listing.clone(),
            c2.providers[0].reply.clone()
        ),
        (None, None)
    );
    assert_eq!(*wire2.posts.lock().unwrap(), 0);
    assert!(check::report(&c2, false).contains("not allowed yet"));
    // A rejected key says so, and no reply is tried.
    let (dir3, wire3) = check_setup("check-badkey", 401, true);
    let c3 = check::run(&dir3, true, NOW);
    assert_eq!(c3.providers[0].listing, Some(Wire::KeyRejected));
    assert_eq!(*wire3.posts.lock().unwrap(), 0);
    let r3 = check::report(&c3, false);
    assert!(
        r3.contains("rejected the saved key") && !r3.contains("Auto can use it"),
        "{r3}"
    );
    for d in [dir, dir2, dir3] {
        let _ = std::fs::remove_dir_all(d);
    }
}

// ---- The keyring and the saved list, read with care ---------------------------

/// A keyring that fails the calls it is told to and otherwise keeps keys in memory.
#[derive(Default)]
struct ShakyVault {
    keys: MemoryVault,
    /// How many reads fail before one answers.
    failed_reads: Mutex<u32>,
    /// How many times the keyring was read.
    reads: Mutex<u32>,
    set_fails: bool,
    delete_fails: bool,
}

impl providers::KeyVault for ShakyVault {
    fn get(&self, id: &str) -> Result<Option<zeroize::Zeroizing<String>>, String> {
        *self.reads.lock().unwrap() += 1;
        let mut left = self.failed_reads.lock().unwrap();
        if *left > 0 {
            *left -= 1;
            return Err("the keyring is locked".into());
        }
        self.keys.get(id)
    }

    fn set(&self, id: &str, key: &str) -> Result<(), String> {
        if self.set_fails {
            return Err("No default store has been set, so cannot search or create entries".into());
        }
        self.keys.set(id, key)
    }

    fn delete(&self, id: &str) -> Result<(), String> {
        if self.delete_fails {
            return Err("no secret service".into());
        }
        self.keys.delete(id)
    }
}

#[test]
fn a_keyring_read_that_failed_is_asked_again_and_not_remembered_as_no_key() {
    use providers::KeyVault;
    use std::time::{Duration, Instant};
    let dir = test_dir("r3b-key-reread");
    let vault = Arc::new(ShakyVault {
        failed_reads: Mutex::new(1),
        ..ShakyVault::default()
    });
    vault.keys.set("openrouter", KEY).unwrap();
    providers::use_vault_for(&dir, vault.clone());
    let start = Instant::now();
    assert!(
        !providers::has_key_at(&dir, "openrouter", start),
        "the keyring could not be read"
    );
    // A page that repaints many times a second does not ask a failing keyring each time.
    assert!(!providers::has_key_at(
        &dir,
        "openrouter",
        start + Duration::from_secs(4)
    ));
    assert_eq!(*vault.reads.lock().unwrap(), 1);
    assert_eq!(providers::KEY_RETRY, Duration::from_secs(5));
    assert!(
        providers::has_key_at(&dir, "openrouter", start + providers::KEY_RETRY),
        "the keyring is asked again"
    );
    // An answer the keyring gave is remembered.
    assert!(providers::has_key(&dir, "openrouter"));
    assert_eq!(*vault.reads.lock().unwrap(), 2);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_keyring_installed_after_the_first_ask_finds_the_saved_key() {
    use providers::KeyVault;
    let dir = test_dir("r3b-key-late-vault");
    assert!(
        !providers::has_key(&dir, "openrouter"),
        "no keyring yet: no key"
    );
    let vault = MemoryVault::default();
    vault.set("openrouter", KEY).unwrap();
    providers::use_default_vault(Some(Arc::new(vault)));
    let found = providers::has_key(&dir, "openrouter");
    // A vault that answered "no key" is remembered until a new one is installed.
    providers::use_default_vault(Some(Arc::new(MemoryVault::default())));
    let gone = providers::has_key(&dir, "openrouter");
    let late = MemoryVault::default();
    late.set("openrouter", KEY).unwrap();
    providers::use_default_vault(Some(Arc::new(late)));
    let back = providers::has_key(&dir, "openrouter");
    providers::use_default_vault(None);
    assert_eq!((found, gone, back), (true, false, true));
    let _ = std::fs::remove_dir_all(dir);
}

/// Add `https://models.example.org/v1` with a key, then edit its address in the saved file by hand.
fn moved_provider(label: &str, granted: &str, address: &str) -> PathBuf {
    let (dir, _wire) = setup(label);
    add_provider(&dir, "https://models.example.org/v1", KEY, NOW).unwrap();
    grant_destination(&dir, granted, CALL_DATA, UserClick::from_click()).unwrap();
    let path = providers::providers_path(&dir);
    let raw = std::fs::read_to_string(&path).unwrap();
    assert_eq!(raw.matches("https://models.example.org/v1").count(), 1);
    std::fs::write(&path, raw.replace("https://models.example.org/v1", address)).unwrap();
    dir
}

#[test]
fn a_saved_provider_moved_to_plain_http_is_not_loaded_and_gets_no_key() {
    let dir = moved_provider(
        "r3b-moved-http",
        "models.example.org",
        "http://models.example.org/v1",
    );
    assert_eq!(providers::load_providers(&dir), Vec::new());
    assert_eq!(usable(&dir, CALL_DATA), Vec::<String>::new());
    assert!(providers::sources(&dir).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_saved_provider_moved_to_another_host_is_not_loaded_and_gets_no_key() {
    let dir = moved_provider(
        "r3b-moved-host",
        "other.example.net",
        "https://other.example.net/v1",
    );
    assert_eq!(providers::load_providers(&dir), Vec::new());
    assert_eq!(usable(&dir, CALL_DATA), Vec::<String>::new());
    assert!(providers::sources(&dir).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_untouched_saved_provider_still_loads() {
    let (dir, _wire) = setup("r3b-saved-loads");
    let p = add_provider(&dir, "https://models.example.org/v1/", KEY, NOW).unwrap();
    assert_eq!(p.base_url, "https://models.example.org/v1");
    assert_eq!(providers::load_providers(&dir), vec![p]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn removing_a_provider_that_was_never_added_says_so_and_reaches_no_other_file() {
    let (dir, _wire) = setup("r3b-remove-unknown");
    assert_eq!(
        providers::remove_provider(&dir, "nope").unwrap_err(),
        "That provider isn't added."
    );
    // An id shaped like a path: `models/listing-x/../../app.json` is the settings file.
    std::fs::create_dir_all(dir.join("models").join("listing-x")).unwrap();
    std::fs::write(dir.join("app.json"), "{}").unwrap();
    assert_eq!(
        providers::remove_provider(&dir, "x/../../app").unwrap_err(),
        "That provider isn't added."
    );
    assert_eq!(std::fs::read_to_string(dir.join("app.json")).unwrap(), "{}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn removing_with_the_keyring_down_keeps_the_provider_and_says_why() {
    let dir = test_dir("r3b-remove-keyring-down");
    providers::use_vault_for(
        &dir,
        Arc::new(ShakyVault {
            delete_fails: true,
            ..ShakyVault::default()
        }),
    );
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    assert_eq!(
        providers::remove_provider(&dir, "openrouter").unwrap_err(),
        "The keyring isn't available, so openrouter wasn't removed. Try again when it is."
    );
    assert_eq!(providers::load_providers(&dir).len(), 1);
    assert!(providers::has_key(&dir, "openrouter"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_keyring_that_refuses_the_key_says_so_in_plain_words_and_saves_no_provider() {
    let dir = test_dir("r3b-add-keyring-off");
    providers::use_vault_for(
        &dir,
        Arc::new(ShakyVault {
            set_fails: true,
            ..ShakyVault::default()
        }),
    );
    assert_eq!(
        add_provider(&dir, OR_URL, KEY, NOW).unwrap_err(),
        "The keyring isn't available, so the key wasn't saved."
    );
    assert!(!providers::providers_path(&dir).exists());
    let _ = std::fs::remove_dir_all(dir);
}

/// A row the loader skips (its address was edited by hand) is not shown or
/// used, but it stays in the file through another provider's add and remove,
/// and it can still be removed together with its key.
#[test]
fn a_skipped_provider_stays_in_the_file_and_can_still_be_removed_with_its_key() {
    let dir = moved_provider(
        "r3b-skipped-kept",
        "models.example.org",
        "http://models.example.org/v1",
    );
    let rows = |dir: &Path| -> Vec<String> {
        let raw = std::fs::read_to_string(providers::providers_path(dir)).unwrap();
        let file: serde_json::Value = serde_json::from_str(&raw).unwrap();
        file["providers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                format!(
                    "{} {}",
                    p["id"].as_str().unwrap(),
                    p["base_url"].as_str().unwrap()
                )
            })
            .collect()
    };
    let skipped = "models http://models.example.org/v1".to_string();
    assert_eq!(providers::load_providers(&dir), Vec::new());
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    assert_eq!(
        rows(&dir),
        vec![skipped.clone(), format!("openrouter {OR_URL}")]
    );
    providers::remove_provider(&dir, "openrouter").unwrap();
    assert_eq!(rows(&dir), vec![skipped]);
    // Still hidden and unused, and its key is still in the keyring.
    assert_eq!(providers::load_providers(&dir), Vec::new());
    assert!(providers::has_key(&dir, "models"));
    assert_eq!(providers::remove_provider(&dir, "models"), Ok(()));
    assert_eq!(rows(&dir), Vec::<String>::new());
    assert!(!providers::has_key(&dir, "models"));
    let _ = std::fs::remove_dir_all(dir);
}

/// A hand-written row whose id is not one an address gives (a path) is never
/// removed by that id: the id would name a file.
#[test]
fn a_saved_row_with_a_path_for_an_id_is_not_removed_by_it() {
    let (dir, _wire) = setup("r3b-skipped-path-id");
    add_provider(&dir, OR_URL, KEY, NOW).unwrap();
    let path = providers::providers_path(&dir);
    let raw = std::fs::read_to_string(&path).unwrap();
    assert_eq!(raw.matches("\"openrouter\"").count(), 1);
    std::fs::write(&path, raw.replace("\"openrouter\"", "\"x/../../app\"")).unwrap();
    std::fs::create_dir_all(dir.join("models").join("listing-x")).unwrap();
    std::fs::write(dir.join("app.json"), "{}").unwrap();
    assert_eq!(
        providers::remove_provider(&dir, "x/../../app").unwrap_err(),
        "That provider isn't added."
    );
    assert_eq!(std::fs::read_to_string(dir.join("app.json")).unwrap(), "{}");
    let _ = std::fs::remove_dir_all(dir);
}
