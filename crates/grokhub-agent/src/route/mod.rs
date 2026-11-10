//! Router-ready model calls (Spike-3b), routed live since Router R1. Every
//! model call names its class and goes through [`call_model`],
//! [`live::stream_routed`] or [`cabin::call_model`], which take the effort from
//! [`Router::choose`]. There is no effort setting: the class table
//! ([`policy`]), difficulty and the ladder ([`ladder`]) decide, and the route
//! log keeps the reasons. xAI goes over [`crate::XaiClient`] (or a test
//! [`ModelClient`]); a provider you added with your own key (R3b) goes over
//! [`providers::call_model`], only under its key and your destination grant.

pub mod adult;
pub mod ask;
pub mod ask_set;
pub mod attention;
pub mod bandit;
pub mod boundary;
pub mod budget;
pub mod cabin;
pub mod capabilities;
pub mod check;
pub mod cleanup;
pub mod coaching;
pub mod connectivity;
pub mod difficulty;
pub mod feedback;
pub mod goal;
pub mod guard;
pub mod handoff;
pub mod heal;
pub mod host;
pub mod identity;
pub mod intent;
pub mod knn;
pub mod ladder;
pub mod learn;
pub mod live;
pub mod local;
pub mod local_autoconfig;
pub mod local_bench;
pub mod local_doctor;
pub mod local_effort;
pub mod local_fit;
pub mod local_flags;
pub mod local_gov;
pub mod local_habit;
pub mod local_health;
pub mod local_http;
pub mod local_image;
pub mod local_kv;
pub mod local_live;
pub mod local_perf;
pub mod local_prewarm;
pub mod local_sampler;
pub mod local_scale;
pub mod local_sidecar;
pub mod local_spec;
pub mod local_swap;
pub mod local_task;
pub mod local_tune;
pub mod local_warm;
pub mod log;
pub mod match_ask;
pub mod policy;
pub mod providers;
pub mod quota;
pub mod refresh;
pub mod score;
pub mod session_metrics;
pub mod signals;
pub mod sizing;
pub mod sources;
pub mod spend;
pub mod table;
pub mod trial;
pub mod trust;
pub mod tune;
pub mod wait;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use std::collections::BTreeMap;

use grokhub_core::model_registry::cost_class::{
    fast_base, fast_entitled, fast_of, route_key, usd_per_m, CostClass, RouteOpts,
};
use grokhub_core::model_registry::discover::split_provider_model;
use grokhub_core::model_registry::profile::{ModelProfile, RuntimeSettings};
use grokhub_core::model_registry::seed::{LONG_CONTEXT_TOKENS, MILLION_CONTEXT};
use grokhub_core::model_registry::{Credential, ModelState, Registry, EFFORT_LADDER};

use table::RoutingTable;

use crate::client::{
    ClientError, InputItem, ModelClient, ResponsesRequest, TextFormat, TurnOutput, Usage,
};
use crate::CancelToken;

pub use live::{route_log, RouteCall, RouteDone};
pub use log::{Chosen, RouteRecord};

/// The provider [`call_model`] knows.
pub const PROVIDER_XAI: &str = "xai";

/// The episode worker's own step (the model that proposes tools).
pub const CLASS_EPISODE: &str = "episode:step";
/// VerifyGate's checker (fresh context).
pub const CLASS_JUDGE: &str = "background:judge";
/// Folding two step summaries into one parent.
pub const CLASS_COMPACT: &str = "background:compact";

/// Effort for background calls (judge and fold). A constant, not a setting.
pub const BACKGROUND_EFFORT: &str = "low";

/// One model call: who answers it and what it is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCall {
    pub provider: String,
    pub model: String,
    /// `None` sends no reasoning field.
    pub effort: Option<String>,
    /// What the call is for (`background:judge`), logged with its tokens.
    pub class: String,
    pub conversation_id: String,
    pub input: Vec<InputItem>,
    pub tools: Vec<Value>,
    /// The chat's span session, so the shadow route can read VerifyGate's verdicts. Empty when none.
    pub session: String,
    /// A time-boxed task's deadline (ms), for the Fast policy (R2b).
    pub deadline_ms: Option<u64>,
    /// The episode just re-planned: this call goes one model and one rung up
    /// ([`STEP_UP_RULE`]). The episode stays on its own model afterwards.
    pub step_up: bool,
    /// A strict JSON schema for the reply, sent only when a Grok model answers.
    pub text_format: Option<TextFormat>,
}

/// The route rule on a call stepped up after a re-plan.
pub const STEP_UP_RULE: &str = "replan:step_up";

impl ModelCall {
    pub fn xai(
        model: &str,
        effort: Option<&str>,
        class: &str,
        conversation_id: &str,
        input: Vec<InputItem>,
    ) -> Self {
        Self {
            provider: PROVIDER_XAI.into(),
            model: model.into(),
            effort: effort.map(str::to_string),
            class: class.into(),
            conversation_id: conversation_id.into(),
            input,
            tools: Vec::new(),
            session: String::new(),
            deadline_ms: None,
            step_up: false,
            text_format: None,
        }
    }

    /// The reply must follow `format` when a Grok model answers it.
    pub fn with_text_format(mut self, format: TextFormat) -> Self {
        self.text_format = Some(format);
        self
    }

    /// The task is time-boxed: it must finish by `deadline_ms`.
    pub fn due_by(mut self, deadline_ms: u64) -> Self {
        self.deadline_ms = Some(deadline_ms);
        self
    }

    pub fn in_session(mut self, session: &str) -> Self {
        self.session = session.into();
        self
    }

    pub fn with_tools(mut self, tools: Vec<Value>) -> Self {
        self.tools = tools;
        self
    }

    /// The wire request this call sends.
    pub fn request(&self) -> ResponsesRequest {
        ResponsesRequest {
            model: self.model.clone(),
            effort: self.effort.clone(),
            input: self.input.clone(),
            conversation_id: self.conversation_id.clone(),
            tools: self.tools.clone(),
            hosted_search: false,
            call_timeout: None,
            text_format: self.text_format,
        }
    }
}

/// Tokens and cost of one routed call, for the step's span.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallTokens {
    pub class: String,
    pub model: String,
    #[serde(default)]
    pub effort: String,
    #[serde(rename = "in")]
    pub input: u64,
    pub cached: u64,
    pub out: u64,
    pub reasoning: u64,
    /// xAI cost ticks (1e-10 USD).
    pub cost_ticks: i64,
}

impl CallTokens {
    pub fn of(call: &ModelCall, usage: &Usage) -> Self {
        Self {
            class: call.class.clone(),
            model: call.model.clone(),
            effort: call.effort.clone().unwrap_or_default(),
            input: usage.input_tokens,
            cached: usage.cached_tokens,
            out: usage.output_tokens,
            reasoning: usage.reasoning_tokens,
            cost_ticks: usage.cost_in_usd_ticks,
        }
    }
}

/// A routed call's answer plus its token line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Routed {
    pub out: TurnOutput,
    pub tokens: CallTokens,
}

/// Send one call to its provider. An unknown provider is an error, never a
/// silent fallback to another one. The router picks the model and effort for a
/// listed class, and pauses when no healthy model can take it; the route record
/// logs what was sent.
pub fn call_model(
    client: &dyn ModelClient,
    call: &ModelCall,
    cancel: &CancelToken,
) -> Result<Routed, ClientError> {
    if call.provider != PROVIDER_XAI {
        return Err(ClientError::Protocol(format!(
            "no route for provider `{}`",
            call.provider
        )));
    }
    let dir = crate::perm::config_dir();
    // Playbooks 8: a banned ask never leaves the computer.
    let mut sent = call.clone();
    adult::scrub(&mut sent.input);
    if let Some(out) = adult::held_reply(&sent.input) {
        return Ok(Routed {
            out,
            tokens: CallTokens::default(),
        });
    }
    // Round 3 item 37: no gameplay advice during a match that bars it.
    if let Some(out) = coaching::held_reply(&call.class, &sent.input) {
        return Ok(Routed {
            out,
            tokens: CallTokens::default(),
        });
    }
    let text = live::last_user_text(&sent.input).0;
    let mut rc = RouteCall {
        provider: &call.provider,
        class: &call.class,
        model: &call.model,
        effort: call.effort.as_deref(),
        episode: &call.conversation_id,
        session: &call.session,
        ctx_tokens: crate::compact::estimate_input_tokens(&call.input),
        text: &text,
        needs_tools: !call.tools.is_empty(),
        deadline_ms: call.deadline_ms,
        step_up: call.step_up,
        ..RouteCall::default()
    };
    rc.providers = true;
    // Failover PR 6: the router hears the turn is explicit up front,
    // from the same scrubbed input the guard below reads.
    rc.adult = adult::explicit(&sent.input);
    let now = grokhub_core::now_ms();
    let decision = live::decide(&dir, &rc, now);
    if decision.paused() {
        route_log(&dir, &rc, &decision, &RouteDone::unseen());
        return Err(ClientError::Protocol(decision.pause_text()));
    }
    sent.effort = decision.send_effort(call.effort.as_deref());
    sent.model = decision.send_model(&call.model);
    let mut on_device = decision.on_device();
    let dest = if on_device {
        grokhub_core::adult_guard::Dest::Local
    } else if decision.on_provider() {
        providers::dest(&dir, &sent.model)
    } else {
        grokhub_core::adult_guard::Dest::Xai
    };
    // 18+ chat runs on this computer's model only, never online.
    if adult::to_local(&sent.input, dest) {
        if let Some(notice) = adult::no_local_notice() {
            let out = adult::notice_answer(&call.class, &notice)?;
            let tokens = CallTokens::of(&sent, &out.usage);
            return Ok(Routed { out, tokens });
        }
        on_device = true;
        sent.model = local::live_tier().to_string();
    }
    let on_provider = decision.on_provider() && !on_device;
    let started = std::time::Instant::now();
    crate::harness::replay::clear_last();
    // Set when Grok took a turn the picked provider failed: logged as Grok's.
    let mut grok_model = String::new();
    let out = if on_device {
        local::serve(&call.class, &text, cancel)
    } else if on_provider {
        let span_id = format!(
            "{}:{now}",
            if call.session.is_empty() {
                &call.conversation_id
            } else {
                &call.session
            }
        );
        let out = providers::call_model(
            &dir,
            &call.class,
            &sent.model,
            sent.effort.as_deref(),
            &sent.request(),
            providers::call_data(rc.sensitive),
            &span_id,
            cancel,
        );
        match out {
            Err(err) if live::provider_fallible(&err) && sent.model != call.model => {
                // The same rule as the chat path: cool the provider so the
                // next pick skips it, and Grok takes the turn. Only when the
                // router rerouted this call away from the caller's own model:
                // a caller that named the provider model gets its error.
                live::cool_down(&dir, &sent.model, now);
                // Failover PR 7: the switch lands on this call's route
                // record like the chat path's (no packet here: this path's
                // callers re-plan their own retries).
                handoff::log_switch(
                    if call.session.is_empty() {
                        &call.conversation_id
                    } else {
                        &call.session
                    },
                    &sent.model,
                    &call.model,
                    &err,
                );
                grok_model = call.model.clone();
                sent.model = grok_model.clone();
                client.stream(&sent.request(), cancel, &mut |_| {})
            }
            out => out,
        }
    } else {
        client.stream(&sent.request(), cancel, &mut |_| {})
    };
    if on_device {
        rc.provider = local::PROVIDER_LOCAL;
    } else if on_provider && grok_model.is_empty() {
        rc.provider = &decision.route.provider;
    }
    rc.effort = sent.effort.as_deref();
    rc.model = &sent.model;
    route_log(
        &dir,
        &rc,
        &decision,
        &RouteDone::of(&out, started.elapsed()),
    );
    let mut out = out?;
    out.text = grokhub_core::adult_guard::screen_reply(&out.text).to_string();
    let tokens = CallTokens::of(&sent, &out.usage);
    Ok(Routed { out, tokens })
}

/// Everything [`Router::choose`] looks at. No I/O behind it: the registry,
/// profiles and routing table are passed in, and the clock is `now_ms`.
#[derive(Debug, Clone, Default)]
pub struct RouteInput<'a> {
    /// `xai` for native calls, `grok_build` for the GB path.
    pub provider: &'a str,
    pub class: &'a str,
    /// The model and effort the call uses today.
    pub current_model: &'a str,
    pub current_effort: Option<&'a str>,
    pub d: f64,
    pub ctx_tokens: u64,
    pub needs_image: bool,
    /// The step offers tools.
    pub needs_tools: bool,
    /// The user's own words this episode ("think hard", "keep it quick").
    pub steer: ladder::Steer,
    /// Rungs the accuracy guard raised this class's start and floor.
    pub bump: u8,
    /// The live ladder's pick for this episode. `None` starts fresh from the class table.
    pub pick: Option<ladder::Pick>,
    /// The model this episode already runs on. `None` at an episode boundary.
    pub episode_model: Option<&'a str>,
    /// D12: `episode_model` is a long chat's pin on a new turn, and the chat
    /// has had this many turns. A cheaper model takes over only when the
    /// saving over the rest of the chat beats re-sending its history
    /// ([`policy::pin_switch`]).
    pub pin_turns: Option<u32>,
    /// R2b: what this call may spend (cost classes, the Fast policy, grants, budget).
    pub spend: spend::Spend,
    /// R3a DE2: the rung this task's signature is known to succeed at, for a fresh pick.
    pub routine: Option<usize>,
    /// R3a: the self-tuned (or canary) class start, clamped into the band.
    pub start: Option<&'a str>,
    /// R3a: the local model's gate. Off by default, so no `local:*` id is ever picked.
    pub local: local::LocalGate,
    /// Brain 2: the turn's goal and reason from the policy row. Moves a
    /// user-facing step's starting effort inside its band ([`goal::goal_start`]).
    pub goal: Option<goal::RouteGoal>,
    /// Newest user text for attention (risk / ambiguity / blast radius). Empty
    /// when the call site has none; never stored on the route record.
    pub text: &'a str,
    /// Learned escalate bias (−2..=+2) from [`feedback`] for this class.
    pub escalate_bias: i8,
    /// This episode already escalated (stronger model mid-flow).
    pub prior_escalated: bool,
    /// Care-first holds `chat:default` and the caller has already dropped keyword steer.
    pub care_first: bool,
    /// The class-table row a typed chat turn reads as ([`ask::AskKind::lane`]):
    /// drives the band, the model needs and the ranking. Eligibility (privacy,
    /// the local gate, cloud-only) stays on `class`.
    pub lane: Option<&'a str>,
    /// Failover PR 5: the turn's size estimate ([`sizing`]), when the call
    /// site sized it. A long task keeps free provider models out of the pick
    /// while a carrier route exists and ranks the paid routes first (xAI,
    /// the plan, ahead of the other paid providers). `None` keeps every
    /// pre-sizing behavior.
    pub size: Option<sizing::SizeEstimate>,
    /// Failover PR 5: a local model is already loaded on the device (the
    /// live path asks [`local::warm_runtime`]). With a tiny or short
    /// estimate, the warm model takes the turn by default
    /// ([`sizing::warm_local_default`]).
    pub local_warm: bool,
    /// Failover PR 3's quota ledger, as last detected: the scorer's hard
    /// filters read it, so a closed, spent or benched route is never
    /// picked in the first place (failover PR 6).
    pub quota: grokhub_core::model_registry::quota::QuotaLedger,
    /// The whole call is explicit (18+): the device is its only route —
    /// [`Router::choose`] keeps it local when the device is a candidate,
    /// and the scorer bars every cloud candidate outright.
    pub adult: bool,
    /// Failover step 9: the episode key when this turn is in the tryout
    /// canary (low risk, nothing sensitive, nothing explicit, no pinned
    /// model; the live path draws it with [`tune::in_canary`]). The
    /// scorer then lets a model on tryout lead a tiny or short turn.
    pub probation_canary: Option<&'a str>,
}

/// The lowest rung an xAI reasoning model takes without a listing that says otherwise.
const LOW_RUNG: usize = 2;

impl RouteInput<'_> {
    /// The row the choice is made for: the turn's lane, else its class.
    pub fn lane(&self) -> &str {
        self.lane.unwrap_or(self.class)
    }
}

/// The pick: provider, model, effort, one plain sentence, and the rules that fired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub provider: String,
    pub model: String,
    pub effort: Option<String>,
    pub reason: String,
    pub rule_ids: Vec<String>,
    pub candidates_n: u32,
    pub settings: Option<RuntimeSettings>,
    /// No healthy, included model could take the step: the call pauses and tells.
    pub no_route: bool,
    /// The episode's (or the call's) model this route stands in for, when it fell back.
    pub replaced: Option<String>,
    /// R2b: the cost class of the route that is sent.
    pub cost_class: CostClass,
    /// The plain model when the Fast variant was swapped in (the episode keeps this one).
    pub base: Option<String>,
    /// A premium route this call wanted but you haven't approved: the cabin asks once.
    pub premium_ask: Option<String>,
}

/// Pure and deterministic. R1 sends its effort for listed classes; R2a sends
/// its model too ([`policy::MODEL_LIVE`]), one per episode.
pub struct Router;

/// The effort's plain label ("Medium", "Extra High", "No effort").
pub fn effort_word(e: Option<&str>) -> &'static str {
    match e {
        None | Some("") => "No effort",
        Some(e) => grokhub_core::effort_label(e),
    }
}

/// The plain clause for the rule that moved effort last.
fn why_clause(plain: &str, rules: &[String]) -> String {
    const MOVES: &[&str] = &[
        "E1",
        "E2",
        "E3",
        "E4",
        "E5",
        "DE1",
        "DE2",
        "DE3",
        "steer:harder",
        "steer:quicker",
        "d<0.3",
        "d>0.7",
        "goal:quality",
        "goal:speed",
        "goal:cost",
        "attn:escalate:host",
        "attn:escalate:blast",
        "attn:escalate:multi",
        "attn:escalate:hard",
        "attn:escalate:steer",
        "attn:escalate:diagnosis",
        "attn:escalate:ambiguous",
        "attn:escalate:learned",
        "attn:escalate:episode",
        "attn:force_top:repair",
        "attn:force_top:prepare",
        "attn:deescalate:light",
    ];
    let last = rules.iter().rev().find(|r| {
        MOVES.contains(&r.as_str())
            || r.starts_with("attn:escalate")
            || r.starts_with("attn:force_top")
            || r.starts_with("attn:deescalate")
    });
    match last.map(String::as_str) {
        Some("E1") => "a tool failed, so it thinks harder".into(),
        Some("E2") => "the check rejected the work".into(),
        Some("E3") => "you corrected the last answer".into(),
        Some("E4") => "the self-check was weak".into(),
        Some("E5") => format!("{plain}, which never runs below High"),
        Some("DE1") => "the last steps went cleanly".into(),
        Some("DE2") => "this task has gone well at this level before".into(),
        Some("DE3") => "most of the budget is used".into(),
        Some("steer:harder") => "you asked it to think hard".into(),
        Some("steer:quicker") => "you asked to keep it quick".into(),
        Some("d<0.3") => format!("{plain}, and this looks routine"),
        Some("d>0.7") => format!("{plain}, and this looks hard"),
        Some("goal:quality") => format!("{plain}, and this one needs care"),
        Some("goal:speed") => format!("{plain}, and a fast reply matters here"),
        Some("goal:cost") => format!("{plain}, kept cheap"),
        Some("attn:force_top:repair") => {
            format!("{plain}; host repair always uses the strongest model")
        }
        Some("attn:force_top:prepare") => {
            format!("{plain}; hard prepare stays on the strongest model")
        }
        Some(r) if r.starts_with("attn:escalate") => {
            format!("{plain}; escalated for more care")
        }
        Some("attn:deescalate:light") => {
            format!("{plain}; light follow-up, cheaper task model")
        }
        _ => plain.to_string(),
    }
}

/// The model side of a route: the model, whether it paused, what it replaced.
struct ModelPick {
    model: String,
    no_route: bool,
    replaced: Option<String>,
}

/// An explicit successor for a redirected slug, when it can take the step.
fn successor(reg: &Registry, id: &str, ids: &[&str]) -> Option<String> {
    let rec = reg.get(id).filter(|r| r.state == ModelState::Redirected)?;
    rec.served_as.clone().filter(|s| ids.contains(&s.as_str()))
}

/// Some cloud model is up for this path and could take a step: healthy, and
/// either within your spend now or only waiting for your OK on its price.
/// A model left out for its price or for this step's needs (image, tools,
/// context) still counts, so only a real outage makes the device the last resort.
fn cloud_can_answer(
    reg: &Registry,
    profiles: &BTreeMap<String, ModelProfile>,
    grok_build: bool,
    spend: &spend::Spend,
    now_ms: u64,
) -> bool {
    let fit = policy::Fit {
        grok_build,
        ..policy::Fit::default()
    };
    reg.models.keys().map(String::as_str).any(|id| {
        !local::is_local(id)
            && policy::fits_any_cost(reg, profiles, id, fit, now_ms)
            && (policy::approved(reg, id, grok_build, spend)
                || policy::route_class(reg, id, grok_build, spend) == CostClass::Premium)
    })
}

/// The 200k gate. From xAI's long-context step every token of a request costs
/// twice as much, and grok-4.7's 500k window fills soon after, so a prompt at
/// or past [`LONG_CONTEXT_TOKENS`] moves the xAI models with a 1M window
/// (grok-4.3, the grok-4.20 family) to the front: doubled, grok-4.3 still costs
/// well under grok-4.7 doubled. The Flagship lane (plan, design, debug,
/// chat:reason, hard prepare, host repair) wants the plan's strongest reasoner,
/// which no 1M model is, so it keeps grok-4.7 while the prompt fits there. A
/// model whose window can't hold the prompt never reaches the ranking
/// ([`policy::fits`]), so past 500k every lane lands on a 1M model.
fn long_context_first(
    ranked: &mut [String],
    reg: &Registry,
    lane: &str,
    ctx_tokens: u64,
    rules: &mut Vec<String>,
) {
    if ctx_tokens < LONG_CONTEXT_TOKENS
        || capabilities::BaselineLane::of(lane) == capabilities::BaselineLane::Flagship
    {
        return;
    }
    let million = |m: &str| {
        !local::is_local(m)
            && !policy::is_new_provider(reg, m)
            && reg
                .get(m)
                .and_then(|r| r.meta.context_length)
                .is_some_and(|c| c >= MILLION_CONTEXT)
    };
    let before = ranked.first().cloned();
    ranked.sort_by_key(|m| !million(m));
    if ranked.first() != before.as_ref() {
        rules.push("ctx:long_1m".into());
    }
}

/// A lane that must pick a reasoning model: the Balanced and Quality tiers,
/// and every class the capability table marks.
pub(crate) fn lane_needs_reasoning(class: &str) -> bool {
    table::SpeedQuality::of(class).needs_reasoning()
        || capabilities::class_need(class).require_reasoning
}

/// Auto's own pool: xAI and the device, plus a model on a provider you set
/// up (its key and a grant covering the data, which [`policy::fits`] checks)
/// that is under your $/M ceiling, answers in text and is no utility model (a
/// safety classifier, an embedding or rerank model). A provider model with no
/// listed price stays out (an OpenRouter router may pick any model at any
/// price, and some providers list no prices at all). Auto picks the model
/// itself, so never one of OpenRouter's own routers.
pub(crate) fn in_auto_pool(reg: &Registry, m: &str, ceiling_usd_per_m: f64) -> bool {
    let over_ceiling = || {
        reg.get(m).is_some_and(|r| {
            r.meta
                .prices
                .completion
                .is_none_or(|p| usd_per_m(p) > ceiling_usd_per_m)
                || r.meta
                    .output_modalities
                    .as_ref()
                    .is_some_and(|o| !o.iter().any(|x| x == "text"))
        })
    };
    !policy::is_new_provider(reg, m)
        || !(policy::utility_model(m)
            || over_ceiling()
            || providers::is_openrouter_router(split_provider_model(m).map_or("", |(_, id)| id)))
}

/// grok-4.20-multi-agent and its dated or `-latest` ids.
pub(crate) fn is_multi_agent(model: &str) -> bool {
    model
        .trim()
        .to_ascii_lowercase()
        .strip_prefix("grok-4.20-multi-agent")
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))
}

/// grok-4.20-multi-agent reads `reasoning.effort` as how many agents run, not
/// how deep one thinks: low and medium run 4 agents, high and xhigh run 16.
/// Sixteen agents is the `+agents16` premium route ([`RouteOpts::multi_agent_16`]),
/// so Auto sends High only on a Flagship lane whose ladder reached High and
/// only under your grant for that route. Every other turn, a cheap one above
/// all, runs 4 agents at Low.
fn multi_agent_effort(
    effort: Option<String>,
    lane: &str,
    model: &str,
    spend: &spend::Spend,
    rules: &mut Vec<String>,
) -> Option<String> {
    let deep = matches!(effort.as_deref(), Some("high" | "xhigh"));
    let key = route_key(
        model,
        RouteOpts {
            multi_agent_16: true,
            ..RouteOpts::default()
        },
    );
    if deep
        && capabilities::BaselineLane::of(lane) == capabilities::BaselineLane::Flagship
        && spend.grants.contains(&key)
    {
        rules.push("multi_agent:16".into());
        Some("high".into())
    } else {
        rules.push("multi_agent:4".into());
        Some("low".into())
    }
}

impl Router {
    fn pick_model(
        input: &RouteInput<'_>,
        reg: &Registry,
        profiles: &BTreeMap<String, ModelProfile>,
        ids: &[&str],
        table: &RoutingTable,
        e2: bool,
        stronger: bool,
        now_ms: u64,
        rules: &mut Vec<String>,
    ) -> ModelPick {
        let current = input.current_model.trim();
        let gb = input.provider == live::PROVIDER_GROK_BUILD;
        let keep = |m: &str| ModelPick {
            model: m.to_string(),
            no_route: false,
            replaced: None,
        };
        if reg.models.is_empty() {
            rules.push("registry:empty".into());
            return keep(current);
        }
        let need_reasoning = lane_needs_reasoning(input.lane());
        // Balanced/Quality Auto pool: reasoning models only. A provider model
        // reasons when its listing's `supported_parameters` name `reasoning`
        // (OpenRouter); a listing that doesn't say leaves out only one named
        // non-reasoning, and the ranking decides.
        let auto_ok = |m: &str| {
            local::is_local(m)
                || !need_reasoning
                || if policy::is_new_provider(reg, m) {
                    reg.get(m)
                        .is_some_and(|r| providers::takes_reasoning(m, &r.meta))
                } else {
                    table::is_reasoning_model(reg, profiles, m)
                }
        };
        let allowed = |m: &str| in_auto_pool(reg, m, input.spend.settings.ceiling_usd_per_m);
        // A provider model ranks after xAI and the device unless the routing
        // table ranks it for this step.
        let xai_first = |ranked: &mut Vec<String>| {
            ranked.sort_by_key(|m| {
                policy::is_new_provider(reg, m) && table.row(input.lane(), m).is_none()
            });
        };
        // Failover PR 6: the scorer's read of this pick — the quota
        // ledger, the task's size and needs, the turn's goal, and the
        // hard facts (sensitive, explicit, the governor's level) its
        // filters run on. It orders every ranked list below in place of
        // PR 5's placeholder long-task sort: hard filters first, then
        // the route ladder — tiers by route and size, the weighed score
        // inside a tier (see score::order).
        let score_ctx = score::ScoreCtx {
            lane: input.lane(),
            size: input.size,
            goal: input.goal.as_ref().map(|g| g.goal),
            sensitive: input.local.sensitive,
            adult: input.adult,
            level: input.local.capacity.level,
            quota: &input.quota,
            ctx_tokens: input.ctx_tokens,
            now_ms,
            canary: input.probation_canary,
            helper: live::in_helper(),
        };
        let attn = attention::decide(&attention::AttentionInput {
            class: input.lane(),
            text: input.text,
            d: input.d,
            needs_tools: input.needs_tools,
            ctx_tokens: input.ctx_tokens,
            hard_tool: input.local.hard_tool,
            steer: input.steer,
            escalate_bias: input.escalate_bias,
            prior_escalated: input.prior_escalated,
        });
        if !gb && reg.entitlement.credential == Credential::None {
            // No sign-in: nothing native is included, but a provider you set
            // up still answers on its own key and grant. Auto ranks those
            // models for the step (the next one after a failed answer).
            let theirs: Vec<&str> = ids
                .iter()
                .copied()
                .filter(|m| policy::is_new_provider(reg, m) && allowed(m))
                .collect();
            // Mid-episode, the model the episode runs on keeps the next step.
            if !e2 {
                if let Some(ep) = input.episode_model.map(str::trim).filter(|m| {
                    theirs.contains(m)
                        && reg.get(m).is_some_and(|r| r.state != ModelState::Degraded)
                }) {
                    rules.push("episode:keep".into());
                    return keep(ep);
                }
            }
            let fitting: Vec<&str> = theirs.iter().copied().filter(|m| auto_ok(m)).collect();
            let pool = if fitting.is_empty() {
                &theirs
            } else {
                &fitting
            };
            let ranked = policy::rank(input.lane(), pool, table, reg, current, input.ctx_tokens);
            let mut ranked = score::order(ranked, reg, table, &score_ctx, true, rules);
            if e2 {
                let away = input.episode_model.unwrap_or(current).trim().to_string();
                ranked.retain(|m| *m != away);
                rules.push("model:next".into());
            }
            if let Some(m) = ranked.first() {
                rules.push("provider:auto".into());
                return keep(m);
            }
            // Otherwise the call keeps its own model.
            rules.push("cost:not_included".into());
            return keep(current);
        }
        if reg.get(current).is_some()
            && !ids.contains(&current)
            && policy::route_class(reg, current, gb, &input.spend) == CostClass::Premium
        {
            // Over your $/M ceiling and not approved: stand in, and the cabin asks once.
            rules.push("cost:premium_ungranted".into());
        }
        let own: Vec<&str> = ids
            .iter()
            .copied()
            .filter(|m| allowed(m))
            .filter(|m| auto_ok(m))
            .collect();
        let preferred: Vec<&str> = ids
            .iter()
            .copied()
            .filter(|m| allowed(m) || table.row(input.lane(), m).is_some())
            .filter(|m| auto_ok(m) || table.row(input.lane(), m).is_some())
            .collect();
        // A degraded model is still eligible, but an episode leaves it when a
        // healthy one can take over.
        let healthy: Vec<&str> = own
            .iter()
            .copied()
            .filter(|m| reg.get(m).is_some_and(|r| r.state != ModelState::Degraded))
            .collect();
        let pool: &[&str] = if healthy.is_empty() { &own } else { &healthy };
        let sound = |m: &str| {
            ids.contains(&m)
                && (healthy.contains(&m)
                    || policy::is_new_provider(reg, m)
                    || !healthy.iter().any(|h| *h != m))
        };
        // Stand in for `from`: its redirect successor, then its family chain, then the ranked list.
        let stand_in = |from: &str, rules: &mut Vec<String>, tag: &str| -> ModelPick {
            if policy::is_new_provider(reg, from) && !ids.contains(&from) {
                // A provider model with no key or no grant for this data: xAI takes it.
                rules.push("provider:ungranted".into());
            }
            if let Some(s) = successor(reg, from, &own) {
                rules.push(format!("{tag}:redirect"));
                return ModelPick {
                    model: s,
                    no_route: false,
                    replaced: Some(from.to_string()),
                };
            }
            if let Some(base) = fast_base(reg, from).filter(|b| own.contains(b)) {
                rules.push(format!("{tag}:base"));
                return ModelPick {
                    model: base.to_string(),
                    no_route: false,
                    replaced: Some(from.to_string()),
                };
            }
            let mut ranked =
                policy::rank(input.lane(), pool, table, reg, current, input.ctx_tokens);
            xai_first(&mut ranked);
            let ranked = score::order(ranked, reg, table, &score_ctx, true, rules);
            let chain = policy::fallback_chain(from, pool);
            match chain.first().or(ranked.first()) {
                Some(m) => {
                    rules.push(format!("{tag}:fallback"));
                    if policy::is_new_provider(reg, m) {
                        rules.push("provider:auto".into());
                    }
                    ModelPick {
                        model: m.clone(),
                        no_route: false,
                        replaced: Some(from.to_string()),
                    }
                }
                None if own.contains(&from) => {
                    rules.push(format!("{tag}:degraded_kept"));
                    keep(from)
                }
                None => {
                    rules.push("model:no_route".into());
                    ModelPick {
                        model: from.to_string(),
                        no_route: true,
                        replaced: None,
                    }
                }
            }
        };
        if let Some(ep) = input.episode_model.map(str::trim).filter(|m| !m.is_empty()) {
            if sound(ep) && !e2 {
                // Mid-flow: break sticky when attention says the ask needs a stronger model.
                let mut ranked = attention::rank_models(
                    attn.kind,
                    input.lane(),
                    &preferred,
                    table,
                    reg,
                    current,
                    input.ctx_tokens,
                );
                xai_first(&mut ranked);
                // Attention already ranked this turn when it speaks up:
                // the scorer then filters only, never re-weighs it.
                let ranked = score::order(
                    ranked,
                    reg,
                    table,
                    &score_ctx,
                    attn.kind == attention::Attention::Stay,
                    rules,
                );
                let stronger = attn.kind.breaks_episode_sticky()
                    && ranked.first().is_some_and(|m| m.as_str() != ep);
                if !stronger {
                    if let Some(turns) = input.pin_turns {
                        // D12: a long chat moves to a cheaper model only when
                        // the saving over its rest beats the uncached re-send.
                        let price = |m: &str| reg.get(m).map(|r| &r.meta.prices);
                        let down = ranked.first().filter(|m| {
                            m.as_str() != ep
                                && sound(m)
                                && price(ep)
                                    .zip(price(m))
                                    .and_then(|(old, new)| {
                                        policy::pin_switch(
                                            old,
                                            new,
                                            input.lane(),
                                            input.ctx_tokens,
                                            turns,
                                        )
                                    })
                                    .is_some_and(|s| s.pays())
                        });
                        if let Some(m) = down {
                            rules.push("pin:downgrade".into());
                            return keep(m);
                        }
                        rules.push("pin:keep".into());
                    }
                    rules.push("episode:keep".into());
                    return keep(ep);
                }
                if input.pin_turns.is_some() {
                    // The turn needs a stronger model: the pin never holds it back.
                    rules.push("pin:upgrade".into());
                }
                rules.push(format!("{}:mid_flow", attn.rule));
            } else if !e2 {
                rules.push("episode:unhealthy".into());
                return stand_in(ep, rules, "episode");
            }
        }
        if stronger {
            // E15: the strongest xAI model above the episode's, then the
            // strongest provider model you set up. Nothing stronger keeps it.
            let away = input.episode_model.unwrap_or(current).trim();
            let floor = attention::reasoning_of(reg, away);
            let strongest = |theirs: bool, above: u8| {
                pool.iter()
                    .copied()
                    .filter(|m| *m != away && policy::is_new_provider(reg, m) == theirs)
                    .filter(|m| {
                        !theirs
                            || !providers::is_openrouter_router(
                                split_provider_model(m).map_or("", |(_, id)| id),
                            )
                    })
                    .filter(|m| attention::reasoning_of(reg, m) > above)
                    .max_by(|a, b| {
                        attention::reasoning_of(reg, a)
                            .cmp(&attention::reasoning_of(reg, b))
                            .then(b.cmp(a))
                    })
            };
            let next = if policy::is_new_provider(reg, away) {
                strongest(true, floor)
            } else {
                strongest(false, floor).or_else(|| strongest(true, 0))
            };
            return match next {
                Some(m) => {
                    rules.push("model:stronger".into());
                    if input.pin_turns.is_some() {
                        rules.push("pin:upgrade".into());
                    }
                    if policy::is_new_provider(reg, m) {
                        rules.push("provider:auto".into());
                    }
                    ModelPick {
                        model: m.to_string(),
                        no_route: false,
                        replaced: None,
                    }
                }
                None => {
                    rules.push("model:strongest_kept".into());
                    keep(away)
                }
            };
        }
        if !gb
            && reg.entitlement.credential == Credential::ApiKey
            && table.rows(input.lane()).is_empty()
            && ids.contains(&current)
            && !policy::is_new_provider(reg, current)
            && !e2
            && !attn.kind.breaks_episode_sticky()
        {
            // A key has no evals (they spend only the plan pool), so with no table
            // row the call keeps its own included model instead of the cheapest.
            rules.push("key:keep".into());
            return keep(current);
        }
        let mut ranked = attention::rank_models(
            attn.kind,
            input.lane(),
            &preferred,
            table,
            reg,
            current,
            input.ctx_tokens,
        );
        xai_first(&mut ranked);
        let mut ranked = score::order(
            ranked,
            reg,
            table,
            &score_ctx,
            attn.kind == attention::Attention::Stay,
            rules,
        );
        long_context_first(&mut ranked, reg, input.lane(), input.ctx_tokens, rules);
        if e2 {
            let away = input.episode_model.unwrap_or(current).trim().to_string();
            ranked.retain(|m| *m != away);
            rules.push("model:next".into());
        }
        // Log attention only when it moves off the quiet Stay baseline.
        if attn.kind != attention::Attention::Stay {
            rules.push(attn.rule.into());
        }
        match ranked.first() {
            Some(m) => {
                let row = table.row(input.lane(), m).is_some();
                rules.push(if row { "table" } else { "model:cheapest" }.into());
                if !row && policy::is_new_provider(reg, m) {
                    // A provider model you set up, with no xAI model to take the step.
                    rules.push("provider:auto".into());
                }
                keep(m)
            }
            None if e2 => keep(input.episode_model.unwrap_or(current)),
            None => stand_in(current, rules, "model"),
        }
    }

    pub fn choose(
        input: &RouteInput<'_>,
        reg: &Registry,
        profiles: &BTreeMap<String, ModelProfile>,
        table: &RoutingTable,
        now_ms: u64,
    ) -> Route {
        connectivity::sync_from_core();
        let mut rules: Vec<String> = Vec::new();
        let provider = if input.provider.is_empty() {
            PROVIDER_XAI
        } else {
            input.provider
        };
        let fit = policy::Fit {
            needs_image: input.needs_image,
            needs_tools: input.needs_tools,
            ctx_tokens: input.ctx_tokens,
            grok_build: provider == live::PROVIDER_GROK_BUILD,
        };
        let tier = table::SpeedQuality::of(input.lane());
        let attn = attention::decide(&attention::AttentionInput {
            class: input.lane(),
            text: input.text,
            d: input.d,
            needs_tools: input.needs_tools,
            ctx_tokens: input.ctx_tokens,
            hard_tool: input.local.hard_tool,
            steer: input.steer,
            escalate_bias: input.escalate_bias,
            prior_escalated: input.prior_escalated,
        });
        // A local route only while the flag is on, for an eligible class, on the live tier.
        // Auto's quality pool drops non-reasoning ones in [`Self::pick_model`].
        // A chat the device answers is trimmed to its window on the way, so
        // the local tier's context length is no bar for it.
        let fit_for = |id: &str| {
            if local::is_local(id) && local::fits_by_trimming(input.class) {
                policy::Fit {
                    ctx_tokens: 0,
                    ..fit
                }
            } else {
                fit
            }
        };
        let ids: Vec<&str> = reg
            .models
            .keys()
            .map(String::as_str)
            .filter(|id| {
                !local::is_local(id)
                    || local::admits(&input.local, id, input.class, input.ctx_tokens)
            })
            .filter(|id| policy::fits(reg, profiles, id, fit_for(id), &input.spend, now_ms))
            .collect();
        // Failover PR 5: a long task never spends itself on a free provider
        // model while a route that can carry it exists — a paid route (xAI,
        // the plan, or a paid provider) or the device through its own
        // branches below, paid first. Free models stay when they are the
        // only cloud there is: a working free route beats a dead stop, and
        // the on-device fallback below is untouched either way.
        let ids: Vec<&str> = if input.size.is_some_and(|s| s.size == sizing::TaskSize::Long) {
            rules.push("size:long".into());
            let free_provider =
                |id: &str| policy::is_new_provider(reg, id) && providers::is_free_model(id);
            if ids
                .iter()
                .any(|id| !local::is_local(id) && !free_provider(id))
            {
                let kept: Vec<&str> = ids
                    .iter()
                    .copied()
                    .filter(|id| !free_provider(id))
                    .collect();
                if kept.len() != ids.len() {
                    rules.push("size:long:no_free".into());
                }
                kept
            } else {
                ids
            }
        } else {
            ids
        };
        let row = policy::class_row(input.lane());
        let band = row.map(|r| ladder::Band::tuned(r, input.bump, input.start));
        let mut pick = match (row, band) {
            (Some(row), Some(band)) => Some(input.pick.clone().unwrap_or_else(|| {
                let user = ladder::user_facing(row.class);
                let hold = ladder::hold_chat_drops(row.class, input.care_first);
                let start = goal::goal_start(
                    band,
                    ladder::start_holding(band, input.d, input.steer, user, hold),
                    input.steer,
                    input.goal.as_ref().filter(|_| user),
                );
                ladder::advance_holding(
                    None,
                    row.class,
                    band,
                    start,
                    &ladder::Obs {
                        new_turn: true,
                        routine: input.routine,
                        ..ladder::Obs::default()
                    },
                    hold,
                )
                .1
            })),
            _ => None,
        };
        // Escalate (not ForceTop — those classes already start high) may raise
        // effort one rung toward High; xhigh stays for steer/E2, not attention alone.
        if let (Some(band), Some(p)) = (band, pick.as_mut()) {
            if attn.kind == attention::Attention::Escalate && p.rung < band.ceiling {
                let high = grokhub_core::model_registry::EFFORT_LADDER
                    .iter()
                    .position(|e| *e == "high")
                    .unwrap_or(band.ceiling);
                let target = (p.rung + 1).min(band.ceiling).min(high);
                if target > p.rung {
                    p.rung = target;
                    p.moved = ladder::Move::Up;
                    p.rules.push(attn.rule.into());
                }
            }
        }
        let e2 = pick.as_ref().is_some_and(|p| {
            p.rules
                .iter()
                .any(|r| r == ladder::NEXT_MODEL || r == STEP_UP_RULE)
        });
        // E15: a second failure at a rung that can't climb moves to a stronger model.
        let stronger = pick
            .as_ref()
            .is_some_and(|p| p.rules.iter().any(|r| r == ladder::NEXT_MODEL));
        let local_paused = |rules: &mut Vec<String>| {
            rules.push("local:paused".into());
            ModelPick {
                model: input.current_model.trim().to_string(),
                no_route: true,
                replaced: None,
            }
        };
        let gov_paused = input.local.capacity.paused();
        let cloud_ids: Vec<&str> = ids
            .iter()
            .copied()
            .filter(|id| !local::is_local(id))
            .collect();
        // No sign-in or key, and the pick is not a provider you added with its
        // own key and grant: no cloud route can answer this step.
        let no_cloud_route = |rules: &[String], pick: &ModelPick| {
            rules.iter().any(|r| r == "cost:not_included")
                && !(cloud_ids.contains(&pick.model.as_str())
                    && policy::is_new_provider(reg, &pick.model))
        };
        let too_hard = local::too_hard_for_local(input.class, input.ctx_tokens, input.d);
        let elig = local::eligible(&input.local, input.class, input.ctx_tokens);
        // With no cloud model left, the device takes what it would take offline,
        // never hard or long work.
        let as_offline = local::LocalGate {
            offline: true,
            ..input.local.clone()
        };
        let any_elig = elig || local::eligible(&as_offline, input.class, input.ctx_tokens);
        let last_resort = !too_hard && any_elig;
        // No sign-in or key at all: the device is the only model there is, so it
        // takes hard work too while the step fits its context (the gate still
        // keeps prepare:hard, cloud-only and hard-tool steps off it).
        let no_keys_ok = any_elig;
        let local_id = reg.models.keys().map(String::as_str).find(|id| {
            local::admits(&as_offline, id, input.class, input.ctx_tokens)
                && policy::fits(reg, profiles, id, fit_for(id), &input.spend, now_ms)
        });
        // Failover PR 5, the warm-local rule (corrected 2026-11-07): a model
        // already loaded on the device takes a less challenging turn by
        // default — no cloud spend, no cold start — unless the turn is
        // pinned to an episode model or carries an explicit pick. The gate
        // admitted the tier into `ids` already, so class eligibility and
        // the governor's say are spent (at Unload nothing is admitted);
        // sizing adds only the size line and, under throttle, tiny only.
        // A cloud candidate must exist for the default to be a choice at
        // all: with no cloud route the device is a fallback, and the
        // fallback paths below own the pick (and its cabin note).
        let throttled = matches!(
            input.local.capacity.level,
            grokhub_core::local_governor::Level::Throttle
        );
        let warm_local = if input.local_warm
            && input.episode_model.is_none()
            && input.pick.is_none()
            && ids.iter().any(|id| !local::is_local(id))
            && sizing::warm_local_default(input.size.map(|s| s.size), throttled)
        {
            ids.iter().find(|id| local::is_local(id)).copied()
        } else {
            None
        };
        // Failover PR 6: an explicit (18+) call's only candidate is the
        // device — the hard filter at the pick itself, ahead of the
        // warm-local default and every cloud branch.
        let adult_local = if input.adult {
            ids.iter().find(|id| local::is_local(id)).copied()
        } else {
            None
        };
        let chosen = if gov_paused && input.local.sensitive && !input.local.cloud_grant {
            // Sensitive data stays on the device, and the device is paused.
            local_paused(&mut rules)
        } else if gov_paused
            && input.local.offline
            && local::eligible_if_running(&input.local, input.class, input.ctx_tokens)
        {
            local_paused(&mut rules)
        } else if local::privacy_only(&input.local) {
            // Sensitive data and no cloud grant: the device or nothing.
            match ids.iter().find(|id| local::is_local(id)) {
                Some(id) => {
                    rules.push("privacy:local_only".into());
                    ModelPick {
                        model: id.to_string(),
                        no_route: false,
                        replaced: None,
                    }
                }
                None => {
                    rules.push("privacy:no_local".into());
                    ModelPick {
                        model: input.current_model.trim().to_string(),
                        no_route: true,
                        replaced: None,
                    }
                }
            }
        } else if connectivity::is_cloud_only(input.class) && input.local.offline {
            // Image / web / hard prepare wait for the cloud (named queue note in the cabin).
            rules.push("offline:cloud_queued".into());
            ModelPick {
                model: input.current_model.trim().to_string(),
                no_route: true,
                replaced: None,
            }
        } else if local::offline_unavailable(&input.local, input.class, input.ctx_tokens) {
            // Offline and no on-device runtime: wait for the network, say why.
            rules.push("offline:local_unavailable".into());
            ModelPick {
                model: input.current_model.trim().to_string(),
                no_route: true,
                replaced: None,
            }
        } else if local::offline_only(&input.local)
            && local::eligible(&input.local, input.class, input.ctx_tokens)
        {
            match ids.iter().find(|id| local::is_local(id)) {
                Some(id) => {
                    rules.push("offline:local".into());
                    ModelPick {
                        model: id.to_string(),
                        no_route: false,
                        replaced: None,
                    }
                }
                None => {
                    rules.push("offline:no_local".into());
                    ModelPick {
                        model: input.current_model.trim().to_string(),
                        no_route: true,
                        replaced: None,
                    }
                }
            }
        } else if local::escalate_after_verify(&input.local) {
            // PR E: on-device answer failed verify — pick cloud and name both models.
            let local_tier = input.local.tier.clone();
            let mut pick = Self::pick_model(
                input, reg, profiles, &cloud_ids, table, e2, stronger, now_ms, &mut rules,
            );
            let no_keys = no_cloud_route(&rules, &pick);
            if !pick.no_route && !local::is_local(&pick.model) && !no_keys {
                rules.push("local:escalate_verify".into());
                pick.replaced = Some(local_tier);
                local::note_local_verify_failed(false);
            } else if no_keys {
                // No sign-in or key: there is no cloud model to escalate to, so
                // the device keeps the step when it may take it.
                if let Some(id) = local_id.filter(|_| no_keys_ok) {
                    rules.push("local:no_keys".into());
                    pick = ModelPick {
                        model: id.to_string(),
                        no_route: false,
                        replaced: Some(input.current_model.trim().to_string()),
                    };
                    local::note_local_verify_failed(false);
                }
            } else if pick.no_route && last_resort {
                // Every plan model down: fall back to local and clear the sticky
                // flag, never for hard or long work.
                if let Some(id) = ids.iter().find(|id| local::is_local(id)) {
                    rules.push("local:cloud_down".into());
                    pick = ModelPick {
                        model: id.to_string(),
                        no_route: false,
                        replaced: Some(input.current_model.trim().to_string()),
                    };
                    local::note_local_verify_failed(false);
                }
            }
            pick
        } else if let Some(id) = adult_local {
            // The 18+ hard filter: an explicit call runs on the device,
            // full stop — never OpenRouter, never Claude, never xAI, no
            // score argues with it. The device has to be a candidate for
            // the step; when it is not (the runtime off, the governor's
            // Unload), the pick falls through and the send path's own
            // guard still forces the turn local or answers with the
            // notice, so no cloud model ever sees it either way.
            rules.push("adult:local_only".into());
            ModelPick {
                model: id.to_string(),
                no_route: false,
                replaced: None,
            }
        } else if let Some(id) = warm_local {
            // The warm-local default (above): the loaded device model takes
            // this less challenging turn. The one place the device wins over
            // a healthy cloud on purpose — because it is already loaded, not
            // because starting it would be cheaper.
            rules.push("size:warm_local".into());
            ModelPick {
                model: id.to_string(),
                no_route: false,
                replaced: None,
            }
        } else {
            // Every cloud model is down, not just left out for its price or this
            // step's needs (those keep the premium ask and the stand-in).
            let cloud_down = !cloud_can_answer(reg, profiles, fit.grok_build, &input.spend, now_ms);
            // Online, the cloud always wins while any cloud model can take the step:
            // the device is the last resort, never a cheaper pick.
            let pool = if cloud_ids.is_empty() && cloud_down {
                &ids
            } else {
                &cloud_ids
            };
            let mut pick = Self::pick_model(
                input, reg, profiles, pool, table, e2, stronger, now_ms, &mut rules,
            );
            // No sign-in or key and no provider you added took it: no cloud model can answer.
            let no_keys = no_cloud_route(&rules, &pick);
            // Cloud pool empty + local already won: retag as cloud_down.
            let cloud_pool_empty_local = !input.local.offline
                && !pick.no_route
                && local::is_local(&pick.model)
                && cloud_ids.is_empty()
                && elig;
            // Online: hard/long work must not stay on local if pick_model somehow chose it.
            if !input.local.offline && !pick.no_route && local::is_local(&pick.model) && too_hard {
                rules.push("local:escalate_hard".into());
                let from = pick.model.clone();
                pick = Self::pick_model(
                    input, reg, profiles, &cloud_ids, table, e2, stronger, now_ms, &mut rules,
                );
                if !pick.no_route {
                    pick.replaced = Some(from);
                }
            } else if (pick.no_route && cloud_down && last_resort)
                || (no_keys && no_keys_ok)
                || cloud_pool_empty_local
            {
                // Plan models down / no healthy cloud route / no keys: fall back to local.
                if let Some(id) = local_id {
                    rules.push(
                        if no_keys {
                            "local:no_keys"
                        } else {
                            "local:cloud_down"
                        }
                        .into(),
                    );
                    pick = ModelPick {
                        model: id.to_string(),
                        no_route: false,
                        replaced: Some(input.current_model.trim().to_string()),
                    };
                }
            } else if pick.no_route
                && gov_paused
                && local::eligible_if_running(&input.local, input.class, input.ctx_tokens)
            {
                // The cloud is down and the device is paused: say which.
                rules.push("local:paused".into());
            }
            pick
        };
        // Offline only (LU-13): whatever branch picked a cloud model (verify
        // escalate, no keys, cloud down, hard work), nothing goes to the cloud.
        let chosen =
            if input.local.offline_switch && !chosen.no_route && !local::is_local(&chosen.model) {
                rules.push("offline:switch".into());
                ModelPick {
                    model: input.current_model.trim().to_string(),
                    no_route: true,
                    replaced: None,
                }
            } else {
                chosen
            };
        let provider = match split_provider_model(&chosen.model) {
            _ if local::is_local(&chosen.model) && !chosen.no_route => local::PROVIDER_LOCAL,
            Some((p, _)) if !chosen.no_route && policy::is_new_provider(reg, &chosen.model) => p,
            _ => provider,
        };
        let gb = provider == live::PROVIDER_GROK_BUILD;
        let premium_ask = rules
            .iter()
            .any(|r| r == "cost:premium_ungranted")
            .then(|| route_key(input.current_model, RouteOpts::default()));
        // The Fast variant goes only under the latency policy, and only on a
        // Grok Build sign-in (or Grok Build's own path): Grok 4.7 Fast isn't on
        // the public API, so a plain API key never gets it.
        let fast_signin = gb || reg.entitlement.credential == Credential::Plan;
        let fast = (fast_signin && !chosen.no_route && ids.contains(&chosen.model.as_str()))
            .then(|| fast_of(reg, &chosen.model))
            .flatten()
            .filter(|f| {
                fast_entitled(reg, f) && policy::fits_any_cost(reg, profiles, f, fit, now_ms)
            })
            .map(str::to_string);
        let mut fast_fallback = None;
        let (model, base) = match (
            &fast,
            fast.as_ref().map(|_| spend::fast_verdict(&input.spend)),
        ) {
            (Some(f), Some(Ok(rule))) => {
                rules.push(rule.into());
                (f.clone(), Some(chosen.model.clone()))
            }
            (Some(f), Some(Err(rule))) => {
                rules.push(rule.into());
                if rule == "fast:budget_tight" {
                    fast_fallback = Some(f.clone());
                }
                (chosen.model.clone(), None)
            }
            _ => (chosen.model.clone(), None),
        };
        if row.is_some_and(|r| r.start == "high")
            && reg.entitlement.credential == Credential::Plan
            && policy::outranked_by_plan_upgrade(reg, profiles, &model, fit)
        {
            // A newer model outside your plan would have taken this: the upgrade nudge counts it.
            rules.push("plan:would_pick".into());
        }
        let effort_rules_at = rules.len();
        // The model that is actually sent, and the effort list its listing names.
        let sent_model = if policy::MODEL_LIVE {
            model.as_str()
        } else {
            input.current_model.trim()
        };
        let menu = profiles
            .get(sent_model)
            .and_then(|p| p.metadata.efforts.clone())
            .or_else(|| reg.get(sent_model).and_then(|r| r.meta.efforts.clone()))
            .filter(|_| !sent_model.to_ascii_lowercase().contains("non-reasoning"));
        // xAI answers 400 to an effort on a model that doesn't take one (2.10.97).
        let takes_effort = menu.is_some() || grokhub_core::accepts_reasoning_effort(sent_model);
        let mut effort: Option<String> = match (row, band, pick) {
            (Some(row), Some(band), Some(pick)) => {
                rules.push(format!("class:{}", row.class));
                if input.bump > 0 {
                    rules.push(format!("guard:+{}", input.bump));
                }
                rules.extend(pick.rules.iter().cloned());
                let rung_name = EFFORT_LADDER[pick.rung];
                let picked = match menu {
                    Some(menu) => {
                        let c = ladder::clamp(pick.rung, &menu, pick.moved, band.floor);
                        if c.as_deref() != Some(rung_name) {
                            rules.push("clamp".into());
                        }
                        c
                    }
                    // A provider you added sends only an effort its own listing names.
                    None if policy::is_new_provider(reg, sent_model) => None,
                    // With no listing, an xAI reasoning model takes low and up:
                    // "none" exists only where a listing names it.
                    None if takes_effort => {
                        Some(EFFORT_LADDER[pick.rung.max(LOW_RUNG)].to_string())
                    }
                    None => None,
                };
                // `minimal` and `max` are not sent: they go out as low and xhigh.
                picked.map(|e| {
                    grokhub_core::parse_reasoning_effort(&e)
                        .map(str::to_string)
                        .unwrap_or(e)
                })
            }
            _ => {
                rules.push("class:unlisted".into());
                // A provider model keeps its own adapter's rules; an xAI one only if it takes an effort.
                input
                    .current_effort
                    .filter(|_| takes_effort || policy::is_new_provider(reg, sent_model))
                    .map(str::to_string)
            }
        };
        if row.is_none() {
            effort = effort.map(|e| {
                grokhub_core::parse_reasoning_effort(&e)
                    .map(str::to_string)
                    .unwrap_or(e)
            });
        }
        if is_multi_agent(sent_model) {
            effort = multi_agent_effort(effort, input.lane(), sent_model, &input.spend, &mut rules);
        }
        let what = row.map(|r| r.plain).unwrap_or("this step");
        let clause = if row.is_some() {
            why_clause(what, &rules[effort_rules_at..])
        } else {
            format!("{what} keeps its set effort")
        };
        // One-line pick reason: tier, effort, why (+ attention when off baseline).
        let tier_word = tier.word();
        // Steer/ladder already names "think hard"; suffix is for host/risk/force-top/learned.
        let attn_suffix = if attn.kind != attention::Attention::Stay
            && attn.rule != "attn:escalate:steer"
            && rules.iter().any(|r| r.starts_with("attn:"))
        {
            format!(" ({})", attn.why)
        } else {
            String::new()
        };
        let reason = match (&chosen.replaced, rules.iter().any(|r| r == "table")) {
            _ if rules.iter().any(|r| r == "local:paused") => local::LOCAL_PAUSED_MSG.to_string(),
            _ if base.is_some() => format!(
                "{tier_word} · {}: you're waiting, so the faster model is worth it.",
                effort_word(effort.as_deref())
            ),
            _ if fast_fallback.is_some() => format!(
                "{tier_word} · {}: this week's budget is tight, so not {}.",
                effort_word(effort.as_deref()),
                fast_fallback.as_deref().unwrap_or_default()
            ),
            (Some(from), _) if rules.iter().any(|r| r == "local:escalate_verify") => {
                local::escalate_note(from, &model)
            }
            (Some(from), _) if rules.iter().any(|r| r == "local:escalate_hard") => {
                format!(
                    "{tier_word} · {}: {from} (local) is too small for this step.",
                    effort_word(effort.as_deref())
                )
            }
            (Some(from), _) if rules.iter().any(|r| r == "local:no_keys") => {
                format!(
                    "{tier_word} · {}: {from} needs a sign-in or key (using local).",
                    effort_word(effort.as_deref())
                )
            }
            (Some(from), _) if rules.iter().any(|r| r == "local:cloud_down") => {
                format!(
                    "{tier_word} · {}: {from} isn't answering (using local).",
                    effort_word(effort.as_deref())
                )
            }
            (Some(from), _) if rules.iter().any(|r| r == "provider:ungranted") => {
                format!(
                    "{tier_word} · {}: {from} needs its key and your OK in Settings first.",
                    effort_word(effort.as_deref())
                )
            }
            (Some(from), _) if rules.iter().any(|r| r == "cost:premium_ungranted") => {
                format!(
                    "{tier_word} · {}: {from} costs more than your price limit, so it needs your OK first.",
                    effort_word(effort.as_deref())
                )
            }
            (Some(from), _) => format!(
                "{tier_word} · {}: {from} isn't answering.",
                effort_word(effort.as_deref())
            ),
            (None, true)
                if model != input.current_model.trim() || input.episode_model.is_none() =>
            {
                let why = table
                    .row(input.lane(), &model)
                    .map(|r| r.why.clone())
                    .unwrap_or_default();
                format!(
                    "{tier_word} · {}: {why}.{attn_suffix}",
                    effort_word(effort.as_deref())
                )
            }
            _ => format!(
                "{tier_word} · {}: {clause}.{attn_suffix}",
                effort_word(effort.as_deref())
            ),
        };
        let settings = profiles
            .get(&model)
            .map(|p| RuntimeSettings::from_profile(p, input.class));
        let cost_class = policy::route_class(reg, &model, gb, &input.spend);
        Route {
            provider: provider.to_string(),
            model,
            effort,
            reason,
            rule_ids: rules,
            candidates_n: ids.len() as u32,
            settings,
            no_route: chosen.no_route,
            replaced: chosen.replaced,
            cost_class,
            base,
            premium_ask,
        }
    }
}

#[cfg(test)]
mod ask_eval;
#[cfg(test)]
mod auto_tests;
#[cfg(test)]
mod knn_tests;

#[cfg(test)]
mod r2a_tests;

#[cfg(test)]
mod r2b_tests;

#[cfg(test)]
mod r3b_tests;

#[cfg(test)]
mod sizing_tests;

#[cfg(test)]
mod score_tests;

#[cfg(test)]
mod goal_tests;

#[cfg(test)]
mod speed_bench_tests;

#[cfg(test)]
mod local_router_tests;

#[cfg(test)]
mod facts_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{ContentPart, StreamEvent};
    use std::sync::Mutex;

    #[test]
    fn routing_reads_what_was_typed_not_the_cabin_notes() {
        let stored = crate::prefix_cache::user_message_text(
            "Playbook for this turn: Code changes. Reproduce first, run the tests.",
            "hi",
        );
        let input = vec![InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText(stored)],
        }];
        assert_eq!(live::last_user_text(&input), ("hi".to_string(), false));
    }

    struct Echo(Mutex<Vec<ResponsesRequest>>);

    impl ModelClient for Echo {
        fn stream(
            &self,
            req: &ResponsesRequest,
            _cancel: &CancelToken,
            _sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            self.0.lock().unwrap().push(req.clone());
            Ok(TurnOutput {
                text: "VERIFY_OK".into(),
                reasoning: String::new(),
                calls: Vec::new(),
                usage: Usage {
                    input_tokens: 900,
                    output_tokens: 7,
                    reasoning_tokens: 3,
                    cost_in_usd_ticks: 1_200,
                    cached_tokens: 640,
                },
                ..Default::default()
            })
        }
    }

    #[test]
    fn call_model_sends_model_and_effort_and_logs_every_token_kind() {
        let echo = Echo(Mutex::new(Vec::new()));
        let input = vec![InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText("hi".into())],
        }];
        let call = ModelCall::xai(
            "grok-4.7",
            Some(BACKGROUND_EFFORT),
            CLASS_JUDGE,
            "ep-1",
            input,
        );
        let routed = call_model(&echo, &call, &CancelToken::new()).unwrap();
        assert_eq!(routed.out.text, "VERIFY_OK");
        assert_eq!(
            routed.tokens,
            CallTokens {
                class: "background:judge".into(),
                model: "grok-4.7".into(),
                effort: "low".into(),
                input: 900,
                cached: 640,
                out: 7,
                reasoning: 3,
                cost_ticks: 1_200,
            }
        );
        let sent = echo.0.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(
            (sent[0].model.as_str(), sent[0].effort.as_deref()),
            ("grok-4.7", Some("low"))
        );
        assert!(!sent[0].hosted_search);
        let line = serde_json::to_string(&routed.tokens).unwrap();
        assert_eq!(
            line,
            r#"{"class":"background:judge","model":"grok-4.7","effort":"low","in":900,"cached":640,"out":7,"reasoning":3,"cost_ticks":1200}"#
        );
    }

    use grokhub_core::model_registry::profile::ProbeResult;
    use grokhub_core::model_registry::{
        CallStatus, Listing, ModelMeta, Observation, Prices, RegistryEvent, SourceKind,
    };

    fn listed(ids: &[&str]) -> Registry {
        let rows = ids
            .iter()
            .map(|id| ModelMeta {
                id: id.to_string(),
                context_length: Some(256_000),
                prices: Prices {
                    prompt: Some(1),
                    cached: Some(1),
                    completion: Some(1),
                    ..Prices::default()
                },
                efforts: Some(vec!["low".into(), "medium".into(), "high".into()]),
                ..ModelMeta::default()
            })
            .collect();
        let mut reg = Registry::default();
        reg.entitlement.credential = Credential::Plan;
        reg.apply_refresh(&[Listing::new(SourceKind::XaiApi, rows)], &[], 1);
        reg
    }

    fn no_table() -> RoutingTable {
        RoutingTable::default()
    }

    fn input<'a>(class: &'a str, model: &'a str, d: f64) -> RouteInput<'a> {
        RouteInput {
            provider: "xai",
            class,
            current_model: model,
            current_effort: Some("high"),
            d,
            ..RouteInput::default()
        }
    }

    #[test]
    fn choose_is_deterministic_and_follows_the_class_table() {
        let reg = listed(&["grok-4.5", "grok-4.7"]);
        let none = BTreeMap::new();
        let a = Router::choose(
            &input("chat:default", "grok-4.7", 0.5),
            &reg,
            &none,
            &no_table(),
            10,
        );
        assert_eq!(
            a,
            Router::choose(
                &input("chat:default", "grok-4.7", 0.5),
                &reg,
                &none,
                &no_table(),
                10
            )
        );
        // Workhorse baseline: prefer the non-flagship reasoner (4.5) over 4.7.
        assert_eq!(
            (a.model.as_str(), a.effort.as_deref()),
            ("grok-4.5", Some("medium"))
        );
        assert_eq!(a.reason, "balanced · Medium: everyday chat.");
        assert_eq!(
            a.rule_ids,
            vec!["model:cheapest".to_string(), "class:chat:default".into()]
        );
        assert_eq!(a.candidates_n, 2);
        let easy = Router::choose(
            &input("chat:default", "grok-4.7", 0.1),
            &reg,
            &none,
            &no_table(),
            10,
        );
        assert_eq!(
            (easy.effort.as_deref(), easy.reason.as_str()),
            (
                Some("low"),
                "balanced · Low: everyday chat, and this looks routine."
            )
        );
        // d > 0.7 goes up one rung but never past high on its own; the menu clamps xhigh anyway.
        let hard = Router::choose(
            &input("plan", "grok-4.7", 0.9),
            &reg,
            &none,
            &no_table(),
            10,
        );
        assert_eq!(hard.effort.as_deref(), Some("high"));
        // prepare:hard never starts below high.
        let prep = Router::choose(
            &input("prepare:hard", "grok-4.7", 0.0),
            &reg,
            &none,
            &no_table(),
            10,
        );
        assert_eq!(prep.effort.as_deref(), Some("high"));
        // An unlisted class keeps today's effort.
        let eval = Router::choose(
            &input("eval:item", "grok-4.7", 0.0),
            &reg,
            &none,
            &no_table(),
            10,
        );
        assert_eq!(
            (
                eval.effort.as_deref(),
                eval.rule_ids.last().map(String::as_str)
            ),
            (Some("high"), Some("class:unlisted"))
        );
    }

    #[test]
    fn choose_skips_ghosts_and_unusable_profiles() {
        let mut reg = listed(&["grok-4.5", "grok-4.7"]);
        let none = BTreeMap::new();
        reg.models.get_mut("grok-4.5").unwrap().state = ModelState::Ghost;
        let r = Router::choose(
            &input("chat:default", "grok-4.5", 0.5),
            &reg,
            &none,
            &no_table(),
            10,
        );
        assert_eq!(
            (r.model.as_str(), r.rule_ids[0].as_str(), r.candidates_n),
            ("grok-4.7", "model:cheapest", 1)
        );
        let mut profiles = BTreeMap::new();
        let meta = reg.get("grok-4.7").unwrap().meta.clone();
        let mut bad = ProbeResult::status("failed");
        bad.failure = Some("JSON mode broke.".into());
        profiles.insert("grok-4.7".to_string(), ModelProfile::build(&meta, bad, 1));
        let none_left = Router::choose(
            &input("chat:default", "grok-4.7", 0.5),
            &reg,
            &profiles,
            &no_table(),
            10,
        );
        assert_eq!(
            (
                none_left.candidates_n,
                none_left.rule_ids[0].as_str(),
                none_left.no_route
            ),
            (0, "model:no_route", true)
        );
        // Queued is not usable yet either.
        profiles.insert(
            "grok-4.7".to_string(),
            ModelProfile::build(&meta, ProbeResult::status("queued"), 1),
        );
        assert_eq!(
            Router::choose(
                &input("chat:default", "grok-4.7", 0.5),
                &reg,
                &profiles,
                &no_table(),
                10
            )
            .candidates_n,
            0
        );
        // An empty registry keeps today's model.
        let empty = Router::choose(
            &input("chat:default", "grok-4.7", 0.5),
            &Registry::default(),
            &none,
            &no_table(),
            10,
        );
        assert_eq!(
            (empty.model.as_str(), empty.rule_ids[0].as_str()),
            ("grok-4.7", "registry:empty")
        );
    }

    fn req(model: &str, effort: Option<&str>, session: &str, text: &str) -> ResponsesRequest {
        ResponsesRequest {
            text_format: None,
            model: model.into(),
            effort: effort.map(str::to_string),
            input: vec![InputItem::Message {
                role: "user".into(),
                content: vec![ContentPart::InputText(text.into())],
            }],
            conversation_id: session.into(),
            tools: Vec::new(),
            hosted_search: true,
            call_timeout: None,
        }
    }

    fn sent_effort(echo: &Echo, r: &ResponsesRequest, class: &str) -> Option<String> {
        live::stream_routed(echo, r, &CancelToken::new(), &mut |_| {}, class).unwrap();
        echo.0.lock().unwrap().last().unwrap().effort.clone()
    }

    /// Golden: the internal call sites send the same effort they sent before R1,
    /// even on a long, hard-looking prompt, and Grok Build's spawn args are unchanged.
    #[test]
    fn internal_call_sites_send_the_same_effort_as_before() {
        let dir = crate::harness::test_dir("route-golden");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let echo = Echo(Mutex::new(Vec::new()));
        let hard = "Debug why this deadlocks, then refactor and prove it. ".repeat(60);
        let golden: &[(&str, &str, &str)] = &[
            (CLASS_COMPACT, "low", "low"),
            (CLASS_JUDGE, "low", "low"),
            ("background:dream", "low", "low"),
            ("background:memory", "low", "low"),
            (grokhub_core::self_review::SELF_REVIEW_CLASS, "low", "low"),
            ("background:review", "low", "low"),
            (crate::unattended::UNATTENDED_CLASS, "low", "low"),
            (crate::eval::EVAL_CLASS, "high", "high"),
        ];
        for (class, before, want) in golden {
            let r = req("grok-4.7", Some(before), "golden-session", &hard);
            assert_eq!(
                sent_effort(&echo, &r, class).as_deref(),
                Some(*want),
                "{class}"
            );
        }
        let body = r#"{"include":["reasoning.encrypted_content"],"input":[{"content":[{"text":"hi","type":"input_text"}],"role":"user","type":"message"}],"max_turns":3,"model":"grok-4.7","prompt_cache_key":"golden-session","reasoning":{"effort":"low"},"store":false,"stream":true,"tools":[{"type":"web_search"},{"type":"x_search"}]}"#;
        live::stream_routed(
            &echo,
            &req("grok-4.7", Some("low"), "golden-session", "hi"),
            &CancelToken::new(),
            &mut |_| {},
            CLASS_COMPACT,
        )
        .unwrap();
        assert_eq!(
            crate::client::responses_body(echo.0.lock().unwrap().last().unwrap()).to_string(),
            body
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A chat session id outside the 10% holdout.
    fn auto_session(tag: &str) -> String {
        (0..)
            .map(|i| format!("{tag}-{i}"))
            .find(|s| !guard::in_holdout(s))
            .unwrap()
    }

    /// The text of the newest user message the client was sent.
    fn sent_user_text(echo: &Echo) -> String {
        let g = echo.0.lock().unwrap();
        let r = g.last().unwrap();
        r.input
            .iter()
            .rev()
            .find_map(|i| match i {
                InputItem::Message { role, content } if role == "user" => {
                    content.iter().find_map(|c| match c {
                        ContentPart::InputText(t) => Some(t.clone()),
                        _ => None,
                    })
                }
                _ => None,
            })
            .unwrap_or_default()
    }

    #[test]
    fn model_and_effort_reach_the_chat_only_when_the_turn_asks() {
        let dir = crate::harness::test_dir("route-ask-note");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let echo = Echo(Mutex::new(Vec::new()));
        let s = auto_session("ask-note");
        // An ordinary turn carries no route note: the words go out as typed.
        live::stream_routed(
            &echo,
            &req("grok-4.7", None, &s, "tell me a joke about creepers"),
            &CancelToken::new(),
            &mut |_| {},
            "chat:default",
        )
        .unwrap();
        assert_eq!(sent_user_text(&echo), "tell me a joke about creepers");
        // Asked: the answering model learns what it really is and at what effort.
        live::stream_routed(
            &echo,
            &req(
                "grok-4.7",
                None,
                &s,
                "btw what model are you and what effort?",
            ),
            &CancelToken::new(),
            &mut |_| {},
            "chat:default",
        )
        .unwrap();
        let (model, effort) = {
            let g = echo.0.lock().unwrap();
            let r = g.last().unwrap();
            (r.model.clone(), r.effort.clone())
        };
        let text = sent_user_text(&echo);
        assert!(
            text.contains(&format!("This turn is answered by `{model}` by xAI")),
            "{text}"
        );
        let effort = effort.expect("a Grok reasoning model gets an effort");
        assert!(
            text.contains(&format!(
                "at {} reasoning effort",
                grokhub_core::effort_label(&effort)
            )),
            "{text}"
        );
        assert_eq!(
            crate::prefix_cache::visible_user_text(&text),
            "btw what model are you and what effort?"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn chat_effort_is_automatic_and_think_hard_lasts_one_episode() {
        let dir = crate::harness::test_dir("route-steer");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let echo = Echo(Mutex::new(Vec::new()));
        let s = auto_session("steer");
        // The call's own effort no longer matters for a listed class.
        assert_eq!(
            sent_effort(
                &echo,
                &req("grok-4.7", Some("xhigh"), &s, "hi"),
                "chat:default"
            )
            .as_deref(),
            Some("low")
        );
        let ask = "think hard on this rename";
        assert_eq!(
            sent_effort(
                &echo,
                &req("grok-4.7", Some("low"), &s, ask),
                "chat:default"
            )
            .as_deref(),
            Some("xhigh")
        );
        assert_eq!(
            sent_effort(
                &echo,
                &req("grok-4.7", Some("low"), &s, ask),
                "chat:default"
            )
            .as_deref(),
            Some("xhigh"),
            "a tool step in the same episode keeps it"
        );
        let next = "Explain how the parser works";
        assert_eq!(
            sent_effort(
                &echo,
                &req("grok-4.7", Some("low"), &s, next),
                "chat:default"
            )
            .as_deref(),
            Some("medium"),
            "the next episode is back at the class start"
        );
        // E1 through the live helper: a failed tool lifts the next step one rung.
        crate::route::ladder::note_tool_error();
        assert_eq!(
            sent_effort(
                &echo,
                &req("grok-4.7", Some("low"), &s, next),
                "chat:default"
            )
            .as_deref(),
            Some("high")
        );
        // E3: a correction keeps the chat's kind (an explanation starts at
        // medium) and starts one higher (medium → high).
        assert_eq!(
            sent_effort(
                &echo,
                &req("grok-4.7", None, &s, "no, wrong file"),
                "chat:default"
            )
            .as_deref(),
            Some("high")
        );
        let routes = crate::route::log::route_records(&dir, log::ROUTE_TAIL_LINES);
        let (_, newest) = routes.last().unwrap();
        assert_eq!(
            (newest.used.effort.as_deref(), newest.reason.as_str()),
            (
                Some("high"),
                "balanced · High: you corrected the last answer."
            )
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A3, A4, E14 and the redo words through the live helper: what the
    /// client is sent, step by step.
    #[test]
    fn stakes_loop_signals_and_a_spent_budget_move_the_sent_effort() {
        let dir = crate::harness::test_dir("route-stage");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let echo = Echo(Mutex::new(Vec::new()));
        let s = auto_session("stage");
        let send = |text: &str| {
            sent_effort(
                &echo,
                &req("grok-4.7", Some("low"), &s, text),
                "chat:default",
            )
        };
        let _ = ladder::take_stage(true);
        let _ = ladder::take_tool_errors();
        // A3: a force push is a destructive turn: Flagship lane, High.
        let push = "git push --force to main after the rebase";
        assert_eq!(send(push).as_deref(), Some("high"));
        let routes = crate::route::log::route_records(&dir, log::ROUTE_TAIL_LINES);
        let rules = &routes.last().unwrap().1.rule_ids;
        assert!(
            rules.contains(&"ask:stakes:destructive".to_string())
                && rules.contains(&"class:prepare:hard".to_string()),
            "{rules:?}"
        );
        // Reading tool output on a stakes turn never drops below its floor (High).
        ladder::note_tool_run("read_file", r#"{"path":"notes.md"}"#, false);
        assert_eq!(send(push).as_deref(), Some("high"));
        // A4 on an everyday turn: a read-only step thinks at low, for that call only.
        let next = "Explain how the parser works";
        assert_eq!(send(next).as_deref(), Some("medium"));
        ladder::note_tool_run("read_file", r#"{"path":"src/parse.rs"}"#, false);
        assert_eq!(send(next).as_deref(), Some("low"));
        assert_eq!(send(next).as_deref(), Some("medium"));
        // E14: reasoning that spent the budget with no answer climbs one rung.
        ladder::note_out_of_budget();
        assert_eq!(send(next).as_deref(), Some("high"));
        // A read while that escalation holds stays on it; the same call again
        // is a loop and one more rung.
        ladder::note_tool_run("grep", r#"{"pattern":"parse"}"#, false);
        assert_eq!(send(next).as_deref(), Some("high"));
        ladder::note_tool_run("grep", r#"{"pattern":"parse"}"#, false);
        assert_eq!(send(next).as_deref(), Some("xhigh"));
        // "try again" is a redo: the next answer starts one rung up (E3).
        assert_eq!(send("try again").as_deref(), Some("high"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn holdout_episodes_run_at_fixed_high_and_are_logged_but_never_named() {
        let dir = crate::harness::test_dir("route-holdout");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let echo = Echo(Mutex::new(Vec::new()));
        let s = (0..)
            .map(|i| format!("hold-{i}"))
            .find(|s| guard::in_holdout(s))
            .unwrap();
        assert_eq!(
            sent_effort(
                &echo,
                &req("grok-4.7", None, &s, "think hard about it"),
                "chat:default"
            )
            .as_deref(),
            Some("high")
        );
        let raw =
            std::fs::read_to_string(crate::harness::span_path(&dir, log::ROUTE_TRACE)).unwrap();
        let rec: crate::harness::Span = serde_json::from_str(raw.lines().last().unwrap()).unwrap();
        let route = rec.route.unwrap();
        assert!(route.holdout && !route.shadow);
        assert_eq!(route.reason, "balanced · High: everyday chat.");
        // prepare:hard is never held out.
        let hard = (0..)
            .map(|i| format!("hard-{i}"))
            .find(|s| guard::in_holdout(s))
            .unwrap();
        assert_eq!(
            sent_effort(
                &echo,
                &req("grok-4.7", None, &hard, "keep it quick"),
                "prepare:hard"
            )
            .as_deref(),
            Some("high")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn prepare_hard_never_goes_below_high_and_clamps_up_on_a_short_menu() {
        let mut reg = listed(&["grok-4.7"]);
        reg.models.get_mut("grok-4.7").unwrap().meta.efforts =
            Some(vec!["low".into(), "medium".into(), "xhigh".into()]);
        let none = BTreeMap::new();
        let mut i = input("prepare:hard", "grok-4.7", 0.0);
        i.steer = ladder::Steer::Quicker;
        let r = Router::choose(&i, &reg, &none, &no_table(), 10);
        assert_eq!(r.effort.as_deref(), Some("xhigh"), "{:?}", r.rule_ids);
        assert!(r.rule_ids.contains(&"clamp".to_string()));
        // A guard revert raises chat:default's start and floor by one.
        let mut g = input("chat:default", "grok-4.5", 0.5);
        g.bump = 1;
        let r = Router::choose(&g, &listed(&["grok-4.5"]), &none, &no_table(), 10);
        assert_eq!(r.effort.as_deref(), Some("high"));
        assert!(r.rule_ids.contains(&"guard:+1".to_string()));
    }

    #[test]
    fn route_records_hold_no_prompt_text() {
        let dir = crate::harness::test_dir("route-why");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let echo = Echo(Mutex::new(Vec::new()));
        // No bare digits: "4512" matched a `ts_ms` and failed a clean run.
        let secret = "PROMPT-FIXTURE my bank PIN is QZXW-PIN and sk-abcdefghijklmnopqrstuv";
        let input = vec![InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText(secret.into())],
        }];
        for _ in 0..14 {
            let call = ModelCall::xai("grok-4.7", Some("low"), CLASS_JUDGE, "ep-9", input.clone());
            call_model(&echo, &call, &CancelToken::new()).unwrap();
        }
        let raw =
            std::fs::read_to_string(crate::harness::span_path(&dir, log::ROUTE_TRACE)).unwrap();
        assert_eq!(raw.lines().count(), 14);
        for needle in ["PROMPT-FIXTURE", "QZXW-PIN", "sk-abc", "bank"] {
            assert!(!raw.contains(needle), "{needle} leaked: {raw}");
        }
        let rec: crate::harness::Span = serde_json::from_str(raw.lines().next().unwrap()).unwrap();
        let route = rec.route.clone().unwrap();
        assert_eq!(
            (route.class.as_str(), route.episode.as_str(), route.shadow),
            ("background:judge", "ep-9", false)
        );
        assert_eq!(
            route.used,
            Chosen {
                provider: "xai".into(),
                model: "grok-4.7".into(),
                effort: Some("low".into())
            }
        );
        assert_eq!(route.chosen, route.used);
        assert_eq!(
            route.rule_ids,
            vec![
                "registry:empty".to_string(),
                "class:background:judge".into()
            ]
        );
        assert_eq!(
            route
                .outcome
                .as_ref()
                .map(|o| (o.ok, o.tokens.input, o.tokens.cached)),
            Some((true, 900, 640))
        );
        assert_eq!(route.outcome.unwrap().cost_usd, 1_200.0 / 1e10);
        assert_eq!(route.span_id, rec.span_ref());
        assert_eq!(route.reason, "balanced · Low: an independent check.");
        let health =
            std::fs::read_to_string(grokhub_core::model_registry::store::health_path(&dir))
                .unwrap();
        assert_eq!(health.lines().count(), 14);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_provider_is_refused_and_sends_nothing() {
        let echo = Echo(Mutex::new(Vec::new()));
        let mut call = ModelCall::xai("m", None, CLASS_COMPACT, "ep-1", Vec::new());
        call.provider = "other".into();
        let err = call_model(&echo, &call, &CancelToken::new()).unwrap_err();
        assert_eq!(
            err,
            ClientError::Protocol("no route for provider `other`".into())
        );
        assert!(echo.0.lock().unwrap().is_empty());
    }

    /// What xAI's listing gives: no effort list on any row.
    fn xai_listed(ids: &[&str]) -> Registry {
        let rows = ids
            .iter()
            .map(|id| ModelMeta {
                id: id.to_string(),
                context_length: Some(256_000),
                prices: Prices {
                    prompt: Some(1),
                    cached: Some(1),
                    completion: Some(1),
                    ..Prices::default()
                },
                ..ModelMeta::default()
            })
            .collect();
        let mut reg = Registry::default();
        reg.entitlement.credential = Credential::Plan;
        reg.apply_refresh(&[Listing::new(SourceKind::XaiApi, rows)], &[], 1);
        reg
    }

    const NON_REASONING: [&str; 2] = [
        "grok-4.20-0309-non-reasoning",
        "grok-4-1-fast-non-reasoning",
    ];
    const CLASSES: [&str; 8] = [
        "chat:default",
        "chat:quick",
        "plan",
        "prepare:hard",
        "eval:item",
        CLASS_JUDGE,
        CLASS_COMPACT,
        "background:summarize",
    ];

    #[test]
    fn a_non_reasoning_model_never_gets_an_effort_in_any_class() {
        let reg = xai_listed(&[NON_REASONING[0], NON_REASONING[1], "grok-4.7"]);
        let none = BTreeMap::new();
        for class in CLASSES {
            for model in NON_REASONING {
                let auto = Router::choose(&input(class, model, 0.9), &reg, &none, &no_table(), 10);
                if auto.model.contains("non-reasoning") {
                    assert_eq!(auto.effort, None, "Auto {} in {class}", auto.model);
                }
            }
        }
        // With only non-reasoning models listed, every step Auto routes to one
        // goes out with no effort.
        let only = xai_listed(&NON_REASONING);
        let mut routed = Vec::new();
        for class in CLASSES {
            let r = Router::choose(
                &input(class, NON_REASONING[0], 0.9),
                &only,
                &none,
                &no_table(),
                10,
            );
            if !r.no_route {
                assert!(r.model.contains("non-reasoning"), "{class}: {}", r.model);
                assert_eq!(r.effort, None, "{class}: {}", r.model);
                routed.push(class);
            }
        }
        assert_eq!(
            routed,
            ["chat:quick", "background:compact", "background:summarize"]
        );
        // grok-4.7 takes an effort with no list on its row.
        let r = Router::choose(
            &input("chat:default", "grok-4.7", 0.5),
            &reg,
            &none,
            &no_table(),
            10,
        );
        assert_eq!(
            (r.model.as_str(), r.effort.as_deref()),
            ("grok-4.7", Some("medium"))
        );
        let eval = Router::choose(
            &input("eval:item", "grok-4.7", 0.0),
            &reg,
            &none,
            &no_table(),
            10,
        );
        assert_eq!(eval.effort.as_deref(), Some("high"));
    }

    #[test]
    fn the_request_body_drops_an_effort_a_model_cant_take() {
        let body = |model: &str| {
            crate::client::responses_body(&req(model, Some("low"), "s", "hi")).to_string()
        };
        for model in NON_REASONING {
            assert_eq!(
                body(model),
                format!(
                    r#"{{"input":[{{"content":[{{"text":"hi","type":"input_text"}}],"role":"user","type":"message"}}],"max_turns":3,"model":"{model}","prompt_cache_key":"s","store":false,"stream":true,"tools":[{{"type":"web_search"}},{{"type":"x_search"}}]}}"#
                )
            );
        }
        assert!(body("grok-4.7").contains(r#""reasoning":{"effort":"low"}"#));
        assert!(body("grok-4.20-multi-agent").contains(r#""reasoning":{"effort":"low"}"#));
        assert!(!body("grok-4").contains("reasoning"));
        // A provider's own model is its adapter's call, not ours.
        assert!(body("openai/gpt-5").contains(r#""reasoning":{"effort":"low"}"#));
    }

    #[test]
    fn an_unlisted_pin_that_answers_stays_routable_with_no_pause_card() {
        let mut reg = xai_listed(&[NON_REASONING[0]]);
        let ok = Observation {
            model: "grok-4.7".into(),
            ts_ms: 5,
            status: CallStatus::Ok,
            latency_ms: 900,
            served_model: Some("grok-4.7".into()),
            endpoint_ok: true,
            reasoning_tokens: 0,
            cost_ticks: 0,
            http: 200,
        };
        let (events, _) = reg.fold(&[ok]);
        let rec = reg.get("grok-4.7").unwrap();
        assert_eq!((rec.state, rec.state.routable()), (ModelState::Live, true));
        let none = BTreeMap::new();
        let heard = heal::heal_messages(&events, &reg, &none, 10);
        assert_eq!(
            heard
                .msgs
                .iter()
                .map(|m| m.key.as_str())
                .collect::<Vec<_>>(),
            Vec::<&str>::new()
        );
        // A redirected pin with nothing to stand in says nothing either: it still answers.
        reg.models.get_mut("grok-4.7").unwrap().state = ModelState::Redirected;
        let only = Registry {
            models: reg
                .models
                .iter()
                .filter(|(k, _)| k.as_str() == "grok-4.7")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            ..reg.clone()
        };
        let moved = [RegistryEvent::State {
            id: "grok-4.7".into(),
            from: ModelState::Live,
            to: ModelState::Redirected,
            reason: String::new(),
        }];
        assert_eq!(heal::heal_messages(&moved, &only, &none, 10).msgs.len(), 0);
    }

    #[test]
    fn a_pin_on_an_alias_is_not_a_ghost() {
        let row = ModelMeta {
            id: "grok-4.7-0901".into(),
            aliases: vec!["grok-4.7".into()],
            context_length: Some(256_000),
            prices: Prices {
                prompt: Some(1),
                cached: Some(1),
                completion: Some(1),
                ..Prices::default()
            },
            ..ModelMeta::default()
        };
        let mut reg = Registry::default();
        reg.entitlement.credential = Credential::Plan;
        reg.apply_refresh(
            &[Listing::new(SourceKind::XaiApi, vec![row])],
            &["grok-4.7".to_string()],
            1,
        );
        assert_eq!(reg.models.get("grok-4.7").map(|r| r.state), None);
        let rec = reg.get("grok-4.7").unwrap();
        assert_eq!(
            (rec.meta.id.as_str(), rec.state),
            ("grok-4.7-0901", ModelState::Live)
        );
        assert!(reg.listed("grok-4.7"));
    }

    /// Quality never lands on a non-reasoning model. When that is the only
    /// listed model, Auto pauses instead of sending it (wait/retry owns the rest).
    /// A non-reasoning request still carries no effort on the wire.
    #[test]
    fn quality_pauses_instead_of_picking_a_lone_non_reasoning_model() {
        let reg = xai_listed(&[NON_REASONING[0]]);
        let none = BTreeMap::new();
        let r = Router::choose(
            &input("chat:default", "grok-4.7", 0.5),
            &reg,
            &none,
            &no_table(),
            10,
        );
        assert_ne!(r.model, NON_REASONING[0]);
        assert!(r.no_route, "{r:?}");
        let body = crate::client::responses_body(&req(NON_REASONING[0], None, "s", "hi"));
        assert_eq!(body.get("reasoning"), None);
        let body = crate::client::responses_body(&req(NON_REASONING[0], Some("high"), "s", "hi"));
        assert_eq!(body.get("reasoning"), None);
    }
}

#[cfg(test)]
mod r3a_tests;

#[cfg(test)]
mod offline_tests;

#[cfg(test)]
mod local_bootstrap_tests;
