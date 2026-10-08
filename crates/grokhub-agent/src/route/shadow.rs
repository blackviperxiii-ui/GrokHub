//! Router R0 shadow mode: every model call site calls [`shadow_log`] after its
//! call. It asks [`super::Router::choose`] what it *would* pick, writes that as
//! the `route` record on a `model-calls` span, and logs the call's status for
//! passive health (`models/health.jsonl`). It never changes the call.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use grokhub_core::model_registry::profile::{read_profiles, ModelProfile};
use grokhub_core::model_registry::store::{append_observation, load_registry, profiles_dir, registry_path};
use grokhub_core::model_registry::{classify_status, CallStatus, Observation, Registry};

use super::cabin::{MODEL_TOOL, MODEL_TRACE};
use super::difficulty::{difficulty, DifficultyInput};
use super::log::{Chosen, RouteOutcome, RouteRecord, RouteSignals, RouteTokens};
use super::signals::{SpanVerifySource, VerifySource};
use super::{RouteInput, Router};
use crate::client::{ClientError, ContentPart, InputItem, ModelClient, ResponsesRequest, StreamEvent, TurnOutput, Usage};
use crate::CancelToken;
use crate::harness::{append_span, current_origin, AccessMode, ModelUsage, Span};

/// xAI cost ticks per USD (1 tick = 1e-10 USD).
pub const TICKS_PER_USD: f64 = 1e10;

/// One call, as the call site knows it. `text` feeds difficulty only and is never stored.
#[derive(Debug, Clone, Copy, Default)]
pub struct ShadowCall<'a> {
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
}

/// How the call went. Counts and ids only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShadowDone {
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

impl ShadowDone {
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
    prev: BTreeMap<String, Chosen>,
}

static CACHE: Mutex<Cache> = Mutex::new(Cache { reg: None, profiles: None, prev: BTreeMap::new() });

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

/// Build the shadow record for one call. Pure apart from the VerifyGate lookup.
pub fn route_record(config_dir: &Path, call: &ShadowCall<'_>, done: &ShadowDone, now_ms: u64) -> RouteRecord {
    let (reg, profiles) = snapshot(config_dir);
    let d = difficulty(&DifficultyInput { text: call.text, planned_tools: call.planned_tools, plan_mode: call.plan_mode, ctx_tokens: call.ctx_tokens });
    let input = RouteInput {
        provider: call.provider,
        class: call.class,
        current_model: call.model,
        current_effort: call.effort,
        pinned: call.pinned,
        d,
        ctx_tokens: call.ctx_tokens,
        needs_image: call.needs_image,
    };
    let route = Router::choose(&input, &reg, &profiles, now_ms);
    let verify = SpanVerifySource { config_dir: config_dir.to_path_buf(), session: call.session.to_string() }
        .verdict(call.episode, 0)
        .map(|v| v.as_str().to_string());
    let chosen = Chosen { provider: route.provider.clone(), model: route.model.clone(), effort: route.effort.clone() };
    let provider = if call.provider.is_empty() { super::PROVIDER_XAI } else { call.provider };
    let used = Chosen { provider: provider.to_string(), model: call.model.to_string(), effort: call.effort.map(str::to_string) };
    let prev = if call.episode.is_empty() {
        None
    } else {
        let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if c.prev.len() > 256 {
            c.prev.clear();
        }
        c.prev.insert(call.episode.to_string(), chosen.clone())
    };
    let u = &done.usage;
    RouteRecord {
        span_id: String::new(),
        episode: call.episode.to_string(),
        step: call.step,
        class: call.class.to_string(),
        d,
        ctx_tokens: call.ctx_tokens,
        signals: RouteSignals {
            failures: call.failures,
            verify: verify.clone(),
            origin: Some(current_origin().as_str().to_string()),
            latency: Some(done.latency_ms),
            budget_pct: None,
            privacy: None,
        },
        candidates_n: route.candidates_n,
        chosen,
        prev,
        rule_ids: route.rule_ids,
        reason: route.reason,
        pinned: call.pinned,
        outcome: (!done.cancelled).then(|| RouteOutcome {
            ok: done.ok,
            verify,
            tokens: RouteTokens { input: u.input_tokens, cached: u.cached_tokens, out: u.output_tokens, reasoning: u.reasoning_tokens },
            cost_usd: u.cost_in_usd_ticks as f64 / TICKS_PER_USD,
        }),
        used,
        shadow: true,
        settings: route.settings,
    }
}

/// The passive-health line for one call, or `None` when it says nothing about the model.
pub fn observation(call: &ShadowCall<'_>, done: &ShadowDone, now_ms: u64) -> Option<Observation> {
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
    })
}

/// Write the shadow route span and the health line for one call. Never fails the call.
pub fn shadow_log(config_dir: &Path, call: &ShadowCall<'_>, done: &ShadowDone) -> RouteRecord {
    let now = grokhub_core::now_ms();
    let mut rec = route_record(config_dir, call, done, now);
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
fn last_user_text(input: &[InputItem]) -> (String, bool) {
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

/// `client.stream`, then the shadow route log for it. The request is sent as given.
pub fn stream_shadowed(
    client: &dyn ModelClient,
    req: &ResponsesRequest,
    cancel: &CancelToken,
    sink: &mut dyn FnMut(StreamEvent),
    class: &str,
) -> Result<TurnOutput, ClientError> {
    let started = std::time::Instant::now();
    let out = client.stream(req, cancel, sink);
    let (text, needs_image) = last_user_text(&req.input);
    let call = ShadowCall {
        provider: super::PROVIDER_XAI,
        class,
        model: &req.model,
        effort: req.effort.as_deref(),
        session: &req.conversation_id,
        ctx_tokens: crate::compact::estimate_input_tokens(&req.input),
        text: &text,
        needs_image,
        ..ShadowCall::default()
    };
    shadow_log(&crate::perm::config_dir(), &call, &ShadowDone::of(&out, started.elapsed()));
    out
}

/// Provider name for turns Grok Build runs itself (effort is set per episode at spawn).
pub const PROVIDER_GROK_BUILD: &str = "grok_build";

/// Shadow-log one Grok Build turn as it is sent. GB makes the model calls, so
/// the record has no outcome and feeds no health; `text` is read for difficulty only.
pub fn shadow_gb_turn(config_dir: &Path, model: &str, effort: Option<&str>, session: &str, text: &str) -> RouteRecord {
    let call = ShadowCall {
        provider: PROVIDER_GROK_BUILD,
        class: DEFAULT_CLASS,
        model,
        effort,
        session,
        text,
        ..ShadowCall::default()
    };
    shadow_log(config_dir, &call, &ShadowDone::unseen())
}
