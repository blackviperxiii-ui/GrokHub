//! Router-ready model calls (Spike-3b). Every model call an episode makes
//! names its provider, model and effort per step and goes through
//! [`call_model`], so effort or model routing can change later in one place.
//! The only provider today is xAI over [`crate::XaiClient`] (or a test
//! [`ModelClient`]); this adds no effort UI and reads no `reasoning_effort`.

pub mod cabin;
pub mod difficulty;
pub mod log;
pub mod policy;
pub mod refresh;
pub mod shadow;
pub mod signals;
pub mod sources;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use std::collections::BTreeMap;

use grokhub_core::model_registry::profile::{clamp_effort, ModelProfile, RuntimeSettings};
use grokhub_core::model_registry::{ModelState, Registry, EFFORT_LADDER};

use crate::client::{ClientError, InputItem, ModelClient, ResponsesRequest, TurnOutput, Usage};
use crate::CancelToken;

pub use log::{why_models_text, why_text, Chosen, RouteRecord};
pub use shadow::{shadow_log, ShadowCall, ShadowDone};

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
        }
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
/// silent fallback to another one. Router R0 logs a shadow route for the call
/// (what it would have picked); the request is the call's own, unchanged.
pub fn call_model(client: &dyn ModelClient, call: &ModelCall, cancel: &CancelToken) -> Result<Routed, ClientError> {
    if call.provider != PROVIDER_XAI {
        return Err(ClientError::Protocol(format!("no route for provider `{}`", call.provider)));
    }
    let started = std::time::Instant::now();
    let out = client.stream(&call.request(), cancel, &mut |_| {});
    let shadow = ShadowCall {
        provider: &call.provider,
        class: &call.class,
        model: &call.model,
        effort: call.effort.as_deref(),
        episode: &call.conversation_id,
        session: &call.session,
        ctx_tokens: crate::compact::estimate_input_tokens(&call.input),
        ..ShadowCall::default()
    };
    shadow_log(&crate::perm::config_dir(), &shadow, &ShadowDone::of(&out, started.elapsed()));
    let out = out?;
    let tokens = CallTokens::of(call, &out.usage);
    Ok(Routed { out, tokens })
}

/// Everything [`Router::choose`] looks at. No I/O behind it: the registry and
/// profiles are passed in, and the clock is `now_ms`.
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
}

/// Router R0: pure and deterministic. In R0 its pick is only logged.
pub struct Router;

fn rung(e: &str) -> Option<usize> {
    EFFORT_LADDER.iter().position(|l| l.eq_ignore_ascii_case(e.trim()))
}

fn cap_word(e: Option<&str>) -> String {
    match e {
        None => "No effort".into(),
        Some(e) => {
            let mut c = e.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}

/// A model the router may pick: routable state, and a usable profile (a queued
/// or failed probe is not usable yet; no profile yet keeps today's behavior).
fn candidate(reg: &Registry, profiles: &BTreeMap<String, ModelProfile>, id: &str, needs_image: bool, now_ms: u64) -> bool {
    let Some(rec) = reg.get(id) else {
        return false;
    };
    let strikes = rec.not_found.iter().filter(|t| now_ms.saturating_sub(**t) <= grokhub_core::model_registry::health::STRIKE_WINDOW_MS).count();
    if !rec.state.routable() || strikes >= grokhub_core::model_registry::health::GHOST_STRIKES {
        return false;
    }
    if needs_image && rec.meta.input_modalities.as_ref().is_some_and(|m| !m.iter().any(|x| x == "image")) {
        return false;
    }
    profiles.get(id).is_none_or(|p| p.usable)
}

impl Router {
    pub fn choose(input: &RouteInput<'_>, reg: &Registry, profiles: &BTreeMap<String, ModelProfile>, now_ms: u64) -> Route {
        let mut rules: Vec<String> = Vec::new();
        let provider = if input.provider.is_empty() { PROVIDER_XAI } else { input.provider };
        let ids: Vec<&String> = reg.models.keys().filter(|id| candidate(reg, profiles, id, input.needs_image, now_ms)).collect();
        let current = input.current_model.trim();
        let model = if input.pinned {
            rules.push("pin".into());
            current.to_string()
        } else if provider == shadow::PROVIDER_GROK_BUILD {
            rules.push("gb:model_keep".into());
            current.to_string()
        } else if reg.models.is_empty() {
            rules.push("registry:empty".into());
            current.to_string()
        } else if ids.iter().any(|id| id.as_str() == current) {
            rules.push("model:keep".into());
            current.to_string()
        } else if let Some(first) = ids.iter().find(|id| id.as_str() == crate::client::DEFAULT_MODEL).or(ids.first()) {
            let why = reg.get(current).map(|r| r.state).unwrap_or(ModelState::NotInPlan);
            rules.push(format!("model:{}", why.as_str()));
            first.to_string()
        } else {
            rules.push("model:no_candidate".into());
            current.to_string()
        };
        let row = policy::class_row(input.class);
        let mut effort: Option<String> = match row {
            None => {
                rules.push("class:unlisted".into());
                input.current_effort.map(str::to_string)
            }
            Some(row) => {
                rules.push(format!("class:{}", row.class));
                let (start, floor, ceiling) = (rung(row.start).unwrap_or(0), rung(row.floor).unwrap_or(0), rung(row.ceiling).unwrap_or(0));
                let high = rung("high").unwrap_or(ceiling);
                let r = if input.d < 0.3 && start > floor {
                    rules.push("d<0.3".into());
                    start - 1
                } else if input.d > 0.7 && start < ceiling.min(high) {
                    rules.push("d>0.7".into());
                    start + 1
                } else {
                    start
                };
                Some(EFFORT_LADDER[r].to_string())
            }
        };
        let menu = profiles
            .get(&model)
            .and_then(|p| p.metadata.efforts.clone())
            .or_else(|| reg.get(&model).and_then(|r| r.meta.efforts.clone()));
        if let (Some(menu), Some(e)) = (menu, effort.clone()) {
            let clamped = clamp_effort(&e, &menu);
            if clamped.as_deref() != Some(e.as_str()) {
                rules.push("clamp".into());
            }
            effort = clamped;
        }
        let what = row.map(|r| r.plain).unwrap_or("this step");
        let looks = if row.is_some() && input.d < 0.3 {
            ", and it looks routine"
        } else if row.is_some() && input.d > 0.7 {
            ", and it looks hard"
        } else {
            ""
        };
        let reason = format!("{} on {model}: {what}{looks}.", cap_word(effort.as_deref()));
        let settings = profiles.get(&model).map(|p| RuntimeSettings::from_profile(p, input.class));
        Route { provider: provider.to_string(), model, effort, reason, rule_ids: rules, candidates_n: ids.len() as u32, settings }
    }
}

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
        reg.apply_refresh(&[Listing::new(SourceKind::XaiApi, rows)], &[], 1);
        reg
    }

    fn input<'a>(class: &'a str, model: &'a str, d: f64) -> RouteInput<'a> {
        RouteInput { provider: "xai", class, current_model: model, current_effort: Some("high"), d, ..RouteInput::default() }
    }

    #[test]
    fn choose_is_deterministic_and_follows_the_class_table() {
        let reg = listed(&["grok-4.5", "grok-4.7"]);
        let none = BTreeMap::new();
        let a = Router::choose(&input("chat:default", "grok-4.7", 0.5), &reg, &none, 10);
        assert_eq!(a, Router::choose(&input("chat:default", "grok-4.7", 0.5), &reg, &none, 10));
        assert_eq!((a.model.as_str(), a.effort.as_deref()), ("grok-4.7", Some("medium")));
        assert_eq!(a.reason, "Medium on grok-4.7: everyday chat.");
        assert_eq!(a.rule_ids, vec!["model:keep".to_string(), "class:chat:default".into()]);
        assert_eq!(a.candidates_n, 2);
        let easy = Router::choose(&input("chat:default", "grok-4.7", 0.1), &reg, &none, 10);
        assert_eq!((easy.effort.as_deref(), easy.reason.as_str()), (Some("low"), "Low on grok-4.7: everyday chat, and it looks routine."));
        // d > 0.7 goes up one rung but never past high on its own; the menu clamps xhigh anyway.
        let hard = Router::choose(&input("plan", "grok-4.7", 0.9), &reg, &none, 10);
        assert_eq!(hard.effort.as_deref(), Some("high"));
        // prepare:hard never starts below high.
        let prep = Router::choose(&input("prepare:hard", "grok-4.7", 0.0), &reg, &none, 10);
        assert_eq!(prep.effort.as_deref(), Some("high"));
        // An unlisted class keeps today's effort.
        let eval = Router::choose(&input("eval:item", "grok-4.7", 0.0), &reg, &none, 10);
        assert_eq!((eval.effort.as_deref(), eval.rule_ids.last().map(String::as_str)), (Some("high"), Some("class:unlisted")));
    }

    #[test]
    fn choose_skips_ghosts_and_unusable_profiles_and_keeps_pins() {
        let mut reg = listed(&["grok-4.5", "grok-4.7"]);
        let none = BTreeMap::new();
        reg.models.get_mut("grok-4.5").unwrap().state = ModelState::Ghost;
        let r = Router::choose(&input("chat:default", "grok-4.5", 0.5), &reg, &none, 10);
        assert_eq!((r.model.as_str(), r.rule_ids[0].as_str(), r.candidates_n), ("grok-4.7", "model:ghost", 1));
        let mut pin = input("chat:default", "grok-4.5", 0.5);
        pin.pinned = true;
        assert_eq!(Router::choose(&pin, &reg, &none, 10).model, "grok-4.5");
        let mut profiles = BTreeMap::new();
        let meta = reg.get("grok-4.7").unwrap().meta.clone();
        let mut bad = ProbeResult::status("failed");
        bad.failure = Some("JSON mode broke.".into());
        profiles.insert("grok-4.7".to_string(), ModelProfile::build(&meta, bad, 1));
        let none_left = Router::choose(&input("chat:default", "grok-4.7", 0.5), &reg, &profiles, 10);
        assert_eq!((none_left.candidates_n, none_left.rule_ids[0].as_str()), (0, "model:no_candidate"));
        // Queued is not usable yet either.
        profiles.insert("grok-4.7".to_string(), ModelProfile::build(&meta, ProbeResult::status("queued"), 1));
        assert_eq!(Router::choose(&input("chat:default", "grok-4.7", 0.5), &reg, &profiles, 10).candidates_n, 0);
        // An empty registry keeps today's model.
        let empty = Router::choose(&input("chat:default", "grok-4.7", 0.5), &Registry::default(), &none, 10);
        assert_eq!((empty.model.as_str(), empty.rule_ids[0].as_str()), ("grok-4.7", "registry:empty"));
    }

    /// Golden request bodies: shadow mode sends exactly what the call built.
    #[test]
    fn shadow_mode_leaves_request_bodies_and_agent_args_byte_identical() {
        let dir = crate::harness::test_dir("route-golden");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let echo = Echo(Mutex::new(Vec::new()));
        let input = vec![InputItem::Message { role: "user".into(), content: vec![ContentPart::InputText("hi".into())] }];
        let req = ResponsesRequest {
            model: "grok-4.7".into(),
            effort: Some("high".into()),
            input,
            conversation_id: "c1".into(),
            tools: Vec::new(),
            hosted_search: true,
            call_timeout: None,
        };
        let golden = r#"{"input":[{"content":[{"text":"hi","type":"input_text"}],"role":"user","type":"message"}],"model":"grok-4.7","reasoning":{"effort":"high"},"stream":true,"tools":[{"type":"web_search"},{"type":"x_search"}]}"#;
        assert_eq!(crate::client::responses_body(&req).to_string(), golden);
        shadow::stream_shadowed(&echo, &req, &CancelToken::new(), &mut |_| {}, "chat:default").unwrap();
        let sent = echo.0.lock().unwrap();
        assert_eq!(sent.len(), 1, "the shadow makes no call of its own");
        assert_eq!(crate::client::responses_body(&sent[0]).to_string(), golden);
        assert_eq!(
            grokhub_acp::agent_args(false, Some("high")),
            vec!["--no-auto-update", "agent", "--reasoning-effort", "high", "stdio"]
        );
        assert_eq!(grokhub_acp::agent_args(true, None), vec!["--no-auto-update", "agent", "--always-approve", "stdio"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn route_records_hold_no_prompt_text_and_why_prints_at_most_ten_lines() {
        let dir = crate::harness::test_dir("route-why");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        assert_eq!(why_text(&dir), "No routed model calls yet. Auto is watching only (shadow): calls still use your model and effort.");
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
        assert_eq!((route.class.as_str(), route.episode.as_str(), route.shadow), ("background:judge", "ep-9", true));
        assert_eq!(route.used, Chosen { provider: "xai".into(), model: "grok-4.7".into(), effort: Some("low".into()) });
        assert_eq!(route.chosen, route.used);
        assert_eq!(route.rule_ids, vec!["registry:empty".to_string(), "class:background:judge".into()]);
        assert_eq!(route.outcome.as_ref().map(|o| (o.ok, o.tokens.input, o.tokens.cached)), Some((true, 900, 640)));
        assert_eq!(route.outcome.unwrap().cost_usd, 1_200.0 / 1e10);
        assert_eq!(route.span_id, rec.span_ref());
        let why = why_text(&dir);
        assert_eq!(why.lines().count(), 10);
        assert_eq!(why.lines().next(), Some("background:judge · Low on grok-4.7: an independent check, and it looks routine."));
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
