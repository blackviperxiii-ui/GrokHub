//! Failover PR 6: the scorer and the route ladder (card §3.4–§3.5).
//!
//! Every candidate the picker ranks is weighed here for *this* task:
//! quality for the task's needs, speed, reliability, expected cost and
//! risk, from the model card (capability priors, roles, data policy,
//! probation, expiry — [`grokhub_core::model_registry::ModelCard`]), the
//! model's own last 7 days ([`ModelRecord::reliability`]) and the quota
//! ledger (PR 3: OpenRouter free/paid state and free ceiling, xAI blocked
//! flags, Claude's benches). Live measurement replaces priors as it
//! accrues: the card's speed slot and the health window are read as they
//! stand, so a model that has actually been answering well outranks its
//! day-one paper.
//!
//! Hard filters run before any weighing, and they are filters, not low
//! scores: an unmet need the card states outright, a route the quota
//! ledger says cannot answer, a training route on a sensitive turn, a
//! probation model on real work, a free model inside its expiry
//! stop-window, any cloud model at all on an 18+ turn, and the on-device
//! model for more than tiny work while the governor throttles (a game, a
//! busy GPU). A filter never empties the ladder on its own: when every
//! candidate is barred, the incoming order stands — a working route
//! beats a dead stop, and the send path keeps its own guards.
//!
//! The ladder itself is the card's, with Viper's corrected rule
//! (2026-11-07) inside it: tiny and short work runs free first, then
//! cheap paid, then xAI, then the rest, then the device; medium work
//! leads with xAI and lets a free model in only when it is a reliable
//! tool model with quota left; a long run is one paid tier scored as a
//! whole — never free — with the device behind paid. The ladder orders
//! inside the standing groups, it does not replace them: a model the
//! routing table ranks for the lane keeps the table's order (the table
//! is the learned preference), a provider model the table does not
//! rank still stands behind the native models, and only on a long run
//! — where PR 5 already broke the table's order — do the paid routes
//! score against each other outright. An incoming order the scorer is
//! not asked to re-weigh (no size estimate, or attention already
//! ranked the turn) keeps its order and only the filters apply.
//! Weights follow the turn's goal ([`super::goal`]): quality, speed or
//! cost lean the same five terms. The bandit keeps steering ordinary
//! chat after the pick, as before.
//!
//! Nothing here is shown and nothing is a setting: the rules that fired
//! land on the route record (`score:*`), like every other router rule.

use grokhub_core::local_governor::Level;
use grokhub_core::model_registry::probe::DAY_MS;
use grokhub_core::model_registry::quota::{utc_day_key, QuotaLedger};
use grokhub_core::model_registry::{Credential, ModelRecord, Registry, Route};

use super::goal::Goal;
use super::policy;
use super::sizing::{SizeEstimate, TaskSize};

/// A free model stops being routed this many days before its announced
/// expiry (card §3.8); inside the wider window it only carries a risk.
const EXPIRY_STOP_DAYS: u64 = 14;
const EXPIRY_RISK_DAYS: u64 = 30;
/// What a full expected cost is worth in the cost term: a call expected
/// to cost this much scores 0.5 on cost, a free or plan-included one 1.0.
const COST_HALF_USD: f64 = 0.02;
/// How much of a plan model's list cost counts against it: the plan is
/// prepaid, so its models keep most of their cost edge over metered
/// routes, but a plan model twice the list price still loses to the
/// cheaper one on cost.
const PLAN_SHADOW: f64 = 0.2;
/// At or under this expected cost a paid model counts as cheap for the
/// tiny/short and medium ladders (its card roles can say so sooner).
const CHEAP_USD: f64 = 0.004;
/// Card roles that mark a paid model as the cheap tier of the ladder.
const CHEAP_ROLES: [&str; 6] = [
    "cheap",
    "cheap_fast",
    "cheap_code",
    "classifier",
    "bulk",
    "low_latency",
];
/// Card roles that mark a model as made for long work.
const LONG_ROLES: [&str; 4] = ["long", "long_code", "long_context", "agentic"];

/// Everything the scorer looks at for one pick. Plain values, no I/O:
/// the caller ([`super::Router::pick_model`]) gathers them from the
/// route input.
#[derive(Debug, Clone, Copy)]
pub struct ScoreCtx<'a> {
    /// The lane the choice is made for (the class row's lane).
    pub lane: &'a str,
    pub size: Option<SizeEstimate>,
    pub goal: Option<Goal>,
    /// The step's data is marked sensitive (the local privacy route).
    pub sensitive: bool,
    /// The whole call is explicit (18+): local only, no exceptions.
    pub adult: bool,
    /// The governor's level for the on-device route.
    pub level: Level,
    /// The quota ledger as last detected (PR 3).
    pub quota: &'a QuotaLedger,
    pub ctx_tokens: u64,
    pub now_ms: u64,
    /// Failover step 9: this turn is in the tryout canary (its episode
    /// key), so a model on tryout leads a tiny or short turn. `None` on
    /// every other turn.
    pub canary: Option<&'a str>,
    /// The call is a helper's (a subagent's), which can spend a whole step
    /// budget of calls.
    pub helper: bool,
}

/// A helper may use the free OpenRouter day only while its whole step
/// budget is at most a quarter of the free calls left, so a fan-out
/// can't spend the day the chat itself needs.
pub const HELPER_FREE_FLOOR: u64 = 4 * crate::subagent_policy::EXPLORE_TURNS as u64;

/// Why a candidate may not take this task at all, as the rule id the
/// route record carries. `None` weighs the candidate instead.
fn bar(rec: &ModelRecord, ctx: &ScoreCtx<'_>) -> Option<&'static str> {
    let card = &rec.meta.card;
    let route = rec.route();
    if ctx.adult && route != Route::Local {
        // 18+ runs on the device only — never OpenRouter, never Claude,
        // never xAI. No score can argue with this.
        return Some("score:adult:cloud");
    }
    if let Some(need) = unmet_need(rec, ctx) {
        return Some(need);
    }
    match route {
        Route::OpenRouterFree => {
            if !ctx.quota.openrouter.permits(true)
                || ctx.quota.openrouter_free_remaining() == Some(0)
            {
                return Some("score:quota:openrouter");
            }
            if ctx.helper
                && ctx
                    .quota
                    .openrouter_free_remaining()
                    .is_some_and(|left| left < HELPER_FREE_FLOOR)
            {
                return Some("score:quota:helper-free");
            }
        }
        Route::OpenRouterPaid => {
            if !ctx.quota.openrouter.permits(false) {
                return Some("score:quota:openrouter");
            }
        }
        Route::Xai => {
            if ctx.quota.xai_blocked() {
                return Some("score:quota:xai");
            }
        }
        Route::Anthropic => {
            if ctx.quota.anthropic_benched(ctx.now_ms) {
                return Some("score:quota:anthropic");
            }
        }
        Route::Local => {}
    }
    if ctx.size.is_some_and(|s| s.size == TaskSize::Long) && route == Route::OpenRouterFree {
        // The corrected rule, at the scorer too (the pick's candidate
        // list is already free of them; stand-ins rank from it as well):
        // a long run never spends a free model's small allowance.
        return Some("score:long:free");
    }
    if rec.probation_failed() {
        // Failed its tryout: out of routing until its retry two weeks on.
        return Some("score:probation:failed");
    }
    if card.probation
        && ctx
            .size
            .is_some_and(|s| matches!(s.size, TaskSize::Medium | TaskSize::Long))
    {
        // A model on tryout takes tiny and short turns only: its probes,
        // its canary turns and its own small work (see super::trial).
        return Some("score:probation");
    }
    if let Some(expires) = card.expires.as_deref() {
        if expires <= utc_day_key(ctx.now_ms + EXPIRY_STOP_DAYS * DAY_MS).as_str() {
            return Some("score:expiring");
        }
    }
    if ctx.sensitive && card.data_policy.trains == Some(true) {
        // Sensitive turns skip routes that train on prompts (card §3.10).
        return Some("score:data:trains");
    }
    if ctx.level == Level::Throttle
        && route == Route::Local
        && !ctx.size.is_some_and(|s| s.size == TaskSize::Tiny)
    {
        // A game (or another app) holds the GPU: the device takes tiny
        // work only. When the device is all there is, the caller's
        // never-empty rule below keeps it rather than dead-stopping.
        return Some("score:gaming:local");
    }
    None
}

/// A need the card states the model cannot meet: a capability scored a
/// plain 0 is a statement, an unknown one is not. (The pick's fit check
/// already bars what the listing itself refuses — image input, tool
/// calls, the context window; this is the card's own word on top.)
fn unmet_need(rec: &ModelRecord, ctx: &ScoreCtx<'_>) -> Option<&'static str> {
    let size = ctx.size?;
    let caps = &rec.meta.card.caps;
    let needs = &size.needs;
    if needs.tools && caps.tools == Some(0) && rec.meta.tool_calling != Some(true) {
        return Some("score:need:tools");
    }
    if needs.vision && caps.vision == Some(0) {
        return Some("score:need:vision");
    }
    if needs.audio && caps.audio == Some(0) {
        return Some("score:need:audio");
    }
    if needs.reasoning && caps.reasoning == Some(0) {
        return Some("score:need:reasoning");
    }
    if needs.long_context && caps.long_context == Some(0) {
        return Some("score:need:long_context");
    }
    None
}

/// Quality for this task's needs: the card's capability scores weighted
/// toward what the task asks for. An unknown axis counts as middling —
/// a model nobody has scored yet is not a bad model.
fn quality(rec: &ModelRecord, ctx: &ScoreCtx<'_>) -> f64 {
    let caps = &rec.meta.card.caps;
    // chat, code, tools, reasoning, long_context, vision, audio.
    let mut w: [f64; 7] = [1.0, 1.0, 0.6, 0.8, 0.4, 0.2, 0.1];
    if let Some(size) = ctx.size {
        if size.needs.tools {
            w[2] = 2.0;
        }
        if size.needs.vision {
            w[5] = 2.5;
        }
        if size.needs.audio {
            w[6] = 2.5;
        }
        if size.needs.reasoning {
            w[3] = 2.0;
        }
        if size.needs.long_context {
            w[4] = 2.0;
        }
    }
    if is_code_lane(ctx.lane) {
        w[1] = 2.0;
        w[3] = w[3].max(1.5);
    }
    let vals = [
        caps.chat,
        caps.code,
        caps.tools,
        caps.reasoning,
        caps.long_context,
        caps.vision,
        caps.audio,
    ];
    let (mut sum, mut wsum) = (0.0, 0.0);
    for (v, w) in vals.iter().zip(w) {
        sum += w * f64::from(v.unwrap_or(50));
        wsum += w;
    }
    let mut q = sum / wsum / 100.0;
    let roles = &rec.meta.card.roles;
    if ctx.size.is_some_and(|s| s.size == TaskSize::Long) {
        if roles.iter().any(|r| LONG_ROLES.contains(&r.as_str())) {
            q += 0.04;
        }
        if roles.iter().any(|r| r == "failover_first") {
            // The card's designated first choice for a long run.
            q += 0.03;
        }
    }
    if roles.iter().any(|r| r == "never_default") {
        // Priced and capable, but the card says never a default pick
        // (the top escalation tier): only a real escalation reaches it,
        // through attention's own ranking, which this scorer leaves be.
        q -= 0.25;
    }
    // Quality is quality *for this task*: past what a task this size
    // can use, a stronger card is not a better pick — the excess does
    // not outrank a cheaper adequate model or the table's order.
    let ceiling = match ctx.size.map(|s| s.size) {
        Some(TaskSize::Tiny) => 0.88,
        Some(TaskSize::Short) => 0.92,
        Some(TaskSize::Medium) => 0.96,
        _ => 1.0,
    };
    q.min(ceiling).clamp(0.0, 1.0)
}

fn is_code_lane(lane: &str) -> bool {
    lane.starts_with("code:")
        || lane.starts_with("repair:")
        || matches!(lane, "plan" | "design" | "debug")
}

/// Speed: the card's tokens-per-second (a prior until measurement
/// replaces it), tempered by a slow first token. Unknown is middling.
fn speed(rec: &ModelRecord) -> f64 {
    let card = &rec.meta.card;
    let Some(tps) = card.speed_tps else {
        return 0.5;
    };
    let mut s = (tps as f64 / 150.0).min(1.0);
    if card.ttft_ms.is_some_and(|ms| ms > 2_000) {
        s *= 0.85;
    }
    s
}

/// Reliability: how the model has actually answered over the last
/// 7 days, success less half its rate-limit pressure. No history yet
/// counts as a fair prior, below a proven record and above a poor one.
fn reliability(rec: &ModelRecord, now_ms: u64) -> f64 {
    match rec.reliability(now_ms) {
        Some(r) => (r.success_rate - 0.5 * r.pressure_rate).clamp(0.0, 1.0),
        None => 0.65,
    }
}

/// The expected cost of this call as a score: free routes and the
/// device cost nothing, so they score 1.0; a metered route scores by
/// what the call is expected to cost. The plan's own models are
/// prepaid, not free — they draw the plan's usage at their listed
/// rates — so on a plan they score by a shadow of the list cost
/// ([`PLAN_SHADOW`]): strongly ahead of any metered route, with the
/// plan's cheaper models still ahead of its pricier ones.
fn cost_score(rec: &ModelRecord, ctx: &ScoreCtx<'_>, credential: Credential) -> f64 {
    match rec.route() {
        Route::Local | Route::OpenRouterFree => 1.0,
        Route::Xai if credential == Credential::Plan => {
            match policy::expected_cost_usd(&rec.meta.prices, ctx.lane, ctx.ctx_tokens) {
                Some(usd) => 1.0 / (1.0 + usd * PLAN_SHADOW / COST_HALF_USD),
                None => 1.0,
            }
        }
        _ => match policy::expected_cost_usd(&rec.meta.prices, ctx.lane, ctx.ctx_tokens) {
            Some(usd) => 1.0 / (1.0 + usd / COST_HALF_USD),
            None => 0.5,
        },
    }
}

/// What could go wrong with this pick, 0.0–1.0: a probation model's
/// priors are provisional, a free route near its daily ceiling can die
/// mid-task, an announced expiry inside the month can pull the model,
/// a retaining route is a worse home for sensitive data, and a model
/// the field notes say to avoid is a risk anywhere.
fn risk(rec: &ModelRecord, ctx: &ScoreCtx<'_>) -> f64 {
    let card = &rec.meta.card;
    let mut risk: f64 = 0.0;
    if card.probation {
        risk += 0.45;
    }
    if rec.route() == Route::OpenRouterFree {
        if let (Some(rem), Some(limit)) = (
            ctx.quota.openrouter_free_remaining(),
            ctx.quota.openrouter.free_daily_limit,
        ) {
            if limit > 0 && rem * 10 < limit {
                risk += 0.3;
            }
        }
    }
    if let Some(expires) = card.expires.as_deref() {
        if expires <= utc_day_key(ctx.now_ms + EXPIRY_RISK_DAYS * DAY_MS).as_str() {
            risk += 0.2;
        }
    }
    if ctx.sensitive && card.data_policy.retains == Some(true) {
        risk += 0.2;
    }
    if card.roles.iter().any(|r| r == "avoid") {
        risk += 0.6;
    }
    risk.min(1.0)
}

/// The five weights (quality, speed, reliability, cost, risk) the
/// turn's goal and size set: the goal leans them first (§3.4's
/// "weights from goal"), then the size — a long run leans on
/// reliability (it must not die halfway) over cost, tiny work on
/// speed and cost.
fn weights(ctx: &ScoreCtx<'_>) -> (f64, f64, f64, f64, f64) {
    let (mut q, mut s, mut r, mut c, k): (f64, f64, f64, f64, f64) = match ctx.goal {
        Some(Goal::Quality) | Some(Goal::Care) => (0.55, 0.10, 0.20, 0.05, 0.20),
        Some(Goal::Speed) => (0.25, 0.40, 0.15, 0.10, 0.20),
        Some(Goal::Cost) => (0.25, 0.10, 0.15, 0.40, 0.20),
        _ => (0.40, 0.15, 0.20, 0.15, 0.20),
    };
    match ctx.size.map(|s| s.size) {
        Some(TaskSize::Long) => {
            r = r.max(0.30);
            if ctx.goal != Some(Goal::Cost) {
                c = c.min(0.05);
            }
        }
        Some(TaskSize::Tiny) => {
            s = s.max(0.25);
            c = c.max(0.20);
            q = q.min(0.30);
        }
        _ => {}
    }
    (q, s, r, c, k)
}

/// One candidate's weighed score for this task.
pub fn evaluate(rec: &ModelRecord, ctx: &ScoreCtx<'_>, credential: Credential) -> f64 {
    let (wq, ws, wr, wc, wk) = weights(ctx);
    wq * quality(rec, ctx) + ws * speed(rec) + wr * reliability(rec, ctx.now_ms)
        - wc * (1.0 - cost_score(rec, ctx, credential))
        - wk * risk(rec, ctx)
}

/// A paid model in the ladder's cheap tier: its card roles say so, or
/// the call is expected to cost next to nothing.
fn cheap(rec: &ModelRecord, ctx: &ScoreCtx<'_>) -> bool {
    if rec
        .meta
        .card
        .roles
        .iter()
        .any(|r| CHEAP_ROLES.contains(&r.as_str()))
    {
        return true;
    }
    policy::expected_cost_usd(&rec.meta.prices, ctx.lane, ctx.ctx_tokens)
        .is_some_and(|usd| usd <= CHEAP_USD)
}

/// The ladder tier of a route for this size (card §3.5, corrected):
/// lower runs first. Only read when the turn was sized — an unsized
/// turn keeps the order it came with.
fn tier(rec: &ModelRecord, ctx: &ScoreCtx<'_>) -> u8 {
    let route = rec.route();
    let size = ctx.size.map(|s| s.size).unwrap_or(TaskSize::Short);
    match size {
        TaskSize::Tiny | TaskSize::Short => match route {
            Route::OpenRouterFree => 0,
            Route::Anthropic | Route::OpenRouterPaid => {
                if cheap(rec, ctx) {
                    1
                } else {
                    3
                }
            }
            Route::Xai => 2,
            Route::Local => 4,
        },
        TaskSize::Medium => match route {
            Route::Xai => 0,
            Route::Anthropic | Route::OpenRouterPaid => {
                if cheap(rec, ctx) {
                    1
                } else {
                    2
                }
            }
            Route::OpenRouterFree => {
                // A free model takes medium work only when it is a
                // proven tool model; anything else waits behind paid.
                let tools_ok = rec.meta.card.caps.tools.is_some_and(|t| t >= 50);
                if tools_ok {
                    3
                } else {
                    4
                }
            }
            Route::Local => 4,
        },
        TaskSize::Long => match route {
            // One paid tier, scored as a whole: the plan's own models
            // are naturally strong here (frontier cards, nothing at the
            // margin), and the scorer — not a fixed order — settles
            // which paid route carries a given long run.
            Route::Xai | Route::Anthropic | Route::OpenRouterPaid => 0,
            Route::Local => 1,
            Route::OpenRouterFree => 2,
        },
    }
}

/// Order `ranked` (the picker's standing order: table, capability fit,
/// cost) into the route ladder for this task. Hard filters drop what
/// may not run; survivors are re-weighed when `reorder` is set and the
/// turn was sized. The ladder sorts inside the standing groups: a
/// model the table ranks for the lane keeps the table's order, an
/// untabled native or local model comes next, an untabled provider
/// model last — tiers and the score order each group. A long run is
/// the one exception (PR 5's precedent): its paid routes form a single
/// tier scored outright, table order notwithstanding. When
/// every candidate is filtered out, the incoming order stands
/// untouched (`score:filters_kept`): a working route beats a dead
/// stop. Rules that fired are pushed onto `rules`, each at most once.
pub fn order(
    ranked: Vec<String>,
    reg: &Registry,
    table: &super::table::RoutingTable,
    ctx: &ScoreCtx<'_>,
    reorder: bool,
    rules: &mut Vec<String>,
) -> Vec<String> {
    if ranked.is_empty() {
        return ranked;
    }
    let mut barred: Vec<&'static str> = Vec::new();
    let mut kept: Vec<String> = Vec::new();
    for id in &ranked {
        match reg.get(id).and_then(|rec| bar(rec, ctx)) {
            Some(rule) => {
                if !barred.contains(&rule) {
                    barred.push(rule);
                }
            }
            None => kept.push(id.clone()),
        }
    }
    if kept.is_empty() {
        rules.push("score:filters_kept".into());
        return ranked;
    }
    rules.extend(barred.iter().map(|r| r.to_string()));
    if !reorder || ctx.size.is_none() {
        // Filters only: the standing order (or attention's) stands.
        let kept_set: std::collections::BTreeSet<&str> = kept.iter().map(String::as_str).collect();
        return ranked
            .into_iter()
            .filter(|id| kept_set.contains(id.as_str()))
            .collect();
    }
    let credential = reg.entitlement.credential;
    let head = ranked.first().cloned();
    let rows = table.rows(ctx.lane);
    let long = ctx.size.is_some_and(|s| s.size == TaskSize::Long);
    // (group, table position, tier, -score): the table's own order
    // leads its group outright; the other groups sort by tier then
    // score. On a long run every candidate shares one group and no
    // table position, so the paid tier scores outright. A degraded
    // model keeps the place the standing order gave it — the picker
    // already spends its health logic on the pools, and a demotion
    // here would hand a live provider a turn the native route was
    // still set to take (and to fail over from, at send time).
    let mut keyed: Vec<(u8, usize, u8, f64, String)> = kept
        .into_iter()
        .map(|id| {
            let rec = reg.get(&id);
            let table_pos = rows.iter().position(|r| r.model == id);
            let (group, pos) = if long {
                (0, 0)
            } else {
                match table_pos {
                    Some(p) => (0, p),
                    None if policy::is_new_provider(reg, &id) => (2, usize::MAX),
                    None => (1, usize::MAX),
                }
            };
            (
                group,
                pos,
                rec.map(|r| tier(r, ctx)).unwrap_or(0),
                rec.map(|r| -evaluate(r, ctx, credential)).unwrap_or(0.0),
                id,
            )
        })
        .collect();
    keyed.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.cmp(&b.1))
            .then(a.2.cmp(&b.2))
            .then(a.3.total_cmp(&b.3))
    });
    let mut out: Vec<String> = keyed.into_iter().map(|k| k.4).collect();
    if out.first() != head.as_ref() {
        rules.push("score:ladder".into());
    }
    canary_lead(&mut out, reg, ctx, rules);
    out
}

/// Failover step 9's canary: on a tiny or short turn the canary drew,
/// one model on tryout that the filters kept leads, picked by the
/// episode so a whole episode stays on one model. Failover still runs
/// down the rest of the ladder behind it.
fn canary_lead(out: &mut Vec<String>, reg: &Registry, ctx: &ScoreCtx<'_>, rules: &mut Vec<String>) {
    let Some(episode) = ctx.canary else {
        return;
    };
    if !ctx
        .size
        .is_some_and(|s| matches!(s.size, TaskSize::Tiny | TaskSize::Short))
    {
        return;
    }
    let on_trial: Vec<usize> = out
        .iter()
        .enumerate()
        .filter(|(_, id)| {
            reg.get(id)
                .is_some_and(|r| r.meta.card.probation && !r.probation_failed())
        })
        .map(|(i, _)| i)
        .collect();
    if on_trial.is_empty() {
        return;
    }
    let pick = on_trial[(super::ladder::turn_hash(episode) % on_trial.len() as u64) as usize];
    let id = out.remove(pick);
    out.insert(0, id);
    rules.push("probation:canary".into());
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::model_registry::quota::{OpenRouterQuota, OpenRouterState, QuotaLedger};
    use grokhub_core::model_registry::{ModelMeta, ModelState, SourceKind};

    use super::super::table::{ClassTable, RoutingTable, SpeedQuality, TableRow};

    const NOW: u64 = 1_800_000_000_000; // 2027-01-15ish; day keys come from the same clock.

    fn rec(id: &str, sources: &[SourceKind]) -> ModelRecord {
        // A refresh would stamp the card from the bundled table.
        let mut meta = ModelMeta::bare(id);
        grokhub_core::model_registry::card::fill(&mut meta, sources);
        ModelRecord {
            meta,
            sources: sources.to_vec(),
            state: ModelState::Live,
            ..ModelRecord::default()
        }
    }

    fn ctx<'a>(quota: &'a QuotaLedger, size: Option<TaskSize>) -> ScoreCtx<'a> {
        ScoreCtx {
            lane: "chat:default",
            size: size.map(|size| SizeEstimate {
                size,
                ..SizeEstimate::default()
            }),
            goal: None,
            sensitive: false,
            adult: false,
            level: Level::Normal,
            quota,
            ctx_tokens: 0,
            now_ms: NOW,
            canary: None,
            helper: false,
        }
    }

    #[test]
    fn an_adult_turn_bars_every_cloud_route() {
        let quota = QuotaLedger::default();
        let mut c = ctx(&quota, Some(TaskSize::Short));
        c.adult = true;
        let grok = rec("grok-4.7", &[SourceKind::XaiApi]);
        let free = rec(
            "openrouter/nvidia/nemotron-3.5-super-vl:free",
            &[SourceKind::OpenAiCompatible],
        );
        assert_eq!(bar(&grok, &c), Some("score:adult:cloud"));
        assert_eq!(bar(&free, &c), Some("score:adult:cloud"));
    }

    #[test]
    fn quota_states_bar_the_routes_they_close() {
        let quota = QuotaLedger {
            openrouter: OpenRouterQuota {
                state: OpenRouterState::Negative,
                ..OpenRouterQuota::default()
            },
            ..QuotaLedger::default()
        };
        let c = ctx(&quota, Some(TaskSize::Short));
        let free = rec(
            "openrouter/nvidia/nemotron-3.5-super-vl:free",
            &[SourceKind::OpenAiCompatible],
        );
        assert_eq!(bar(&free, &c), Some("score:quota:openrouter"));
        // Unknown (never checked) is not a closed account.
        let quota = QuotaLedger::default();
        let c = ctx(&quota, Some(TaskSize::Short));
        assert_eq!(bar(&free, &c), None);
        // A spent free day bars free models even on a paid account.
        let quota = QuotaLedger {
            openrouter: OpenRouterQuota {
                state: OpenRouterState::Paid,
                free_daily_remaining: Some(0),
                ..OpenRouterQuota::default()
            },
            ..QuotaLedger::default()
        };
        let c = ctx(&quota, Some(TaskSize::Short));
        assert_eq!(bar(&free, &c), Some("score:quota:openrouter"));
    }

    #[test]
    fn a_helper_stays_off_a_free_day_that_is_nearly_spent() {
        let free = rec(
            "openrouter/nvidia/nemotron-3.5-super-vl:free",
            &[SourceKind::OpenAiCompatible],
        );
        let left = |n: u64| QuotaLedger {
            openrouter: OpenRouterQuota {
                state: OpenRouterState::Paid,
                free_daily_remaining: Some(n),
                ..OpenRouterQuota::default()
            },
            ..QuotaLedger::default()
        };
        assert_eq!(HELPER_FREE_FLOOR, 80);
        let quota = left(79);
        let chat = ctx(&quota, Some(TaskSize::Short));
        assert_eq!(bar(&free, &chat), None, "the chat itself may still use it");
        let helper = ScoreCtx {
            helper: true,
            ..ctx(&quota, Some(TaskSize::Short))
        };
        assert_eq!(bar(&free, &helper), Some("score:quota:helper-free"));
        let quota = left(80);
        let helper = ScoreCtx {
            helper: true,
            ..ctx(&quota, Some(TaskSize::Short))
        };
        assert_eq!(bar(&free, &helper), None);
        let quota = QuotaLedger::default();
        let helper = ScoreCtx {
            helper: true,
            ..ctx(&quota, Some(TaskSize::Short))
        };
        assert_eq!(
            bar(&free, &helper),
            None,
            "an unread day is not a spent one"
        );
    }

    #[test]
    fn helper_scope_marks_only_its_own_thread() {
        assert!(!crate::route::live::in_helper());
        {
            let _helper = crate::route::live::HelperScope::enter();
            assert!(crate::route::live::in_helper());
            let other = std::thread::spawn(crate::route::live::in_helper)
                .join()
                .unwrap();
            assert!(!other);
        }
        assert!(!crate::route::live::in_helper());
    }

    #[test]
    fn a_long_run_bars_free_and_probation_models() {
        let quota = QuotaLedger::default();
        let c = ctx(&quota, Some(TaskSize::Long));
        let free = rec(
            "openrouter/nvidia/nemotron-3.5-super-vl:free",
            &[SourceKind::OpenAiCompatible],
        );
        assert_eq!(bar(&free, &c), Some("score:long:free"));
        let probation = rec(
            "openrouter/thinkingmachines/inkling:free",
            &[SourceKind::OpenAiCompatible],
        );
        let medium = ctx(&quota, Some(TaskSize::Medium));
        assert_eq!(bar(&probation, &medium), Some("score:probation"));
        // …but a probation model may still take a short turn.
        let short = ctx(&quota, Some(TaskSize::Short));
        assert_eq!(bar(&probation, &short), None);
    }

    #[test]
    fn a_sensitive_turn_bars_a_training_route_only() {
        let quota = QuotaLedger::default();
        let mut c = ctx(&quota, Some(TaskSize::Short));
        let free = rec(
            "openrouter/nvidia/nemotron-3.5-super-vl:free",
            &[SourceKind::OpenAiCompatible],
        );
        // NVIDIA's free route may train (the bundled card says so).
        assert_eq!(free.meta.card.data_policy.trains, Some(true));
        c.sensitive = true;
        assert_eq!(bar(&free, &c), Some("score:data:trains"));
        c.sensitive = false;
        assert_eq!(bar(&free, &c), None);
    }

    #[test]
    fn gaming_throttle_bars_local_above_tiny() {
        let quota = QuotaLedger::default();
        let mut c = ctx(&quota, Some(TaskSize::Medium));
        c.level = Level::Throttle;
        let local = rec("local:small", &[SourceKind::Local]);
        assert_eq!(local.route(), Route::Local);
        assert_eq!(bar(&local, &c), Some("score:gaming:local"));
        let tiny = ScoreCtx {
            size: Some(SizeEstimate {
                size: TaskSize::Tiny,
                ..SizeEstimate::default()
            }),
            ..c
        };
        assert_eq!(bar(&local, &tiny), None);
    }

    #[test]
    fn the_ladder_orders_tiers_then_scores() {
        let mut reg = Registry::default();
        reg.entitlement.credential = Credential::Plan;
        for r in [
            rec("grok-4.7", &[SourceKind::XaiApi]),
            rec(
                "openrouter/nvidia/nemotron-3.5-super-vl:free",
                &[SourceKind::OpenAiCompatible],
            ),
            rec("local:small", &[SourceKind::Local]),
        ] {
            reg.models.insert(r.meta.id.clone(), r);
        }
        let quota = QuotaLedger::default();
        let no_table = RoutingTable::default();
        let ranked = || {
            vec![
                "local:small".to_string(),
                "grok-4.7".to_string(),
                "openrouter/nvidia/nemotron-3.5-super-vl:free".to_string(),
            ]
        };
        // Short, nothing tabled: the native model leads, the untabled
        // provider model stands behind it, the device last.
        let c = ctx(&quota, Some(TaskSize::Short));
        let mut rules = Vec::new();
        let out = order(ranked(), &reg, &no_table, &c, true, &mut rules);
        assert_eq!(
            out,
            vec![
                "grok-4.7".to_string(),
                "local:small".to_string(),
                "openrouter/nvidia/nemotron-3.5-super-vl:free".to_string(),
            ]
        );
        assert!(rules.contains(&"score:ladder".to_string()));
        // Short, the table ranks the free model for the lane: the
        // table's order leads, as it always has.
        let row = |m: &str| TableRow {
            model: m.into(),
            effort: Some("medium".into()),
            quality: Some(1.0),
            est_cost_usd: None,
            p50_latency_ms: None,
            why: "the class preference".into(),
        };
        let mut table = RoutingTable::default();
        table.classes.insert(
            "chat:default".into(),
            ClassTable {
                speed_quality: SpeedQuality::Balanced,
                ranked: vec![row("openrouter/nvidia/nemotron-3.5-super-vl:free")],
            },
        );
        let mut rules = Vec::new();
        let out = order(ranked(), &reg, &table, &c, true, &mut rules);
        assert_eq!(
            out.first().map(String::as_str),
            Some("openrouter/nvidia/nemotron-3.5-super-vl:free")
        );
        // Long: one paid tier scored outright — the plan's frontier
        // model carries it, the device behind paid, the free model
        // filtered out, the table's order notwithstanding.
        let c = ctx(&quota, Some(TaskSize::Long));
        let mut rules = Vec::new();
        let out = order(ranked(), &reg, &table, &c, true, &mut rules);
        assert_eq!(out, vec!["grok-4.7".to_string(), "local:small".to_string()]);
        assert!(rules.contains(&"score:long:free".to_string()));
    }

    #[test]
    fn filters_never_empty_the_ladder() {
        let mut reg = Registry::default();
        let r = rec(
            "openrouter/nvidia/nemotron-3.5-super-vl:free",
            &[SourceKind::OpenAiCompatible],
        );
        reg.models.insert(r.meta.id.clone(), r);
        let quota = QuotaLedger {
            openrouter: OpenRouterQuota {
                state: OpenRouterState::Negative,
                ..OpenRouterQuota::default()
            },
            ..QuotaLedger::default()
        };
        let c = ctx(&quota, Some(TaskSize::Short));
        let mut rules = Vec::new();
        let out = order(
            vec!["openrouter/nvidia/nemotron-3.5-super-vl:free".into()],
            &reg,
            &RoutingTable::default(),
            &c,
            true,
            &mut rules,
        );
        assert_eq!(out.len(), 1);
        assert!(rules.contains(&"score:filters_kept".to_string()));
    }

    #[test]
    fn prices_are_per_million_and_plan_models_cost_nothing_at_the_margin() {
        let quota = QuotaLedger::default();
        let c = ctx(&quota, Some(TaskSize::Long));
        let grok = rec("grok-4.7", &[SourceKind::XaiApi]);
        let sonnet = rec("anthropic/claude-sonnet-5.5", &[SourceKind::Anthropic]);
        // Both frontier cards: on a plan the Grok's cost term is perfect,
        // the metered Claude pays its (small) expected cost.
        assert!(evaluate(&grok, &c, Credential::Plan) > evaluate(&sonnet, &c, Credential::Plan));
        // On an API key both are metered and the gap narrows to quality.
        let key_gap =
            evaluate(&grok, &c, Credential::ApiKey) - evaluate(&sonnet, &c, Credential::ApiKey);
        assert!(key_gap.abs() < 0.2, "gap {key_gap}");
    }

    #[test]
    fn an_expiring_model_is_barred_inside_the_stop_window() {
        let quota = QuotaLedger::default();
        // dots-3-note-preview leaves 2026-12-31; at NOW (2027-01) it is gone.
        let dots = rec(
            "openrouter/dots-studio/dots-3-note-preview:free",
            &[SourceKind::OpenAiCompatible],
        );
        assert_eq!(dots.meta.card.expires.as_deref(), Some("2026-12-31"));
        let c = ctx(&quota, Some(TaskSize::Short));
        assert_eq!(bar(&dots, &c), Some("score:expiring"));
        // Back in November it was outside the 14-day window and routable.
        let mut early = ctx(&quota, Some(TaskSize::Short));
        early.now_ms = 1_793_548_800_000; // 2026-11-01
        assert_eq!(bar(&dots, &early), None);
    }

    #[test]
    fn an_unmet_need_the_card_states_bars_the_model() {
        let quota = QuotaLedger::default();
        let mut r = rec("grok-4.7", &[SourceKind::XaiApi]);
        r.meta.card.caps.vision = Some(0);
        let mut c = ctx(&quota, Some(TaskSize::Short));
        c.size = Some(SizeEstimate {
            size: TaskSize::Short,
            needs: super::super::sizing::SizeNeeds {
                vision: true,
                ..Default::default()
            },
            ..SizeEstimate::default()
        });
        assert_eq!(bar(&r, &c), Some("score:need:vision"));
        let plain = ctx(&quota, Some(TaskSize::Short));
        assert_eq!(bar(&r, &plain), None);
    }

    #[test]
    fn a_failed_tryout_is_barred_and_the_canary_hands_a_tryout_model_small_turns_only() {
        use grokhub_core::model_registry::probation::{Probation, Stage};
        let mut reg = Registry::default();
        reg.entitlement.credential = Credential::Plan;
        let trial = "openrouter/vendor/new:free";
        let mut on_trial = rec(trial, &[SourceKind::OpenAiCompatible]);
        on_trial.probation = Some(Probation::start(NOW));
        on_trial.meta.card.probation = true;
        for r in [rec("grok-4.7", &[SourceKind::XaiApi]), on_trial] {
            reg.models.insert(r.meta.id.clone(), r);
        }
        let quota = QuotaLedger::default();
        let table = RoutingTable::default();
        let ranked = || vec!["grok-4.7".to_string(), trial.to_string()];
        // No canary: the native model leads a short turn.
        let c = ctx(&quota, Some(TaskSize::Short));
        let mut rules = Vec::new();
        assert_eq!(
            order(ranked(), &reg, &table, &c, true, &mut rules)[0],
            "grok-4.7"
        );
        // In the canary, the model on tryout leads it, with grok behind.
        let canary = ScoreCtx {
            canary: Some("episode-1"),
            ..c
        };
        let mut rules = Vec::new();
        let out = order(ranked(), &reg, &table, &canary, true, &mut rules);
        assert_eq!(out, vec![trial.to_string(), "grok-4.7".to_string()]);
        assert!(rules.contains(&"probation:canary".to_string()), "{rules:?}");
        // A medium turn is real work: the canary doesn't touch it.
        let medium = ScoreCtx {
            size: Some(SizeEstimate {
                size: TaskSize::Medium,
                ..SizeEstimate::default()
            }),
            ..canary
        };
        let mut rules = Vec::new();
        let out = order(ranked(), &reg, &table, &medium, true, &mut rules);
        assert_eq!(out, vec!["grok-4.7".to_string()]);
        assert!(!rules.contains(&"probation:canary".to_string()));
        // Failed: barred on any turn, the canary included.
        if let Some(p) = reg.models.get_mut(trial).unwrap().probation.as_mut() {
            p.stage = Stage::Failed;
        }
        assert_eq!(
            bar(reg.get(trial).unwrap(), &canary),
            Some("score:probation:failed")
        );
        let mut rules = Vec::new();
        let out = order(ranked(), &reg, &table, &canary, true, &mut rules);
        assert_eq!(out, vec!["grok-4.7".to_string()]);
    }
}
