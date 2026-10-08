//! Router R2b acceptance: no premium or extra-spend route without your grant,
//! Grok 4.7 Fast only while you wait (never background, automation or
//! proactive, never with the toggle off, back to Grok 4.7 when the budget is
//! tight), revoking a grant stops the next premium route, and the week's
//! budget pauses background and asks before user-facing work at 100%.

use std::collections::BTreeMap;

use grokhub_core::model_registry::cost_class::{route_key, CostClass};
use grokhub_core::model_registry::profile::ModelProfile;
use grokhub_core::model_registry::store::save_registry;
use grokhub_core::model_registry::{Credential, Listing, ModelMeta, Prices, Registry, SourceKind};

use super::budget::{self, BudgetNotes, BUDGET_PAUSE_MSG};
use super::live::{self, RouteCall, RouteDone};
use super::spend::{self, fast_verdict, Latency, Spend, SpendSettings};
use super::table::{table_path, ClassTable, RoutingTable, SpeedQuality, TableRow};
use super::{Router, RouteInput};
use crate::client::Usage;
use crate::harness::{grant_premium, revoke_grant, Origin, OriginScope, UserClick};

const IDS: [&str; 4] = ["grok-4.7", "grok-4.6", "grok-4.5", "grok-4.3"];
const FAST: &str = "grok-4.7-fast";
const NOW: u64 = 1_000_000;

/// grok-4.x at $5/M out per minor version step (grok-4.3 $15/M … grok-4.7
/// $35/M), plus grok-heavy at $60/M, and Grok 4.7 Fast in Grok Build's list.
fn meta(id: &str) -> ModelMeta {
    let v: u64 = id.rsplit('.').next().and_then(|n| n.parse().ok()).unwrap_or(12);
    ModelMeta {
        id: id.into(),
        context_length: Some(256_000),
        prices: Prices { prompt: Some(10_000 * v), cached: Some(2_500 * v), completion: Some(50_000 * v), ..Prices::default() },
        efforts: Some(vec!["low".into(), "medium".into(), "high".into(), "xhigh".into()]),
        tool_calling: Some(true),
        api_shape: Some("responses".into()),
        ..ModelMeta::default()
    }
}

fn fleet(cred: Credential) -> Registry {
    let mut reg = Registry::default();
    reg.entitlement.credential = cred;
    let mut api: Vec<ModelMeta> = IDS.iter().map(|id| meta(id)).collect();
    api.push(meta("grok-heavy"));
    let gb = Listing::new(SourceKind::GrokBuild, IDS.iter().chain([FAST].iter()).map(|id| ModelMeta::bare(id)).collect());
    reg.apply_refresh(&[Listing::new(SourceKind::XaiApi, api), gb], &[], 1);
    reg
}

fn no_profiles() -> BTreeMap<String, ModelProfile> {
    BTreeMap::new()
}

/// A table that ranks grok-4.7 first for chat and for background summaries.
fn table() -> RoutingTable {
    let row = |m: &str| TableRow { model: m.into(), effort: Some("medium".into()), quality: Some(1.0), est_cost_usd: None, p50_latency_ms: None, why: "the best balance in your plan".into() };
    let ct = |sq| ClassTable { speed_quality: sq, ranked: vec![row("grok-4.7"), row("grok-4.3")] };
    let mut t = RoutingTable { schema: 1, version: 1, ..RoutingTable::default() };
    t.classes.insert("chat:default".into(), ct(SpeedQuality::Balanced));
    t.classes.insert("background:summarize".into(), ct(SpeedQuality::Speed));
    t
}

fn waiting() -> Spend {
    Spend { latency: Latency { waiting_live: true, ..Latency::default() }, ..Spend::default() }
}

fn ask<'a>(model: &'a str, spend: Spend) -> RouteInput<'a> {
    RouteInput { provider: "xai", class: "chat:default", current_model: model, d: 0.5, needs_tools: true, spend, ..RouteInput::default() }
}

#[test]
fn fast_goes_to_a_live_waiting_turn_and_falls_back_to_grok_4_7_when_the_budget_is_tight() {
    let reg = fleet(Credential::Plan);
    let r = Router::choose(&ask("grok-4.7", waiting()), &reg, &no_profiles(), &table(), NOW);
    assert_eq!((r.model.as_str(), r.cost_class, r.base.as_deref()), (FAST, CostClass::AutonomousPremium, Some("grok-4.7")));
    assert!(r.rule_ids.contains(&"fast:waiting".to_string()), "{:?}", r.rule_ids);
    assert_eq!(r.reason, "Using grok-4.7-fast at Medium: you're waiting, so the faster model is worth it.");
    // Budget tight: Grok 4.7, and the record says why.
    let tight = Spend { budget_tight: true, ..waiting() };
    let r = Router::choose(&ask("grok-4.7", tight), &reg, &no_profiles(), &table(), NOW);
    assert_eq!((r.model.as_str(), r.cost_class, r.base.clone()), ("grok-4.7", CostClass::Included, None));
    assert!(r.rule_ids.contains(&"fast:budget_tight".to_string()), "{:?}", r.rule_ids);
    assert_eq!(r.reason, "Using grok-4.7 at Medium: this week's budget is tight, so not grok-4.7-fast.");
    // The toggle off: never.
    let off = Spend { settings: SpendSettings { fast_when_waiting: false, ..SpendSettings::default() }, ..waiting() };
    let r = Router::choose(&ask("grok-4.7", off), &reg, &no_profiles(), &table(), NOW);
    assert_eq!((r.model.as_str(), r.rule_ids.contains(&"fast:off".to_string())), ("grok-4.7", true));
    // Nobody waiting: plain Grok 4.7.
    let r = Router::choose(&ask("grok-4.7", Spend::default()), &reg, &no_profiles(), &table(), NOW);
    assert_eq!(r.model, "grok-4.7");
    // A pin is kept as picked.
    let mut pin = ask("grok-4.7", waiting());
    pin.pinned = true;
    assert_eq!(Router::choose(&pin, &reg, &no_profiles(), &table(), NOW).model, "grok-4.7");
    // Not entitled (GB's free tier drops it): never a candidate.
    let mut free = reg.clone();
    free.models.get_mut(FAST).unwrap().state = grokhub_core::model_registry::ModelState::NotInPlan;
    assert_eq!(Router::choose(&ask("grok-4.7", waiting()), &free, &no_profiles(), &table(), NOW).model, "grok-4.7");
    // Grok 4.7 Fast is never in the pool on its own: a pin to it stands in.
    let mut on_fast = ask(FAST, Spend::default());
    on_fast.pinned = true;
    let r = Router::choose(&on_fast, &reg, &no_profiles(), &table(), NOW);
    assert_eq!((r.model.as_str(), r.rule_ids[0].as_str(), r.no_route), ("grok-4.7", "pin:base", false));
}

/// The live path, with the latency signals built from the call's origin and
/// class the way the cabin sends them.
#[test]
fn fast_is_never_chosen_for_background_automation_or_proactive_and_the_toggle_turns_it_off() {
    let dir = crate::harness::test_dir("r2b-fast-live");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet(Credential::Plan)).unwrap();
    std::fs::create_dir_all(table_path(&dir).parent().unwrap()).unwrap();
    std::fs::write(table_path(&dir), serde_json::to_string(&table()).unwrap()).unwrap();
    spend::set_spend(SpendSettings::default());
    let call = |class: &'static str, ep: &'static str| RouteCall { provider: "xai", class, model: "grok-4.7", episode: ep, session: "s", text: "rename the helper", needs_tools: true, ..RouteCall::default() };
    let picked = |c: &RouteCall<'_>| live::decide(&dir, c, NOW).route;
    let r = picked(&call("chat:default", "ep-user"));
    assert_eq!((r.model.as_str(), r.cost_class), (FAST, CostClass::AutonomousPremium), "{:?}", r.rule_ids);
    let r = picked(&call("background:summarize", "ep-bg"));
    assert_eq!(r.model, "grok-4.7");
    assert!(r.rule_ids.contains(&"fast:background".to_string()), "{:?}", r.rule_ids);
    for origin in [Origin::Automation, Origin::Proactive] {
        let _o = OriginScope::enter(origin);
        let r = picked(&call("chat:default", "ep-auto"));
        assert_eq!(r.model, "grok-4.7", "{origin:?}: {:?}", r.rule_ids);
    }
    spend::set_spend(SpendSettings { fast_when_waiting: false, ..SpendSettings::default() });
    assert_eq!(picked(&call("chat:default", "ep-off")).model, "grok-4.7");
    spend::set_spend(SpendSettings::default());
    // A long chain in one turn is latency-bound; a time box close to its end is at risk.
    let mut chain = call("chat:default", "ep-chain");
    for _ in 0..spend::CHAIN_LATENCY_STEPS {
        chain.step += 1;
        let r = picked(&chain);
        assert_eq!(r.model, FAST);
        if r.rule_ids.contains(&"fast:chain".to_string()) {
            break;
        }
    }
    assert!(picked(&chain).rule_ids.contains(&"fast:chain".to_string()));
    let boxed = RouteCall { deadline_ms: Some(NOW + 60_000), ..call("chat:default", "ep-box") };
    assert!(picked(&boxed).rule_ids.contains(&"fast:time_box".to_string()));
    let _ = std::fs::remove_dir_all(dir);
}

/// A tiny xorshift so the property run is the same on every machine.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[(self.next() % xs.len() as u64) as usize]
    }

    fn coin(&mut self) -> bool {
        self.next().is_multiple_of(2)
    }
}

#[test]
fn no_premium_or_extra_spend_route_without_a_grant_over_random_signals() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let models = ["grok-4.7", "grok-4.6", "grok-4.5", "grok-4.3", "grok-heavy", FAST, "gone"];
    let classes = ["chat:default", "plan", "code:multi-file", "background:summarize", "background:judge", "prepare:hard", "eval:item"];
    let creds = [Credential::Plan, Credential::ApiKey, Credential::None];
    let mut premium_seen = 0;
    for _ in 0..3_000 {
        let cred = *rng.pick(&creds);
        let reg = fleet(cred);
        let current = *rng.pick(&models);
        let grants: Vec<String> = models.iter().filter(|_| rng.next().is_multiple_of(5)).map(|m| route_key(m, Default::default())).collect();
        let spend = Spend {
            settings: SpendSettings { fast_when_waiting: rng.coin(), weekly_cap_usd: 0.0, ceiling_usd_per_m: *rng.pick(&[5.0, 15.0, 25.0, 40.0, 100.0]) },
            latency: Latency { waiting_live: rng.coin(), chain_latency_bound: rng.coin(), time_box_at_risk: rng.coin() },
            background: rng.coin(),
            budget_tight: rng.coin(),
            grants: grants.clone(),
            providers: Vec::new(),
        };
        let provider = if rng.next().is_multiple_of(4) { live::PROVIDER_GROK_BUILD } else { "xai" };
        let input = RouteInput {
            provider,
            class: rng.pick(&classes),
            current_model: current,
            pinned: rng.coin(),
            d: (rng.next() % 100) as f64 / 100.0,
            needs_tools: rng.coin(),
            episode_model: rng.coin().then(|| *rng.pick(&models)),
            spend: spend.clone(),
            ..RouteInput::default()
        };
        let tbl = if rng.coin() { table() } else { RoutingTable::default() };
        let r = Router::choose(&input, &reg, &no_profiles(), &tbl, NOW);
        let why = format!("{input:?} → {} {:?} {:?}", r.model, r.cost_class, r.rule_ids);
        assert!(!matches!(r.cost_class, CostClass::ExtraSpend | CostClass::NewProvider), "{why}");
        if r.no_route {
            continue;
        }
        match r.cost_class {
            CostClass::Premium => {
                premium_seen += 1;
                assert!(grants.contains(&route_key(&r.model, Default::default())), "{why}");
            }
            CostClass::AutonomousPremium => {
                assert!(!input.pinned && fast_verdict(&spend).is_ok() && r.base.is_some(), "{why}");
            }
            CostClass::Unknown => {
                // Only the call's own model, kept because nothing native is included.
                assert!(r.model == current && r.rule_ids.iter().any(|x| x == "cost:not_included" || x.starts_with("pin") || x.starts_with("model:") || x.starts_with("episode")), "{why}");
                assert!(cred == Credential::None || current == "gone" || provider == live::PROVIDER_GROK_BUILD || r.rule_ids.iter().any(|x| x.ends_with("degraded_kept")), "{why}");
            }
            _ => {}
        }
    }
    assert!(premium_seen > 0, "the run never reached a granted premium route");
}

#[test]
fn a_premium_route_waits_for_your_grant_and_revoking_it_stops_the_next_one() {
    let dir = crate::harness::test_dir("r2b-premium");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet(Credential::ApiKey)).unwrap();
    spend::set_spend(SpendSettings::default());
    let _ = live::take_premium_ask();
    let call = RouteCall { provider: "xai", class: "chat:default", model: "grok-heavy", episode: "ep-p", session: "s", text: "hi", pinned: true, needs_tools: true, ..RouteCall::default() };
    let first = live::decide(&dir, &call, NOW).route;
    // Auto stands in with an included model while it asks; nothing pauses.
    assert_eq!((first.model.as_str(), first.no_route, first.cost_class), ("grok-4.3", false, CostClass::Included), "{:?}", first.rule_ids);
    assert!(first.rule_ids.contains(&"cost:premium_ungranted".to_string()), "{:?}", first.rule_ids);
    assert_eq!(live::take_premium_ask().as_deref(), Some("premium:grok-heavy"));
    let g = grant_premium(&dir, "premium:grok-heavy", UserClick::from_click()).unwrap();
    let granted = live::decide(&dir, &call, NOW).route;
    assert_eq!((granted.model.as_str(), granted.cost_class), ("grok-heavy", CostClass::Premium));
    assert_eq!(live::take_premium_ask(), None);
    revoke_grant(&dir, &g.id).unwrap();
    let after = live::decide(&dir, &call, NOW).route;
    assert_eq!(after.model, "grok-4.3", "{:?}", after.rule_ids);
    assert_eq!(live::take_premium_ask().as_deref(), Some("premium:grok-heavy"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn at_100_percent_background_pauses_and_chat_asks_until_you_say_go_on() {
    let dir = crate::harness::test_dir("r2b-budget");
    let _cfg = crate::perm::ConfigGuard::set(&dir);
    save_registry(&dir, &fleet(Credential::ApiKey)).unwrap();
    spend::set_spend(SpendSettings { weekly_cap_usd: 1.0, ..SpendSettings::default() });
    budget::forget_week();
    let now = grokhub_core::now_ms();
    let call = |class: &'static str| RouteCall { provider: "xai", class, model: "grok-4.3", episode: "ep-b", session: "s", text: "hi", ..RouteCall::default() };
    // One chat call that cost $0.85: 85%, tight but not used up.
    let spent = |usd: f64| RouteDone { ok: true, http: 200, usage: Usage { cost_in_usd_ticks: (usd * 1e10) as i64, ..Usage::default() }, ..RouteDone::default() };
    let d = live::decide(&dir, &call("chat:default"), now);
    live::route_log(&dir, &call("chat:default"), &d, &spent(0.85));
    budget::forget_week();
    let d = live::decide(&dir, &call("chat:default"), now);
    assert_eq!((d.budget_pct, d.paused()), (Some(85), false));
    let rec = live::route_log(&dir, &call("chat:default"), &d, &spent(0.15));
    assert_eq!((rec.signals.budget_pct, rec.cost_class.as_str()), (Some(85), "included"));
    budget::forget_week();
    let _ = live::take_budget_ask();
    let bg = live::decide(&dir, &call("background:summarize"), now);
    assert_eq!((bg.budget_pct, bg.paused(), bg.pause_msg()), (Some(100), true, BUDGET_PAUSE_MSG));
    let chat = live::decide(&dir, &call("chat:default"), now);
    assert_eq!((chat.paused(), chat.pause_msg()), (true, BUDGET_PAUSE_MSG));
    assert!(live::take_budget_ask());
    // You said to keep going this week: chat goes, background still waits.
    BudgetNotes { go_on_week: budget::week_of(now), ..BudgetNotes::default() }.save(&dir);
    assert!(!live::decide(&dir, &call("chat:default"), now).paused());
    assert!(live::decide(&dir, &call("background:summarize"), now).paused());
    spend::set_spend(SpendSettings::default());
    budget::forget_week();
    let _ = std::fs::remove_dir_all(dir);
}
