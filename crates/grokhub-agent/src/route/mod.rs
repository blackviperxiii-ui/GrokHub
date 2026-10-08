//! Router-ready model calls (Spike-3b), routed live since Router R1. Every
//! model call names its class and goes through [`call_model`],
//! [`live::stream_routed`] or [`cabin::call_model`], which take the effort from
//! [`Router::choose`]. There is no effort setting: the class table
//! ([`policy`]), difficulty and the ladder ([`ladder`]) decide, and `/why`
//! shows the reasons. xAI goes over [`crate::XaiClient`] (or a test
//! [`ModelClient`]); a provider you added with your own key (R3b) goes over
//! [`providers::call_model`], only under its key and your destination grant.

pub mod budget;
pub mod cabin;
pub mod difficulty;
pub mod guard;
pub mod heal;
pub mod ladder;
pub mod learn;
pub mod local;
pub mod log;
pub mod policy;
pub mod providers;
pub mod refresh;
pub mod live;
pub mod signals;
pub mod spend;
pub mod sources;
pub mod table;
pub mod tune;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use std::collections::BTreeMap;

use grokhub_core::model_registry::profile::{ModelProfile, RuntimeSettings};
use grokhub_core::model_registry::cost_class::{fast_base, fast_entitled, fast_of, route_key, CostClass, RouteOpts};
use grokhub_core::model_registry::discover::split_provider_model;
use grokhub_core::model_registry::{Credential, ModelState, Registry, EFFORT_LADDER};

use table::RoutingTable;

use crate::client::{ClientError, InputItem, ModelClient, ResponsesRequest, TurnOutput, Usage};
use crate::CancelToken;

pub use log::{why_models_text, why_text, Chosen, RouteRecord};
pub use live::{route_log, RouteCall, RouteDone};

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
}

impl ModelCall {
    pub fn xai(model: &str, effort: Option<&str>, class: &str, conversation_id: &str, input: Vec<InputItem>) -> Self {
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
        }
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
pub fn call_model(client: &dyn ModelClient, call: &ModelCall, cancel: &CancelToken) -> Result<Routed, ClientError> {
    if call.provider != PROVIDER_XAI {
        return Err(ClientError::Protocol(format!("no route for provider `{}`", call.provider)));
    }
    let dir = crate::perm::config_dir();
    let text = live::last_user_text(&call.input).0;
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
        ..RouteCall::default()
    };
    rc.providers = true;
    let now = grokhub_core::now_ms();
    let decision = live::decide(&dir, &rc, now);
    if decision.paused() {
        route_log(&dir, &rc, &decision, &RouteDone::unseen());
        return Err(ClientError::Protocol(decision.pause_msg().into()));
    }
    let mut sent = call.clone();
    sent.effort = decision.send_effort(call.effort.as_deref());
    sent.model = decision.send_model(&call.model);
    let started = std::time::Instant::now();
    let on_device = decision.on_device();
    let out = if on_device {
        local::serve(&call.class, &text)
    } else if decision.on_provider() {
        let span_id = format!("{}:{now}", if call.session.is_empty() { &call.conversation_id } else { &call.session });
        providers::call_model(&dir, &sent.model, sent.effort.as_deref(), &sent.request(), providers::call_data(rc.sensitive), &span_id, cancel)
    } else {
        client.stream(&sent.request(), cancel, &mut |_| {})
    };
    if on_device {
        rc.provider = local::PROVIDER_LOCAL;
    } else if decision.on_provider() {
        rc.provider = &decision.route.provider;
    }
    rc.effort = sent.effort.as_deref();
    rc.model = &sent.model;
    route_log(&dir, &rc, &decision, &RouteDone::of(&out, started.elapsed()));
    let out = out?;
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
    /// The user pinned the model (`/model`, a skill or an automation).
    pub pinned: bool,
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
    /// R2b: what this call may spend (cost classes, the Fast policy, grants, budget).
    pub spend: spend::Spend,
    /// R3a DE2: the rung this task's signature is known to succeed at, for a fresh pick.
    pub routine: Option<usize>,
    /// R3a: the self-tuned (or canary) class start, clamped into the band.
    pub start: Option<&'a str>,
    /// R3a: the local model's gate. Off by default, so no `local:*` id is ever picked.
    pub local: local::LocalGate,
    /// R3b: the provider model you picked in Settings for this (your own) chat.
    /// Taken only while it is a candidate (its key and a grant covering the data).
    pub prefer: Option<&'a str>,
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
    /// The pinned or episode model this route stands in for, when it fell back.
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
    const MOVES: &[&str] = &["E1", "E2", "E3", "E4", "E5", "DE1", "DE2", "DE3", "steer:harder", "steer:quicker", "d<0.3", "d>0.7"];
    let last = rules.iter().rev().find(|r| MOVES.contains(&r.as_str()));
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

impl Router {
    fn pick_model(input: &RouteInput<'_>, reg: &Registry, ids: &[&str], table: &RoutingTable, e2: bool, rules: &mut Vec<String>) -> ModelPick {
        let current = input.current_model.trim();
        let gb = input.provider == live::PROVIDER_GROK_BUILD;
        let keep = |m: &str| ModelPick { model: m.to_string(), no_route: false, replaced: None };
        if reg.models.is_empty() {
            rules.push("registry:empty".into());
            return keep(current);
        }
        if !gb && reg.entitlement.credential == Credential::None {
            // No sign-in: nothing native is included, so the call keeps its own model.
            rules.push("cost:not_included".into());
            return keep(current);
        }
        if reg.get(current).is_some() && !ids.contains(&current) && policy::route_class(reg, current, gb, &input.spend) == CostClass::Premium {
            // Over your $/M ceiling and not approved: stand in, and the cabin asks once.
            rules.push("cost:premium_ungranted".into());
        }
        // R3b: a provider you added is never a stand-in or a fallback, and never
        // the cheapest pick: only a pin or the class preference (a table row) takes it.
        let own: Vec<&str> = ids.iter().copied().filter(|m| !policy::is_new_provider(reg, m)).collect();
        let preferred: Vec<&str> = ids.iter().copied().filter(|m| !policy::is_new_provider(reg, m) || table.row(input.class, m).is_some()).collect();
        // A degraded model is still eligible, but a pin or an episode leaves it
        // when a healthy one can take over.
        let healthy: Vec<&str> = own.iter().copied().filter(|m| reg.get(m).is_some_and(|r| r.state != ModelState::Degraded)).collect();
        let pool: &[&str] = if healthy.is_empty() { &own } else { &healthy };
        let sound = |m: &str| ids.contains(&m) && (healthy.contains(&m) || policy::is_new_provider(reg, m) || !healthy.iter().any(|h| *h != m));
        // Stand in for `from`: its redirect successor, then its family chain, then the ranked list.
        // Left out only for its cost (a Fast variant or an unapproved premium
        // route): the plain model first, then any included one, never a pause.
        let priced_out = |m: &str| (policy::is_new_provider(reg, m) && !ids.contains(&m)) || reg.get(m).is_some_and(|r| r.state.routable()) && !ids.contains(&m) && policy::fits_any_cost(reg, &BTreeMap::new(), m, policy::Fit { grok_build: gb, ..policy::Fit::default() }, 0);
        let stand_in = |from: &str, rules: &mut Vec<String>, tag: &str| -> ModelPick {
            if policy::is_new_provider(reg, from) && !ids.contains(&from) {
                // A provider pin with no key or no grant for this data: xAI takes it.
                rules.push("provider:ungranted".into());
            }
            if let Some(s) = successor(reg, from, &own) {
                rules.push(format!("{tag}:redirect"));
                return ModelPick { model: s, no_route: false, replaced: Some(from.to_string()) };
            }
            if let Some(base) = fast_base(reg, from).filter(|b| own.contains(b)) {
                rules.push(format!("{tag}:base"));
                return ModelPick { model: base.to_string(), no_route: false, replaced: Some(from.to_string()) };
            }
            let ranked = policy::rank(input.class, pool, table, reg, current, input.ctx_tokens);
            let chain = policy::fallback_chain(from, pool);
            match chain.first().or(if input.pinned && !priced_out(from) { None } else { ranked.first() }) {
                Some(m) => {
                    rules.push(format!("{tag}:fallback"));
                    ModelPick { model: m.clone(), no_route: false, replaced: Some(from.to_string()) }
                }
                None if own.contains(&from) => {
                    rules.push(format!("{tag}:degraded_kept"));
                    keep(from)
                }
                None => {
                    rules.push("model:no_route".into());
                    ModelPick { model: from.to_string(), no_route: true, replaced: None }
                }
            }
        };
        if let Some(p) = input.prefer.map(str::trim).filter(|p| ids.contains(p) && policy::is_new_provider(reg, p)) {
            rules.push("provider:picked".into());
            return keep(p);
        }
        if input.pinned {
            if sound(current) {
                rules.push("pin".into());
                return keep(current);
            }
            return stand_in(current, rules, "pin");
        }
        if let Some(ep) = input.episode_model.map(str::trim).filter(|m| !m.is_empty()) {
            if sound(ep) && !e2 {
                rules.push("episode:keep".into());
                return keep(ep);
            }
            if !e2 {
                rules.push("episode:unhealthy".into());
                return stand_in(ep, rules, "episode");
            }
        }
        if !gb && reg.entitlement.credential == Credential::ApiKey && table.rows(input.class).is_empty() && ids.contains(&current) && !e2 {
            // A key has no evals (they spend only the plan pool), so with no table
            // row the call keeps its own included model instead of the cheapest.
            rules.push("key:keep".into());
            return keep(current);
        }
        let mut ranked = policy::rank(input.class, &preferred, table, reg, current, input.ctx_tokens);
        if e2 {
            let away = input.episode_model.unwrap_or(current).trim().to_string();
            ranked.retain(|m| *m != away);
            rules.push("model:next".into());
        }
        match ranked.first() {
            Some(m) => {
                rules.push(if table.row(input.class, m).is_some() { "table".to_string() } else { "model:cheapest".to_string() });
                keep(m)
            }
            None if e2 => keep(input.episode_model.unwrap_or(current)),
            None => stand_in(current, rules, "model"),
        }
    }

    pub fn choose(input: &RouteInput<'_>, reg: &Registry, profiles: &BTreeMap<String, ModelProfile>, table: &RoutingTable, now_ms: u64) -> Route {
        let mut rules: Vec<String> = Vec::new();
        let provider = if input.provider.is_empty() { PROVIDER_XAI } else { input.provider };
        let fit = policy::Fit { needs_image: input.needs_image, needs_tools: input.needs_tools, ctx_tokens: input.ctx_tokens, grok_build: provider == live::PROVIDER_GROK_BUILD };
        // A local route only while the flag is on, for an eligible class, on the live tier.
        let ids: Vec<&str> = reg
            .models
            .keys()
            .map(String::as_str)
            .filter(|id| !local::is_local(id) || local::admits(&input.local, id, input.class, input.ctx_tokens))
            .filter(|id| policy::fits(reg, profiles, id, fit, &input.spend, now_ms))
            .collect();
        let row = policy::class_row(input.class);
        let band = row.map(|r| ladder::Band::tuned(r, input.bump, input.start));
        let pick = match (row, band) {
            (Some(row), Some(band)) => Some(input.pick.clone().unwrap_or_else(|| {
                let start = ladder::start_rung(band, input.d, input.steer, ladder::user_facing(row.class));
                ladder::advance(None, row.class, band, start, &ladder::Obs { new_turn: true, routine: input.routine, ..ladder::Obs::default() }).1
            })),
            _ => None,
        };
        let e2 = pick.as_ref().is_some_and(|p| p.rules.iter().any(|r| r == "E2:next_model"));
        let chosen = if local::privacy_only(&input.local) {
            // Sensitive data and no cloud grant: the device or nothing.
            match ids.iter().find(|id| local::is_local(id)) {
                Some(id) => {
                    rules.push("privacy:local_only".into());
                    ModelPick { model: id.to_string(), no_route: false, replaced: None }
                }
                None => {
                    rules.push("privacy:no_local".into());
                    ModelPick { model: input.current_model.trim().to_string(), no_route: true, replaced: None }
                }
            }
        } else {
            Self::pick_model(input, reg, &ids, table, e2, &mut rules)
        };
        let provider = match split_provider_model(&chosen.model) {
            _ if local::is_local(&chosen.model) && !chosen.no_route => local::PROVIDER_LOCAL,
            Some((p, _)) if !chosen.no_route && policy::is_new_provider(reg, &chosen.model) => p,
            _ => provider,
        };
        let gb = provider == live::PROVIDER_GROK_BUILD;
        let premium_ask = rules.iter().any(|r| r == "cost:premium_ungranted").then(|| route_key(input.current_model, RouteOpts::default()));
        // Auto only: a pin is kept as picked. The Fast variant goes only under the latency policy.
        let fast = (!input.pinned && !chosen.no_route && ids.contains(&chosen.model.as_str()))
            .then(|| fast_of(reg, &chosen.model))
            .flatten()
            .filter(|f| fast_entitled(reg, f) && policy::fits_any_cost(reg, profiles, f, fit, now_ms))
            .map(str::to_string);
        let mut fast_fallback = None;
        let (model, base) = match (&fast, fast.as_ref().map(|_| spend::fast_verdict(&input.spend))) {
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
        if row.is_some_and(|r| r.start == "high") && reg.entitlement.credential == Credential::Plan && policy::outranked_by_plan_upgrade(reg, profiles, &model, fit) {
            // A newer model outside your plan would have taken this: the upgrade nudge counts it.
            rules.push("plan:would_pick".into());
        }
        let effort_rules_at = rules.len();
        let mut effort: Option<String> = match (row, band, pick) {
            (Some(row), Some(band), Some(pick)) => {
                rules.push(format!("class:{}", row.class));
                if input.bump > 0 {
                    rules.push(format!("guard:+{}", input.bump));
                }
                rules.extend(pick.rules.iter().cloned());
                // The effort list of the model that is actually sent.
                let sent_model = if policy::MODEL_LIVE { model.as_str() } else { input.current_model.trim() };
                let menu = profiles
                    .get(sent_model)
                    .and_then(|p| p.metadata.efforts.clone())
                    .or_else(|| reg.get(sent_model).and_then(|r| r.meta.efforts.clone()));
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
                    None => Some(rung_name.to_string()),
                };
                // `minimal` and `max` are not sent: they go out as low and xhigh.
                picked.map(|e| grokhub_core::parse_reasoning_effort(&e).map(str::to_string).unwrap_or(e))
            }
            _ => {
                rules.push("class:unlisted".into());
                input.current_effort.map(str::to_string)
            }
        };
        if row.is_none() {
            effort = effort.map(|e| grokhub_core::parse_reasoning_effort(&e).map(str::to_string).unwrap_or(e));
        }
        let what = row.map(|r| r.plain).unwrap_or("this step");
        let clause = if row.is_some() { why_clause(what, &rules[effort_rules_at..]) } else { format!("{what} keeps its set effort") };
        let reason = match (&chosen.replaced, rules.iter().any(|r| r == "table")) {
            _ if base.is_some() => format!("Using {model} at {}: you're waiting, so the faster model is worth it.", effort_word(effort.as_deref())),
            _ if fast_fallback.is_some() => format!(
                "Using {model} at {}: this week's budget is tight, so not {}.",
                effort_word(effort.as_deref()),
                fast_fallback.as_deref().unwrap_or_default()
            ),
            _ if rules.iter().any(|r| r == "provider:picked") => format!("Using {model} at {}: you picked it in Settings.", effort_word(effort.as_deref())),
            (Some(from), _) if rules.iter().any(|r| r == "provider:ungranted") => {
                format!("Using {model} at {}: {from} needs its key and your OK in Settings first.", effort_word(effort.as_deref()))
            }
            (Some(from), _) if rules.iter().any(|r| r == "cost:premium_ungranted") => {
                format!("Using {model} at {}: {from} costs more than your price limit, so it needs your OK first.", effort_word(effort.as_deref()))
            }
            (Some(from), _) => format!("Using {model} at {}: {from} isn't answering.", effort_word(effort.as_deref())),
            (None, true) if model != input.current_model.trim() || input.episode_model.is_none() => {
                let why = table.row(input.class, &model).map(|r| r.why.clone()).unwrap_or_default();
                format!("Using {model} at {}: it's {why}.", effort_word(effort.as_deref()))
            }
            _ => format!("{}: {clause}.", effort_word(effort.as_deref())),
        };
        let settings = profiles.get(&model).map(|p| RuntimeSettings::from_profile(p, input.class));
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
mod r2a_tests;

#[cfg(test)]
mod r2b_tests;

#[cfg(test)]
mod r3b_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{ContentPart, StreamEvent};
    use std::sync::Mutex;

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
            })
        }
    }

    #[test]
    fn call_model_sends_model_and_effort_and_logs_every_token_kind() {
        let echo = Echo(Mutex::new(Vec::new()));
        let input = vec![InputItem::Message { role: "user".into(), content: vec![ContentPart::InputText("hi".into())] }];
        let call = ModelCall::xai("grok-4.7", Some(BACKGROUND_EFFORT), CLASS_JUDGE, "ep-1", input);
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
        assert_eq!((sent[0].model.as_str(), sent[0].effort.as_deref()), ("grok-4.7", Some("low")));
        assert!(!sent[0].hosted_search);
        let line = serde_json::to_string(&routed.tokens).unwrap();
        assert_eq!(
            line,
            r#"{"class":"background:judge","model":"grok-4.7","effort":"low","in":900,"cached":640,"out":7,"reasoning":3,"cost_ticks":1200}"#
        );
    }

    use grokhub_core::model_registry::profile::ProbeResult;
    use grokhub_core::model_registry::{Listing, ModelMeta, Prices, SourceKind};

    fn listed(ids: &[&str]) -> Registry {
        let rows = ids
            .iter()
            .map(|id| ModelMeta {
                id: id.to_string(),
                context_length: Some(256_000),
                prices: Prices { prompt: Some(1), cached: Some(1), completion: Some(1), ..Prices::default() },
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
        RouteInput { provider: "xai", class, current_model: model, current_effort: Some("high"), d, ..RouteInput::default() }
    }

    #[test]
    fn choose_is_deterministic_and_follows_the_class_table() {
        let reg = listed(&["grok-4.5", "grok-4.7"]);
        let none = BTreeMap::new();
        let a = Router::choose(&input("chat:default", "grok-4.7", 0.5), &reg, &none, &no_table(), 10);
        assert_eq!(a, Router::choose(&input("chat:default", "grok-4.7", 0.5), &reg, &none, &no_table(), 10));
        assert_eq!((a.model.as_str(), a.effort.as_deref()), ("grok-4.7", Some("medium")));
        assert_eq!(a.reason, "Medium: everyday chat.");
        // No table yet: equal prices, so the session's own model wins the tie.
        assert_eq!(a.rule_ids, vec!["model:cheapest".to_string(), "class:chat:default".into()]);
        assert_eq!(a.candidates_n, 2);
        let easy = Router::choose(&input("chat:default", "grok-4.7", 0.1), &reg, &none, &no_table(), 10);
        assert_eq!((easy.effort.as_deref(), easy.reason.as_str()), (Some("low"), "Low: everyday chat, and this looks routine."));
        // d > 0.7 goes up one rung but never past high on its own; the menu clamps xhigh anyway.
        let hard = Router::choose(&input("plan", "grok-4.7", 0.9), &reg, &none, &no_table(), 10);
        assert_eq!(hard.effort.as_deref(), Some("high"));
        // prepare:hard never starts below high.
        let prep = Router::choose(&input("prepare:hard", "grok-4.7", 0.0), &reg, &none, &no_table(), 10);
        assert_eq!(prep.effort.as_deref(), Some("high"));
        // An unlisted class keeps today's effort.
        let eval = Router::choose(&input("eval:item", "grok-4.7", 0.0), &reg, &none, &no_table(), 10);
        assert_eq!((eval.effort.as_deref(), eval.rule_ids.last().map(String::as_str)), (Some("high"), Some("class:unlisted")));
    }

    #[test]
    fn choose_skips_ghosts_and_unusable_profiles_and_stands_in_for_a_dead_pin() {
        let mut reg = listed(&["grok-4.5", "grok-4.7"]);
        let none = BTreeMap::new();
        reg.models.get_mut("grok-4.5").unwrap().state = ModelState::Ghost;
        let r = Router::choose(&input("chat:default", "grok-4.5", 0.5), &reg, &none, &no_table(), 10);
        assert_eq!((r.model.as_str(), r.rule_ids[0].as_str(), r.candidates_n), ("grok-4.7", "model:cheapest", 1));
        let mut pin = input("chat:default", "grok-4.7", 0.5);
        pin.pinned = true;
        let kept = Router::choose(&pin, &reg, &none, &no_table(), 10);
        assert_eq!((kept.model.as_str(), kept.rule_ids[0].as_str(), kept.replaced.clone()), ("grok-4.7", "pin", None));
        // A pin that went away is stood in for by its family, and says so.
        pin.current_model = "grok-4.5";
        let r = Router::choose(&pin, &reg, &none, &no_table(), 10);
        assert_eq!((r.model.as_str(), r.rule_ids[0].as_str(), r.replaced.as_deref()), ("grok-4.7", "pin:fallback", Some("grok-4.5")));
        assert_eq!(r.reason, "Using grok-4.7 at Medium: grok-4.5 isn't answering.");
        let mut profiles = BTreeMap::new();
        let meta = reg.get("grok-4.7").unwrap().meta.clone();
        let mut bad = ProbeResult::status("failed");
        bad.failure = Some("JSON mode broke.".into());
        profiles.insert("grok-4.7".to_string(), ModelProfile::build(&meta, bad, 1));
        let none_left = Router::choose(&input("chat:default", "grok-4.7", 0.5), &reg, &profiles, &no_table(), 10);
        assert_eq!((none_left.candidates_n, none_left.rule_ids[0].as_str(), none_left.no_route), (0, "model:no_route", true));
        // Queued is not usable yet either.
        profiles.insert("grok-4.7".to_string(), ModelProfile::build(&meta, ProbeResult::status("queued"), 1));
        assert_eq!(Router::choose(&input("chat:default", "grok-4.7", 0.5), &reg, &profiles, &no_table(), 10).candidates_n, 0);
        // An empty registry keeps today's model.
        let empty = Router::choose(&input("chat:default", "grok-4.7", 0.5), &Registry::default(), &none, &no_table(), 10);
        assert_eq!((empty.model.as_str(), empty.rule_ids[0].as_str()), ("grok-4.7", "registry:empty"));
    }

    fn req(model: &str, effort: Option<&str>, session: &str, text: &str) -> ResponsesRequest {
        ResponsesRequest {
            model: model.into(),
            effort: effort.map(str::to_string),
            input: vec![InputItem::Message { role: "user".into(), content: vec![ContentPart::InputText(text.into())] }],
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
            assert_eq!(sent_effort(&echo, &r, class).as_deref(), Some(*want), "{class}");
        }
        let body = r#"{"input":[{"content":[{"text":"hi","type":"input_text"}],"role":"user","type":"message"}],"model":"grok-4.7","reasoning":{"effort":"low"},"stream":true,"tools":[{"type":"web_search"},{"type":"x_search"}]}"#;
        live::stream_routed(&echo, &req("grok-4.7", Some("low"), "golden-session", "hi"), &CancelToken::new(), &mut |_| {}, CLASS_COMPACT).unwrap();
        assert_eq!(crate::client::responses_body(echo.0.lock().unwrap().last().unwrap()).to_string(), body);
        assert_eq!(
            grokhub_acp::agent_args(false, Some("high")),
            vec!["--no-auto-update", "agent", "--reasoning-effort", "high", "stdio"]
        );
        assert_eq!(grokhub_acp::agent_args(true, None), vec!["--no-auto-update", "agent", "--always-approve", "stdio"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A chat session id outside the 10% holdout.
    fn auto_session(tag: &str) -> String {
        (0..).map(|i| format!("{tag}-{i}")).find(|s| !guard::in_holdout(s)).unwrap()
    }

    #[test]
    fn chat_effort_is_automatic_and_think_hard_lasts_one_episode() {
        let dir = crate::harness::test_dir("route-steer");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let echo = Echo(Mutex::new(Vec::new()));
        let s = auto_session("steer");
        // The call's own effort no longer matters for a listed class.
        assert_eq!(sent_effort(&echo, &req("grok-4.7", Some("xhigh"), &s, "hi"), "chat:default").as_deref(), Some("low"));
        let ask = "think hard on this rename";
        assert_eq!(sent_effort(&echo, &req("grok-4.7", Some("low"), &s, ask), "chat:default").as_deref(), Some("xhigh"));
        assert_eq!(sent_effort(&echo, &req("grok-4.7", Some("low"), &s, ask), "chat:default").as_deref(), Some("xhigh"), "a tool step in the same episode keeps it");
        let next = "Debug why the parser fails";
        assert_eq!(sent_effort(&echo, &req("grok-4.7", Some("low"), &s, next), "chat:default").as_deref(), Some("medium"), "the next episode is back at the class start");
        // E1 through the live helper: a failed tool lifts the next step one rung.
        crate::route::ladder::note_tool_error();
        assert_eq!(sent_effort(&echo, &req("grok-4.7", Some("low"), &s, next), "chat:default").as_deref(), Some("high"));
        // E3: a correction starts the next turn one higher than its start (low → medium).
        assert_eq!(sent_effort(&echo, &req("grok-4.7", None, &s, "no, wrong file"), "chat:default").as_deref(), Some("medium"));
        let (effort, reason) = live::last_user_pick().unwrap();
        assert_eq!((effort.as_deref(), reason.as_str()), (Some("medium"), "Medium: you corrected the last answer."));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn holdout_episodes_run_at_fixed_high_and_are_logged_but_never_named() {
        let dir = crate::harness::test_dir("route-holdout");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let echo = Echo(Mutex::new(Vec::new()));
        let s = (0..).map(|i| format!("hold-{i}")).find(|s| guard::in_holdout(s)).unwrap();
        assert_eq!(sent_effort(&echo, &req("grok-4.7", None, &s, "think hard about it"), "chat:default").as_deref(), Some("high"));
        let raw = std::fs::read_to_string(crate::harness::span_path(&dir, log::ROUTE_TRACE)).unwrap();
        let rec: crate::harness::Span = serde_json::from_str(raw.lines().last().unwrap()).unwrap();
        let route = rec.route.unwrap();
        assert!(route.holdout && !route.shadow);
        assert_eq!(route.reason, "High: everyday chat.");
        assert!(!why_text(&dir).to_ascii_lowercase().contains("holdout"));
        // prepare:hard is never held out.
        let hard = (0..).map(|i| format!("hard-{i}")).find(|s| guard::in_holdout(s)).unwrap();
        assert_eq!(sent_effort(&echo, &req("grok-4.7", None, &hard, "keep it quick"), "prepare:hard").as_deref(), Some("high"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn prepare_hard_never_goes_below_high_and_clamps_up_on_a_short_menu() {
        let mut reg = listed(&["grok-4.7"]);
        reg.models.get_mut("grok-4.7").unwrap().meta.efforts = Some(vec!["low".into(), "medium".into(), "xhigh".into()]);
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
    fn route_records_hold_no_prompt_text_and_why_prints_at_most_ten_lines() {
        let dir = crate::harness::test_dir("route-why");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        assert_eq!(why_text(&dir), "No routed model calls yet. Effort is automatic: each step's reason shows here once it runs.");
        let echo = Echo(Mutex::new(Vec::new()));
        let secret = "PROMPT-FIXTURE my bank PIN is 4512 and sk-abcdefghijklmnopqrstuv";
        let input = vec![InputItem::Message { role: "user".into(), content: vec![ContentPart::InputText(secret.into())] }];
        for _ in 0..14 {
            let call = ModelCall::xai("grok-4.7", Some("low"), CLASS_JUDGE, "ep-9", input.clone());
            call_model(&echo, &call, &CancelToken::new()).unwrap();
        }
        let raw = std::fs::read_to_string(crate::harness::span_path(&dir, log::ROUTE_TRACE)).unwrap();
        assert_eq!(raw.lines().count(), 14);
        for needle in ["PROMPT-FIXTURE", "4512", "sk-abc", "bank"] {
            assert!(!raw.contains(needle), "{needle} leaked: {raw}");
        }
        let rec: crate::harness::Span = serde_json::from_str(raw.lines().next().unwrap()).unwrap();
        let route = rec.route.clone().unwrap();
        assert_eq!((route.class.as_str(), route.episode.as_str(), route.shadow), ("background:judge", "ep-9", false));
        assert_eq!(route.used, Chosen { provider: "xai".into(), model: "grok-4.7".into(), effort: Some("low".into()) });
        assert_eq!(route.chosen, route.used);
        assert_eq!(route.rule_ids, vec!["registry:empty".to_string(), "class:background:judge".into()]);
        assert_eq!(route.outcome.as_ref().map(|o| (o.ok, o.tokens.input, o.tokens.cached)), Some((true, 900, 640)));
        assert_eq!(route.outcome.unwrap().cost_usd, 1_200.0 / 1e10);
        assert_eq!(route.span_id, rec.span_ref());
        let why = why_text(&dir);
        assert_eq!(why.lines().count(), 10);
        assert_eq!(why.lines().next(), Some("background:judge · grok-4.7 · Low: an independent check."));
        let health = std::fs::read_to_string(grokhub_core::model_registry::store::health_path(&dir)).unwrap();
        assert_eq!(health.lines().count(), 14);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_provider_is_refused_and_sends_nothing() {
        let echo = Echo(Mutex::new(Vec::new()));
        let mut call = ModelCall::xai("m", None, CLASS_COMPACT, "ep-1", Vec::new());
        call.provider = "other".into();
        let err = call_model(&echo, &call, &CancelToken::new()).unwrap_err();
        assert_eq!(err, ClientError::Protocol("no route for provider `other`".into()));
        assert!(echo.0.lock().unwrap().is_empty());
    }
}

#[cfg(test)]
mod r3a_tests;
