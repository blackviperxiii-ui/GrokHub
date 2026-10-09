//! Router R3a self-tuning (Jeremy, Oct 7: "the router tunes itself"). Pure:
//! the steps, the live state and the clock are passed in, and nothing here
//! reads or writes a file ([`super::learn`] does the I/O).
//!
//! Live outcome data feeds per-model, per-class scorecards. The weekly pass
//! builds candidates from them: a class's first model ([`Change::Order`]) or
//! its start rung ([`Change::Start`]), both inside the class floor and ceiling.
//! A candidate that is cheaper or faster runs in shadow (logged only), then as
//! a canary on [`CANARY_SHARE`] of eligible low-risk steps, and is promoted
//! only when quality holds and cost or latency improves by [`MIN_GAIN_PCT`].
//! For [`WATCH_DAYS`] after promotion a regression rolls it back at once. A
//! candidate that would cost more money is never applied here: it becomes a
//! Suggestion card and waits for your Accept click.
//!
//! The tuner never touches floors, ceilings, cost classes, consent, the
//! hard-class list, harness policy or the budget cap, and never lowers
//! `prepare:hard`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use grokhub_core::model_registry::EFFORT_LADDER;

use super::ladder::{rung, turn_hash, Band, PREPARE_HARD};
use super::policy::{class_row, CLASS_TABLE};
use super::table::{RoutingTable, TableRow};

/// Eligible steps a candidate is computed on, logged only, before its canary.
pub const SHADOW_MIN_STEPS: u32 = 200;
/// Share of eligible low-risk steps the canary serves.
pub const CANARY_SHARE: f64 = 0.05;
/// Canary steps needed before a promotion is judged.
pub const CANARY_MIN_N: u32 = 50;
/// VerifyGate pass rate may drop at most this many points.
pub const QUALITY_MAX_DROP_PTS: f64 = 2.0;
/// Rework rate may rise at most this many points.
pub const REWORK_MAX_RISE_PTS: f64 = 2.0;
/// Cost per step or p50 latency must improve by at least this much.
pub const MIN_GAIN_PCT: f64 = 10.0;
pub const MAX_PROMOTIONS_PER_WEEK: usize = 2;
/// Days a promotion is watched for a regression.
pub const WATCH_DAYS: u64 = 7;
/// New steps in the class before a rollback is judged.
pub const ROLLBACK_MIN_N: u32 = 30;
/// A rolled-back or failed candidate is not tried again for this long.
pub const REJECT_DAYS: u64 = 14;
/// Scorecards cover this rolling window.
pub const SCORECARD_DAYS: u64 = 28;
/// Steps a model (or rung) needs on its scorecard before it can be a candidate.
pub const CARD_MIN_N: u32 = 20;
/// Cost-raising Suggestion cards a week (inside the self-review's 5).
pub const ROUTER_CARDS_PER_WEEK: usize = 2;
/// A cost-raising card needs this much better pass rate.
pub const CARD_MIN_QUALITY_GAIN_PTS: f64 = 5.0;

pub const DAY_MS: u64 = 24 * 60 * 60 * 1000;
pub const WEEK_MS: u64 = 7 * DAY_MS;

/// Which arm a step ran in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Arm {
    #[default]
    Live,
    /// Served by this candidate.
    Canary(String),
}

/// One finished routed step: counts only, never prompt content.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Step {
    pub at: u64,
    /// The table class (after aliases).
    pub class: String,
    pub model: String,
    pub effort: Option<String>,
    pub arm: Arm,
    /// VerifyGate passed (or, with no check, the Spike-7 outcome was a success). `None` when nothing says.
    pub pass: Option<bool>,
    /// A retry, an E1/E2 escalation or a correction followed.
    pub rework: bool,
    pub cost_usd: f64,
    pub latency_ms: u64,
}

/// Pass, rework, cost and latency over a set of steps.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Snap {
    pub n: u32,
    /// VerifyGate pass rate, percent.
    pub pass_pct: f64,
    /// Rework rate, percent.
    pub rework_pct: f64,
    pub cost_per_step: f64,
    pub p50_ms: u64,
    pub p95_ms: u64,
}

fn pct(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 * 100.0 / whole as f64
    }
}

fn quantile(sorted: &[u64], q: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let i = ((sorted.len() as f64 - 1.0) * q).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

impl Snap {
    pub fn of<'a>(steps: impl IntoIterator<Item = &'a Step>) -> Self {
        let steps: Vec<&Step> = steps.into_iter().collect();
        let judged: Vec<bool> = steps.iter().filter_map(|s| s.pass).collect();
        let mut lat: Vec<u64> = steps.iter().map(|s| s.latency_ms).collect();
        lat.sort_unstable();
        let n = steps.len();
        Self {
            n: n as u32,
            pass_pct: pct(judged.iter().filter(|p| **p).count(), judged.len()),
            rework_pct: pct(steps.iter().filter(|s| s.rework).count(), n),
            cost_per_step: if n == 0 { 0.0 } else { steps.iter().map(|s| s.cost_usd).sum::<f64>() / n as f64 },
            p50_ms: quantile(&lat, 0.5),
            p95_ms: quantile(&lat, 0.95),
        }
    }

    /// `self` is worse than `base` past either quality threshold.
    pub fn quality_fell(&self, base: &Snap) -> bool {
        base.pass_pct - self.pass_pct > QUALITY_MAX_DROP_PTS || self.rework_pct - base.rework_pct > REWORK_MAX_RISE_PTS
    }

    /// Percent cheaper per step than `base` (negative when pricier).
    pub fn cost_gain_pct(&self, base: &Snap) -> f64 {
        if base.cost_per_step <= 0.0 {
            0.0
        } else {
            (base.cost_per_step - self.cost_per_step) * 100.0 / base.cost_per_step
        }
    }

    /// Percent faster at p50 than `base`.
    pub fn latency_gain_pct(&self, base: &Snap) -> f64 {
        if base.p50_ms == 0 {
            0.0
        } else {
            (base.p50_ms as f64 - self.p50_ms as f64) * 100.0 / base.p50_ms as f64
        }
    }
}

/// One model at one effort in one class over the rolling window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scorecard {
    pub class: String,
    pub model: String,
    pub effort: Option<String>,
    #[serde(flatten)]
    pub snap: Snap,
}

/// `{config}/models/scorecards.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Scorecards {
    #[serde(default)]
    pub built_at: u64,
    #[serde(default)]
    pub window_days: u64,
    #[serde(default)]
    pub cards: Vec<Scorecard>,
}

/// Scorecards from the last [`SCORECARD_DAYS`] of steps, one per (class, model, effort).
pub fn scorecards(steps: &[Step], now_ms: u64) -> Scorecards {
    let since = now_ms.saturating_sub(SCORECARD_DAYS * DAY_MS);
    let mut groups: BTreeMap<(String, String, Option<String>), Vec<&Step>> = BTreeMap::new();
    for s in steps.iter().filter(|s| s.at >= since && s.at <= now_ms) {
        groups.entry((s.class.clone(), s.model.clone(), s.effort.clone())).or_default().push(s);
    }
    let cards = groups.into_iter().map(|((class, model, effort), v)| Scorecard { class, model, effort, snap: Snap::of(v) }).collect();
    Scorecards { built_at: now_ms, window_days: SCORECARD_DAYS, cards }
}

impl Scorecards {
    /// One model in one class, all efforts together.
    pub fn model(&self, class: &str, model: &str, steps: &[Step]) -> Snap {
        let since = self.built_at.saturating_sub(SCORECARD_DAYS * DAY_MS);
        Snap::of(steps.iter().filter(|s| s.class == class && s.model == model && s.at >= since))
    }
}

/// What a candidate changes. Only these two things are ever tuned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    /// This model goes first in the class's routing-table order.
    Order { model: String },
    /// The class starts at this rung (inside its floor and ceiling).
    Start { effort: String },
}

impl Change {
    pub fn label(&self) -> String {
        match self {
            Self::Order { model } => format!("{model} first"),
            Self::Start { effort } => format!("start at {}", grokhub_core::effort_label(effort)),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Computed next to the live table and logged only.
    #[default]
    Shadow,
    /// Serving [`CANARY_SHARE`] of eligible low-risk steps.
    Canary,
    /// Promoted; watched for [`WATCH_DAYS`].
    Watch,
    /// Watched a week with no regression.
    Kept,
    /// Rolled back, failed its canary, or undone; not retried until `until`.
    Rejected,
    /// Costs more money: a Suggestion card waits for your Accept.
    Card,
    /// You accepted the card.
    Accepted,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub id: String,
    pub class: String,
    pub change: Option<Change>,
    #[serde(default)]
    pub stage: Stage,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub canary_at: Option<u64>,
    #[serde(default)]
    pub promoted_at: Option<u64>,
    #[serde(default)]
    pub ended_at: Option<u64>,
    /// Rejected until this time.
    #[serde(default)]
    pub until: Option<u64>,
    #[serde(default)]
    pub reason: String,
    /// The live arm when it was promoted (what a rollback compares against).
    #[serde(default)]
    pub baseline: Option<Snap>,
    /// The canary arm when it was promoted.
    #[serde(default)]
    pub promoted_snap: Option<Snap>,
    /// For a card: about how much more it costs, percent.
    #[serde(default)]
    pub cost_rise_pct: Option<f64>,
    /// The tuning value this class had before the promotion (`None`: none).
    #[serde(default)]
    pub before: Option<String>,
}

impl Candidate {
    pub fn active(&self) -> bool {
        matches!(self.stage, Stage::Shadow | Stage::Canary | Stage::Watch)
    }
}

/// The applied tuning: `{config}/route_tuning.json`, written only through the
/// ChangeLedger. `starts` is merged over the class table (clamped) and `orders`
/// over the routing table at read time, so a table rebuild can't undo a promotion.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tuning {
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub starts: BTreeMap<String, String>,
    #[serde(default)]
    pub orders: BTreeMap<String, String>,
}

impl Tuning {
    pub fn is_empty(&self) -> bool {
        self.starts.is_empty() && self.orders.is_empty()
    }

    /// The class's current value for `change`'s kind.
    pub fn value(&self, class: &str, change: &Change) -> Option<String> {
        match change {
            Change::Order { .. } => self.orders.get(class).cloned(),
            Change::Start { .. } => self.starts.get(class).cloned(),
        }
    }

    /// Put `value` (or nothing) in for the class's `change` kind.
    pub fn set(&mut self, class: &str, change: &Change, value: Option<String>) {
        let map = match change {
            Change::Order { .. } => &mut self.orders,
            Change::Start { .. } => &mut self.starts,
        };
        match value {
            Some(v) => map.insert(class.to_string(), v),
            None => map.remove(class),
        };
    }

    pub fn apply(&mut self, class: &str, change: &Change) {
        let v = match change {
            Change::Order { model } => model.clone(),
            Change::Start { effort } => effort.clone(),
        };
        self.set(class, change, Some(v));
    }

    pub fn has(&self, class: &str, change: &Change) -> bool {
        let want = match change {
            Change::Order { model } => model,
            Change::Start { effort } => effort,
        };
        self.value(class, change).as_deref() == Some(want.as_str())
    }
}

/// `table` with `model` first in `class` (a row is added when it has none).
pub fn with_order(table: &RoutingTable, class: &str, model: &str) -> RoutingTable {
    let mut t = table.clone();
    let Some(ct) = t.classes.get_mut(class) else {
        return t;
    };
    let row = match ct.ranked.iter().position(|r| r.model == model) {
        Some(i) => ct.ranked.remove(i),
        None => TableRow { model: model.into(), effort: None, quality: None, est_cost_usd: None, p50_latency_ms: None, why: "the self-tuned pick".into() },
    };
    ct.ranked.insert(0, row);
    t
}

/// The live table: the routing table with every tuned order merged in.
pub fn tuned_table(table: &RoutingTable, tuning: &Tuning) -> RoutingTable {
    tuning.orders.iter().fold(table.clone(), |t, (class, model)| with_order(&t, class, model))
}

/// `{config}/models/tune_state.json`: candidates and what happened to them. Not a ledger target.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TuneState {
    #[serde(default)]
    pub candidates: Vec<Candidate>,
    /// When the weekly candidate pass last ran.
    #[serde(default)]
    pub built_at: u64,
    /// Registry and profile hashes the last pass saw (a change runs it again).
    #[serde(default)]
    pub inputs_hash: String,
    /// When each cost-raising card went out.
    #[serde(default)]
    pub cards_at: Vec<u64>,
    #[serde(default)]
    pub seq: u64,
}

impl TuneState {
    /// The class's shadow or canary candidate, if any (one per class at a time).
    pub fn testing(&self, class: &str) -> Option<&Candidate> {
        self.candidates.iter().find(|c| c.class == class && matches!(c.stage, Stage::Shadow | Stage::Canary))
    }

    pub fn promotions_since(&self, since: u64) -> usize {
        self.candidates.iter().filter(|c| c.promoted_at.is_some_and(|t| t >= since)).count()
    }

    fn blocked(&self, class: &str, change: &Change, now_ms: u64) -> bool {
        self.candidates.iter().any(|c| {
            c.class == class
                && (c.active() || c.stage == Stage::Card && c.change.as_ref() == Some(change) || c.stage == Stage::Rejected && c.change.as_ref() == Some(change) && c.until.is_some_and(|u| u > now_ms))
        })
    }
}

/// A low-risk step the canary may serve: a soft class (not `prepare:hard`, not
/// `repair:*`, not a hard tool step), not in the R1 holdout, not under a
/// VerifyGate reject, not pinned by you.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StepRisk<'a> {
    pub class: &'a str,
    pub holdout: bool,
    pub under_reject: bool,
    pub pinned: bool,
    /// The step offers a send, delete, credential or other hard-class tool.
    pub hard_tool: bool,
}

pub fn canary_eligible(r: &StepRisk<'_>) -> bool {
    let class = class_row(r.class).map(|row| row.class).unwrap_or(r.class);
    class_row(class).is_some() && class != PREPARE_HARD && !class.starts_with("repair:") && !r.holdout && !r.under_reject && !r.pinned && !r.hard_tool
}

/// Deterministic by episode: the same episode always lands the same way, so
/// it keeps one model throughout.
pub fn in_canary(episode: &str, candidate: &str) -> bool {
    if episode.is_empty() {
        return false;
    }
    let buckets = 10_000u64;
    turn_hash(&format!("{episode}\u{1f}{candidate}")) % buckets < (CANARY_SHARE * buckets as f64) as u64
}

/// What the tuner may route to: the filters a candidate must pass (fit,
/// entitlement, cost class and grant, privacy). The caller builds it from the
/// registry and the R2b spend rules; tests pass a fake.
pub trait Filters {
    /// Auto may route `model` in `class` with no new grant.
    fn allowed(&self, class: &str, model: &str) -> bool;
    /// Expected cost of one step of `class` on `model` (list prices × predicted tokens).
    fn expected_cost(&self, class: &str, model: &str) -> Option<f64>;
}

/// What the weekly pass proposes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Proposals {
    /// Cheaper or faster: they go to shadow on their own.
    pub auto: Vec<Candidate>,
    /// Cost more money: Suggestion cards only.
    pub cards: Vec<Candidate>,
}

fn effort_rung(e: &Option<String>) -> Option<usize> {
    e.as_deref().and_then(rung)
}

/// Build candidates from the scorecards for classes with nothing in test.
/// `live_top` is each class's live first model, `band` its tuned band.
pub fn propose(
    cards: &Scorecards,
    steps: &[Step],
    state: &TuneState,
    live_top: &BTreeMap<String, String>,
    bands: &BTreeMap<String, Band>,
    filters: &dyn Filters,
    now_ms: u64,
) -> Proposals {
    let mut out = Proposals::default();
    let mut seq = state.seq;
    let mut id = |class: &str| {
        seq += 1;
        format!("{class}#{seq}")
    };
    for row in CLASS_TABLE {
        let class = row.class;
        if class == PREPARE_HARD || class.starts_with("repair:") || state.testing(class).is_some() {
            continue;
        }
        let (Some(top), Some(band)) = (live_top.get(class), bands.get(class)) else {
            continue;
        };
        let live = cards.model(class, top, steps);
        if live.n < CARD_MIN_N {
            continue;
        }
        // The class's first model.
        let mut models: Vec<&str> = cards.cards.iter().filter(|c| c.class == class && c.model != *top).map(|c| c.model.as_str()).collect();
        models.sort();
        models.dedup();
        let mut best: Option<(f64, &str)> = None;
        let mut pricier: Option<(f64, f64, &str)> = None;
        for m in models {
            let change = Change::Order { model: m.to_string() };
            if !filters.allowed(class, m) || state.blocked(class, &change, now_ms) {
                continue;
            }
            let snap = cards.model(class, m, steps);
            if snap.n < CARD_MIN_N {
                continue;
            }
            let rise = match (filters.expected_cost(class, m), filters.expected_cost(class, top)) {
                (Some(a), Some(b)) if b > 0.0 => (a - b) * 100.0 / b,
                _ => -snap.cost_gain_pct(&live),
            };
            if rise > 0.0 {
                let gain = snap.pass_pct - live.pass_pct;
                if gain >= CARD_MIN_QUALITY_GAIN_PTS && !snap.quality_fell(&live) && pricier.is_none_or(|(g, _, _)| gain > g) {
                    pricier = Some((gain, rise, m));
                }
                continue;
            }
            let gain = snap.cost_gain_pct(&live).max(snap.latency_gain_pct(&live));
            if !snap.quality_fell(&live) && gain >= MIN_GAIN_PCT && best.is_none_or(|(g, _)| gain > g) {
                best = Some((gain, m));
            }
        }
        let new = |change: Change, stage: Stage, id: String, rise: Option<f64>| Candidate {
            id,
            class: class.to_string(),
            change: Some(change),
            stage,
            created_at: now_ms,
            cost_rise_pct: rise,
            ..Candidate::default()
        };
        if let Some((_, m)) = best {
            out.auto.push(new(Change::Order { model: m.to_string() }, Stage::Shadow, id(class), None));
            continue;
        }
        // The class's start rung, on the live model. Lower is cheaper; higher costs more.
        let at = |r: usize| Snap::of(steps.iter().filter(|s| s.class == class && s.model == *top && effort_rung(&s.effort) == Some(r) && s.at + SCORECARD_DAYS * DAY_MS >= now_ms));
        let start = at(band.start);
        if band.start > band.floor {
            let r = band.start - 1;
            let change = Change::Start { effort: EFFORT_LADDER[r].to_string() };
            let lower = at(r);
            if lower.n >= CARD_MIN_N && start.n >= CARD_MIN_N && !lower.quality_fell(&start) && !state.blocked(class, &change, now_ms) {
                let gain = lower.cost_gain_pct(&start).max(lower.latency_gain_pct(&start));
                if gain >= MIN_GAIN_PCT {
                    out.auto.push(new(change, Stage::Shadow, id(class), None));
                    continue;
                }
            }
        }
        if let Some((_, rise, m)) = pricier {
            out.cards.push(new(Change::Order { model: m.to_string() }, Stage::Card, id(class), Some(rise)));
            continue;
        }
        if band.start < band.ceiling {
            let r = band.start + 1;
            let change = Change::Start { effort: EFFORT_LADDER[r].to_string() };
            let higher = at(r);
            if higher.n >= CARD_MIN_N && start.n >= CARD_MIN_N && higher.pass_pct - start.pass_pct >= CARD_MIN_QUALITY_GAIN_PTS && !state.blocked(class, &change, now_ms) {
                let rise = (-higher.cost_gain_pct(&start)).max(0.0);
                out.cards.push(new(change, Stage::Card, id(class), Some(rise)));
            }
        }
    }
    out
}

/// What one candidate's evidence says now.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Wait,
    /// Shadow is done: start the canary.
    StartCanary,
    Promote { cost_gain_pct: f64, latency_gain_pct: f64 },
    /// The canary failed, or a promotion regressed (`rollback` when a file must change back).
    Reject { why: String, rollback: bool },
    /// Watched a week with no regression.
    Keep,
}

/// Evidence for one candidate: shadow steps (and violations), its canary arm,
/// the live arm over the same time, and the class since promotion.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Evidence {
    pub shadow_steps: u32,
    pub violations: u32,
    pub canary: Snap,
    pub live: Snap,
    pub since_promotion: Snap,
}

/// Judge one candidate. `promotions_week` counts promotions in the last 7 days.
pub fn judge(c: &Candidate, ev: &Evidence, promotions_week: usize, now_ms: u64) -> Verdict {
    match c.stage {
        Stage::Shadow if ev.violations > 0 => Verdict::Reject { why: "it broke a routing filter in shadow".into(), rollback: false },
        Stage::Shadow if ev.shadow_steps >= SHADOW_MIN_STEPS => Verdict::StartCanary,
        Stage::Canary => {
            if ev.canary.n >= ROLLBACK_MIN_N && ev.canary.quality_fell(&ev.live) {
                return Verdict::Reject { why: format!("quality fell in its canary ({:.0}% vs {:.0}% passed)", ev.canary.pass_pct, ev.live.pass_pct), rollback: false };
            }
            if ev.canary.n < CANARY_MIN_N {
                return Verdict::Wait;
            }
            let (cost, lat) = (ev.canary.cost_gain_pct(&ev.live), ev.canary.latency_gain_pct(&ev.live));
            if cost < MIN_GAIN_PCT && lat < MIN_GAIN_PCT {
                return Verdict::Reject { why: format!("it saved only {:.0}% cost and {:.0}% time", cost.max(0.0), lat.max(0.0)), rollback: false };
            }
            if promotions_week >= MAX_PROMOTIONS_PER_WEEK {
                return Verdict::Wait;
            }
            Verdict::Promote { cost_gain_pct: cost, latency_gain_pct: lat }
        }
        Stage::Watch => {
            let base = c.baseline.unwrap_or_default();
            if ev.since_promotion.n >= ROLLBACK_MIN_N && ev.since_promotion.quality_fell(&base) {
                return Verdict::Reject {
                    why: format!("quality fell after it went live ({:.0}% vs {:.0}% passed)", ev.since_promotion.pass_pct, base.pass_pct),
                    rollback: true,
                };
            }
            if c.promoted_at.is_some_and(|t| now_ms.saturating_sub(t) >= WATCH_DAYS * DAY_MS) {
                return Verdict::Keep;
            }
            Verdict::Wait
        }
        _ => Verdict::Wait,
    }
}

/// The weekly self-review's one router line, with real numbers.
pub fn review_line(state: &TuneState, now_ms: u64) -> String {
    let since = now_ms.saturating_sub(WEEK_MS);
    let kept: Vec<&Candidate> = state.candidates.iter().filter(|c| matches!(c.stage, Stage::Watch | Stage::Kept) && c.promoted_at.is_some_and(|t| t >= since)).collect();
    let rolled = state.candidates.iter().filter(|c| c.stage == Stage::Rejected && c.promoted_at.is_some() && c.ended_at.is_some_and(|t| t >= since)).count();
    let detail: Vec<String> = kept
        .iter()
        .map(|c| {
            let plain = class_row(&c.class).map(|r| r.plain).unwrap_or(c.class.as_str());
            let (p, b) = (c.promoted_snap.unwrap_or_default(), c.baseline.unwrap_or_default());
            let cost = p.cost_gain_pct(&b);
            let lat = p.latency_gain_pct(&b);
            let gain = if cost >= lat { format!("\u{2212}{cost:.0}% cost") } else { format!("\u{2212}{lat:.0}% time") };
            let dq = p.pass_pct - b.pass_pct;
            let quality = if dq.abs() < 0.5 { "quality same".to_string() } else { format!("quality {dq:+.1} pts") };
            format!("{plain}: {gain}, {quality}")
        })
        .collect();
    let changes = if kept.len() == 1 { "change" } else { "changes" };
    if detail.is_empty() {
        format!("Router: 0 changes kept, {rolled} rolled back.")
    } else {
        format!("Router: {} {changes} kept ({}), {rolled} rolled back.", kept.len(), detail.join("; "))
    }
}

/// `/why table` lines for the tuner: the live tuning, any candidate and canary status.
pub fn why_lines(tuning: &Tuning, version: u32, state: &TuneState, ev: &BTreeMap<String, Evidence>) -> Vec<String> {
    let mut out = vec![if tuning.is_empty() { format!("Self-tuning v{version}: nothing tuned yet") } else { format!("Self-tuning v{version}:") }];
    for (class, e) in &tuning.starts {
        out.push(format!("  {class} starts at {}", grokhub_core::effort_label(e)));
    }
    for (class, m) in &tuning.orders {
        out.push(format!("  {class} → {m} first"));
    }
    for c in state.candidates.iter().filter(|c| matches!(c.stage, Stage::Shadow | Stage::Canary | Stage::Watch | Stage::Card)) {
        let what = c.change.as_ref().map(Change::label).unwrap_or_default();
        let e = ev.get(&c.id).copied().unwrap_or_default();
        let status = match c.stage {
            Stage::Shadow => format!("shadow, {}/{SHADOW_MIN_STEPS} steps", e.shadow_steps),
            Stage::Canary => format!("canary on {:.0}% of steps, {}/{CANARY_MIN_N}, passed {:.0}% vs {:.0}% live", CANARY_SHARE * 100.0, e.canary.n, e.canary.pass_pct, e.live.pass_pct),
            Stage::Watch => format!("live, watched {WATCH_DAYS} days"),
            _ => "waiting on your OK (costs more)".into(),
        };
        out.push(format!("  candidate {}: {} ({status})", c.class, what));
    }
    out
}

/// The cost-raising card's words.
pub fn card_text(c: &Candidate) -> (String, String) {
    let plain = class_row(&c.class).map(|r| r.plain).unwrap_or(c.class.as_str());
    let rise = c.cost_rise_pct.unwrap_or(0.0).round();
    match &c.change {
        Some(Change::Start { effort }) => {
            let level = grokhub_core::effort_label(effort);
            (format!("Start {plain} at {level}?"), format!("{plain} would do better at {level}, which costs about {rise:.0}% more. Start it at {level}?"))
        }
        Some(Change::Order { model }) => (format!("Use {model} for {plain}?"), format!("{plain} would do better on {model}, which costs about {rise:.0}% more. Use it first?")),
        None => (String::new(), String::new()),
    }
}

/// "Oct 6" (UTC) for a card.
fn month_day(ms: u64) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let stamp = grokhub_core::oauth::unix_ms_to_rfc3339(ms);
    let month = stamp.get(5..7).and_then(|m| m.parse::<usize>().ok()).unwrap_or(1);
    let day = stamp.get(8..10).and_then(|d| d.parse::<u32>().ok()).unwrap_or(1);
    format!("{} {day}", MONTHS[month.clamp(1, 12) - 1])
}

/// The info-only "why this model" card for a promoted candidate, built from
/// its canary and the live arm it beat: `(title, body)`. `None` until it was
/// promoted. It asks nothing; the router keeps changing routes on its own.
pub fn why_card_text(c: &Candidate) -> Option<(String, String)> {
    let (change, at, won, beat) = (c.change.as_ref()?, c.promoted_at?, c.promoted_snap?, c.baseline?);
    let plain = class_row(&c.class).map(|r| r.plain).unwrap_or(c.class.as_str());
    let what = match change {
        Change::Order { model } => model.clone(),
        Change::Start { effort } => grokhub_core::effort_label(effort).to_string(),
    };
    let title = format!("Why {what} for {plain}: {:.0}% pass vs {:.0}%, promoted {}", won.pass_pct, beat.pass_pct, month_day(at));
    let body = format!(
        "Auto now runs {plain} with {}: {:.0}% cheaper and {:.0}% faster over {} canary steps. Info only; Auto keeps tuning on its own.",
        change.label(),
        won.cost_gain_pct(&beat),
        won.latency_gain_pct(&beat),
        won.n
    );
    Some((title, body))
}

#[cfg(test)]
#[path = "tune_tests.rs"]
mod tests;
