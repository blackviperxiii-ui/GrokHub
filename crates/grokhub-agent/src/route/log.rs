//! The `route` record on each model-call span, and `/why`.
//! Records hold ids, counts and one plain sentence. Never prompt text, never secrets.

use std::path::Path;

use serde::{Deserialize, Serialize};

use grokhub_core::model_registry::profile::RuntimeSettings;
use grokhub_core::model_registry::Registry;

use crate::harness::{read_spans_tail, Span};

/// Span session for model calls (shared with [`super::cabin::MODEL_TRACE`]).
pub const ROUTE_TRACE: &str = super::cabin::MODEL_TRACE;
/// `/why` prints at most this many reasons.
pub const WHY_LINES: usize = 10;
/// Span lines `/why` reads from the end of the model-call log.
pub const WHY_SCAN_LINES: usize = 400;

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
    /// No spend budget exists yet, so this stays `null`.
    pub budget_pct: Option<u8>,
    /// No privacy routing exists yet, so this stays `null`.
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
}

/// `{span_id, episode, step, class, d, ctx_tokens, signals, candidates_n,
/// chosen, prev, rule_ids, reason, pinned, outcome}` plus `used` (what the call
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
    pub pinned: bool,
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
}

// `d` and `cost_usd` are finite (rounded rules and token math), so equality is total.
impl Eq for RouteRecord {}

/// A multi-turn task should get at least this share of its prompt from the cache.
pub const CACHE_FLOOR_PCT: u64 = 80;

/// Every route record in the scanned tail, oldest first.
fn scanned_routes(config_dir: &Path) -> Vec<(u64, RouteRecord)> {
    let (spans, _) = read_spans_tail(config_dir, ROUTE_TRACE, WHY_SCAN_LINES);
    spans.into_iter().filter_map(|s: Span| s.route.map(|r| (s.ts_ms, *r))).collect()
}

/// The last [`WHY_LINES`] route records, oldest first.
pub fn last_routes(config_dir: &Path) -> Vec<(u64, RouteRecord)> {
    let mut out = scanned_routes(config_dir);
    let skip = out.len().saturating_sub(WHY_LINES);
    out.drain(..skip);
    out
}

/// `cached_tokens / prompt_tokens` of the newest multi-turn episode, after
/// its first call (which never has a cache), as a percent.
pub fn episode_cache_pct(routes: &[(u64, RouteRecord)]) -> Option<(String, u64)> {
    let newest = routes.iter().rev().filter(|(_, r)| !r.episode.is_empty() && r.outcome.is_some()).find_map(|(_, r)| {
        let calls: Vec<&RouteRecord> = routes.iter().map(|(_, x)| x).filter(|x| x.episode == r.episode && x.outcome.is_some()).collect();
        (calls.len() >= 2).then_some((r.episode.clone(), calls))
    })?;
    let (episode, calls) = newest;
    let (cached, input) = calls[1..].iter().filter_map(|r| r.outcome.as_ref()).fold((0, 0), |(c, i), o| (c + o.tokens.cached, i + o.tokens.input));
    (input > 0).then(|| (episode, cached * 100 / input))
}

/// `/why`: one line per recent route, at most [`WHY_LINES`]. When the newest
/// multi-turn task got less than [`CACHE_FLOOR_PCT`] of its prompt from the
/// cache, the oldest line makes room for that flag.
pub fn why_text(config_dir: &Path) -> String {
    let scanned = scanned_routes(config_dir);
    let mut routes = last_routes(config_dir);
    if routes.is_empty() {
        return "No routed model calls yet. Effort is automatic: each step's reason shows here once it runs.".into();
    }
    let flag = episode_cache_pct(&scanned).filter(|(_, pct)| *pct < CACHE_FLOOR_PCT).map(|(_, pct)| {
        format!("Cache: the last multi-turn task got {pct}% of its prompt from the cache (under {CACHE_FLOOR_PCT}%), so it paid for more of it.")
    });
    if flag.is_some() && routes.len() >= WHY_LINES {
        routes.remove(0);
    }
    let mut lines: Vec<String> = routes
        .iter()
        .map(|(_, r)| {
            let same = r.used.effort == r.chosen.effort;
            let used = if same {
                String::new()
            } else {
                format!(" (this session sent {})", super::effort_word(r.used.effort.as_deref()))
            };
            format!("{} · {} · {}{}", r.class, r.used.model, r.reason, used)
        })
        .collect();
    lines.extend(flag);
    lines.join("\n")
}

/// `/why models`: one line per model with state, usable and reason.
pub fn why_models_text(reg: &Registry, profiles: &std::collections::BTreeMap<String, grokhub_core::model_registry::profile::ModelProfile>) -> String {
    if reg.models.is_empty() {
        return "No model list yet. It refreshes 30 s after start, then every 6 hours.".into();
    }
    reg.models
        .values()
        .map(|r| {
            let (usable, why) = match profiles.get(&r.meta.id) {
                Some(p) if p.usable => ("usable".to_string(), String::new()),
                Some(p) => ("not usable".to_string(), format!(" {}", p.unusable_reason.clone().unwrap_or_default())),
                None => ("no profile yet".to_string(), String::new()),
            };
            format!("{} · {} · {}. {}{}", r.meta.id, r.state.as_str(), usable, r.reason, why).trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
