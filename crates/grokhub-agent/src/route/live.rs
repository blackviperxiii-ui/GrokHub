//! Router live routing: every model call site asks [`decide`] before it
//! sends, sends the model and effort the router picked (for a class the table
//! lists), then calls [`route_log`], which writes the `route` record on a
//! `model-calls` span and the call's status for passive health
//! (`models/health.jsonl`).
//!
//! R2a: one model per episode. The model is picked at an episode boundary (a
//! new user message), on a second VerifyGate reject at the ceiling (E2), or
//! when the episode's model goes unhealthy; otherwise every step of the
//! episode keeps it, so the prompt cache stays warm. A pin is kept while it
//! answers and stood in for (same family first) when it doesn't. With no
//! healthy, included model the call pauses instead of sending.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use grokhub_core::model_registry::profile::{read_profiles, ModelProfile};
use grokhub_core::model_registry::store::{append_observation, load_registry, profiles_dir, registry_path};
use grokhub_core::model_registry::{classify_status, CallStatus, Observation, Registry};

use super::cabin::{MODEL_TOOL, MODEL_TRACE};
use grokhub_core::model_registry::EFFORT_LADDER;
use grokhub_core::outcome::is_correction;

use super::difficulty::{difficulty, DifficultyInput};
use super::guard::{holdout_eligible, in_holdout, load_overrides, HOLDOUT_EFFORT};
use super::ladder::{self, rung, start_rung, take_tool_errors, turn_hash, user_facing, Band, Obs, Pick, Steer};
use super::log::{Chosen, RouteOutcome, RouteRecord, RouteSignals, RouteTokens};
use super::budget::{self, Budget, BudgetNotes};
use super::policy::{class_row, expected_cost_usd, MODEL_LIVE, POLICY_LIVE};
use super::spend::{self, Latency, Spend};
use super::table::{load_table, table_path, RoutingTable};
use super::signals::{SpanVerifySource, VerifySignal, VerifySource};
use super::{Route, RouteInput, Router};
use crate::client::{ClientError, ContentPart, InputItem, ModelClient, ResponsesRequest, StreamEvent, TurnOutput, Usage};
use crate::CancelToken;
use crate::harness::{append_span, current_origin, AccessMode, ModelUsage, Origin, Span};

/// xAI cost ticks per USD (1 tick = 1e-10 USD).
pub const TICKS_PER_USD: f64 = 1e10;

/// One call, as the call site knows it. `text` feeds difficulty only and is never stored.
#[derive(Debug, Clone, Copy, Default)]
pub struct RouteCall<'a> {
    pub provider: &'a str,
    pub class: &'a str,
    pub model: &'a str,
    pub effort: Option<&'a str>,
    pub episode: &'a str,
    pub step: u32,
    /// The chat's span session, for VerifyGate lookups.
    pub session: &'a str,
    pub ctx_tokens: u64,
    pub text: &'a str,
    pub pinned: bool,
    pub failures: u32,
    pub planned_tools: u32,
    pub plan_mode: bool,
    pub needs_image: bool,
    /// The step offers tools.
    pub needs_tools: bool,
    /// Who started the call. `None` reads this thread's `OriginScope`.
    pub origin: Option<Origin>,
    /// A time-boxed task's deadline (ms), for the Fast policy.
    pub deadline_ms: Option<u64>,
}

/// How the call went. Counts and ids only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteDone {
    pub ok: bool,
    /// HTTP status (200 on success, 0 when there was none) and the error text for classifying it.
    pub http: u16,
    pub error: String,
    pub usage: Usage,
    pub latency_ms: u64,
    pub served_model: Option<String>,
    /// The call was cancelled, or its result is not ours to see (Grok Build
    /// runs the turn): no outcome and nothing about the model is learned.
    pub cancelled: bool,
}

fn http_of(err: &ClientError) -> u16 {
    match err {
        ClientError::KeyOffer { status, .. } | ClientError::Auth { status, .. } | ClientError::Server { status, .. } => *status,
        ClientError::RateLimited { .. } => 429,
        ClientError::Protocol(msg) => msg
            .strip_prefix("HTTP ")
            .and_then(|r| r.split(':').next())
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or(0),
        _ => 0,
    }
}

impl RouteDone {
    /// A call whose result the cabin never sees (a Grok Build turn).
    pub fn unseen() -> Self {
        Self { cancelled: true, ..Self::default() }
    }

    pub fn of(out: &Result<TurnOutput, ClientError>, elapsed: Duration) -> Self {
        let latency_ms = elapsed.as_millis() as u64;
        let served_model = crate::sse::take_served_model();
        match out {
            Ok(turn) => Self { ok: true, http: 200, usage: turn.usage.clone(), latency_ms, served_model, ..Self::default() },
            Err(e) => Self {
                http: http_of(e),
                error: e.to_string(),
                latency_ms,
                cancelled: matches!(e, ClientError::Cancelled),
                ..Self::default()
            },
        }
    }

    /// A cabin call that reported its usage (or failed with `error`).
    pub fn cabin(ok: bool, usage: Option<&ModelUsage>, error: &str, elapsed: Duration, served_model: Option<String>) -> Self {
        let u = usage.copied().unwrap_or_default();
        let http = if ok {
            200
        } else {
            error
                .split_whitespace()
                .find_map(|w| w.trim_matches(|c: char| !c.is_ascii_digit()).parse::<u16>().ok().filter(|n| (400..600).contains(n)))
                .unwrap_or(0)
        };
        Self {
            ok,
            http,
            error: error.to_string(),
            usage: Usage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                reasoning_tokens: u.reasoning_tokens,
                cost_in_usd_ticks: u.cost_in_usd_ticks,
                cached_tokens: u.cached_tokens,
            },
            latency_ms: elapsed.as_millis() as u64,
            served_model,
            cancelled: false,
        }
    }
}

type Stamp = (PathBuf, Option<SystemTime>);

struct Cache {
    reg: Option<(Stamp, Arc<Registry>)>,
    profiles: Option<(Stamp, Arc<BTreeMap<String, ModelProfile>>)>,
    table: Option<(Stamp, Arc<RoutingTable>)>,
    prev: BTreeMap<String, Chosen>,
    /// The model each episode (and class) runs on, with the turn it was picked for.
    episodes: BTreeMap<String, (u64, String)>,
    /// Routed calls so far in each episode's current turn (a latency-bound chain).
    steps: BTreeMap<String, (u64, u32)>,
}

static CACHE: Mutex<Cache> = Mutex::new(Cache { reg: None, profiles: None, table: None, prev: BTreeMap::new(), episodes: BTreeMap::new(), steps: BTreeMap::new() });

/// Episodes remembered at once; a long-running cabin stays small.
const EPISODES_CAP: usize = 256;

/// The user's pinned chat model (`cfg.model`), set by the cabin. Empty is Auto.
static PIN: Mutex<String> = Mutex::new(String::new());

/// The cabin sets the pin on start and whenever the model setting changes.
pub fn set_pin(model: &str) {
    *PIN.lock().unwrap_or_else(|e| e.into_inner()) = model.trim().to_string();
}

fn pinned_model() -> String {
    PIN.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The text a call that paused returns: no healthy, included model could take it.
pub const NO_ROUTE_MSG: &str = "No model in your plan is answering right now, so GrokHub paused this step. Home has what failed and your options.";

/// A pause the cabin hasn't shown yet: (class, model, what failed).
static NO_ROUTE: Mutex<Option<(String, String, String)>> = Mutex::new(None);

/// Take the newest pause, for the cabin's needs-attention line and card.
pub fn take_no_route() -> Option<(String, String, String)> {
    NO_ROUTE.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// R2b: a premium route a call wanted, not yet asked about (its route key).
static PREMIUM_ASK: Mutex<Option<String>> = Mutex::new(None);
/// R2b: user-facing work paused at 100% of the week's budget.
static BUDGET_ASK: Mutex<bool> = Mutex::new(false);

/// Take the newest premium route a call wanted, for the cabin's hard money card.
pub fn take_premium_ask() -> Option<String> {
    PREMIUM_ASK.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// True once after a user-facing call paused at 100%: the cabin shows the ask.
pub fn take_budget_ask() -> bool {
    std::mem::take(&mut *BUDGET_ASK.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Count this call in its episode's turn; returns the steps so far.
fn note_step(key: &str, turn: u64) -> u32 {
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if c.steps.len() >= EPISODES_CAP && !c.steps.contains_key(key) {
        c.steps.clear();
    }
    let slot = c.steps.entry(key.to_string()).or_insert((turn, 0));
    if slot.0 != turn {
        *slot = (turn, 0);
    }
    slot.1 += 1;
    slot.1
}

/// This week's budget, or none in a unit test that didn't pin a config dir.
fn week_budget(config_dir: &Path, reg: &Registry, cap_usd: f64, now_ms: u64) -> Budget {
    if cfg!(test) && !crate::perm::config_pinned() {
        return Budget::default();
    }
    Budget::of(&budget::load_week(config_dir, reg, now_ms), reg.entitlement.credential, cap_usd)
}

fn mtime(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// The saved registry and profiles, re-read only when their files change.
pub fn snapshot(config_dir: &Path) -> (Arc<Registry>, Arc<BTreeMap<String, ModelProfile>>) {
    let reg_stamp = (config_dir.to_path_buf(), mtime(&registry_path(config_dir)));
    let prof_stamp = (config_dir.to_path_buf(), mtime(&profiles_dir(config_dir)));
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let reg = match &c.reg {
        Some((s, r)) if *s == reg_stamp => r.clone(),
        _ => {
            let r = Arc::new(load_registry(config_dir));
            c.reg = Some((reg_stamp, r.clone()));
            r
        }
    };
    let profiles = match &c.profiles {
        Some((s, p)) if *s == prof_stamp => p.clone(),
        _ => {
            let p = Arc::new(read_profiles(config_dir));
            c.profiles = Some((prof_stamp, p.clone()));
            p
        }
    };
    (reg, profiles)
}

/// The saved routing table, re-read only when its file changes.
pub fn table_snapshot(config_dir: &Path) -> Arc<RoutingTable> {
    let stamp = (config_dir.to_path_buf(), mtime(&table_path(config_dir)));
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    match &c.table {
        Some((s, t)) if *s == stamp => t.clone(),
        _ => {
            let t = Arc::new(load_table(config_dir));
            c.table = Some((stamp, t.clone()));
            t
        }
    }
}

/// The model this episode already runs on, unless `turn` is a new user message.
fn episode_model(key: &str, turn: u64) -> Option<String> {
    let c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    c.episodes.get(key).filter(|(t, _)| *t == turn).map(|(_, m)| m.clone())
}

fn keep_episode_model(key: String, turn: u64, model: &str) {
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if c.episodes.len() >= EPISODES_CAP && !c.episodes.contains_key(&key) {
        c.episodes.clear();
    }
    c.episodes.insert(key, (turn, model.to_string()));
}

/// The request crosses the model's long-context threshold (xAI bills the
/// whole request at about twice the price there), so it should be compacted
/// first. Nothing tells yet whether a task needs its whole context, so this
/// always asks for the compact.
pub fn crosses_long_context(config_dir: &Path, model: &str, ctx_tokens: u64) -> bool {
    let (reg, profiles) = snapshot(config_dir);
    let threshold = profiles
        .get(model.trim())
        .and_then(|p| p.metadata.prices.long_context_threshold)
        .or_else(|| reg.get(model).and_then(|r| r.meta.prices.long_context_threshold));
    threshold.is_some_and(|t| t > 0 && ctx_tokens >= t)
}

/// What the router decided for one call, before it is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub route: Route,
    /// Difficulty, rounded to hundredths.
    pub d: u32,
    /// The call is in the internal 10% control arm (fixed High).
    pub holdout: bool,
    /// A third VerifyGate reject: the 1a recovery ladder has the episode now.
    pub recover: bool,
    /// The class is in the table, so the router's effort is sent.
    pub live: bool,
    /// R2b: the week's budget used, percent.
    pub budget_pct: Option<u8>,
    /// R2b: the budget paused this call (background over its share or at 100%,
    /// or user-facing work at 100% before you said to go on). The message to return.
    pub budget_pause: Option<&'static str>,
}

impl Decision {
    /// The model to send: the router's for a listed class, the call's own otherwise.
    pub fn send_model(&self, call_model: &str) -> String {
        if self.live && MODEL_LIVE {
            self.route.model.clone()
        } else {
            call_model.to_string()
        }
    }

    /// The router found no healthy, included model, or the budget stops it: pause instead of sending.
    pub fn paused(&self) -> bool {
        self.live && MODEL_LIVE && (self.route.no_route || self.budget_pause.is_some())
    }

    /// What a paused call returns.
    pub fn pause_msg(&self) -> &'static str {
        self.budget_pause.unwrap_or(NO_ROUTE_MSG)
    }

    /// The effort to send: the router's for a listed class, the call's own otherwise.
    pub fn send_effort(&self, call_effort: Option<&str>) -> Option<String> {
        if self.live {
            self.route.effort.clone()
        } else {
            call_effort.map(str::to_string)
        }
    }

    fn d(&self) -> f64 {
        self.d as f64 / 100.0
    }
}

/// The id the ladder and the holdout key on: the episode, else the chat session.
fn episode_key<'a>(call: &RouteCall<'a>) -> &'a str {
    if call.episode.is_empty() {
        call.session
    } else {
        call.episode
    }
}

/// VerifyGate rejects so far in the episode, and whether its latest check passed.
fn verify_counts(config_dir: &Path, call: &RouteCall<'_>) -> (Option<u32>, bool) {
    if call.episode.is_empty() || call.session.is_empty() {
        return (None, false);
    }
    let checks = SpanVerifySource { config_dir: config_dir.to_path_buf(), session: call.session.to_string() }.checks(call.episode);
    let rejects = checks.iter().filter(|c| **c == VerifySignal::Reject).count() as u32;
    (Some(rejects), checks.last() == Some(&VerifySignal::Ok))
}

/// Pick the provider, model and effort for one call and advance its episode's ladder.
pub fn decide(config_dir: &Path, call: &RouteCall<'_>, now_ms: u64) -> Decision {
    let (reg, profiles) = snapshot(config_dir);
    let settings = spend::spend_settings();
    let b = week_budget(config_dir, &reg, settings.weekly_cap_usd, now_ms);
    let d = difficulty(&DifficultyInput { text: call.text, planned_tools: call.planned_tools, plan_mode: call.plan_mode, ctx_tokens: call.ctx_tokens });
    let steer = Steer::from_text(call.text);
    let row = class_row(call.class);
    let bump = row.and_then(|r| load_overrides(config_dir).get(r.class).map(|o| o.steps)).unwrap_or(0);
    let key = episode_key(call);
    let holdout = holdout_eligible(call.class) && in_holdout(key);
    let pick = match row {
        Some(_) if holdout => Some(Pick { rung: rung(HOLDOUT_EFFORT).unwrap_or(0), rules: vec!["holdout".into()], ..Pick::default() }),
        Some(r) if user_facing(r.class) && !key.is_empty() => {
            let band = Band::of(r, bump);
            let (rejects, verify_ok) = verify_counts(config_dir, call);
            let obs = Obs {
                tool_errors: take_tool_errors(),
                rejects,
                verify_ok,
                correction: is_correction(call.text),
                // No self-check score yet, so E4 stays quiet. DE3 reads the week's budget.
                low_check: None,
                budget_pct: b.pct,
                ..Obs::default()
            };
            Some(ladder::step(key, r.class, turn_hash(call.text), band, start_rung(band, d, steer, true), obs))
        }
        _ => None,
    };
    let recover = pick.as_ref().is_some_and(|p| p.recover);
    let class_key = format!("{key}\u{1f}{}", row.map(|r| r.class).unwrap_or(call.class));
    let turn = turn_hash(call.text);
    let sticky = if key.is_empty() { None } else { episode_model(&class_key, turn) };
    let pin = pinned_model();
    let pinned = call.pinned || (!pin.is_empty() && pin == call.model.trim() && user_facing(call.class));
    let table = table_snapshot(config_dir);
    let origin = call.origin.unwrap_or_else(current_origin);
    let facing = row.is_some_and(|r| user_facing(r.class));
    let steps = if key.is_empty() { 1 } else { note_step(&class_key, turn) };
    let spend = Spend {
        settings,
        latency: Latency::of(origin, facing, steps, call.deadline_ms, now_ms),
        background: !facing || spend::background_origin(origin),
        budget_tight: b.tight(),
        grants: spend::premium_grants(config_dir),
    };
    let input = RouteInput {
        provider: call.provider,
        class: call.class,
        current_model: call.model,
        current_effort: call.effort,
        pinned,
        d,
        ctx_tokens: call.ctx_tokens,
        needs_image: call.needs_image,
        needs_tools: call.needs_tools,
        steer,
        bump,
        pick,
        episode_model: sticky.as_deref(),
        spend: spend.clone(),
    };
    let route = Router::choose(&input, &reg, &profiles, &table, now_ms);
    let live = POLICY_LIVE && row.is_some();
    let budget_pause = if !live || route.no_route || call.provider == PROVIDER_GROK_BUILD {
        None
    } else if spend.background {
        let est = reg.get(&route.model).and_then(|r| expected_cost_usd(&r.meta.prices, call.class, call.ctx_tokens)).unwrap_or(0.0);
        (!b.admits_background(est)).then_some(if b.used_up() { budget::BUDGET_PAUSE_MSG } else { budget::BACKGROUND_PAUSE_MSG })
    } else if b.used_up() && BudgetNotes::load(config_dir).must_ask(&b) {
        *BUDGET_ASK.lock().unwrap_or_else(|e| e.into_inner()) = true;
        Some(budget::BUDGET_PAUSE_MSG)
    } else {
        None
    };
    if live && !route.no_route && !key.is_empty() {
        // The plain model: a Fast swap is decided again on every call.
        keep_episode_model(class_key, turn, route.base.as_deref().unwrap_or(&route.model));
    }
    if let Some(ask) = &route.premium_ask {
        *PREMIUM_ASK.lock().unwrap_or_else(|e| e.into_inner()) = Some(ask.clone());
    }
    if live && route.no_route {
        let failed = reg.get(call.model).map(|r| r.reason.clone()).unwrap_or_else(|| "It isn't in your model list.".into());
        *NO_ROUTE.lock().unwrap_or_else(|e| e.into_inner()) = Some((call.class.to_string(), call.model.trim().to_string(), failed));
    }
    Decision { route, d: (d * 100.0).round() as u32, holdout, recover, live, budget_pct: b.pct, budget_pause }
}

/// The route record for one call. `call.effort` is what was actually sent.
pub fn route_record(config_dir: &Path, call: &RouteCall<'_>, decision: &Decision, done: &RouteDone) -> RouteRecord {
    let route = &decision.route;
    let verify = SpanVerifySource { config_dir: config_dir.to_path_buf(), session: call.session.to_string() }
        .verdict(call.episode, 0)
        .map(|v| v.as_str().to_string());
    let chosen = Chosen { provider: route.provider.clone(), model: route.model.clone(), effort: route.effort.clone() };
    let provider = if call.provider.is_empty() { super::PROVIDER_XAI } else { call.provider };
    let used = Chosen { provider: provider.to_string(), model: call.model.to_string(), effort: call.effort.map(str::to_string) };
    let key = episode_key(call);
    let prev = if key.is_empty() {
        None
    } else {
        let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if c.prev.len() > 256 {
            c.prev.clear();
        }
        c.prev.insert(format!("{key}\u{1f}{}", call.class), chosen.clone())
    };
    let u = &done.usage;
    RouteRecord {
        span_id: String::new(),
        episode: key.to_string(),
        step: call.step,
        class: call.class.to_string(),
        d: decision.d(),
        ctx_tokens: call.ctx_tokens,
        signals: RouteSignals {
            failures: call.failures,
            verify: verify.clone(),
            origin: Some(current_origin().as_str().to_string()),
            latency: Some(done.latency_ms),
            budget_pct: decision.budget_pct,
            privacy: None,
        },
        candidates_n: route.candidates_n,
        chosen,
        prev,
        rule_ids: route.rule_ids.clone(),
        reason: route.reason.clone(),
        pinned: call.pinned,
        outcome: (!done.cancelled).then(|| RouteOutcome {
            ok: done.ok,
            verify,
            tokens: RouteTokens { input: u.input_tokens, cached: u.cached_tokens, out: u.output_tokens, reasoning: u.reasoning_tokens },
            cost_usd: u.cost_in_usd_ticks as f64 / TICKS_PER_USD,
            limit: done.http == 429 && classify_status(done.http, &done.error) == CallStatus::FreeUsageExhausted,
        }),
        used,
        shadow: !decision.live,
        holdout: decision.holdout,
        settings: route.settings.clone(),
        cost_class: route.cost_class.as_str().to_string(),
    }
}

/// The last user-facing pick (effort and reason), for the composer's Auto chip.
static LAST_USER: Mutex<Option<(Option<String>, String)>> = Mutex::new(None);

/// Effort and plain reason of the newest user-facing routed call, if any.
pub fn last_user_pick() -> Option<(Option<String>, String)> {
    LAST_USER.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn note_user_pick(call: &RouteCall<'_>, decision: &Decision) {
    if decision.live && user_facing(call.class) {
        *LAST_USER.lock().unwrap_or_else(|e| e.into_inner()) = Some((call.effort.map(str::to_string), decision.route.reason.clone()));
    }
}

/// The class table's start effort for `class` (no difficulty, no ladder), as it goes on the wire.
/// `None` for an unlisted class.
pub fn start_effort(class: &str) -> Option<String> {
    let row = class_row(class)?;
    let e = EFFORT_LADDER[Band::of(row, 0).start];
    Some(grokhub_core::parse_reasoning_effort(e).unwrap_or(e).to_string())
}

/// The passive-health line for one call, or `None` when it says nothing about the model.
pub fn observation(call: &RouteCall<'_>, done: &RouteDone, now_ms: u64) -> Option<Observation> {
    if done.cancelled || call.model.trim().is_empty() || call.provider == "grok_build" {
        return None;
    }
    let status = if done.ok { CallStatus::Ok } else if done.http == 0 { CallStatus::Error } else { classify_status(done.http, &done.error) };
    Some(Observation {
        model: call.model.trim().to_string(),
        ts_ms: now_ms,
        status,
        latency_ms: done.latency_ms,
        served_model: done.served_model.clone(),
        endpoint_ok: true,
        reasoning_tokens: done.usage.reasoning_tokens,
        cost_ticks: done.usage.cost_in_usd_ticks,
        http: done.http,
    })
}

/// Write the route span and the health line for one call. Never fails the call.
/// `call.effort` is what was sent.
pub fn route_log(config_dir: &Path, call: &RouteCall<'_>, decision: &Decision, done: &RouteDone) -> RouteRecord {
    let now = grokhub_core::now_ms();
    let mut rec = route_record(config_dir, call, decision, done);
    let key = episode_key(call);
    if !key.is_empty() {
        ladder::finish(key, class_row(call.class).map(|r| r.class).unwrap_or(call.class), done.ok);
    }
    note_user_pick(call, decision);
    // This crate's own tests drive fake clients with made-up model ids: only a
    // test that pinned a scratch config folder gets the span and health lines.
    if cfg!(test) && !crate::perm::config_pinned() {
        return rec;
    }
    let args = serde_json::json!({
        "provider": rec.used.provider,
        "model": rec.used.model,
        "effort": rec.used.effort.clone().unwrap_or_default(),
    })
    .to_string();
    let result = match (done.ok, done.cancelled) {
        (true, _) => "ok",
        (false, true) if call.provider == PROVIDER_GROK_BUILD => "sent",
        (false, true) => "cancelled",
        (false, false) => "error",
    };
    let mut span = Span::soft_allow(MODEL_TRACE, MODEL_TOOL, &args, result, "", AccessMode::Supervised, &rec.used.provider)
        .from_origin(current_origin())
        .on_path("model");
    span.access = String::new();
    span.episode = call.episode.to_string();
    if done.ok {
        let u = &done.usage;
        span.usage = Some(ModelUsage {
            input_tokens: u.input_tokens,
            cached_tokens: u.cached_tokens,
            output_tokens: u.output_tokens,
            reasoning_tokens: u.reasoning_tokens,
            cost_in_usd_ticks: u.cost_in_usd_ticks,
        });
    }
    rec.span_id = span.span_ref();
    span.route = Some(Box::new(rec.clone()));
    let _ = append_span(config_dir, &span);
    if let Some(obs) = observation(call, done, now) {
        let _ = append_observation(config_dir, &obs);
    }
    rec
}

thread_local! {
    static CLASS: std::cell::Cell<&'static str> = const { std::cell::Cell::new(DEFAULT_CLASS) };
}

/// The class a chat-loop call logs when no [`ClassScope`] is open.
pub const DEFAULT_CLASS: &str = "chat:default";

/// Names the route class for chat-loop calls made on this thread (an eval run,
/// say), like `OriginScope` does for origin. Restores the old class on drop.
pub struct ClassScope {
    prev: &'static str,
}

impl ClassScope {
    pub fn enter(class: &'static str) -> Self {
        Self { prev: CLASS.with(|c| c.replace(class)) }
    }
}

impl Drop for ClassScope {
    fn drop(&mut self) {
        CLASS.with(|c| c.set(self.prev));
    }
}

pub fn current_class() -> &'static str {
    CLASS.with(std::cell::Cell::get)
}

/// The newest user text in a request (difficulty only; never stored).
pub(crate) fn last_user_text(input: &[InputItem]) -> (String, bool) {
    let mut image = false;
    let mut text = String::new();
    for item in input.iter().rev() {
        if let InputItem::Message { role, content } = item {
            if role == "user" {
                for part in content {
                    match part {
                        ContentPart::InputText(t) => text.push_str(t),
                        ContentPart::InputImage(_) => image = true,
                    }
                }
                break;
            }
        }
    }
    (text, image)
}

/// The single helper every native model call goes through: the router picks
/// the effort (for a listed class), the request is sent with it, and the route
/// record logs what was sent.
pub fn stream_routed(
    client: &dyn ModelClient,
    req: &ResponsesRequest,
    cancel: &CancelToken,
    sink: &mut dyn FnMut(StreamEvent),
    class: &str,
) -> Result<TurnOutput, ClientError> {
    let dir = crate::perm::config_dir();
    let (text, needs_image) = last_user_text(&req.input);
    let mut call = RouteCall {
        provider: super::PROVIDER_XAI,
        class,
        model: &req.model,
        effort: req.effort.as_deref(),
        session: &req.conversation_id,
        ctx_tokens: crate::compact::estimate_input_tokens(&req.input),
        text: &text,
        needs_image,
        needs_tools: !req.tools.is_empty() || req.hosted_search,
        ..RouteCall::default()
    };
    let decision = decide(&dir, &call, grokhub_core::now_ms());
    if decision.paused() {
        route_log(&dir, &call, &decision, &RouteDone::unseen());
        return Err(ClientError::Protocol(decision.pause_msg().into()));
    }
    let effort = decision.send_effort(req.effort.as_deref());
    let model = decision.send_model(&req.model);
    let mut sent = req.clone();
    sent.effort = effort.clone();
    sent.model = model.clone();
    let started = std::time::Instant::now();
    let out = client.stream(&sent, cancel, sink);
    call.effort = effort.as_deref();
    call.model = &model;
    route_log(&dir, &call, &decision, &RouteDone::of(&out, started.elapsed()));
    out
}

/// Provider name for turns Grok Build runs itself.
pub const PROVIDER_GROK_BUILD: &str = "grok_build";

/// Route one Grok Build turn as it is sent. GB makes the model calls, so the
/// record has no outcome and feeds no health; `text` is read for difficulty
/// only. `sent` is the effort the turn really runs at: GB takes effort per
/// episode at spawn (R0 Step-0: live `set_config_option` is not verified), so a
/// session already running keeps its spawn effort. `None` means this turn spawns
/// fresh and runs at the router's pick, which is returned with the model it
/// spawns with (R2a: Auto's pick from Grok Build's own plan list; a pin is kept
/// while it is listed). A running session keeps the model it spawned with.
/// `origin` is who started the turn: only your own typed turn waits live (R2b).
pub fn route_gb_turn(config_dir: &Path, model: &str, pinned: bool, sent: Option<Option<&str>>, session: &str, text: &str, origin: Origin) -> (String, Option<String>) {
    let mut call = RouteCall {
        provider: PROVIDER_GROK_BUILD,
        class: DEFAULT_CLASS,
        model,
        session,
        text,
        pinned,
        origin: Some(origin),
        ..RouteCall::default()
    };
    let decision = decide(config_dir, &call, grokhub_core::now_ms());
    let (spawn_model, effort) = match sent {
        Some(e) => (model.to_string(), e.map(str::to_string)),
        None if decision.route.no_route => (model.to_string(), decision.route.effort.clone()),
        None => (decision.send_model(model), decision.route.effort.clone()),
    };
    call.effort = effort.as_deref();
    call.model = &spawn_model;
    route_log(config_dir, &call, &decision, &RouteDone::unseen());
    (spawn_model, effort)
}
