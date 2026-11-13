//! The `route` record on each model-call span.
//! Records hold ids, counts and one plain sentence. Never prompt text, never secrets.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use grokhub_core::model_registry::profile::RuntimeSettings;

use crate::harness::{span_path, Span, SpanTail};

/// Span session for model calls (shared with [`super::cabin::MODEL_TRACE`]).
pub const ROUTE_TRACE: &str = super::cabin::MODEL_TRACE;

/// Provider, model and effort.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chosen {
    pub provider: String,
    pub model: String,
    /// `None` sends no reasoning field.
    pub effort: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RouteSignals {
    pub failures: u32,
    /// VerifyGate's last word on this episode (`ok` / `reject`), or `null`.
    pub verify: Option<String>,
    pub origin: Option<String>,
    /// This call's wall time, ms.
    pub latency: Option<u64>,
    /// The week's budget used, percent (R2b), or `null` with no cap and no limit hit.
    pub budget_pct: Option<u8>,
    /// `local_only` when sensitive data with no cloud grant kept the step on the device (R3a; off by default).
    pub privacy: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteTokens {
    #[serde(rename = "in")]
    pub input: u64,
    pub cached: u64,
    pub out: u64,
    pub reasoning: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RouteOutcome {
    pub ok: bool,
    pub verify: Option<String>,
    pub tokens: RouteTokens,
    pub cost_usd: f64,
    /// R2b: the call hit the plan's usage limit (429 `free-usage-exhausted`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub limit: bool,
    /// Hosted search the reply ran and was billed for.
    #[serde(default, skip_serializing_if = "crate::client::HostedUsage::is_empty")]
    pub hosted: crate::client::HostedUsage,
    /// Pages the reply cited.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub sources: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// `{span_id, episode, step, class, d, ctx_tokens, signals, candidates_n,
/// chosen, prev, rule_ids, reason, outcome}` plus `used` (what the call
/// really sent) and `settings` (the profile's runtime settings for `chosen`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RouteRecord {
    #[serde(default)]
    pub span_id: String,
    #[serde(default)]
    pub episode: String,
    #[serde(default)]
    pub step: u32,
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub d: f64,
    #[serde(default)]
    pub ctx_tokens: u64,
    #[serde(default)]
    pub signals: RouteSignals,
    #[serde(default)]
    pub candidates_n: u32,
    /// What the router picked. Its model and effort are sent for listed classes.
    #[serde(default)]
    pub chosen: Chosen,
    /// The previous route's pick in the same episode, if any.
    #[serde(default)]
    pub prev: Option<Chosen>,
    #[serde(default)]
    pub rule_ids: Vec<String>,
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub outcome: Option<RouteOutcome>,
    /// What the call actually sent.
    #[serde(default)]
    pub used: Chosen,
    /// The pick was logged only (an unlisted class, or an R0 record).
    #[serde(default)]
    pub shadow: bool,
    /// The episode is in the accuracy guard's internal control arm. Never shown.
    #[serde(default)]
    pub holdout: bool,
    #[serde(default)]
    pub settings: Option<RuntimeSettings>,
    /// R2b: the sent route's cost class (`included`, `autonomous_premium`, `premium`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cost_class: String,
    /// R3a self-tuning: `canary:<candidate>` when a canary served the step,
    /// `shadow:<candidate>` when one was computed next to it (`!` when it broke a filter).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tune: Option<String>,
    /// How long the router's decision took, per step (µs). Speed only.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub timing_us: BTreeMap<String, u32>,
    /// Brain 2: the turn's goal (`speed`, `quality`, `cost`, `balanced`) and
    /// the policy row's reason, empty when no Brief named one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub goal: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub goal_reason: String,
    /// An on-device step: the governor's level (`normal`, `throttle`,
    /// `ramp_up`) and the installed model's hardware tier (`T1`…`T5`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub gov_state: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tier: String,
    /// This request rewrote the cached prompt prefix: where, and why. Its
    /// cache share drops with it, so the cause sits next to the numbers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix_break: Option<BreakNote>,
    /// The exact request this call sent, by hash (`harness::replay`); empty
    /// when none was stored (no learned-tier key, the store switched off, or
    /// an on-device step).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub request: String,
    /// Failover PR 7: models that stopped inside this call and the ones that
    /// took over, in order. Empty on any call that never switched.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub handoffs: Vec<HandoffLine>,
}

/// One model switch inside a call (failover PR 7): the model that stopped,
/// the one that took over, and the rule that names why it stopped
/// (`handoff:limit`, `handoff:credit`, `handoff:outage`, …). Ids and a rule
/// only — never prompt text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffLine {
    pub from: String,
    pub to: String,
    pub rule: String,
}

/// Where a request first stopped repeating the one before it, and why
/// ([`crate::prefix_guard::PrefixBreak`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BreakNote {
    /// `system`, `tools` or `input`.
    pub part: String,
    /// Segment index in cache order.
    pub index: u32,
    /// `compaction`, `trim`, `model_switch`, `effort_switch`, `tools_appended` or `unexpected`.
    pub reason: String,
}

impl From<&crate::prefix_guard::PrefixBreak> for BreakNote {
    fn from(b: &crate::prefix_guard::PrefixBreak) -> Self {
        Self {
            part: b.part.as_str().to_string(),
            index: u32::try_from(b.index).unwrap_or(u32::MAX),
            reason: b.reason.as_str().to_string(),
        }
    }
}

// `d` and `cost_usd` are finite (rounded rules and token math), so equality is total.
impl Eq for RouteRecord {}

/// A multi-turn task should get at least this share of its prompt from the cache.
pub const CACHE_FLOOR_PCT: u64 = 80;

/// Lines of the model-call log kept followed in memory: the most any reader
/// scans (the week's spend and the accuracy guard).
pub const ROUTE_TAIL_LINES: usize = 20_000;

static ROUTE_TAIL: Mutex<Option<SpanTail<(u64, RouteRecord)>>> = Mutex::new(None);

fn keep_route(s: Span) -> Option<(u64, RouteRecord)> {
    s.route.map(|r| (s.ts_ms, *r))
}

/// Route records in the newest `lines` lines of the model-call log,
/// `(ts_ms, record)`, oldest first: what `read_spans_tail` over the log
/// returns, from a tail kept in memory that parses only lines appended since
/// the last read.
pub fn route_records(config_dir: &Path, lines: usize) -> Vec<(u64, RouteRecord)> {
    with_route_records(config_dir, lines, |records| records.cloned().collect())
}

/// [`route_records`] borrowed: `f` reads them in place, with no copy.
pub fn with_route_records<R>(
    config_dir: &Path,
    lines: usize,
    f: impl FnOnce(&mut dyn Iterator<Item = &(u64, RouteRecord)>) -> R,
) -> R {
    let path = span_path(config_dir, ROUTE_TRACE);
    let mut slot = ROUTE_TAIL.lock().unwrap_or_else(|e| e.into_inner());
    let tail = match slot.take() {
        Some(t) if t.path() == path => slot.insert(t),
        _ => slot.insert(SpanTail::new(path, ROUTE_TAIL_LINES, keep_route)),
    };
    tail.refresh();
    let mut records = tail.newest_iter(lines.min(ROUTE_TAIL_LINES));
    f(&mut records)
}
