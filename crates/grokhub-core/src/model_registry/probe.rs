//! The onboarding capability probe: fixed synthetic prompts, compiled in, never
//! user data. It checks a tool call round-trip, JSON mode, caching and speed per
//! advertised effort, inside a hard token cap and a daily run cap. It only runs
//! on `included` routes. The transport and the egress guard are injected: the
//! real ones (in grokhub-agent) send through `guard_egress` with the key the
//! native engine already loads.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::cost_class::CostClass;
use super::profile::{EffortTiming, ProbeResult};
use super::record::ModelMeta;
use super::EFFORT_LADDER;

/// One run is one model (the plan's "≤ 5 probes a day").
pub const PROBE_RUNS_PER_DAY: u32 = 5;
pub const PROBE_CALLS_PER_RUN: u32 = 12;
pub const PROBE_MAX_OUTPUT_TOKENS: u64 = 64;
/// In + out + reasoning. Hard: a call that could cross it is not sent.
pub const PROBE_TOKEN_CAP_PER_RUN: u64 = 6_000;
pub const PROBE_TIMEOUT_SECS: u64 = 30;
pub const PROBE_MIN_GAP_SECS: u64 = 10;
/// Reasoning tokens assumed for a call before any call has reported some.
pub const PROBE_REASONING_ALLOWANCE: u64 = 256;
/// Retry a model after a failed or capped probe this long later (or on its next `changed`).
pub const PROBE_RETRY_MS: u64 = 24 * 60 * 60 * 1000;

pub const PROBE_TOOL_NAME: &str = "probe_echo";
pub const PROBE_TOOL_PROMPT: &str = "Call the probe_echo tool once with word set to ping. Do not write anything else.";
pub const PROBE_JSON_PROMPT: &str = "Reply with a JSON object where a is \"ok\", b is 3 and c is true.";
pub const PROBE_SPEED_PROMPT: &str = "Count from 1 to 10, separated by spaces.";
/// The cache check sends this exact prefix twice (a few thousand characters).
pub const PROBE_CACHE_LINE: &str = "GrokHub probe cache line: the quick brown fox jumps over the lazy dog while seven calm owls count the stars above the quiet harbor. ";
pub const PROBE_CACHE_REPEAT: usize = 24;
pub const PROBE_CACHE_QUESTION: &str = "How many owls are in the line above? Answer with one number.";

pub const FAIL_TOOL: &str = "Tool calls didn't come back in the right shape.";
pub const FAIL_JSON: &str = "JSON mode broke.";
pub const FAIL_TIMEOUT: &str = "Didn't answer within 30 s.";

/// Every fixed text a probe can send, for the no-user-data check.
pub fn probe_texts() -> Vec<String> {
    vec![
        PROBE_TOOL_PROMPT.into(),
        PROBE_JSON_PROMPT.into(),
        PROBE_SPEED_PROMPT.into(),
        cache_prefix(),
        PROBE_CACHE_QUESTION.into(),
    ]
}

pub fn cache_prefix() -> String {
    PROBE_CACHE_LINE.repeat(PROBE_CACHE_REPEAT)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeKind {
    Tool,
    Json,
    Cache,
    Speed,
}

impl ProbeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tool => "tool",
            Self::Json => "json",
            Self::Cache => "cache",
            Self::Speed => "speed",
        }
    }
}

/// One probe call: a Responses API body built only from the consts above.
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeCall {
    pub model: String,
    pub effort: Option<String>,
    pub kind: ProbeKind,
    pub body: Value,
    pub timeout_secs: u64,
}

/// What came back. Counts and shape only; `text` is the model's reply to a
/// fixed prompt and never leaves the probe.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProbeReply {
    pub status: u16,
    pub timed_out: bool,
    pub latency_ms: u64,
    pub served_model: Option<String>,
    pub input_tokens: u64,
    pub cached_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub cost_ticks: i64,
    pub text: String,
    /// `(name, arguments)` of each function call.
    pub tool_calls: Vec<(String, String)>,
}

pub trait ProbeTransport {
    fn send(&mut self, call: &ProbeCall) -> Result<ProbeReply, String>;
}

/// What a probe run needs from the outside world.
pub struct ProbeEnv<'a> {
    pub transport: &'a mut dyn ProbeTransport,
    /// The egress guard for the xAI destination; `false` stops the run.
    pub guard: &'a mut dyn FnMut() -> bool,
    /// Wait this many ms between calls.
    pub wait: &'a mut dyn FnMut(u64),
    /// Called after each call (the agent writes a `background:probe` span).
    pub on_call: &'a mut dyn FnMut(&ProbeCall, &ProbeReply),
}

fn body(model: &str, effort: Option<&str>, input: Value) -> Value {
    let mut b = json!({
        "model": model,
        "input": input,
        "max_output_tokens": PROBE_MAX_OUTPUT_TOKENS,
        "store": false,
    });
    if let Some(e) = effort {
        b["reasoning"] = json!({"effort": e});
    }
    b
}

fn user(text: &str) -> Value {
    json!([{"role": "user", "content": text}])
}

pub fn tool_call(model: &str, effort: Option<&str>) -> ProbeCall {
    let mut b = body(model, effort, user(PROBE_TOOL_PROMPT));
    b["tools"] = json!([{
        "type": "function",
        "name": PROBE_TOOL_NAME,
        "description": "Echo one fixed word.",
        "parameters": {"type": "object", "properties": {"word": {"type": "string"}}, "required": ["word"], "additionalProperties": false}
    }]);
    ProbeCall { model: model.into(), effort: effort.map(str::to_string), kind: ProbeKind::Tool, body: b, timeout_secs: PROBE_TIMEOUT_SECS }
}

pub fn json_call(model: &str, effort: Option<&str>) -> ProbeCall {
    let mut b = body(model, effort, user(PROBE_JSON_PROMPT));
    b["text"] = json!({"format": {
        "type": "json_schema",
        "name": "probe_shape",
        "strict": true,
        "schema": {
            "type": "object",
            "properties": {"a": {"type": "string"}, "b": {"type": "integer"}, "c": {"type": "boolean"}},
            "required": ["a", "b", "c"],
            "additionalProperties": false
        }
    }});
    ProbeCall { model: model.into(), effort: effort.map(str::to_string), kind: ProbeKind::Json, body: b, timeout_secs: PROBE_TIMEOUT_SECS }
}

pub fn cache_call(model: &str, effort: Option<&str>) -> ProbeCall {
    let input = json!([
        {"role": "system", "content": cache_prefix()},
        {"role": "user", "content": PROBE_CACHE_QUESTION}
    ]);
    ProbeCall { model: model.into(), effort: effort.map(str::to_string), kind: ProbeKind::Cache, body: body(model, effort, input), timeout_secs: PROBE_TIMEOUT_SECS }
}

pub fn speed_call(model: &str, effort: Option<&str>) -> ProbeCall {
    ProbeCall {
        model: model.into(),
        effort: effort.map(str::to_string),
        kind: ProbeKind::Speed,
        body: body(model, effort, user(PROBE_SPEED_PROMPT)),
        timeout_secs: PROBE_TIMEOUT_SECS,
    }
}

/// Input tokens a call could use (4 characters a token, rounded up, plus framing).
pub fn estimate_input(call: &ProbeCall) -> u64 {
    (call.body.to_string().len() as u64).div_ceil(4) + 16
}

fn tool_ok(reply: &ProbeReply) -> bool {
    reply.tool_calls.len() == 1
        && reply.tool_calls[0].0 == PROBE_TOOL_NAME
        && serde_json::from_str::<Value>(&reply.tool_calls[0].1).ok().is_some_and(|v| v == json!({"word": "ping"}))
}

fn json_ok(reply: &ProbeReply) -> bool {
    serde_json::from_str::<Value>(reply.text.trim()).ok().is_some_and(|v| v == json!({"a": "ok", "b": 3, "c": true}))
}

/// The model's advertised efforts, lowest first (only names on the ladder).
pub fn efforts_low_first(meta: &ModelMeta) -> Vec<String> {
    let mut e: Vec<(usize, String)> = meta
        .efforts
        .clone()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|x| EFFORT_LADDER.iter().position(|l| *l == x).map(|r| (r, x)))
        .collect();
    e.sort_by_key(|(r, _)| *r);
    e.dedup();
    e.into_iter().map(|(_, x)| x).collect()
}

/// Run the probe for one model. `cost` must be `included` or nothing is sent.
pub fn run_probe(meta: &ModelMeta, cost: CostClass, env: ProbeEnv<'_>) -> ProbeResult {
    if cost != CostClass::Included {
        return ProbeResult::status("not_run: cost_class");
    }
    let efforts = efforts_low_first(meta);
    let lowest = efforts.first().cloned();
    let mut plan: Vec<ProbeCall> = Vec::new();
    if meta.tool_calling != Some(false) {
        plan.push(tool_call(&meta.id, lowest.as_deref()));
    }
    if meta.structured_output != Some(false) {
        plan.push(json_call(&meta.id, lowest.as_deref()));
    }
    plan.push(cache_call(&meta.id, lowest.as_deref()));
    plan.push(cache_call(&meta.id, lowest.as_deref()));
    if efforts.is_empty() {
        plan.push(speed_call(&meta.id, None));
    }
    for e in &efforts {
        plan.push(speed_call(&meta.id, Some(e)));
    }
    plan.truncate(PROBE_CALLS_PER_RUN as usize);
    let mut out = ProbeResult::status("ok");
    let mut max_reasoning = PROBE_REASONING_ALLOWANCE;
    let mut cache_first: Option<u64> = None;
    for call in &plan {
        let next = estimate_input(call) + PROBE_MAX_OUTPUT_TOKENS + max_reasoning;
        if out.tokens + next > PROBE_TOKEN_CAP_PER_RUN {
            break;
        }
        if out.calls > 0 {
            (env.wait)(PROBE_MIN_GAP_SECS * 1000);
        }
        if !(env.guard)() {
            if out.calls == 0 {
                return ProbeResult::status("not_run: egress");
            }
            break;
        }
        let reply = match env.transport.send(call) {
            Ok(r) => r,
            Err(_) => ProbeReply { status: 0, ..ProbeReply::default() },
        };
        out.calls += 1;
        out.tokens += reply.input_tokens + reply.output_tokens + reply.reasoning_tokens;
        out.cost_ticks += reply.cost_ticks;
        max_reasoning = max_reasoning.max(reply.reasoning_tokens);
        (env.on_call)(call, &reply);
        if reply.timed_out || reply.latency_ms > PROBE_TIMEOUT_SECS * 1000 {
            out.failure = Some(FAIL_TIMEOUT.into());
            break;
        }
        if !(200..300).contains(&reply.status) {
            out.failure = Some(if reply.status == 0 { "The check couldn't reach it.".into() } else { format!("It answered HTTP {}.", reply.status) });
            break;
        }
        match call.kind {
            ProbeKind::Tool => {
                out.tool_call = Some(tool_ok(&reply));
                if out.tool_call == Some(false) {
                    out.failure = Some(FAIL_TOOL.into());
                    break;
                }
            }
            ProbeKind::Json => {
                out.json_mode = Some(json_ok(&reply));
                if out.json_mode == Some(false) {
                    out.failure = Some(FAIL_JSON.into());
                    break;
                }
            }
            ProbeKind::Cache => match cache_first {
                None => cache_first = Some(reply.cached_tokens),
                Some(_) => {
                    out.cached_tokens = Some(reply.cached_tokens);
                    out.caching = Some(reply.cached_tokens > 0);
                }
            },
            ProbeKind::Speed => out.efforts.push(EffortTiming {
                effort: call.effort.clone().unwrap_or_else(|| "none".into()),
                measured: "probe".into(),
                reply_ms: Some(reply.latency_ms),
                tokens_per_sec: (reply.latency_ms > 0).then(|| reply.output_tokens * 1000 / reply.latency_ms),
            }),
        }
    }
    for e in &efforts {
        if !out.efforts.iter().any(|t| &t.effort == e) {
            out.efforts.push(EffortTiming { effort: e.clone(), measured: "passive".into(), reply_ms: None, tokens_per_sec: None });
        }
    }
    if out.failure.is_some() {
        out.status = "failed".into();
    }
    out
}

/// The onboarding queue: today's default model first, then in listing order,
/// at most [`PROBE_RUNS_PER_DAY`] runs a day. Stored as `models/onboarding.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Onboarding {
    #[serde(default)]
    pub queue: Vec<String>,
    /// Days since the epoch (UTC) of `runs_today`.
    #[serde(default)]
    pub day: u64,
    #[serde(default)]
    pub runs_today: u32,
}

pub const DAY_MS: u64 = 24 * 60 * 60 * 1000;

impl Onboarding {
    /// Queue `ids` (added or changed models). `default_model` goes to the front.
    pub fn enqueue(&mut self, ids: &[String], default_model: &str) {
        for id in ids {
            if !self.queue.contains(id) {
                self.queue.push(id.clone());
            }
        }
        if let Some(pos) = self.queue.iter().position(|q| q == default_model.trim()) {
            let d = self.queue.remove(pos);
            self.queue.insert(0, d);
        }
    }

    pub fn queued(&self, id: &str) -> bool {
        self.queue.iter().any(|q| q == id)
    }

    /// The next model to probe, spending one of today's runs, or `None` when the
    /// queue is empty or today's runs are spent.
    pub fn take_next(&mut self, now_ms: u64) -> Option<String> {
        let day = now_ms / DAY_MS;
        if day != self.day {
            self.day = day;
            self.runs_today = 0;
        }
        if self.queue.is_empty() || self.runs_today >= PROBE_RUNS_PER_DAY {
            return None;
        }
        self.runs_today += 1;
        Some(self.queue.remove(0))
    }
}

/// One line of `models/probe_log.jsonl` per run. Counts only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeLogLine {
    pub ts_ms: u64,
    pub model: String,
    pub status: String,
    pub calls: u32,
    pub tokens: u64,
    pub cost_ticks: i64,
    pub usable: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_registry::profile::ModelProfile;

    /// A counting fake: it never touches the network.
    struct Fake {
        sent: Vec<ProbeCall>,
        reply: Box<dyn FnMut(&ProbeCall) -> ProbeReply>,
    }

    impl ProbeTransport for Fake {
        fn send(&mut self, call: &ProbeCall) -> Result<ProbeReply, String> {
            self.sent.push(call.clone());
            Ok((self.reply)(call))
        }
    }

    fn good(call: &ProbeCall) -> ProbeReply {
        let mut r = ProbeReply { status: 200, latency_ms: 400, input_tokens: 60, output_tokens: 10, reasoning_tokens: 20, cost_ticks: 1_000, ..ProbeReply::default() };
        match call.kind {
            ProbeKind::Tool => r.tool_calls = vec![(PROBE_TOOL_NAME.into(), r#"{"word":"ping"}"#.into())],
            ProbeKind::Json => r.text = r#"{"a":"ok","b":3,"c":true}"#.into(),
            ProbeKind::Cache => {
                r.input_tokens = 900;
                r.cached_tokens = 768;
            }
            ProbeKind::Speed => r.text = "1 2 3 4 5 6 7 8 9 10".into(),
        }
        r
    }

    fn meta() -> ModelMeta {
        ModelMeta { id: "grok-4.7".into(), efforts: Some(vec!["high".into(), "low".into(), "medium".into()]), ..ModelMeta::default() }
    }

    fn run(m: &ModelMeta, cost: CostClass, fake: &mut Fake, guard_ok: bool) -> (ProbeResult, u32, Vec<u64>, u32) {
        let mut guards = 0;
        let mut waits = Vec::new();
        let mut logged = 0;
        let mut guard = || {
            guards += 1;
            guard_ok
        };
        let mut wait = |ms| waits.push(ms);
        let mut on_call = |_: &ProbeCall, _: &ProbeReply| logged += 1;
        let r = run_probe(m, cost, ProbeEnv { transport: fake, guard: &mut guard, wait: &mut wait, on_call: &mut on_call });
        (r, guards, waits, logged)
    }

    #[test]
    fn a_good_model_passes_every_check_lowest_effort_first_with_one_guard_per_call() {
        let mut fake = Fake { sent: Vec::new(), reply: Box::new(good) };
        let (r, guards, waits, logged) = run(&meta(), CostClass::Included, &mut fake, true);
        assert_eq!(r.status, "ok");
        assert_eq!((r.tool_call, r.json_mode, r.caching, r.cached_tokens), (Some(true), Some(true), Some(true), Some(768)));
        assert_eq!(fake.sent.len(), 7);
        assert_eq!(guards, 7);
        assert_eq!(logged, 7);
        assert_eq!(waits, vec![10_000; 6]);
        assert_eq!(fake.sent[0].effort.as_deref(), Some("low"));
        let speed: Vec<_> = r.efforts.iter().map(|t| (t.effort.as_str(), t.measured.as_str())).collect();
        assert_eq!(speed, vec![("low", "probe"), ("medium", "probe"), ("high", "probe")]);
        assert_eq!(r.efforts[0].tokens_per_sec, Some(25));
        assert_eq!(r.tokens, 5 * 90 + 2 * 930);
        assert_eq!(r.cost_ticks, 7_000);
        assert!(fake.sent.iter().all(|c| c.body["max_output_tokens"] == 64 && c.body["store"] == false));
    }

    #[test]
    fn a_non_included_route_never_probes() {
        for cost in [CostClass::Metered, CostClass::Unknown] {
            let mut fake = Fake { sent: Vec::new(), reply: Box::new(good) };
            let (r, guards, _, _) = run(&meta(), cost, &mut fake, true);
            assert_eq!(r.status, "not_run: cost_class");
            assert_eq!((fake.sent.len(), guards), (0, 0));
        }
    }

    #[test]
    fn a_refused_egress_sends_nothing() {
        let mut fake = Fake { sent: Vec::new(), reply: Box::new(good) };
        let (r, guards, _, _) = run(&meta(), CostClass::Included, &mut fake, false);
        assert_eq!((r.status.as_str(), fake.sent.len(), guards), ("not_run: egress", 0, 1));
    }

    #[test]
    fn a_bad_tool_call_shape_is_unusable_with_its_reason() {
        let mut fake = Fake {
            sent: Vec::new(),
            reply: Box::new(|c| {
                let mut r = good(c);
                if c.kind == ProbeKind::Tool {
                    r.tool_calls = vec![("probe_echo".into(), r#"{"text":"ping"}"#.into())];
                }
                r
            }),
        };
        let (r, _, _, _) = run(&meta(), CostClass::Included, &mut fake, true);
        assert_eq!((r.status.as_str(), r.tool_call, r.failure.as_deref()), ("failed", Some(false), Some(FAIL_TOOL)));
        assert_eq!(fake.sent.len(), 1);
        let p = ModelProfile::build(&meta(), r, 1);
        assert_eq!((p.usable, p.unusable_reason.as_deref()), (false, Some(FAIL_TOOL)));
    }

    #[test]
    fn broken_json_and_a_slow_answer_are_unusable() {
        let mut fake = Fake {
            sent: Vec::new(),
            reply: Box::new(|c| {
                let mut r = good(c);
                if c.kind == ProbeKind::Json {
                    r.text = "{\"a\":\"ok\"".into();
                }
                r
            }),
        };
        assert_eq!(run(&meta(), CostClass::Included, &mut fake, true).0.failure.as_deref(), Some(FAIL_JSON));
        let mut slow = Fake { sent: Vec::new(), reply: Box::new(|c| ProbeReply { latency_ms: 30_001, ..good(c) }) };
        assert_eq!(run(&meta(), CostClass::Included, &mut slow, true).0.failure.as_deref(), Some(FAIL_TIMEOUT));
    }

    #[test]
    fn a_run_stops_before_crossing_the_token_cap_and_marks_the_rest_passive() {
        let mut fake = Fake { sent: Vec::new(), reply: Box::new(|c| ProbeReply { reasoning_tokens: 1_400, ..good(c) }) };
        let (r, _, _, _) = run(&meta(), CostClass::Included, &mut fake, true);
        assert!(r.tokens <= PROBE_TOKEN_CAP_PER_RUN, "{}", r.tokens);
        assert!(fake.sent.len() < 7, "{}", fake.sent.len());
        assert_eq!(r.status, "ok");
        assert!(r.efforts.iter().any(|t| t.measured == "passive" && t.reply_ms.is_none()));
    }

    #[test]
    fn the_sixth_run_in_a_day_is_refused_and_the_default_goes_first() {
        let mut q = Onboarding::default();
        let ids: Vec<String> = (0..8).map(|i| format!("grok-{i}")).collect();
        q.enqueue(&ids, "grok-6");
        assert_eq!(q.queue[0], "grok-6");
        let day = 20_000 * DAY_MS;
        let got: Vec<_> = (0..5).map(|_| q.take_next(day + 5).unwrap()).collect();
        assert_eq!(got, vec!["grok-6", "grok-0", "grok-1", "grok-2", "grok-3"]);
        assert_eq!(q.take_next(day + 10), None);
        assert_eq!(q.take_next(day + DAY_MS).as_deref(), Some("grok-4"));
    }

    #[test]
    fn probe_prompts_contain_no_user_data() {
        let seeded = ["Jeremy's bank PIN is 4512", "sk-abcdefghijklmnopqrstuv", "meet Sam at 5pm", "MEMORY.md", "/home/"];
        let mut fake = Fake { sent: Vec::new(), reply: Box::new(good) };
        run(&meta(), CostClass::Included, &mut fake, true);
        let wire: String = fake.sent.iter().map(|c| c.body.to_string()).collect();
        for s in seeded {
            assert!(!wire.contains(s), "{s}");
            assert!(!probe_texts().iter().any(|t| t.contains(s)), "{s}");
        }
    }
}
