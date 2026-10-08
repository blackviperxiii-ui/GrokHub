//! Router-ready model calls (Spike-3b), routed live since Router R1. Every
//! model call names its class and goes through [`call_model`],
//! [`live::stream_routed`] or [`cabin::call_model`], which take the effort from
//! [`Router::choose`]. There is no effort setting: the class table
//! ([`policy`]), difficulty and the ladder ([`ladder`]) decide, and `/why`
//! shows the reasons. The only provider today is xAI over [`crate::XaiClient`]
//! (or a test [`ModelClient`]).

pub mod cabin;
pub mod difficulty;
pub mod guard;
pub mod ladder;
pub mod log;
pub mod policy;
pub mod refresh;
pub mod live;
pub mod signals;
pub mod sources;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use std::collections::BTreeMap;

use grokhub_core::model_registry::profile::{ModelProfile, RuntimeSettings};
use grokhub_core::model_registry::{ModelState, Registry, EFFORT_LADDER};

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
/// silent fallback to another one. The router picks the effort for a listed
/// class (the model stays the call's own until R2); the route record logs what was sent.
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
        ..RouteCall::default()
    };
    let decision = live::decide(&dir, &rc, grokhub_core::now_ms());
    let mut sent = call.clone();
    sent.effort = decision.send_effort(call.effort.as_deref());
    let started = std::time::Instant::now();
    let out = client.stream(&sent.request(), cancel, &mut |_| {});
    rc.effort = sent.effort.as_deref();
    route_log(&dir, &rc, &decision, &RouteDone::of(&out, started.elapsed()));
    let out = out?;
    let tokens = CallTokens::of(&sent, &out.usage);
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
    /// The user's own words this episode ("think hard", "keep it quick").
    pub steer: ladder::Steer,
    /// Rungs the accuracy guard raised this class's start and floor.
    pub bump: u8,
    /// The live ladder's pick for this episode. `None` starts fresh from the class table.
    pub pick: Option<ladder::Pick>,
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

/// Pure and deterministic. R1 sends its effort for listed classes; its model
/// pick is still only logged until R2 ([`policy::MODEL_LIVE`]).
pub struct Router;

/// The effort's plain label ("Medium", "Extra High", "No effort").
pub fn effort_word(e: Option<&str>) -> &'static str {
    match e {
        None | Some("") => "No effort",
        Some(e) => grokhub_core::effort_label(e),
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

/// The plain clause for the rule that moved effort last.
fn why_clause(plain: &str, rules: &[String]) -> String {
    const MOVES: &[&str] = &["E1", "E2", "E3", "E4", "E5", "DE1", "DE3", "steer:harder", "steer:quicker", "d<0.3", "d>0.7"];
    let last = rules.iter().rev().find(|r| MOVES.contains(&r.as_str()));
    match last.map(String::as_str) {
        Some("E1") => "a tool failed, so it thinks harder".into(),
        Some("E2") => "the check rejected the work".into(),
        Some("E3") => "you corrected the last answer".into(),
        Some("E4") => "the self-check was weak".into(),
        Some("E5") => format!("{plain}, which never runs below High"),
        Some("DE1") => "the last steps went cleanly".into(),
        Some("DE3") => "most of the budget is used".into(),
        Some("steer:harder") => "you asked it to think hard".into(),
        Some("steer:quicker") => "you asked to keep it quick".into(),
        Some("d<0.3") => format!("{plain}, and this looks routine"),
        Some("d>0.7") => format!("{plain}, and this looks hard"),
        _ => plain.to_string(),
    }
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
        } else if provider == live::PROVIDER_GROK_BUILD {
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
        let effort_rules_at = rules.len();
        let mut effort: Option<String> = match row {
            None => {
                rules.push("class:unlisted".into());
                input.current_effort.map(str::to_string)
            }
            Some(row) => {
                rules.push(format!("class:{}", row.class));
                let band = ladder::Band::of(row, input.bump);
                if input.bump > 0 {
                    rules.push(format!("guard:+{}", input.bump));
                }
                let pick = input.pick.clone().unwrap_or_else(|| {
                    let start = ladder::start_rung(band, input.d, input.steer, ladder::user_facing(row.class));
                    ladder::advance(None, row.class, band, start, &ladder::Obs { new_turn: true, ..ladder::Obs::default() }).1
                });
                rules.extend(pick.rules.iter().cloned());
                // The effort list of the model that is actually sent.
                let sent_model = if policy::MODEL_LIVE { model.as_str() } else { current };
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
                    None => Some(rung_name.to_string()),
                };
                // `minimal` and `max` are not sent: they go out as low and xhigh.
                picked.map(|e| grokhub_core::parse_reasoning_effort(&e).map(str::to_string).unwrap_or(e))
            }
        };
        if row.is_none() {
            effort = effort.map(|e| grokhub_core::parse_reasoning_effort(&e).map(str::to_string).unwrap_or(e));
        }
        let what = row.map(|r| r.plain).unwrap_or("this step");
        let clause = if row.is_some() { why_clause(what, &rules[effort_rules_at..]) } else { format!("{what} keeps its set effort") };
        let reason = format!("{}: {clause}.", effort_word(effort.as_deref()));
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
        assert_eq!(a.reason, "Medium: everyday chat.");
        assert_eq!(a.rule_ids, vec!["model:keep".to_string(), "class:chat:default".into()]);
        assert_eq!(a.candidates_n, 2);
        let easy = Router::choose(&input("chat:default", "grok-4.7", 0.1), &reg, &none, 10);
        assert_eq!((easy.effort.as_deref(), easy.reason.as_str()), (Some("low"), "Low: everyday chat, and this looks routine."));
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
        let r = Router::choose(&i, &reg, &none, 10);
        assert_eq!(r.effort.as_deref(), Some("xhigh"), "{:?}", r.rule_ids);
        assert!(r.rule_ids.contains(&"clamp".to_string()));
        // A guard revert raises chat:default's start and floor by one.
        let mut g = input("chat:default", "grok-4.5", 0.5);
        g.bump = 1;
        let r = Router::choose(&g, &listed(&["grok-4.5"]), &none, 10);
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
