//! Router live routing: every model call site asks [`decide`] before it
//! sends, sends the model and effort the router picked (for a class the table
//! lists), then calls [`route_log`], which writes the `route` record on a
//! `model-calls` span and the call's status for passive health
//! (`models/health.jsonl`).
//!
//! R2a: one model per episode. The model is picked at an episode boundary (a
//! new user message), on a second VerifyGate reject at the ceiling (E2), or
//! when the episode's model goes unhealthy; otherwise every step of the
//! episode keeps it, so the prompt cache stays warm. Nothing you set pins,
//! forces or steers the model or the effort: the router alone picks them. With
//! no healthy, included model the call pauses instead of sending.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use grokhub_core::model_registry::discover::split_provider_model;
use grokhub_core::model_registry::profile::{read_profiles, ModelProfile};
use grokhub_core::model_registry::store::{
    append_observation, load_quota, load_registry, profiles_dir, registry_path,
};
use grokhub_core::model_registry::{classify_status, health, CallStatus, Observation, Registry};

use super::cabin::{MODEL_TOOL, MODEL_TRACE};
use grokhub_core::model_registry::EFFORT_LADDER;
use grokhub_core::outcome::is_correction;

use super::budget::{self, Budget, BudgetNotes};
use super::difficulty::{estimate, DifficultyInput};
use super::guard::{holdout_eligible, in_holdout, load_overrides, HOLDOUT_EFFORT};
use super::ladder::{self, rung, take_tool_errors, turn_hash, user_facing, Band, Obs, Pick};
use super::learn;
use super::local::LocalGate;
use super::log::{Chosen, RouteOutcome, RouteRecord, RouteSignals, RouteTokens};
use super::policy::ClassRow;
use super::policy::{class_row, expected_cost_usd, MODEL_LIVE, POLICY_LIVE};
use super::signals::{SpanVerifySource, VerifySignal, VerifySource};
use super::spend::{self, Latency, Spend};
use super::table::{load_table, table_path, RoutingTable};
use super::tune::{self, StepRisk};
use super::{Route, RouteInput, Router};
use crate::client::{
    ClientError, CompactOutput, CompactRequest, ContentPart, InputItem, ModelClient,
    ResponsesRequest, StreamEvent, TurnOutput, Usage,
};
use crate::harness::{append_span, current_origin, AccessMode, ModelUsage, Origin, Span};
use crate::timing::{self, Breakdown};
use crate::CancelToken;
use grokhub_core::model_registry::cost_class::CostClass;
use grokhub_core::outcome::topic_signature;

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
    /// R3a DE2: the task's Spike-7 signature (`skill:<name>`). Empty reads the topic of `text`.
    pub signature: &'a str,
    /// R3a: the step's data is marked sensitive (the local privacy route; off by default).
    pub sensitive: bool,
    /// R3a: you granted sending sensitive data to the cloud.
    pub cloud_grant: bool,
    /// The step offers a send, delete, credential or other hard-class tool.
    pub hard_tool: bool,
    /// Failover PR 6: the whole call is explicit (18+) — the router
    /// keeps it on the device. Set by the call sites from the same
    /// scrubbed input the send-time guard reads.
    pub adult: bool,
    /// R3b: the call site can send to a provider you added (native calls only).
    pub providers: bool,
    /// The episode just re-planned: one model and one rung up for this call only.
    pub step_up: bool,
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
    /// The call went through a batch queue ([`ModelClient::batches`]): its
    /// wall time is queueing, so neither tuning nor health learns from it.
    pub batched: bool,
    /// Hosted search the reply ran, and how many pages it cited.
    pub hosted: crate::client::HostedUsage,
    pub sources: u32,
}

fn http_of(err: &ClientError) -> u16 {
    match err {
        ClientError::KeyOffer { status, .. }
        | ClientError::Auth { status, .. }
        | ClientError::PaymentRequired { status, .. }
        | ClientError::Server { status, .. } => *status,
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
        Self {
            cancelled: true,
            ..Self::default()
        }
    }

    pub fn of(out: &Result<TurnOutput, ClientError>, elapsed: Duration) -> Self {
        let latency_ms = elapsed.as_millis() as u64;
        let served_model = crate::sse::take_served_model();
        match out {
            Ok(turn) => Self {
                ok: true,
                http: 200,
                usage: turn.usage.clone(),
                latency_ms,
                served_model,
                hosted: turn.hosted,
                sources: u32::try_from(turn.sources.len()).unwrap_or(u32::MAX),
                ..Self::default()
            },
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
    pub fn cabin(
        ok: bool,
        usage: Option<&ModelUsage>,
        error: &str,
        elapsed: Duration,
        served_model: Option<String>,
    ) -> Self {
        let u = usage.copied().unwrap_or_default();
        let http = if ok {
            200
        } else {
            error
                .split_whitespace()
                .find_map(|w| {
                    w.trim_matches(|c: char| !c.is_ascii_digit())
                        .parse::<u16>()
                        .ok()
                        .filter(|n| (400..600).contains(n))
                })
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
            ..Self::default()
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

static CACHE: Mutex<Cache> = Mutex::new(Cache {
    reg: None,
    profiles: None,
    table: None,
    prev: BTreeMap::new(),
    episodes: BTreeMap::new(),
    steps: BTreeMap::new(),
});

/// Episodes remembered at once; a long-running cabin stays small.
const EPISODES_CAP: usize = 256;

/// The text a call that paused returns: no healthy, included model could take
/// it. The cabin waits and resumes it on its own ([`super::wait`]).
pub static NO_ROUTE_MSG: std::sync::LazyLock<&'static str> = std::sync::LazyLock::new(|| {
    grokhub_core::brand::filled("No model in your plan is answering right now. {product} keeps checking and picks this back up on its own once one answers; Home shows the wait.")
});

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
    Budget::of(
        &budget::load_week(config_dir, reg, now_ms),
        reg.entitlement.credential,
        cap_usd,
    )
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

/// Requests sent per model this second, per config dir ([`health::Pacer`]).
static PACE: Mutex<BTreeMap<PathBuf, health::Pacer>> = Mutex::new(BTreeMap::new());

/// The registry the router picks from: a live model already at its request
/// limit this second reads as busy (degraded), so failover takes the next one.
pub(super) fn paced(config_dir: &Path, reg: Arc<Registry>, now_ms: u64) -> Arc<Registry> {
    let pace = PACE.lock().unwrap_or_else(|e| e.into_inner());
    let p = pace.get(config_dir);
    // (model, cooling after an outage rather than at its pace limit)
    let busy: Vec<(String, bool)> = reg
        .models
        .iter()
        .filter(|(_, r)| r.state == grokhub_core::model_registry::ModelState::Live)
        .filter_map(|(id, _)| {
            if p.is_some_and(|p| p.at_limit(id, now_ms)) {
                Some((id.clone(), false))
            } else if cooling(config_dir, id, now_ms) {
                Some((id.clone(), true))
            } else {
                None
            }
        })
        .collect();
    if busy.is_empty() {
        return reg;
    }
    let mut view = (*reg).clone();
    for (id, cooled) in busy {
        if let Some(r) = view.models.get_mut(&id) {
            health::mark_busy(r);
            if cooled {
                r.reason = "It was too busy to answer a moment ago.".into();
            }
        }
    }
    Arc::new(view)
}

/// Models that just failed with an outage (at capacity, overloaded, down),
/// per config dir, until when (ms). They read as busy so the next pick, the
/// resumed step included, goes to another model instead of the same wall.
static COOLING: Mutex<BTreeMap<PathBuf, BTreeMap<String, u64>>> = Mutex::new(BTreeMap::new());

/// How long a model that failed with an outage is routed around.
pub(crate) const OUTAGE_COOL_MS: u64 = 60_000;

/// `model` just failed with an outage at `now_ms`.
pub(crate) fn cool_down(config_dir: &Path, model: &str, now_ms: u64) {
    COOLING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(config_dir.to_path_buf())
        .or_default()
        .insert(model.to_string(), now_ms.saturating_add(OUTAGE_COOL_MS));
}

/// True while `model` is cooling after an outage.
pub(crate) fn cooling(config_dir: &Path, model: &str, now_ms: u64) -> bool {
    COOLING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(config_dir)
        .and_then(|m| m.get(model))
        .is_some_and(|until| now_ms < *until)
}

/// One request to `model` goes out now.
pub(super) fn note_send(config_dir: &Path, model: &str, now_ms: u64) {
    PACE.lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(config_dir.to_path_buf())
        .or_default()
        .note(model, now_ms);
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

/// D12: from here (prompt tokens) a chat pins its model between turns. Its
/// history is big enough that a switch re-sends a lot uncached, so a cheaper
/// model takes over only when [`super::policy::pin_switch`] says it pays.
pub const SESSION_KEEP_TOKENS: u64 = 16_000;

/// A chat's pin: the model its last typed turn went to, that turn, and how
/// many typed turns the chat has had.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionPin {
    model: String,
    turn: u64,
    turns: u32,
}

static SESSION_MODELS: Mutex<Option<BTreeMap<String, SessionPin>>> = Mutex::new(None);

/// The model a chat's last typed turn went to, and the chat's turns so far.
pub(super) fn session_model(key: &str) -> Option<(String, u32)> {
    SESSION_MODELS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|m| m.get(key).map(|p| (p.model.clone(), p.turns)))
}

pub(super) fn keep_session_model(key: &str, turn: u64, model: &str) {
    let mut g = SESSION_MODELS.lock().unwrap_or_else(|e| e.into_inner());
    let m = g.get_or_insert_with(BTreeMap::new);
    if m.len() >= EPISODES_CAP && !m.contains_key(key) {
        m.clear();
    }
    let pin = m.entry(key.to_string()).or_insert(SessionPin {
        model: String::new(),
        turn,
        turns: 1,
    });
    if pin.turn != turn {
        pin.turn = turn;
        pin.turns = pin.turns.saturating_add(1);
    }
    pin.model = model.to_string();
}

fn episode_model(key: &str, turn: u64) -> Option<String> {
    let c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    c.episodes
        .get(key)
        .filter(|(t, _)| *t == turn)
        .map(|(_, m)| m.clone())
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
        .or_else(|| {
            reg.get(model)
                .and_then(|r| r.meta.prices.long_context_threshold)
        });
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
    /// R3a: the self-tuning mark for the route record (`canary:<id>`, `shadow:<id>`).
    pub tune: Option<String>,
    /// How long each step of this decision took (µs), for the route record.
    pub timing: BTreeMap<String, u32>,
    /// Brain 2: the turn's goal, logged with the route.
    pub goal: Option<super::goal::RouteGoal>,
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

    /// The router sent this step to the on-device model (R3a; never while the flag is off).
    pub fn on_device(&self) -> bool {
        self.live && !self.paused() && self.route.provider == super::local::PROVIDER_LOCAL
    }

    /// The router sent this step to a provider you added (R3b): only with its key and your grant.
    pub fn on_provider(&self) -> bool {
        self.live && !self.paused() && self.route.cost_class == CostClass::NewProvider
    }

    /// What a paused call returns.
    pub fn pause_msg(&self) -> &'static str {
        match self.budget_pause {
            Some(m) => m,
            None if self.route.rule_ids.iter().any(|r| r == "local:paused") => {
                super::local::LOCAL_PAUSED_MSG
            }
            None if self.route.rule_ids.iter().any(|r| r == "offline:switch") => {
                *grokhub_core::offline::OFFLINE_MSG
            }
            None => *NO_ROUTE_MSG,
        }
    }

    /// What a paused call returns, with the reason the device can't take an offline turn.
    pub fn pause_text(&self) -> String {
        let rule = |id: &str| self.route.rule_ids.iter().any(|r| r == id);
        if self.budget_pause.is_none()
            && grokhub_core::offline::enabled()
            && rule("offline:local_unavailable")
        {
            let why = super::local::unavailable_reason();
            return format!(
                "{}: {why}. {}",
                super::local::OFFLINE_NO_LOCAL_MSG,
                *grokhub_core::offline::OFFLINE_MSG
            );
        }
        if self.budget_pause.is_none()
            && self
                .route
                .rule_ids
                .iter()
                .any(|r| r == "offline:local_unavailable")
        {
            return super::local::offline_pause_text(&super::local::unavailable_reason());
        }
        self.pause_msg().to_string()
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

/// The sizing tracker's key for a call: its episode and turn, the same pair
/// the ladder's step count uses ([`super::sizing`]).
fn sizing_key(call: &RouteCall<'_>) -> String {
    format!("{}\u{1f}{}", episode_key(call), turn_hash(call.text))
}

/// VerifyGate rejects so far in the episode, and whether its latest check passed.
fn verify_counts(config_dir: &Path, call: &RouteCall<'_>) -> (Option<u32>, bool) {
    if call.episode.is_empty() || call.session.is_empty() {
        return (None, false);
    }
    let checks = SpanVerifySource {
        config_dir: config_dir.to_path_buf(),
        session: call.session.to_string(),
    }
    .checks(call.episode);
    let rejects = checks
        .iter()
        .filter(|c| **c == VerifySignal::Reject)
        .count() as u32;
    (Some(rejects), checks.last() == Some(&VerifySignal::Ok))
}

/// Pick the provider, model and effort for one call and advance its episode's ladder.
pub fn decide(config_dir: &Path, call: &RouteCall<'_>, now_ms: u64) -> Decision {
    let mut laps = Breakdown::start();
    let (reg, profiles) = snapshot(config_dir);
    let reg = paced(config_dir, reg, now_ms);
    laps.mark("route:snapshot");
    let settings = spend::spend_settings();
    let b = week_budget(config_dir, &reg, settings.weekly_cap_usd, now_ms);
    laps.mark("route:week_budget");
    // The on-device estimator runs only offline: online it would load the local
    // model for every call, and the cloud is the route anyway.
    super::connectivity::sync_from_core();
    let rt = super::connectivity::state()
        .prefer_local()
        .then(super::local::runtime)
        .flatten();
    let key = episode_key(call);
    let base_row = class_row(call.class);
    let turn = turn_hash(call.text);
    // A typed chat turn reads its own kind: the lane (its band and model
    // needs) and the effort to start at, blended with its nearest labeled
    // turns when the embedding model is on disk. Everything else keeps its class.
    let ask = (base_row.is_some_and(|r| r.class == super::ask::CHAT_CLASS)
        && !call.text.trim().is_empty())
    .then(|| {
        let cx = super::ask::AskContext {
            ctx_tokens: call.ctx_tokens,
            needs_image: call.needs_image,
            planned_tools: call.planned_tools,
            plan_mode: call.plan_mode,
            prior: super::ask::prior(key),
        };
        let read = super::ask::read(call.text, &cx);
        super::knn::blend_turn(config_dir, key, call.text, turn, &cx, read, now_ms)
    });
    let lane = ask.as_ref().map(|a| a.lane());
    let d = match &ask {
        Some(a) if rt.is_none() => a.d,
        _ => estimate(
            &DifficultyInput {
                text: call.text,
                planned_tools: call.planned_tools,
                plan_mode: call.plan_mode,
                ctx_tokens: call.ctx_tokens,
            },
            rt.as_deref(),
        ),
    };
    let care_first = crate::brain::care_first_enabled(config_dir);
    let steer = ladder::steer_for_turn(call.text, care_first);
    laps.mark("route:difficulty");
    let row = lane.and_then(class_row).or(base_row);
    let bump = row
        .and_then(|r| load_overrides(config_dir).get(r.class).map(|o| o.steps))
        .unwrap_or(0);
    laps.mark("route:overrides");
    let holdout = holdout_eligible(call.class) && in_holdout(key);
    let facing = row.is_some_and(|r| user_facing(r.class));
    let (rejects, verify_ok) = if facing && !key.is_empty() {
        verify_counts(config_dir, call)
    } else {
        (None, false)
    };
    laps.mark("route:verify_counts");
    let tuned = learn::live_tune(config_dir);
    let risk = StepRisk {
        class: call.class,
        holdout,
        under_reject: rejects.unwrap_or(0) > 0 && !verify_ok,
        hard_tool: call.hard_tool,
    };
    let st = learn::step_tune(&tuned, lane.unwrap_or(call.class), key, &risk);
    laps.mark("route:tune");
    // Brain 2: the goal the turn's Brief named; user-facing steps only, never the holdout.
    // Brain 4: a goal the cabin pinned on a background run (cost while you're active) is
    // logged with it; only user-facing steps move by a goal.
    let goal = if holdout {
        None
    } else if facing {
        super::goal::goal_for(&[call.session, call.episode])
    } else {
        super::goal::pinned_for(&[call.session, call.episode])
    };
    let mut routine = None;
    // C10: a step of a typed chat turn that hit trouble, and the turn's band
    // and ladder class for the learner.
    let mut trouble = false;
    let mut chat_band: Option<(Band, &'static str)> = None;
    let pick = match row {
        Some(_) if holdout => Some(Pick {
            rung: rung(HOLDOUT_EFFORT).unwrap_or(0),
            rules: vec!["holdout".into()],
            ..Pick::default()
        }),
        Some(r) if user_facing(r.class) && !key.is_empty() => {
            let mut band = Band::tuned(r, bump, st.start.as_deref());
            // The ladder's state, its chat caps and `finish` all key on the
            // call's own class, so a chat keeps one effort history across lanes.
            let own = base_row.map_or(r.class, |b| b.class);
            if let Some(a) = &ask {
                // Start where the read says, shifted by what tuning and the
                // guard learned for the lane, inside its floor and ceiling.
                let shift = band.start as isize - ladder::Band::of(r, 0).start as isize;
                band.start = (a.effort as isize + shift)
                    .clamp(band.floor as isize, band.ceiling.max(band.floor) as isize)
                    as usize;
            }
            let new_turn = ladder::is_new_turn(key, own, turn);
            if new_turn {
                let sig = if call.signature.is_empty() {
                    topic_signature(call.text)
                } else {
                    call.signature.to_string()
                };
                routine = ladder::routine_rung(
                    r.class,
                    &learn::routine_for(config_dir, &sig, r.class, now_ms),
                    band,
                    now_ms,
                );
            }
            let tool_errors = take_tool_errors();
            let stage = ladder::take_stage(new_turn);
            if ask.is_some() {
                trouble = tool_errors > 0
                    || stage.same_tool_failed
                    || stage.looped
                    || stage.check_failed
                    || stage.out_of_budget
                    || (rejects.unwrap_or(0) > 0 && !verify_ok);
                if trouble && !new_turn {
                    super::knn::note_trouble(key);
                }
                chat_band = Some((band, own));
            }
            let obs = Obs {
                tool_errors,
                rejects,
                verify_ok,
                // "That's wrong" or "try again": the next answer starts one rung up.
                correction: is_correction(call.text) || super::ask::asks_redo(call.text),
                // No self-check score yet, so E4 stays quiet. DE3 reads the week's budget.
                low_check: None,
                budget_pct: b.pct,
                routine,
                stage,
                keep_start: super::capabilities::BaselineLane::of(r.class)
                    == super::capabilities::BaselineLane::Flagship,
                ..Obs::default()
            };
            let hold = ladder::hold_chat_drops(own, care_first);
            let start = super::goal::goal_start(
                band,
                // The read already weighed difficulty into the start.
                ladder::start_holding(band, if ask.is_some() { 0.5 } else { d }, steer, true, hold),
                steer,
                goal.as_ref(),
            );
            let mut pick = ladder::step_holding(key, own, turn, band, start, obs, hold);
            if call.step_up {
                // Only this call: the saved state keeps its rung, so the episode drops back after.
                pick.rung = (pick.rung + 1).min(band.ceiling.max(band.floor));
                pick.moved = ladder::Move::Up;
                pick.rules.push(super::STEP_UP_RULE.into());
            }
            Some(pick)
        }
        _ => None,
    };
    if let (Some(r), true, true) = (row, holdout, ask.is_some()) {
        // The holdout's turns are logged for the R1 guard, never steered.
        chat_band = Some((Band::of(r, bump), base_row.map_or(r.class, |b| b.class)));
    }
    let redo = ask.is_some() && (is_correction(call.text) || super::ask::asks_redo(call.text));
    if ask.is_some() && !key.is_empty() {
        // The bank learns from turns the read started, never the holdout's fixed rung.
        match &pick {
            Some(p) if !holdout => super::knn::note_rung(key, turn, p.rung),
            _ => super::knn::forget(key),
        }
        // C10: a new turn settles the last one for the learner.
        super::bandit::on_call(config_dir, key, turn, trouble, redo, now_ms);
    }
    laps.mark("route:ladder");
    let recover = pick.as_ref().is_some_and(|p| p.recover);
    // Keyed by the call's own class, so a chat keeps one model across lanes.
    let class_key = format!(
        "{key}\u{1f}{}",
        base_row.map(|r| r.class).unwrap_or(call.class)
    );
    let mut pin_turns = None;
    let sticky = if key.is_empty() {
        None
    } else {
        episode_model(&class_key, turn).or_else(|| {
            // D12: a long chat keeps its model between turns: a switch
            // re-sends the whole history uncached. Attention still breaks it
            // for a turn that needs a stronger model, and a cheaper one takes
            // over only when the saving beats the re-send.
            let (model, turns) = (ask.is_some() && call.ctx_tokens >= SESSION_KEEP_TOKENS)
                .then(|| session_model(key))
                .flatten()?;
            pin_turns = Some(turns);
            Some(model)
        })
    };
    // PR E: a failed verify on an on-device turn asks the next online pick to escalate.
    if rejects.unwrap_or(0) > 0 && !verify_ok {
        let was_local = sticky.as_deref().is_some_and(super::local::is_local)
            || super::local::is_local(call.model)
            || call.provider == super::local::PROVIDER_LOCAL;
        if was_local {
            super::local::note_local_verify_failed(true);
        }
    }
    let base_table = table_snapshot(config_dir);
    let table: Arc<RoutingTable> = match (&st.canary_order, tuned.tuning.orders.is_empty()) {
        (None, true) => base_table,
        (order, _) => {
            let mut t = tune::tuned_table(&base_table, &tuned.tuning);
            if let (Some(m), Some(r)) = (order, row) {
                t = tune::with_order(&t, r.class, m);
            }
            Arc::new(t)
        }
    };
    laps.mark("route:table");
    let origin = call.origin.unwrap_or_else(current_origin);
    let steps = if key.is_empty() {
        1
    } else {
        note_step(&class_key, turn)
    };
    let grants = spend::premium_grants(config_dir);
    laps.mark("route:premium_grants");
    // R3b: a provider you added only with its key and a grant covering this call's data.
    let providers = if call.providers {
        super::providers::usable(config_dir, super::providers::call_data(call.sensitive))
    } else {
        Vec::new()
    };
    laps.mark("route:providers");
    let spend = Spend {
        settings,
        latency: Latency::of(origin, facing, steps, call.deadline_ms, now_ms),
        background: !facing || spend::background_origin(origin),
        budget_tight: b.tight(),
        grants,
        providers,
    };
    let class_key_for_bias = row.map(|r| r.class).unwrap_or(call.class);
    let escalate_bias = super::feedback::load(config_dir).bias(class_key_for_bias);
    let prior_escalated = sticky.as_ref().is_some_and(|_| {
        // Episode already on a model: treat as prior escalate when sticky differs from current.
        sticky.as_deref().is_some_and(|m| m != call.model)
    });
    // Failover PR 5: size the turn before it runs (a long task goes to Grok,
    // not to a free model's small allowance), folded with what this turn has
    // already done — a turn that outgrows its size mid-flight is upgraded to
    // long, so its remaining picks and failovers treat it as one.
    let fresh_size = super::sizing::estimate(&super::sizing::SizeInput {
        d,
        planned_tools: call.planned_tools,
        plan_mode: call.plan_mode,
        needs_tools: call.needs_tools,
        needs_image: call.needs_image,
        reasoning: super::lane_needs_reasoning(lane.unwrap_or(call.class)),
        ctx_tokens: call.ctx_tokens,
        session_kept_tokens: if pin_turns.is_some() || steps > 1 {
            call.ctx_tokens
        } else {
            0
        },
        learned_output_tokens: super::sizing::learned_output_tokens(config_dir, call.class, now_ms),
    });
    let (size_est, size_upgraded) = if key.is_empty() {
        (fresh_size, false)
    } else {
        super::sizing::turn_estimate(&sizing_key(call), turn, fresh_size, call.ctx_tokens, now_ms)
    };
    super::sizing::note_latest(call.session, turn, size_est.size);
    let input = RouteInput {
        provider: call.provider,
        class: call.class,
        current_model: call.model,
        current_effort: call.effort,
        d,
        ctx_tokens: call.ctx_tokens,
        needs_image: call.needs_image,
        needs_tools: call.needs_tools,
        steer,
        bump,
        pick,
        episode_model: sticky.as_deref(),
        pin_turns,
        spend: spend.clone(),
        routine,
        start: st.start.as_deref(),
        local: LocalGate::now(call.sensitive, call.cloud_grant, call.hard_tool),
        goal: goal.clone(),
        text: call.text,
        escalate_bias,
        prior_escalated,
        care_first,
        lane,
        size: Some(size_est),
        // Failover PR 5: whether a local model is already loaded, for the
        // warm-local default. Never starts the sidecar to find out.
        local_warm: super::local::warm_runtime().is_some(),
        // Failover PR 6: the scorer's hard filters read the quota ledger
        // (PR 3) and the turn's explicitness, decided by the call site.
        quota: load_quota(config_dir),
        adult: call.adult,
        // Failover step 9: one low-risk episode in twenty that runs in the
        // open (no tuner mark, nothing sensitive or explicit, no pinned
        // model, no step-up) lets a model on tryout lead its small turns.
        probation_canary: (tune::canary_eligible(&risk)
            && st.mark.is_none()
            && pin_turns.is_none()
            && !call.step_up
            && !call.sensitive
            && !call.adult
            && !key.is_empty()
            && tune::in_canary(key, super::trial::CANARY))
        .then_some(key),
    };
    let mut route = Router::choose(&input, &reg, &profiles, &table, now_ms);
    if let (Some(a), Some(p), Some((band, own))) = (&ask, &input.pick, chat_band)
        && !key.is_empty()
    {
        let model = route.base.as_deref().unwrap_or(&route.model);
        // C9: only a turn the read routed in the cloud, in the open, outside a canary.
        let ours = !route.no_route
            && !route.rule_ids.iter().any(|r| r == "probation:canary")
            && !call.step_up
            && !call.sensitive
            && st.mark.is_none()
            && !super::local::is_local(model)
            && grokhub_core::adult_guard::check(super::ask::own_words(call.text))
                == grokhub_core::adult_guard::Risk::Clean;
        if ours {
            let attn = route
                .rule_ids
                .iter()
                .any(|r| r.starts_with("attn:escalate") || r.starts_with("attn:force_top"));
            let guard = if attn {
                Some(super::bandit::ATTENTION_GUARD)
            } else if a.stakes != super::ask::Stakes::None {
                Some("stakes")
            } else if super::capabilities::BaselineLane::of(a.lane())
                == super::capabilities::BaselineLane::Flagship
            {
                Some("flagship")
            } else if d >= 0.7 {
                Some("hard")
            } else if call.planned_tools > 0 || call.plan_mode || call.hard_tool {
                Some("agentic")
            } else if redo {
                Some("redo")
            } else if p.moved == ladder::Move::Up {
                Some("climb")
            } else {
                None
            };
            let steer = super::bandit::Steer {
                config_dir,
                key,
                turn,
                own,
                ctx: super::bandit::context(a.lane(), a.kind.id(), d),
                band,
                guard,
                hold: holdout,
            };
            route = super::bandit::steer(&steer, route, &input, &reg, &profiles, &table, now_ms);
        } else {
            super::bandit::skip(key, turn);
        }
    }
    if let Some(a) = &ask {
        route.rule_ids.splice(0..0, a.rules.iter().cloned());
        super::ask::remember(key, a.kind);
        if !route.no_route && !key.is_empty() {
            keep_session_model(key, turn, route.base.as_deref().unwrap_or(&route.model));
        }
    }
    if size_upgraded {
        // The turn outgrew its size on this call (sizing::TurnSizer): from
        // here on it is a long task, with a long task's routes.
        route.rule_ids.push("size:upgrade:long".into());
    }
    laps.mark("route:choose");
    let mut tune_mark = st.mark.clone();
    if let (Some(c), Some(r)) = (&st.shadow, row) {
        let (mark, arm) = shadow_mark(c, r, &input, &reg, &profiles, &table, now_ms);
        // C11: the candidate's pick sits next to the live one in the propensity log.
        if let Some(arm) = arm.filter(|_| ask.is_some()) {
            super::bandit::note_shadow(key, turn, &c.id, arm);
        }
        tune_mark = Some(mark);
    }
    laps.mark("route:shadow");
    let live = POLICY_LIVE && row.is_some();
    let budget_pause = if !live || route.no_route || call.provider == PROVIDER_GROK_BUILD {
        None
    } else if spend.background {
        let est = reg
            .get(&route.model)
            .and_then(|r| expected_cost_usd(&r.meta.prices, call.class, call.ctx_tokens))
            .unwrap_or(0.0);
        (!b.admits_background(est)).then_some(if b.used_up() {
            *budget::BUDGET_PAUSE_MSG
        } else {
            budget::BACKGROUND_PAUSE_MSG
        })
    } else if b.used_up() && BudgetNotes::load(config_dir).must_ask(&b) {
        *BUDGET_ASK.lock().unwrap_or_else(|e| e.into_inner()) = true;
        Some(*budget::BUDGET_PAUSE_MSG)
    } else {
        None
    };
    // Item 34: between boundaries the segment's first effort is sent again.
    if live
        && !route.no_route
        && !key.is_empty()
        && facing
        && !holdout
        && super::boundary::enabled(config_dir)
    {
        let floor = row.map(|r| ladder::Band::of(r, bump).floor);
        let model = route.base.clone().unwrap_or_else(|| route.model.clone());
        let (effort, held) =
            super::boundary::hold(&class_key, key, turn, &model, route.effort.take(), floor);
        route.effort = effort;
        if held {
            route.rule_ids.push(super::boundary::HELD_RULE.into());
        }
    }
    if live && !route.no_route && !key.is_empty() && !call.step_up {
        // A stepped-up call keeps the episode's own model, so the next call drops back.
        // The plain model: a Fast swap is decided again on every call.
        keep_episode_model(
            class_key,
            turn,
            route.base.as_deref().unwrap_or(&route.model),
        );
    }
    if let Some(ask) = &route.premium_ask {
        *PREMIUM_ASK.lock().unwrap_or_else(|e| e.into_inner()) = Some(ask.clone());
    }
    // No route answers at all, so nothing is falling back to the device either.
    super::local::note_fallback(
        !route.no_route
            && route
                .rule_ids
                .iter()
                .any(|r| r == "local:cloud_down" || r == "local:no_keys"),
    );
    // An offline turn leaves a line: where it went (local, kept, cloud queue).
    if let Some(rule) = route.rule_ids.iter().find(|r| r.starts_with("offline:")) {
        let what = format!("{} → {rule}", call.class);
        super::local_health::log_to(config_dir, "offline_turn", Some(&what), None);
    }
    // A governor pause lifts on its own and Offline only is the user's own
    // switch: neither is a model to wait for.
    if live
        && route.no_route
        && !route
            .rule_ids
            .iter()
            .any(|r| r == "local:paused" || r == "offline:switch")
    {
        let failed = reg
            .get(call.model)
            .map(|r| r.reason.clone())
            .unwrap_or_else(|| "It isn't in your model list.".into());
        *NO_ROUTE.lock().unwrap_or_else(|e| e.into_inner()) = Some((
            call.class.to_string(),
            call.model.trim().to_string(),
            failed,
        ));
    }
    if !route.no_route && budget_pause.is_none() {
        note_send(config_dir, &route.model, now_ms);
    }
    laps.mark("route:budget_check");
    let timing = laps.finish("route:decide");
    Decision {
        route,
        d: (d * 100.0).round() as u32,
        holdout,
        recover,
        live,
        budget_pct: b.pct,
        budget_pause,
        tune: tune_mark,
        timing,
        goal,
    }
}

/// A shadow candidate computed next to the live pick, logged only:
/// `shadow:<id>`, or `shadow:<id>!` when it could not be served inside the
/// filters (fit, entitlement, cost class, privacy, floors). With it, the arm
/// (`model@effort`) the candidate would have sent this call.
fn shadow_mark(
    c: &tune::Candidate,
    row: &ClassRow,
    input: &RouteInput<'_>,
    reg: &Registry,
    profiles: &BTreeMap<String, ModelProfile>,
    table: &RoutingTable,
    now_ms: u64,
) -> (String, Option<String>) {
    let mut shadow = input.clone();
    shadow.pick = None;
    shadow.episode_model = None;
    shadow.pin_turns = None;
    let ok = match &c.change {
        Some(tune::Change::Order { model }) => {
            let t = tune::with_order(table, row.class, model);
            let r = Router::choose(&shadow, reg, profiles, &t, now_ms);
            !r.no_route && r.model == *model && r.cost_class == CostClass::Included
        }
        Some(tune::Change::Start { effort }) => {
            let floor = Band::of(row, input.bump).floor;
            shadow.start = Some(effort);
            rung(effort).is_some_and(|x| x >= floor)
                && !Router::choose(&shadow, reg, profiles, table, now_ms).no_route
        }
        None => false,
    };
    let arm = shadow_arm(c, row, input, reg, profiles, table, now_ms);
    (format!("shadow:{}{}", c.id, if ok { "" } else { "!" }), arm)
}

/// What the candidate would send in this call's own place (its pick, pin and
/// sticky model kept): its model first, or its start shifted onto the pick.
fn shadow_arm(
    c: &tune::Candidate,
    row: &ClassRow,
    input: &RouteInput<'_>,
    reg: &Registry,
    profiles: &BTreeMap<String, ModelProfile>,
    table: &RoutingTable,
    now_ms: u64,
) -> Option<String> {
    let mut alt = input.clone();
    let ordered;
    let table = match c.change.as_ref()? {
        tune::Change::Order { model } => {
            ordered = tune::with_order(table, row.class, model);
            &ordered
        }
        tune::Change::Start { effort } => {
            let band = Band::tuned(row, input.bump, input.start);
            let pick = alt.pick.as_mut()?;
            let shift = rung(effort)? as isize - band.start as isize;
            pick.rung = (pick.rung as isize + shift)
                .clamp(band.floor as isize, band.ceiling.max(band.floor) as isize)
                as usize;
            table
        }
    };
    let r = Router::choose(&alt, reg, profiles, table, now_ms);
    (!r.no_route)
        .then(|| super::bandit::arm_id(r.base.as_deref().unwrap_or(&r.model), r.effort.as_deref()))
}

/// The route record for one call. `call.effort` is what was actually sent.
pub fn route_record(
    config_dir: &Path,
    call: &RouteCall<'_>,
    decision: &Decision,
    done: &RouteDone,
) -> RouteRecord {
    let route = &decision.route;
    let verify = SpanVerifySource {
        config_dir: config_dir.to_path_buf(),
        session: call.session.to_string(),
    }
    .verdict(call.episode, 0)
    .map(|v| v.as_str().to_string());
    let chosen = Chosen {
        provider: route.provider.clone(),
        model: route.model.clone(),
        effort: route.effort.clone(),
    };
    let provider = if call.provider.is_empty() {
        super::PROVIDER_XAI
    } else {
        call.provider
    };
    let used = Chosen {
        provider: provider.to_string(),
        model: call.model.to_string(),
        effort: call.effort.map(str::to_string),
    };
    let key = episode_key(call);
    let prev = if key.is_empty() {
        None
    } else {
        let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if c.prev.len() > 256 {
            c.prev.clear();
        }
        c.prev
            .insert(format!("{key}\u{1f}{}", call.class), chosen.clone())
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
            latency: (!done.batched).then_some(done.latency_ms),
            budget_pct: decision.budget_pct,
            privacy: route
                .rule_ids
                .iter()
                .any(|r| r == "privacy:local_only")
                .then(|| "local_only".to_string()),
        },
        candidates_n: route.candidates_n,
        chosen,
        prev,
        rule_ids: route.rule_ids.clone(),
        reason: route.reason.clone(),
        outcome: (!done.cancelled).then(|| RouteOutcome {
            ok: done.ok,
            verify,
            tokens: RouteTokens {
                input: u.input_tokens,
                cached: u.cached_tokens,
                out: u.output_tokens,
                reasoning: u.reasoning_tokens,
            },
            cost_usd: u.cost_in_usd_ticks as f64 / TICKS_PER_USD,
            limit: done.http == 429
                && classify_status(done.http, &done.error) == CallStatus::FreeUsageExhausted,
            hosted: done.hosted,
            sources: done.sources,
        }),
        used,
        shadow: !decision.live,
        holdout: decision.holdout,
        settings: route.settings.clone(),
        cost_class: route.cost_class.as_str().to_string(),
        tune: decision.tune.clone(),
        timing_us: decision.timing.clone(),
        goal: decision
            .goal
            .as_ref()
            .map(|g| g.goal.as_str().to_string())
            .unwrap_or_default(),
        goal_reason: decision
            .goal
            .as_ref()
            .map(|g| g.reason.clone())
            .unwrap_or_default(),
        gov_state: if decision.on_device() {
            super::local_gov::level().label().to_string()
        } else {
            String::new()
        },
        tier: if decision.on_device() {
            super::local::hw_tier(config_dir)
                .unwrap_or_default()
                .to_string()
        } else {
            String::new()
        },
        // Set by `route_log`, from the request this call just sent.
        prefix_break: None,
        request: crate::harness::replay::last_request(),
        // Set by `route_log`, from the switches stashed while the call ran.
        handoffs: Vec::new(),
    }
}

/// The class table's start effort for `class` (no difficulty, no ladder), as it goes on the wire.
/// `None` for an unlisted class.
pub fn start_effort(class: &str) -> Option<String> {
    let row = class_row(class)?;
    let e = EFFORT_LADDER[Band::of(row, 0).start];
    Some(
        grokhub_core::parse_reasoning_effort(e)
            .unwrap_or(e)
            .to_string(),
    )
}

/// The passive-health line for one call, or `None` when it says nothing about the model.
pub fn observation(call: &RouteCall<'_>, done: &RouteDone, now_ms: u64) -> Option<Observation> {
    if done.cancelled || call.model.trim().is_empty() || call.provider == "grok_build" {
        return None;
    }
    let status = if done.ok {
        CallStatus::Ok
    } else if done.http == 0 {
        CallStatus::Error
    } else {
        classify_status(done.http, &done.error)
    };
    Some(Observation {
        model: call.model.trim().to_string(),
        ts_ms: now_ms,
        status,
        // 0 is "no sample": health skips it ([`grokhub_core::model_registry::health`]).
        latency_ms: if done.batched { 0 } else { done.latency_ms },
        served_model: done.served_model.clone(),
        endpoint_ok: true,
        reasoning_tokens: done.usage.reasoning_tokens,
        cost_ticks: done.usage.cost_in_usd_ticks,
        http: done.http,
    })
}

/// The health line for a call that failed and was answered by another model,
/// so the model it was sent to is never counted healthy. Never fails the call.
fn note_failure(config_dir: &Path, call: &RouteCall<'_>, err: &ClientError, elapsed: Duration) {
    if cfg!(test) && !crate::perm::config_pinned() {
        return;
    }
    let done = RouteDone {
        http: http_of(err),
        error: err.to_string(),
        latency_ms: elapsed.as_millis() as u64,
        ..RouteDone::default()
    };
    if let Some(obs) = observation(call, &done, grokhub_core::now_ms()) {
        let _ = append_observation(config_dir, &obs);
    }
    // An account-shaped failure on the native route stamps the quota
    // ledger's xAI re-read, so the next refresh re-checks the key's
    // blocked state at once instead of waiting out the poll interval.
    if matches!(done.http, 402 | 403 | 429) {
        super::quota::note_native_error(config_dir, call.model, done.http, grokhub_core::now_ms());
    }
}

/// xAI couldn't answer for a reason another model could get past: it is down,
/// overloaded, rate limited or unreachable after the client's own retries. A
/// refused key, a cancel, a prompt too long for any model, or this computer
/// being offline is not an outage another cloud model fixes.
fn xai_outage(err: &ClientError) -> bool {
    err.is_retryable() || matches!(err, ClientError::IdleTimeout)
}

/// A provider you added failed in a way another route can get past: a limit
/// (429), out of credit (402), a server fault (5xx), a refused key, a dropped
/// stream or the wire itself. A refusal about the prompt, a prompt too long
/// for the next model too, a cancel, or this computer being offline is not
/// one: the next route would hit the same wall.
pub(crate) fn provider_fallible(err: &ClientError) -> bool {
    if err.is_context_length() {
        return false;
    }
    matches!(
        err,
        ClientError::RateLimited { .. }
            | ClientError::PaymentRequired { .. }
            | ClientError::Server { .. }
            | ClientError::Auth { .. }
            | ClientError::KeyOffer { .. }
            | ClientError::Disconnect
            | ClientError::IdleTimeout
    ) || matches!(err, ClientError::Transport(m) if !grokhub_core::offline::is_refusal(m))
        // OpenRouter's own account settings left this model no endpoint: the
        // next route can take the turn, so the raw notice never ends a chat.
        || matches!(err, ClientError::Protocol(m) if m == super::providers::POLICY_REFUSED)
}

/// The provider model you set up that takes a turn xAI couldn't: its key and a
/// grant for this call's data, it fits the turn (tools, images, context), it
/// passes Auto's own pick filter ([`super::providers::auto_pickable`]: listed
/// price under your $/M ceiling, text out, reasoning on a reasoning lane, no
/// router), and it ranks best for the class. A provider that has been busy
/// (429) or much slower than the others lately ranks after a healthy one
/// ([`health::lagging_providers`]); within each, Grok on a provider goes
/// first, so the reply keeps its voice. `None` when no provider you added can
/// take it.
pub(crate) fn outage_stand_in(
    config_dir: &Path,
    call: &RouteCall<'_>,
    now_ms: u64,
) -> Option<String> {
    let providers =
        super::providers::usable(config_dir, super::providers::call_data(call.sensitive));
    if providers.is_empty() {
        return None;
    }
    let (reg, profiles) = snapshot(config_dir);
    let spend = Spend {
        settings: spend::spend_settings(),
        providers,
        ..Spend::default()
    };
    let fit = super::policy::Fit {
        needs_image: call.needs_image,
        needs_tools: call.needs_tools,
        ctx_tokens: call.ctx_tokens,
        grok_build: false,
    };
    let need = super::providers::PickNeed {
        tools: call.needs_tools,
        reasoning: super::lane_needs_reasoning(call.class),
        ctx_tokens: call.ctx_tokens,
        ceiling_usd_per_m: spend.settings.ceiling_usd_per_m,
    };
    let pool: Vec<&str> = reg
        .models
        .iter()
        .filter(|(id, r)| {
            super::policy::is_new_provider(&reg, id)
                && super::policy::fits(&reg, &profiles, id, fit, &spend, now_ms)
                && super::providers::auto_pickable(id, &r.meta, &need)
        })
        .map(|(id, _)| id.as_str())
        .collect();
    let table = load_table(config_dir);
    let mut ranked =
        super::policy::rank(call.class, &pool, &table, &reg, call.model, call.ctx_tokens);
    let lagging = health::lagging_providers(&health::provider_healths(&reg, now_ms));
    // Stable: the rank decides within each group.
    ranked.sort_by_key(|m| {
        (
            split_provider_model(m).is_some_and(|(p, _)| lagging.contains(p)),
            !m.to_ascii_lowercase().contains("grok"),
        )
    });
    ranked.into_iter().next()
}

/// Write the route span and the health line for one call. Never fails the call.
/// `call.effort` is what was sent.
pub fn route_log(
    config_dir: &Path,
    call: &RouteCall<'_>,
    decision: &Decision,
    done: &RouteDone,
) -> RouteRecord {
    let _total = timing::lap("route:log");
    let now = grokhub_core::now_ms();
    let mut rec = timing::time("route:log_record", || {
        route_record(config_dir, call, decision, done)
    });
    // The request this call sent may have rewritten its cached prefix.
    // The chat path names its conversation in `session`, the router's own
    // calls in `episode`.
    rec.prefix_break = crate::prefix_guard::take_staged(call.session)
        .or_else(|| crate::prefix_guard::take_staged(call.episode))
        .as_ref()
        .map(super::log::BreakNote::from);
    // Failover PR 7: the switches this call made, stashed as they happened.
    rec.handoffs = super::handoff::take_stashed(call.session);
    if rec.handoffs.is_empty() && !call.episode.is_empty() {
        rec.handoffs = super::handoff::take_stashed(call.episode);
    }
    let key = episode_key(call);
    if !key.is_empty() {
        let class = class_row(call.class).map(|r| r.class).unwrap_or(call.class);
        ladder::finish(key, class, done.ok);
        if class == super::ask::CHAT_CLASS {
            super::knn::note_done(key, done.ok, done.cancelled);
            super::bandit::note_call(key, done.ok, done.cancelled, done.usage.cost_in_usd_ticks);
        }
    }
    // This crate's own tests drive fake clients with made-up model ids: only a
    // test that pinned a scratch config folder gets the span and health lines.
    if cfg!(test) && !crate::perm::config_pinned() {
        return rec;
    }
    if !key.is_empty() && !done.cancelled && !decision.route.no_route {
        // D13: the chat's cache hit rate, switches and escalations. Counts only.
        timing::time("route:log_session", || {
            super::session_metrics::note_call(
                config_dir,
                key,
                &super::session_metrics::CallFacts {
                    model: &rec.used.model,
                    rule_ids: &rec.rule_ids,
                    prompt_tokens: done.usage.input_tokens,
                    cached_tokens: done.usage.cached_tokens,
                    ok: done.ok,
                },
                now,
            )
        });
        // Failover PR 5: what the call wrote and whether a free or on-device
        // model carried it, for the turn's mid-flight size upgrade.
        if done.ok {
            super::sizing::note_served(
                &sizing_key(call),
                turn_hash(call.text),
                done.usage.output_tokens,
                super::local::is_local(&rec.used.model)
                    || super::providers::is_free_model(&rec.used.model),
            );
        }
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
    let mut span = Span::soft_allow(
        MODEL_TRACE,
        MODEL_TOOL,
        &args,
        result,
        "",
        AccessMode::Supervised,
        &rec.used.provider,
    )
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
    let _ = timing::time("route:log_span", || append_span(config_dir, &span));
    if let Some(obs) = observation(call, done, now) {
        let _ = timing::time("route:log_health", || append_observation(config_dir, &obs));
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
        Self {
            prev: CLASS.with(|c| c.replace(class)),
        }
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

thread_local! {
    static HELPER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Marks this thread's calls as a helper's (a subagent's) until dropped,
/// so routing can keep helpers off a free route that is nearly spent.
pub struct HelperScope {
    prev: bool,
}

impl HelperScope {
    pub fn enter() -> Self {
        Self {
            prev: HELPER.with(|h| h.replace(true)),
        }
    }
}

impl Drop for HelperScope {
    fn drop(&mut self) {
        HELPER.with(|h| h.set(self.prev));
    }
}

/// This thread is running a helper's calls.
pub fn in_helper() -> bool {
    HELPER.with(std::cell::Cell::get)
}

/// The newest user text in a request (difficulty only; never stored), without
/// the `<cabin-volatile>` notes, so style or playbook words never steer routing.
/// Messages the app itself writes in the user's place (a cabin note, a
/// background notice, the ledger, standing rules, a compaction summary) are
/// skipped: they arrive mid-run, and reading one as the ask made the router
/// treat it as a new turn and move the effort mid-run.
pub(crate) fn last_user_text(input: &[InputItem]) -> (String, bool) {
    for item in input.iter().rev() {
        let InputItem::Message { role, content } = item else {
            continue;
        };
        if role != "user" {
            continue;
        }
        let mut image = false;
        let mut text = String::new();
        for part in content {
            match part {
                ContentPart::InputText(t) => text.push_str(t),
                ContentPart::InputImage(_) => image = true,
            }
        }
        let text = crate::prefix_cache::visible_user_text(&text).to_string();
        if app_written(&text) {
            continue;
        }
        return (text, image);
    }
    (String::new(), false)
}

/// A user-role message the app wrote, not the person.
fn app_written(text: &str) -> bool {
    const TAGS: [&str; 6] = [
        "<cabin-note>",
        "<background-notice>",
        "<harness_ledger>",
        "<standing_rules>",
        "<conversation_summary>",
        "This session is being continued",
    ];
    let t = text.trim_start();
    TAGS.iter().any(|tag| t.starts_with(tag))
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
    let entered = std::time::Instant::now();
    crate::harness::replay::clear_last();
    let dir = crate::perm::config_dir();
    // Playbooks 8: a banned ask never leaves the computer, this turn or a later one.
    let mut sent = req.clone();
    super::adult::scrub(&mut sent.input);
    if let Some(held) = super::adult::held_reply(&sent.input) {
        sink(StreamEvent::TextDelta(held.text.clone()));
        return Ok(held);
    }
    // Round 3 item 37: no gameplay advice during a match that bars it.
    if let Some(held) = super::coaching::held_reply(class, &sent.input) {
        sink(StreamEvent::TextDelta(held.text.clone()));
        return Ok(held);
    }
    let (text, needs_image) = last_user_text(&sent.input);
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
        providers: true,
        // Failover PR 6: the router hears the turn is explicit up
        // front, from the same scrubbed input the guard below reads.
        adult: super::adult::explicit(&sent.input),
        ..RouteCall::default()
    };
    let now = grokhub_core::now_ms();
    let decision = decide(&dir, &call, now);
    if decision.paused() {
        route_log(&dir, &call, &decision, &RouteDone::unseen());
        return Err(ClientError::Protocol(decision.pause_text()));
    }
    let effort = decision.send_effort(req.effort.as_deref());
    let mut on_device = decision.on_device();
    let dest = if on_device {
        grokhub_core::adult_guard::Dest::Local
    } else if decision.on_provider() {
        super::providers::dest(&dir, &decision.send_model(&req.model))
    } else {
        grokhub_core::adult_guard::Dest::Xai
    };
    // 18+ chat runs on this computer's model only, never online. With none
    // ready and an 18+ voice on, every turn gets the notice (with the
    // self-check's reason) and nothing is sent: a clean ask in that voice is
    // an 18+ turn too. With the voice off, it's decided per message: a clean
    // ask still goes online with the chat's older 18+ parts left out; an 18+
    // ask gets the notice. The app's own background work never goes online with it.
    if super::adult::to_local(&sent.input, dest) {
        match super::adult::no_local_notice() {
            None => on_device = true,
            Some(notice) => {
                let online = !class.trim().starts_with("background:")
                    && !super::adult::voice_on(&sent.input)
                    && super::adult::leave_adult_out(&mut sent.input);
                if !online {
                    let out = super::adult::notice_answer(class, &notice)?;
                    sink(StreamEvent::TextDelta(out.text.clone()));
                    return Ok(out);
                }
            }
        }
    }
    let mut on_provider = decision.on_provider() && !on_device;
    let model = if on_device && !decision.on_device() {
        super::local::live_tier().to_string()
    } else {
        decision.send_model(&req.model)
    };
    // A local tier has no effort menu, so the router picks none for it; the
    // sidecar maps the turn's own effort onto what the model's template renders.
    sent.effort = if on_device {
        effort.clone().or_else(|| req.effort.clone())
    } else {
        effort.clone()
    };
    sent.model = model.clone();
    // On this computer's model with 18+ on, only the hard limits stay.
    if on_device {
        super::adult::unlock_local(&mut sent.input);
    }
    // Model and effort show nowhere; only a turn that asks gets told, truthfully.
    let asks_route = super::ask::route_question(&text).any();
    let route_note = |model: &str, effort: Option<&str>, local: bool| {
        let name = local
            .then(|| super::local_sidecar::model_name(&dir))
            .flatten();
        super::ask::route_note(&super::identity::describe(model, name.as_deref()), effort)
    };
    // A turn that asks about the online models or a key gets the live
    // connection check, so the answer is the wire's, never a guess. It
    // rides under the route note (one write: a second would replace it).
    let connections = (!class.trim().starts_with("background:")
        && super::check::connection_question(&text))
    .then(|| {
        super::check::note(&super::check::report_for_turn(
            &dir,
            &req.conversation_id,
            &text,
            call.adult,
        ))
    });
    // A turn that asks why this computer's model isn't ready, or to check its
    // folder, gets the self-check's findings: the answer comes from the files,
    // never "I can't reach your files, check it yourself".
    let local_check = (!class.trim().starts_with("background:")
        && super::local_doctor::question(&text))
    .then(|| super::local_doctor::note(&super::local_doctor::diagnose(&dir)));
    let notes: Vec<String> = asks_route
        .then(|| route_note(&model, sent.effort.as_deref(), on_device))
        .into_iter()
        .chain(connections)
        .chain(local_check)
        .collect();
    if !notes.is_empty() {
        super::ask::add_route_note(&mut sent.input, &notes.join("\n"));
    }
    let started = std::time::Instant::now();
    timing::record("send:pre_request", started.duration_since(entered));
    let enter = timing::take_enter();
    if let Some(t) = enter {
        timing::record("send:enter_to_request", started.duration_since(t));
    }
    let mut first = false;
    let sink = &mut |ev: StreamEvent| {
        if !first {
            first = true;
            timing::record("send:first_token", entered.elapsed());
            if let Some(t) = enter {
                timing::record("send:enter_to_first_token", t.elapsed());
            }
        }
        sink(ev)
    };
    // Failover PR 7: a switch inside this call tells the chat one plain
    // line — why it moved, never which models. Background work stays silent,
    // like the loading note below, and so does small talk: the line is about
    // the app, not the chat, and it only helps while work is under way.
    let switch_line = |sink: &mut dyn FnMut(StreamEvent), err: &ClientError| {
        if !class.trim().starts_with("background:") && !super::intent::casual_turn(&text) {
            sink(StreamEvent::ReasoningDelta(
                super::handoff::chat_line(super::handoff::stop_rule(err)).to_string(),
            ));
        }
    };
    // Reply text passes the hard-limit check before the chat shows it.
    let mut reply = grokhub_core::adult_guard::ReplyGuard::default();
    // xAI's error and how long it took, when another model answered instead.
    let mut xai_failed: Option<(ClientError, Duration)> = None;
    // The provider model that answered when xAI was down.
    let mut stood_in: Option<String> = None;
    // A provider's error, the model that made it, and how long it took, when
    // another route took the turn instead: it goes on the provider's own
    // health line, never on the answering model's.
    let mut provider_failed: Option<(String, ClientError, Duration)> = None;
    // Grok took a turn a provider failed: the turn is logged as Grok's.
    let mut grok_took_turn = false;
    let out = if on_device {
        // A model that isn't loaded yet (unloaded for a game, or idle) loads
        // again by itself; the chat shows why the answer takes a moment.
        if !class.trim().starts_with("background:") && super::local::warm_runtime().is_none() {
            sink(StreamEvent::ReasoningDelta(
                super::local::LOCAL_LOADING_NOTE.into(),
            ));
        }
        // Pass the full request so cabin web_search / web_fetch tool calls round-trip
        // through the agent loop (OpenAI-compatible tools on the local sidecar).
        let out = super::local::serve_request(class, &sent, cancel);
        // The chat shows streamed text only, so the device's answer goes out
        // as one delta, like a provider's; without this the reply is saved to
        // the session but never reaches the screen.
        if let Ok(turn) = &out {
            if !turn.text.is_empty() {
                super::adult::guard(
                    &mut reply,
                    &mut *sink,
                    StreamEvent::TextDelta(turn.text.clone()),
                );
            }
        }
        out
    } else if on_provider {
        // An Anthropic or OpenRouter answer streams; the rest arrive as one delta.
        // Either way the text reaches the chat through the reply guard.
        let span_id = format!("{}:{now}", req.conversation_id);
        let out = super::providers::stream_model(
            &dir,
            class,
            &model,
            effort.as_deref(),
            &sent,
            super::providers::call_data(call.sensitive),
            &span_id,
            cancel,
            &mut |ev: StreamEvent| super::adult::guard(&mut reply, &mut *sink, ev),
        );
        match out {
            Err(err) if provider_fallible(&err) => {
                // The provider hit a limit, ran out of credit or fell over:
                // cool it so the next pick, the resumed step included, skips
                // it, and Grok takes this turn. The provider showed nothing,
                // so no work repeats.
                cool_down(&dir, &model, now);
                super::handoff::note_switch(&req.conversation_id, &model, &req.model, &err);
                switch_line(&mut *sink, &err);
                provider_failed = Some((model.clone(), err, started.elapsed()));
                grok_took_turn = true;
                on_provider = false;
                if asks_route {
                    let n = route_note(&req.model, effort.as_deref(), false);
                    super::ask::add_route_note(&mut sent.input, &n);
                }
                sent.model = req.model.clone();
                let out = client.stream(&sent, cancel, &mut |ev: StreamEvent| {
                    super::adult::guard(&mut reply, &mut *sink, ev)
                });
                if let Err(err) = &out {
                    if xai_outage(err) {
                        cool_down(&dir, &req.model, now);
                    }
                }
                out
            }
            out => out,
        }
    } else {
        // `streamed`: reply text reached the chat. Thinking alone isn't an
        // answer, so a stand-in can still take a turn xAI only thought about.
        let mut streamed = false;
        let out = client.stream(&sent, cancel, &mut |ev: StreamEvent| {
            if matches!(&ev, StreamEvent::TextDelta(t) if !t.is_empty()) {
                streamed = true;
            }
            super::adult::guard(&mut reply, &mut *sink, ev)
        });
        if let Err(err) = &out {
            if xai_outage(err) {
                cool_down(&dir, &model, now);
            }
        }
        // xAI down for an ordinary turn: a provider you set up answers, and
        // failing that this computer's model (below). Only when xAI showed
        // nothing yet.
        let stand_in = match &out {
            Err(err) if !streamed && xai_outage(err) => outage_stand_in(&dir, &call, now),
            _ => None,
        };
        match out {
            Err(err) if stand_in.is_some() => {
                let m = stand_in.unwrap_or_default();
                super::handoff::note_switch(&req.conversation_id, &model, &m, &err);
                switch_line(&mut *sink, &err);
                xai_failed = Some((err, started.elapsed()));
                on_provider = true;
                if asks_route {
                    let n = route_note(&m, effort.as_deref(), false);
                    super::ask::add_route_note(&mut sent.input, &n);
                }
                let span_id = format!("{}:{now}", req.conversation_id);
                let out = super::providers::stream_model(
                    &dir,
                    class,
                    &m,
                    effort.as_deref(),
                    &sent,
                    super::providers::call_data(call.sensitive),
                    &span_id,
                    cancel,
                    &mut |ev: StreamEvent| super::adult::guard(&mut reply, &mut *sink, ev),
                );
                match out {
                    Err(e2) if provider_fallible(&e2) && super::local::available() => {
                        // The stand-in hit the same kind of wall: cool it so
                        // it isn't picked again, note it on its own health
                        // line, and this computer's model answers instead.
                        // The provider showed nothing, so no work repeats.
                        cool_down(&dir, &m, now);
                        super::handoff::note_switch(
                            &req.conversation_id,
                            &m,
                            super::local::live_tier(),
                            &e2,
                        );
                        switch_line(&mut *sink, &e2);
                        provider_failed = Some((m, e2, started.elapsed()));
                        on_provider = false;
                        on_device = true;
                        if asks_route {
                            let n =
                                route_note(super::local::live_tier(), sent.effort.as_deref(), true);
                            super::ask::add_route_note(&mut sent.input, &n);
                        }
                        let out = super::local::serve_request(class, &sent, cancel);
                        if let Ok(turn) = &out {
                            if !turn.text.is_empty() {
                                super::adult::guard(
                                    &mut reply,
                                    &mut *sink,
                                    StreamEvent::TextDelta(turn.text.clone()),
                                );
                            }
                        }
                        out
                    }
                    Err(e2) if provider_fallible(&e2) => {
                        // No local model to land on: the error stands, but
                        // the stand-in still cools so a resumed step doesn't
                        // re-pick the model that just failed.
                        cool_down(&dir, &m, now);
                        stood_in = Some(m);
                        Err(e2)
                    }
                    out => {
                        stood_in = Some(m);
                        out
                    }
                }
            }
            // An ordinary turn falls back to this computer's model when xAI is
            // down and no provider could take it. Only when xAI showed nothing
            // yet: a half-streamed xAI answer is never finished by another model.
            Err(err)
                if !streamed
                    && !matches!(err, ClientError::Cancelled)
                    && super::local::available()
                    && (xai_outage(&err)
                        || matches!(&err, ClientError::Transport(m)
                            if grokhub_core::offline::is_refusal(m))) =>
            {
                super::handoff::note_switch(
                    &req.conversation_id,
                    &model,
                    super::local::live_tier(),
                    &err,
                );
                switch_line(&mut *sink, &err);
                xai_failed = Some((err, started.elapsed()));
                on_device = true;
                if asks_route {
                    let n = route_note(super::local::live_tier(), sent.effort.as_deref(), true);
                    super::ask::add_route_note(&mut sent.input, &n);
                }
                let out = super::local::serve_request(class, &sent, cancel);
                if let Ok(turn) = &out {
                    // The chat shows streamed text, so the local answer goes out
                    // as one delta, like a provider's.
                    if !turn.text.is_empty() {
                        super::adult::guard(
                            &mut reply,
                            &mut *sink,
                            StreamEvent::TextDelta(turn.text.clone()),
                        );
                    }
                }
                out
            }
            out => out,
        }
    };
    // The held rest of the reply, or the line that replaced it; what the chat
    // showed is what the history keeps.
    let rest = reply.finish();
    if !rest.is_empty() {
        sink(StreamEvent::TextDelta(rest));
    }
    let out = out.map(|mut turn| {
        turn.text = if reply.replaced() {
            reply.shown()
        } else {
            grokhub_core::adult_guard::screen_reply(&turn.text).to_string()
        };
        turn
    });
    timing::record("send:model_call", started.elapsed());
    call.effort = effort.as_deref();
    call.model = &model;
    if let Some((err, elapsed)) = &xai_failed {
        // xAI's health line says it failed; the answer is logged as the model's that gave it.
        note_failure(&dir, &call, err, *elapsed);
        call.model = stood_in.as_deref().unwrap_or(super::local::live_tier());
    }
    if let Some((failed_model, err, elapsed)) = &provider_failed {
        // The provider's health line says it failed; the answer is logged as
        // the route's that gave it.
        let mut failed_call = call;
        failed_call.model = failed_model;
        note_failure(&dir, &failed_call, err, *elapsed);
    }
    if grok_took_turn {
        call.model = &sent.model;
    }
    if on_device {
        call.provider = super::local::PROVIDER_LOCAL;
    } else if let Some(m) = &stood_in {
        call.provider = split_provider_model(m).map_or(&decision.route.provider, |(p, _)| p);
    } else if on_provider {
        call.provider = &decision.route.provider;
    }
    // Failover PR 7: the step's hand-off chain learns how this call ended —
    // who served it, or who failed it last — for the resume packet.
    match &out {
        Ok(_) => super::handoff::note_served(&req.conversation_id, call.model),
        Err(e) if !matches!(e, ClientError::Cancelled) => {
            super::handoff::note_failure(&req.conversation_id, call.model, e)
        }
        _ => {}
    }
    let mut done = RouteDone::of(&out, started.elapsed());
    // Only the xAI client queues; a local or provider answer was timed live.
    done.batched = client.batches() && !on_device && !on_provider;
    route_log(&dir, &call, &decision, &done);
    out
}

/// xAI's server-side compaction of `req`, when the router would send this
/// compaction to xAI. `None` when it would go to this computer's model or a
/// provider, the router pauses it, the ask is held, or the client has no
/// compaction endpoint: the caller summarizes through [`stream_routed`] instead,
/// which decides again. The call keeps the chat's own model (`req.model`): the
/// compaction item is read back by that model, so the router's pick for the
/// summary class is not used. A success is logged like any routed call.
pub fn compact_routed(
    client: &dyn ModelClient,
    req: &CompactRequest,
    cancel: &CancelToken,
) -> Option<Result<CompactOutput, ClientError>> {
    if !client.compacts_server_side() {
        return None;
    }
    let dir = crate::perm::config_dir();
    // Playbooks 8: a banned ask never leaves the computer, here either.
    let mut sent = req.clone();
    super::adult::scrub(&mut sent.input);
    // An 18+ history is never summarized online either.
    if super::adult::held_reply(&sent.input).is_some() || super::adult::explicit(&sent.input) {
        return None;
    }
    let (text, needs_image) = last_user_text(&sent.input);
    let call = RouteCall {
        provider: super::PROVIDER_XAI,
        class: super::CLASS_COMPACT,
        model: &req.model,
        session: &req.conversation_id,
        ctx_tokens: crate::compact::estimate_input_tokens(&req.input),
        text: &text,
        needs_image,
        providers: true,
        ..RouteCall::default()
    };
    let decision = decide(&dir, &call, grokhub_core::now_ms());
    if decision.paused() || decision.on_device() || decision.on_provider() {
        return None;
    }
    let started = std::time::Instant::now();
    let out = client.compact(&sent, cancel)?;
    if let Ok(done) = &out {
        let turn = Ok(TurnOutput {
            usage: done.usage.clone(),
            ..TurnOutput::default()
        });
        route_log(
            &dir,
            &call,
            &decision,
            &RouteDone::of(&turn, started.elapsed()),
        );
    }
    Some(out)
}

/// Provider name for turns Grok Build runs itself.
pub const PROVIDER_GROK_BUILD: &str = "grok_build";

/// Route one Grok Build turn as it is sent. GB makes the model calls, so the
/// record has no outcome and feeds no health; `text` is read for difficulty
/// only. `sent` is the effort the turn really runs at: GB takes effort per
/// episode at spawn (R0 Step-0: live `set_config_option` is not verified), so a
/// session already running keeps its spawn effort. `None` means this turn spawns
/// fresh and runs at the router's pick, which is returned with the model it
/// spawns with (R2a: Auto's pick from Grok Build's own plan list). A running
/// session keeps the model it spawned with.
/// `origin` is who started the turn: only your own typed turn waits live (R2b).
pub fn route_gb_turn(
    config_dir: &Path,
    model: &str,
    sent: Option<Option<&str>>,
    session: &str,
    text: &str,
    origin: Origin,
) -> (String, Option<String>) {
    let _lap = timing::lap("route:gb_turn");
    if let Some(t) = timing::take_enter() {
        timing::record("send:enter_to_gb_route", t.elapsed());
    }
    let mut call = RouteCall {
        provider: PROVIDER_GROK_BUILD,
        class: DEFAULT_CLASS,
        model,
        session,
        text,
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
