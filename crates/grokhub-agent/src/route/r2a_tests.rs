//! Router R2a acceptance: chaos fixtures (zero network) heal with the right
//! route and the right message tier, one model per episode, fallbacks stay on
//! `included` routes, and the live path keeps an episode on its model.

use std::collections::BTreeMap;
use std::sync::Mutex;

use grokhub_core::model_registry::profile::{write_profile, EffortTiming, ModelProfile, ProbeResult};
use grokhub_core::model_registry::store::save_registry;
use grokhub_core::model_registry::cost_class::CostClass;
use grokhub_core::model_registry::{CallStatus, Credential, Listing, ModelMeta, ModelState, Observation, Prices, Registry, RegistryEvent, SourceKind};

use super::heal::{heal_messages, HealNotes, Tier};
use super::ladder::{rung, Pick};
use super::table::{build_table, table_path, ClassTable, EvalCache, RoutingTable, SpeedQuality, TableInputs, TableRow};
use super::{live, Router, RouteInput};
use crate::client::{ClientError, ContentPart, InputItem, ModelClient, ResponsesRequest, StreamEvent, TurnOutput, Usage};
use crate::CancelToken;

pub(super) const IDS: [&str; 4] = ["grok-4.7", "grok-4.6", "grok-4.5", "grok-4.3"];
const NOW: u64 = 1_000_000;

fn meta(id: &str, image: bool) -> ModelMeta {
    let v: u64 = id.rsplit('.').next().and_then(|n| n.parse().ok()).unwrap_or(1);
    let mut modalities = vec!["text".to_string()];
    if image {
        modalities.push("image".into());
    }
    ModelMeta {
        id: id.into(),
        context_length: Some(256_000),
        max_output: Some(32_000),
        // Newer costs more.
        prices: Prices { prompt: Some(10_000 * v), cached: Some(2_500 * v), completion: Some(50_000 * v), long_context_threshold: Some(200_000), ..Prices::default() },
        efforts: Some(vec!["low".into(), "medium".into(), "high".into(), "xhigh".into()]),
        tool_calling: Some(true),
        structured_output: Some(true),
        input_modalities: Some(modalities),
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
        efforts: vec![EffortTiming { effort: "medium".into(), measured: "probe".into(), reply_ms: Some(900), tokens_per_sec: Some(40) }],
        ..ProbeResult::default()
    }
}

fn listing(metas: Vec<ModelMeta>, tier: &str) -> Listing {
    let mut l = Listing::new(SourceKind::XaiApi, metas);
    l.tier = Some(tier.into());
    l
}

/// Four grok-4.x models on a plan sign-in, every profile usable.
pub(super) fn fleet() -> (Registry, BTreeMap<String, ModelProfile>) {
    let mut reg = Registry::default();
    reg.entitlement.credential = Credential::Plan;
    reg.apply_refresh(&[listing(IDS.iter().map(|id| meta(id, true)).collect(), "SuperGrok Heavy")], &[], 1);
    let profiles = IDS.iter().map(|id| (id.to_string(), ModelProfile::build(&meta(id, true), ok_probe(), 1))).collect();
    (reg, profiles)
}

fn obs(model: &str, status: CallStatus, http: u16, ts_ms: u64, served: Option<&str>) -> Observation {
    Observation { model: model.into(), ts_ms, status, latency_ms: 700, served_model: served.map(str::to_string), endpoint_ok: true, reasoning_tokens: 0, cost_ticks: 0, http }
}

fn burst(model: &str, status: CallStatus, http: u16, n: u64, from: u64) -> Vec<Observation> {
    (0..n).map(|i| obs(model, status, http, from + i, None)).collect()
}

fn ask<'a>(model: &'a str, pinned: bool) -> RouteInput<'a> {
    RouteInput { provider: "xai", class: "chat:default", current_model: model, current_effort: None, pinned, d: 0.5, needs_tools: true, ..RouteInput::default() }
}

type Apply = fn(&mut Registry, u64) -> Vec<RegistryEvent>;
/// (fixture, the pin's state after, pinned route, its rule, the one message tier for a pin, for Auto)
type Fixture = (&'static str, Apply, ModelState, &'static str, &'static str, Option<Tier>, Option<Tier>);

/// The pinned model in every fixture.
const PIN: &str = "grok-4.6";

fn storm(reg: &mut Registry, t: u64) -> Vec<RegistryEvent> {
    reg.fold(&burst(PIN, CallStatus::NotFound, 404, 4, t)).0
}
fn busy(reg: &mut Registry, t: u64) -> Vec<RegistryEvent> {
    reg.fold(&burst(PIN, CallStatus::Pressure, 429, 3, t)).0
}
fn flap(reg: &mut Registry, t: u64) -> Vec<RegistryEvent> {
    reg.fold(&burst(PIN, CallStatus::Pressure, 503, 3, t)).0
}
fn redirect(reg: &mut Registry, t: u64) -> Vec<RegistryEvent> {
    reg.fold(&[obs(PIN, CallStatus::Ok, 200, t, Some("grok-4.7"))]).0
}
fn downgrade(reg: &mut Registry, t: u64) -> Vec<RegistryEvent> {
    let metas = IDS.iter().filter(|id| **id != PIN).map(|id| meta(id, true)).collect();
    reg.apply_refresh(&[listing(metas, "SuperGrok")], &[], t)
}
fn placeholder(reg: &mut Registry, t: u64) -> Vec<RegistryEvent> {
    let metas = IDS.iter().map(|id| if *id == PIN { ModelMeta::bare(id) } else { meta(id, true) }).collect();
    reg.apply_refresh(&[listing(metas, "SuperGrok Heavy")], &[], t)
}

#[test]
fn chaos_fixtures_heal_with_the_right_route_and_exactly_the_planned_messages() {
    let fixtures: &[Fixture] = &[
        ("404 storm", storm, ModelState::Ghost, "grok-4.7", "pin:fallback", Some(Tier::HomeUpdate), None),
        ("429 burst", busy, ModelState::Degraded, "grok-4.7", "pin:fallback", Some(Tier::WorkRow), None),
        ("503 flap", flap, ModelState::Quarantined, "grok-4.7", "pin:fallback", Some(Tier::WorkRow), None),
        ("redirected slug", redirect, ModelState::Redirected, "grok-4.7", "pin:redirect", Some(Tier::HomeUpdate), None),
        ("tier downgrade", downgrade, ModelState::NotInPlan, "grok-4.7", "pin:fallback", Some(Tier::HomeUpdate), Some(Tier::HomeUpdate)),
        ("placeholder row", placeholder, ModelState::Ghost, "grok-4.7", "pin:fallback", Some(Tier::HomeUpdate), None),
    ];
    for (name, apply, state, model, rule, pin_tier, auto_tier) in fixtures {
        for pinned in [true, false] {
            let (mut reg, profiles) = fleet();
            let table = build_table(&TableInputs { reg: &reg, profiles: &profiles }, &EvalCache::default(), NOW);
            let events = apply(&mut reg, NOW);
            assert_eq!(reg.get(PIN).unwrap().state, *state, "{name}");
            let route = Router::choose(&ask(PIN, pinned), &reg, &profiles, &table, NOW + 10);
            assert!(!route.no_route, "{name}");
            assert_ne!(route.model, PIN, "{name}: never the broken model");
            if pinned {
                assert_eq!((route.model.as_str(), route.rule_ids[0].as_str()), (*model, *rule), "{name}");
            }
            let mut notes = HealNotes::default();
            let pin = if pinned { PIN } else { "" };
            let told = notes.admit(heal_messages(&events, &reg, &profiles, pin, NOW), NOW);
            let tiers: Vec<Tier> = told.iter().map(|m| m.tier).collect();
            let want = if pinned { pin_tier } else { auto_tier };
            assert_eq!(tiers, want.iter().copied().collect::<Vec<_>>(), "{name} pinned={pinned}: {told:?}");
            // The incident goes on: nothing more is said.
            let again = apply(&mut reg, NOW + 1_000);
            assert!(notes.admit(heal_messages(&again, &reg, &profiles, pin, NOW + 1_000), NOW + 1_000).is_empty(), "{name} pinned={pinned}");
        }
    }
}

#[test]
fn the_messages_read_as_the_plan_words_them() {
    let (mut reg, profiles) = fleet();
    let ev = busy(&mut reg, NOW);
    let told = heal_messages(&ev, &reg, &profiles, PIN, NOW).msgs;
    assert_eq!(told[0].text, "grok-4.6 isn't answering, so I'm using grok-4.7 for now. Your pick is saved.");
    let (mut reg, profiles) = fleet();
    let ev = downgrade(&mut reg, NOW);
    let told = heal_messages(&ev, &reg, &profiles, PIN, NOW).msgs;
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].text, "Your plan changed, so grok-4.6 isn't included anymore. I'll use grok-4.7.");
    let (mut reg, profiles) = fleet();
    let mut old = meta(PIN, true);
    old.deprecated = Some(true);
    let ev = reg.apply_refresh(&[listing(IDS.iter().map(|id| if *id == PIN { old.clone() } else { meta(id, true) }).collect(), "SuperGrok Heavy")], &[], NOW);
    let told = heal_messages(&ev, &reg, &profiles, PIN, NOW).msgs;
    assert_eq!(told[0].text, "grok-4.6 was retired by xAI, so I'm using grok-4.7 where you picked it. Your pick shows (retired) until you choose another in Settings.");
    assert_eq!(reg.pin_label(PIN), "grok-4.6 (retired)");
    let r = Router::choose(&ask(PIN, true), &reg, &profiles, &RoutingTable::default(), NOW);
    assert_eq!((r.model.as_str(), r.replaced.as_deref()), ("grok-4.7", Some(PIN)), "a retired pin falls back");
}

#[test]
fn a_flap_tells_once_per_incident_and_again_after_it_recovers() {
    let (mut reg, profiles) = fleet();
    let mut notes = HealNotes::default();
    let mut told = 0;
    for round in 0..3u64 {
        let t = NOW + round * 10_000_000;
        let ev = flap(&mut reg, t);
        told += notes.admit(heal_messages(&ev, &reg, &profiles, PIN, t), t).len();
        // Its half-open check passes: healthy again, the incident ends.
        let until = reg.get(PIN).unwrap().breaker.open_until_ms;
        reg.mark_half_open(PIN);
        let ev: Vec<RegistryEvent> = reg.half_open_result(PIN, true, until).into_iter().collect();
        told += notes.admit(heal_messages(&ev, &reg, &profiles, PIN, until), until).len();
        assert_eq!(reg.get(PIN).unwrap().state, ModelState::Live);
    }
    assert_eq!(told, 3, "one row per incident");
}

#[test]
fn fallback_never_picks_a_route_that_is_not_included() {
    let (mut reg, profiles) = fleet();
    // grok-4.8 is in Grok Build's list only: its native cost class is unknown.
    let mut gb = Listing::new(SourceKind::GrokBuild, vec![ModelMeta::bare("grok-4.8")]);
    gb.tier = None;
    let mut api = listing(IDS.iter().map(|id| meta(id, true)).collect(), "SuperGrok Heavy");
    api.tier = Some("SuperGrok Heavy".into());
    reg.apply_refresh(&[api.clone(), gb.clone()], &[], 2);
    assert_eq!(reg.get("grok-4.8").unwrap().state, ModelState::Live);
    for id in IDS {
        reg.fold(&burst(id, CallStatus::Error, 500, 3, NOW));
    }
    for pinned in [true, false] {
        let r = Router::choose(&ask(PIN, pinned), &reg, &profiles, &RoutingTable::default(), NOW + 1);
        assert!(r.no_route, "pinned={pinned}: {:?}", r.rule_ids);
        assert_ne!(r.model, "grok-4.8");
    }
    // R2b: a key at list price is included up to your $/M ceiling. grok-4.5
    // ($25/M out) is over the default $15/M, so it's premium: Auto stands in
    // with grok-4.3 ($15/M) and the cabin asks once.
    let (mut keyed, profiles) = fleet();
    keyed.entitlement.credential = Credential::ApiKey;
    let r = Router::choose(&ask("grok-4.5", false), &keyed, &profiles, &RoutingTable::default(), NOW);
    assert_eq!((r.model.as_str(), r.rule_ids[0].as_str(), r.replaced.as_deref()), ("grok-4.3", "cost:premium_ungranted", None));
    assert_eq!((r.cost_class, r.premium_ask.as_deref()), (CostClass::Included, Some("premium:grok-4.5")));
    // Under a $30/M ceiling it is included, and with no table row a key keeps the call's own model.
    let mut wide = ask("grok-4.5", false);
    wide.spend.settings.ceiling_usd_per_m = 30.0;
    let r = Router::choose(&wide, &keyed, &profiles, &RoutingTable::default(), NOW);
    assert_eq!((r.model.as_str(), r.rule_ids[0].as_str(), r.premium_ask.clone()), ("grok-4.5", "key:keep", None));
    // No sign-in: nothing native is included, the call keeps its own model.
    keyed.entitlement.credential = Credential::None;
    let r = Router::choose(&ask("grok-4.5", false), &keyed, &profiles, &RoutingTable::default(), NOW);
    assert_eq!((r.model.as_str(), r.rule_ids[0].as_str()), ("grok-4.5", "cost:not_included"));
}

#[test]
fn a_vision_step_never_picks_a_model_without_image_input() {
    let (mut reg, mut profiles) = fleet();
    let text_only = meta("grok-4.7", false);
    reg.apply_refresh(&[listing(IDS.iter().map(|id| if *id == "grok-4.7" { text_only.clone() } else { meta(id, true) }).collect(), "SuperGrok Heavy")], &[], 2);
    profiles.insert("grok-4.7".into(), ModelProfile::build(&text_only, ok_probe(), 2));
    let table = top_first("grok-4.7");
    let mut i = ask("grok-4.7", false);
    assert_eq!(Router::choose(&i, &reg, &profiles, &table, NOW).model, "grok-4.7");
    i.needs_image = true;
    let r = Router::choose(&i, &reg, &profiles, &table, NOW);
    assert_ne!(r.model, "grok-4.7");
    assert!(reg.get(&r.model).unwrap().meta.input_modalities.as_ref().unwrap().contains(&"image".to_string()));
}

/// A hand-made table with `top` first in chat:default, the rest after.
pub(super) fn top_first(top: &str) -> RoutingTable {
    let mut order: Vec<&str> = vec![top];
    order.extend(IDS.iter().filter(|id| **id != top));
    let ranked = order
        .iter()
        .map(|m| TableRow { model: m.to_string(), effort: Some("medium".into()), quality: Some(0.8), est_cost_usd: Some(0.01), p50_latency_ms: Some(900), why: "the best balance for everyday chat in your plan".into() })
        .collect();
    let mut t = RoutingTable { schema: 1, version: 1, ..RoutingTable::default() };
    t.classes.insert("chat:default".into(), ClassTable { speed_quality: SpeedQuality::Balanced, ranked });
    t
}

#[test]
fn one_model_per_episode_over_fifty_episodes() {
    let (mut reg, profiles) = fleet();
    let tables = [top_first("grok-4.7"), top_first("grok-4.5")];
    let e2 = Pick { rung: rung("xhigh").unwrap(), rules: vec!["E2".into(), "E2:next_model".into()], ..Pick::default() };
    let mut per_episode = Vec::new();
    for ep in 0..50u64 {
        let mut model: Option<String> = None;
        let mut switches = 0;
        let mut quarantined: Option<String> = None;
        for step in 0..6u64 {
            let t = NOW + ep * 1_000 + step;
            // The table changes mid-episode every fifth episode.
            let table = &tables[((ep % 5 == 0) && step >= 2) as usize];
            // Every seventh episode its model goes unhealthy at step 3.
            let unhealthy = ep % 7 == 3 && step == 3;
            if unhealthy {
                let m = model.clone().unwrap();
                reg.fold(&burst(&m, CallStatus::Error, 0, 3, t));
                quarantined = Some(m);
            }
            // Every eleventh episode the check rejects twice at the ceiling at step 4.
            let reject = ep % 11 == 5 && step == 4;
            let mut i = ask("grok-4.7", false);
            i.episode_model = model.as_deref();
            i.pick = reject.then(|| e2.clone());
            let r = Router::choose(&i, &reg, &profiles, table, t);
            if let Some(prev) = &model {
                if *prev != r.model {
                    switches += 1;
                    assert!(unhealthy || reject, "episode {ep} step {step} switched {prev} → {} on {:?}", r.model, r.rule_ids);
                }
            }
            model = Some(r.model);
        }
        if let Some(m) = quarantined {
            reg.mark_half_open(&m);
            reg.half_open_result(&m, true, u64::MAX / 2);
        }
        per_episode.push(switches);
    }
    per_episode.sort_unstable();
    assert!(per_episode[per_episode.len() / 2] <= 1, "{per_episode:?}");
    assert!(per_episode.iter().filter(|s| **s > 0).count() >= 5, "the fixture does switch on E2 and unhealthy: {per_episode:?}");
}

struct Echo(Mutex<Vec<ResponsesRequest>>);

impl ModelClient for Echo {
    fn stream(&self, req: &ResponsesRequest, _cancel: &CancelToken, _sink: &mut dyn FnMut(StreamEvent)) -> Result<TurnOutput, ClientError> {
        self.0.lock().unwrap().push(req.clone());
        Ok(TurnOutput { text: "ok".into(), reasoning: String::new(), calls: Vec::new(), usage: Usage { input_tokens: 900, cached_tokens: 800, ..Usage::default() } })
    }
}

fn req(model: &str, session: &str, text: &str) -> ResponsesRequest {
    ResponsesRequest {
        model: model.into(),
        effort: None,
        input: vec![InputItem::Message { role: "user".into(), content: vec![ContentPart::InputText(text.into())] }],
        conversation_id: session.into(),
        tools: Vec::new(),
        hosted_search: true,
        call_timeout: None,
    }
}

fn write_table(dir: &std::path::Path, t: &RoutingTable, bump_secs: u64) {
    let path = table_path(dir);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, serde_json::to_string_pretty(t).unwrap()).unwrap();
    // A new mtime so the live snapshot re-reads it (file times can be coarse).
    let f = std::fs::File::options().write(true).open(&path).unwrap();
    f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(bump_secs)).unwrap();
}

#[test]
fn the_live_path_keeps_an_episode_on_its_model_and_pauses_with_no_route() {
    let dir = crate::harness::test_dir("route-r2a-live");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    let (reg, profiles) = fleet();
    save_registry(&dir, &reg).unwrap();
    for p in profiles.values() {
        write_profile(&dir, &mut p.clone()).unwrap();
    }
    write_table(&dir, &top_first("grok-4.5"), 10);
    let echo = Echo(Mutex::new(Vec::new()));
    let send = |model: &str, text: &str| {
        live::stream_routed(&echo, &req(model, "r2a-session", text), &CancelToken::new(), &mut |_| {}, "chat:default").map(|_| echo.0.lock().unwrap().last().unwrap().model.clone())
    };
    // Auto: the table's top model, for every step of the episode.
    assert_eq!(send("grok-4.7", "plan the trip").unwrap(), "grok-4.5");
    write_table(&dir, &top_first("grok-4.3"), 20);
    assert_eq!(send("grok-4.7", "plan the trip").unwrap(), "grok-4.5", "a table change waits for the next episode");
    assert_eq!(send("grok-4.7", "now book it").unwrap(), "grok-4.3", "a new user message is a new episode");
    // A pin is kept while it answers.
    live::set_pin("grok-4.6");
    assert_eq!(send("grok-4.6", "and again").unwrap(), "grok-4.6");
    live::set_pin("");
    // Nothing healthy: the call pauses, nothing is sent, and the cabin hears why.
    let mut dead = reg.clone();
    for id in IDS {
        dead.fold(&burst(id, CallStatus::Error, 0, 3, NOW));
    }
    save_registry(&dir, &dead).unwrap();
    let f = std::fs::File::options().write(true).open(grokhub_core::model_registry::store::registry_path(&dir)).unwrap();
    f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(30)).unwrap();
    let before = echo.0.lock().unwrap().len();
    let _ = live::take_no_route();
    assert_eq!(send("grok-4.7", "one more").unwrap_err(), ClientError::Protocol(live::NO_ROUTE_MSG.into()));
    assert_eq!(echo.0.lock().unwrap().len(), before);
    let (class, model, why) = live::take_no_route().unwrap();
    assert_eq!((class.as_str(), model.as_str()), ("chat:default", "grok-4.7"));
    assert_eq!(why, "It failed 3 times in a row (server errors or timeouts), so it rests for a while.");
    let _ = std::fs::remove_dir_all(dir);
}

// ------------------------------------------------------------- the table

use grokhub_core::model_registry::probe::{ProbeCall, ProbeReply, ProbeTransport};

use super::table::{eval_texts, load_table, rebuild, run_evals, write_table as save_table, EvalEnv, TableFile, EVAL_SET, EVAL_TOKEN_CAP_PER_BUILD};
use crate::harness::{undo_change, UndoAsk};

/// Saves the fleet into a fresh config dir.
fn seeded(name: &str) -> (std::path::PathBuf, Registry, BTreeMap<String, ModelProfile>) {
    let dir = crate::harness::test_dir(name);
    let (reg, profiles) = fleet();
    save_registry(&dir, &reg).unwrap();
    for p in profiles.values() {
        write_profile(&dir, &mut p.clone()).unwrap();
    }
    (dir, reg, profiles)
}

#[test]
fn the_table_is_deterministic_and_rebuilds_only_when_its_inputs_change() {
    let (reg, profiles) = fleet();
    let inp = TableInputs { reg: &reg, profiles: &profiles };
    let a = serde_json::to_string_pretty(&build_table(&inp, &EvalCache::default(), NOW)).unwrap();
    let b = serde_json::to_string_pretty(&build_table(&inp, &EvalCache::default(), NOW)).unwrap();
    assert_eq!(a, b, "byte-identical for the same inputs");

    let (dir, _, profiles) = seeded("route-r2a-rebuild");
    let first = rebuild(&dir, None, false, NOW);
    assert!(first.written);
    assert_eq!(first.version, 1);
    let bytes = std::fs::read(table_path(&dir)).unwrap();
    // Nothing changed: nothing is written, the file is untouched.
    let again = rebuild(&dir, None, false, NOW + 1);
    assert!(!again.written);
    assert_eq!(again.version, 1);
    assert_eq!(std::fs::read(table_path(&dir)).unwrap(), bytes);
    // A profile changes (grok-4.7's check failed): its hash moves and the table drops it.
    let mut broken = profiles["grok-4.7"].clone();
    broken.probe.failure = Some("Its check timed out.".into());
    let mut broken = ModelProfile::build(&meta("grok-4.7", true), broken.probe, 2);
    write_profile(&dir, &mut broken).unwrap();
    let moved = rebuild(&dir, None, false, NOW + 2);
    assert!(moved.written);
    assert_eq!(moved.version, 2);
    let t = load_table(&dir);
    assert!(t.classes.values().all(|c| c.ranked.iter().all(|r| r.model != "grok-4.7")));
    assert_eq!(t.rows("chat:default").len(), 3);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_table_never_ranks_unusable_out_of_plan_or_metered_models() {
    let (mut reg, mut profiles) = fleet();
    downgrade(&mut reg, 2); // grok-4.6 leaves the plan
    let gb = Listing::new(SourceKind::GrokBuild, vec![meta("grok-4.8", true)]);
    let api = listing(IDS.iter().filter(|id| **id != PIN).map(|id| meta(id, true)).collect(), "SuperGrok");
    reg.apply_refresh(&[api, gb], &[], 3);
    profiles.insert("grok-4.8".into(), ModelProfile::build(&meta("grok-4.8", true), ok_probe(), 3));
    let mut bad = ok_probe();
    bad.failure = Some("It never answered.".into());
    profiles.insert("grok-4.3".into(), ModelProfile::build(&meta("grok-4.3", true), bad, 3));
    let t = build_table(&TableInputs { reg: &reg, profiles: &profiles }, &EvalCache::default(), NOW);
    let ranked: std::collections::BTreeSet<&str> = t.classes.values().flat_map(|c| c.ranked.iter().map(|r| r.model.as_str())).collect();
    assert_eq!(ranked.into_iter().collect::<Vec<_>>(), vec!["grok-4.5", "grok-4.7"]);
}

#[test]
fn prepare_hard_rows_never_start_below_high() {
    let (mut reg, mut profiles) = fleet();
    // grok-4.5 only offers low and medium.
    let mut low = meta("grok-4.5", true);
    low.efforts = Some(vec!["low".into(), "medium".into()]);
    reg.apply_refresh(&[listing(IDS.iter().map(|id| if *id == "grok-4.5" { low.clone() } else { meta(id, true) }).collect(), "SuperGrok Heavy")], &[], 2);
    profiles.insert("grok-4.5".into(), ModelProfile::build(&low, ok_probe(), 2));
    let t = build_table(&TableInputs { reg: &reg, profiles: &profiles }, &EvalCache::default(), NOW);
    let rows = t.rows(super::ladder::PREPARE_HARD);
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|r| r.model != "grok-4.5"));
    for r in rows {
        assert!(rung(r.effort.as_deref().unwrap()).unwrap() >= rung("high").unwrap(), "{r:?}");
    }
    assert!(t.rows("chat:default").iter().any(|r| r.model == "grok-4.5"), "it still ranks where low or medium is enough");
}

/// Answers every eval right, and keeps what it was sent.
struct Grader(Vec<ProbeCall>, u64);

impl ProbeTransport for Grader {
    fn send(&mut self, call: &ProbeCall) -> Result<ProbeReply, String> {
        self.0.push(call.clone());
        let prompt = call.body["input"][0]["content"].as_str().unwrap_or_default();
        let item = EVAL_SET.iter().find(|i| i.prompt == prompt).unwrap();
        let mut reply = ProbeReply { status: 200, latency_ms: 800, input_tokens: 60, output_tokens: 10, reasoning_tokens: self.1, ..ProbeReply::default() };
        match item.check {
            super::table::Check::Exact(t) | super::table::Check::Json(t) => reply.text = t.into(),
            super::table::Check::Regex(_) => reply.text = String::new(),
            super::table::Check::Tool(n, a) => reply.tool_calls = vec![(n.into(), a.into())],
        }
        Ok(reply)
    }
}

#[test]
fn evals_send_only_the_fixed_set_and_stop_at_the_token_cap() {
    let dir = crate::harness::test_dir("route-r2a-evals");
    // User data the evals must never carry.
    std::fs::write(dir.join("memory.md"), "Viper's bank PIN is 4242").unwrap();
    let (reg, profiles) = fleet();
    let inp = TableInputs { reg: &reg, profiles: &profiles };
    let mut cache = EvalCache::default();
    // Each call reasons hard, so the cap is reached partway.
    let mut grader = Grader(Vec::new(), 2_000);
    let (mut allow, mut spans) = (|| true, 0u32);
    let mut on_call = |_: &ProbeCall, _: &ProbeReply| spans += 1;
    let run = run_evals(&inp, &mut cache, EvalEnv { transport: &mut grader, guard: &mut allow, on_call: &mut on_call });
    assert!(run.capped, "{run:?}");
    assert!(run.tokens <= EVAL_TOKEN_CAP_PER_BUILD, "{run:?}");
    assert_eq!(run.calls as usize, grader.0.len());
    assert_eq!(spans, run.calls, "each call is one background:eval span");
    let texts = eval_texts();
    for c in &grader.0 {
        let body = c.body.to_string();
        assert!(!body.contains("4242") && !body.contains("Viper"));
        assert!(texts.contains(&c.body["input"][0]["content"].as_str().unwrap()));
        assert_eq!(c.body["store"], serde_json::json!(false));
    }
    // What ran is scored; what the cap left out ranks with quality null.
    let t = build_table(&inp, &cache, NOW);
    let all: Vec<&TableRow> = t.classes.values().flat_map(|c| c.ranked.iter()).collect();
    assert!(all.iter().any(|r| r.quality == Some(1.0)), "the fake answers everything right");
    assert!(all.iter().any(|r| r.quality.is_none()), "the cap left pairs unscored");
    // One eval build a day.
    assert!(cache.take_build(NOW));
    assert!(!cache.take_build(NOW + 1));
    assert!(cache.take_build(NOW + grokhub_core::model_registry::probe::DAY_MS));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn undo_puts_the_previous_table_back_byte_identical() {
    let (dir, ..) = seeded("route-r2a-undo");
    assert!(rebuild(&dir, None, false, NOW).written);
    let v1 = std::fs::read(table_path(&dir)).unwrap();
    let mut t2 = top_first("grok-4.3");
    assert!(save_table(&dir, &mut t2, "test").unwrap());
    assert_eq!(t2.version, 2);
    assert!(models_file(&dir, "routing_table.v1.json").exists());
    assert_ne!(std::fs::read(table_path(&dir)).unwrap(), v1);
    // The ranking didn't change: nothing written.
    assert!(!save_table(&dir, &mut top_first("grok-4.3"), "again").unwrap());
    undo_change(&dir, &TableFile { path: &table_path(&dir) }, UndoAsk::from_click()).unwrap();
    assert_eq!(std::fs::read(table_path(&dir)).unwrap(), v1);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn only_five_old_tables_are_kept() {
    let (dir, ..) = seeded("route-r2a-keep");
    for (i, top) in IDS.iter().cycle().take(9).enumerate() {
        let mut t = top_first(top);
        t.built_at = i as u64;
        save_table(&dir, &mut t, "test").unwrap();
    }
    let olds: Vec<String> = std::fs::read_dir(grokhub_core::model_registry::store::models_dir(&dir))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("routing_table.v"))
        .collect();
    assert_eq!(olds.len(), 5, "{olds:?}");
    assert!(olds.contains(&"routing_table.v8.json".to_string()) && !olds.contains(&"routing_table.v3.json".to_string()));
    let _ = std::fs::remove_dir_all(dir);
}

fn models_file(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    grokhub_core::model_registry::store::models_dir(dir).join(name)
}
