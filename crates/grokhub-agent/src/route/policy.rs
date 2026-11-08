//! The §14.3 class table, as data. Router R1 turns it on for effort: every
//! listed class's calls send the effort the router picks. Effort rungs are
//! names on [`grokhub_core::model_registry::EFFORT_LADDER`].
//!
//! R2a adds the model side: which models may take a call ([`fits`]), how they
//! rank ([`rank`]: the routing table's order, then the lowest expected cost,
//! then the session's own model), and the fallback chain a pinned or failing
//! model falls back along ([`fallback_chain`]).

use std::collections::BTreeMap;

use grokhub_core::model_registry::cost_class::{classify, route_key, CostClass, RouteOpts};
use grokhub_core::model_registry::discover::split_provider_model;
use grokhub_core::model_registry::profile::ModelProfile;
use grokhub_core::model_registry::{health, ModelState, Prices, Registry};

use super::spend::{allowed, Spend};
use super::table::RoutingTable;

/// R1: listed classes send the router's effort.
pub const POLICY_LIVE: bool = true;
/// R2a: Auto picks the model too (one per episode); a pin is kept while it answers.
pub const MODEL_LIVE: bool = true;

/// One class: where effort starts, and the band it may move in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassRow {
    pub class: &'static str,
    pub start: &'static str,
    pub floor: &'static str,
    pub ceiling: &'static str,
    /// Plain words for the reason sentence.
    pub plain: &'static str,
}

const fn row(
    class: &'static str,
    start: &'static str,
    floor: &'static str,
    ceiling: &'static str,
    plain: &'static str,
) -> ClassRow {
    ClassRow {
        class,
        start,
        floor,
        ceiling,
        plain,
    }
}

/// start / floor / ceiling per class (§14.3).
pub const CLASS_TABLE: &[ClassRow] = &[
    row(
        "background:classify",
        "low",
        "none",
        "medium",
        "background sorting",
    ),
    row(
        "background:triage",
        "low",
        "none",
        "medium",
        "background triage",
    ),
    row(
        "background:redact",
        "low",
        "none",
        "medium",
        "background redaction",
    ),
    row(
        "background:summarize",
        "low",
        "none",
        "medium",
        "a background summary",
    ),
    // Judge and dream use a reasoning model at low/medium (start low).
    row(
        "background:judge",
        "low",
        "low",
        "high",
        "an independent check",
    ),
    row(
        "background:compact",
        "low",
        "low",
        "medium",
        "folding old context",
    ),
    row("background:memory", "low", "low", "medium", "memory upkeep"),
    row(
        "background:dream",
        "low",
        "low",
        "medium",
        "the nightly memory tidy",
    ),
    // Speed tier: never pays for high effort (ceiling medium).
    row("chat:quick", "low", "none", "medium", "a quick answer"),
    row("chat:default", "medium", "low", "xhigh", "everyday chat"),
    // Math, logic and proofs typed in chat: reasoning pays here.
    row("chat:reason", "high", "medium", "xhigh", "math and logic"),
    // Small edits reason at low; plan/design/repair diagnose at high.
    row(
        "code:edit-small",
        "low",
        "low",
        "xhigh",
        "a small code edit",
    ),
    row("plan", "high", "medium", "xhigh", "planning"),
    row("design", "high", "medium", "xhigh", "design work"),
    row(
        "code:multi-file",
        "high",
        "medium",
        "xhigh",
        "a multi-file change",
    ),
    row("debug", "high", "medium", "xhigh", "debugging"),
    row("desktop:soft", "medium", "low", "high", "a desktop step"),
    row(
        "prepare:hard",
        "high",
        "high",
        "xhigh",
        "preparing a hard action",
    ),
    row(
        "repair:diagnose",
        "high",
        "medium",
        "xhigh",
        "a computer check",
    ),
];

/// R3a: a class's band with its self-tuned start merged over the table row,
/// clamped inside the floor and ceiling ([`super::ladder::Band::tuned`]).
pub fn tuned_band(row: &ClassRow, bump: u8, tuning: &super::tune::Tuning) -> super::ladder::Band {
    super::ladder::Band::tuned(row, bump, tuning.starts.get(row.class).map(String::as_str))
}

/// Call-site names that already exist, mapped onto table classes.
pub const CLASS_ALIASES: &[(&str, &str)] = &[
    ("episode:step", "desktop:soft"),
    ("background:review", "background:judge"),
    ("chat:turn", "chat:default"),
];

/// The table row for a call class (after aliases), or `None` for a class the
/// table doesn't list (the route then keeps today's effort).
pub fn class_row(class: &str) -> Option<&'static ClassRow> {
    let c = class.trim();
    let c = CLASS_ALIASES
        .iter()
        .find(|(from, _)| *from == c)
        .map(|(_, to)| *to)
        .unwrap_or(c);
    CLASS_TABLE.iter().find(|r| r.class == c)
}

/// Tokens a class's call usually sends and gets back, and the share of the
/// prompt a warm cache covers, for the expected-cost tie-break and the table.
pub fn predicted_tokens(class: &str) -> (u64, u64, u64) {
    let class = class_row(class).map(|r| r.class).unwrap_or(class);
    match class {
        c if c.starts_with("background:") => (2_000, 300, 0),
        "chat:quick" => (3_000, 200, 50),
        "chat:default" => (8_000, 600, 80),
        "chat:reason" => (8_000, 2_000, 80),
        "code:edit-small" => (12_000, 1_000, 80),
        "plan" | "design" => (16_000, 2_000, 70),
        "code:multi-file" | "debug" => (40_000, 3_000, 80),
        "desktop:soft" => (20_000, 400, 85),
        "prepare:hard" => (6_000, 500, 50),
        _ => (8_000, 800, 60),
    }
}

/// List price × predicted tokens with the cache hit in, in USD. A call at or
/// over the model's long-context threshold pays the long prices on every
/// token, cached ones too ([`Prices::rates_at`]). `None` when the listing has
/// no prices.
pub fn expected_cost_usd(prices: &Prices, class: &str, ctx_tokens: u64) -> Option<f64> {
    let (mut input, out, hit_pct) = predicted_tokens(class);
    input = input.max(ctx_tokens);
    let (prompt, cached, completion) = prices.rates_at(input)?;
    let hit = input * hit_pct / 100;
    // Prices are USD cents per 100M tokens.
    let cents = ((input - hit) * prompt + hit * cached + out * completion) as f64 / 100_000_000.0;
    Some(cents / 100.0)
}

/// D12: most turns a pinned chat is expected to run past this one.
pub const PIN_TURNS_CAP: u32 = 8;

/// D12: a chat that has had `turns_so_far` turns is expected to run about as
/// many again, at least one and at most [`PIN_TURNS_CAP`].
pub fn expected_remaining_turns(turns_so_far: u32) -> u32 {
    turns_so_far.clamp(1, PIN_TURNS_CAP)
}

/// D12: what moving a pinned chat to another model costs and saves, in xAI
/// ticks (1e-10 USD: a rate in USD cents per 100M tokens times tokens).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinSwitch {
    /// The whole history re-sent uncached on the new model.
    pub switch_ticks: i64,
    /// The expected rest of the chat, each turn's old cost less its new cost.
    pub saving_ticks: i64,
    pub remaining_turns: u32,
}

impl PinSwitch {
    /// The saving beats the re-send.
    pub fn pays(&self) -> bool {
        self.saving_ticks > self.switch_ticks
    }
}

/// One more turn of a chat with `history` tokens behind it: the history read
/// from the cache, the class's new (uncached) share of a prompt and its reply.
fn turn_ticks(prices: &Prices, class: &str, history: u64) -> Option<i64> {
    let (input, out, hit_pct) = predicted_tokens(class);
    let fresh = input * (100 - hit_pct.min(100)) / 100;
    let (prompt, cached, completion) = prices.rates_at(history + fresh)?;
    Some((history * cached + fresh * prompt + out * completion) as i64)
}

/// D12: the price of moving a long chat (`history` prompt tokens, after
/// `turns_so_far` turns) from the model priced `old` to the one priced `new`.
/// `None` when either listing has no prompt or completion price.
pub fn pin_switch(
    old: &Prices,
    new: &Prices,
    class: &str,
    history: u64,
    turns_so_far: u32,
) -> Option<PinSwitch> {
    let per_turn = turn_ticks(old, class, history)? - turn_ticks(new, class, history)?;
    let (uncached, _, _) = new.rates_at(history)?;
    let remaining_turns = expected_remaining_turns(turns_so_far);
    Some(PinSwitch {
        switch_ticks: (history * uncached) as i64,
        saving_ticks: per_turn * remaining_turns as i64,
        remaining_turns,
    })
}

/// What a step needs from a model.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Fit {
    pub needs_image: bool,
    pub needs_tools: bool,
    pub ctx_tokens: u64,
    /// The Grok Build path: GB's own plan list decides, not the native cost class.
    pub grok_build: bool,
}

/// A model the router may route a step to: a routable state with no live
/// ghost strike, a usable profile (none yet keeps today's behavior), a cost
/// class `spend` allows ([`approved`]), and it fits the step (context, image
/// input, tools, the Responses API).
pub fn fits(
    reg: &Registry,
    profiles: &BTreeMap<String, ModelProfile>,
    id: &str,
    fit: Fit,
    spend: &Spend,
    now_ms: u64,
) -> bool {
    fits_any_cost(reg, profiles, id, fit, now_ms) && approved(reg, id, fit.grok_build, spend)
}

/// [`fits`] without the cost check: a Fast variant the latency policy may
/// pick, or a model outside the plan the upgrade nudge counts.
pub fn fits_any_cost(
    reg: &Registry,
    profiles: &BTreeMap<String, ModelProfile>,
    id: &str,
    fit: Fit,
    now_ms: u64,
) -> bool {
    let Some(rec) = reg.get(id) else {
        return false;
    };
    let strikes = rec
        .not_found
        .iter()
        .filter(|t| now_ms.saturating_sub(**t) <= health::STRIKE_WINDOW_MS)
        .count();
    if !rec.state.routable() || strikes >= health::GHOST_STRIKES {
        return false;
    }
    let profile = profiles.get(id);
    if profile.is_some_and(|p| !p.usable) {
        return false;
    }
    let modalities = profile
        .and_then(|p| p.metadata.input_modalities.as_ref())
        .or(rec.meta.input_modalities.as_ref());
    if fit.needs_image && modalities.is_some_and(|m| !m.iter().any(|x| x == "image")) {
        return false;
    }
    if fit.needs_tools
        && (rec.meta.tool_calling == Some(false)
            || profile.is_some_and(|p| p.runtime.tool_adapter == "none"))
    {
        return false;
    }
    if !fit.grok_build
        && rec
            .meta
            .api_shape
            .as_deref()
            .is_some_and(|s| s != "responses")
    {
        return false;
    }
    !rec.meta.context_length.is_some_and(|c| fit.ctx_tokens > c)
}

/// The cost class of `id`'s plain route for this sign-in (R2b). On the GB
/// path, Grok Build's own plan list is the pool.
pub fn route_class(reg: &Registry, id: &str, grok_build: bool, spend: &Spend) -> CostClass {
    classify(
        reg,
        id,
        reg.entitlement.credential,
        grok_build,
        RouteOpts::default(),
        spend.settings.ceiling_usd_per_m,
    )
}

/// May Auto route here on its own: `included`, or `premium` under your grant
/// for this route. Fast variants are never in the pool; [`super::Router`]
/// swaps one in only under the latency policy.
pub fn approved(reg: &Registry, id: &str, grok_build: bool, spend: &Spend) -> bool {
    match route_class(reg, id, grok_build, spend) {
        // R3b: a provider you added, only with its key and a grant covering this call's data.
        CostClass::NewProvider => {
            split_provider_model(id).is_some_and(|(p, _)| spend.providers.iter().any(|x| x == p))
        }
        class => allowed(class, &route_key(id, RouteOpts::default()), spend),
    }
}

/// A model that answers something other than a chat (a safety classifier,
/// an embedding or rerank model): Auto never adds it to its pool. Matched on
/// whole words of the id (`llama-guard-4`, `text-embedding-3`), so
/// `vanguard-70b` stays a chat model; the router also leaves out a model
/// whose listing says it answers in something other than text.
pub fn utility_model(id: &str) -> bool {
    const WORDS: [&str; 10] = [
        "safety",
        "guard",
        "moderation",
        "embed",
        "embedding",
        "embeddings",
        "rerank",
        "reranker",
        "shield",
        "shieldgemma",
    ];
    id.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| WORDS.contains(&w))
}

/// A model on a provider you added (R3b), not xAI: only its own sources list it.
pub fn is_new_provider(reg: &Registry, id: &str) -> bool {
    reg.get(id)
        .is_some_and(|r| !r.sources.is_empty() && r.sources.iter().all(|s| s.is_new_provider()))
}

/// A newer model of `model`'s family is outside your plan (`not_in_plan`) and
/// could take the step: a quality step would have picked it. Counted for the
/// upgrade nudge only; it is never routed to.
pub fn outranked_by_plan_upgrade(
    reg: &Registry,
    profiles: &BTreeMap<String, ModelProfile>,
    model: &str,
    fit: Fit,
) -> bool {
    let Some((fam, v)) = family(model) else {
        return false;
    };
    reg.models.values().any(|r| {
        r.state == ModelState::NotInPlan
            && family(&r.meta.id).is_some_and(|(f, w)| f == fam && w > v)
            && !r.meta.context_length.is_some_and(|c| fit.ctx_tokens > c)
            && !(fit.needs_image
                && r.meta
                    .input_modalities
                    .as_ref()
                    .is_some_and(|m| !m.iter().any(|x| x == "image")))
            && profiles
                .get(&r.meta.id)
                .is_none_or(|p| p.usable || p.probe.status.starts_with("not_run"))
    })
}

/// A model id's family and version: `grok-4.7` is (`grok-*`, [4, 7]),
/// `grok-4-fast` is (`grok-*-fast`, [4]). `None` without a version.
pub fn family(id: &str) -> Option<(String, Vec<u32>)> {
    let parts: Vec<&str> = id.trim().split('-').collect();
    let at = parts.iter().position(|p| {
        !p.is_empty()
            && p.split('.')
                .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    })?;
    let version = parts[at]
        .split('.')
        .filter_map(|n| n.parse().ok())
        .collect();
    let mut key: Vec<&str> = parts.clone();
    key[at] = "*";
    Some((key.join("-"), version))
}

/// The fallback chain for `from`: the other candidates of the same provider
/// and family, newest first (grok-4.7 → grok-4.6 → grok-4.5 → grok-4.3).
/// Built from `candidates` (already entitled, healthy and included), never
/// hard-coded. Local models come next once there are any; after that the
/// call pauses and tells.
pub fn fallback_chain(from: &str, candidates: &[&str]) -> Vec<String> {
    let Some((key, _)) = family(from) else {
        return Vec::new();
    };
    let mut same: Vec<(Vec<u32>, &str)> = candidates
        .iter()
        .filter(|c| c.trim() != from.trim())
        .filter_map(|c| family(c).filter(|(k, _)| *k == key).map(|(_, v)| (v, *c)))
        .collect();
    same.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(b.1)));
    same.into_iter().map(|(_, c)| c.to_string()).collect()
}

/// Each candidate's capability fit for `class` (the baseline score
/// [`rank`] orders by), scored once per model.
fn fit_scores(class: &str, candidates: &[&str], reg: &Registry) -> Vec<f64> {
    let need = super::capabilities::class_need(class);
    let lane = super::capabilities::BaselineLane::of(class);
    let caps: Vec<_> = candidates
        .iter()
        .map(|id| {
            let mut caps = super::capabilities::derive_caps(
                &reg.get(id)
                    .map(|r| r.meta.clone())
                    .unwrap_or_else(|| grokhub_core::model_registry::ModelMeta::bare(id)),
            );
            super::capabilities::apply_seed(id, &mut caps);
            caps
        })
        .collect();
    let plan_max_r = super::capabilities::plan_max_reasoning(caps.iter());
    caps.iter()
        .map(|c| super::capabilities::baseline_score(c, &need, lane, plan_max_r))
        .collect()
}

/// Rank the candidates for `class`: the routing table's order first, then
/// (when the table has no row) capability fit for the class, then the lowest
/// expected cost, then the session's own model, then the id.
/// Degraded models stay eligible but go last.
pub fn rank(
    class: &str,
    candidates: &[&str],
    table: &RoutingTable,
    reg: &Registry,
    current: &str,
    ctx_tokens: u64,
) -> Vec<String> {
    let rows = table.rows(class);
    let fits = fit_scores(class, candidates, reg);
    // (degraded, table position, fit, cost, not the session's own, id), once each.
    let mut keyed: Vec<(bool, usize, f64, f64, bool, &str)> = candidates
        .iter()
        .zip(fits)
        .map(|(&id, fit)| {
            let rec = reg.get(id);
            (
                rec.is_some_and(|r| r.state == ModelState::Degraded),
                rows.iter()
                    .position(|r| r.model == id)
                    .unwrap_or(usize::MAX),
                fit,
                rec.and_then(|r| expected_cost_usd(&r.meta.prices, class, ctx_tokens))
                    .unwrap_or(f64::MAX),
                id.trim() != current,
                id,
            )
        })
        .collect();
    keyed.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.cmp(&b.1))
            .then(if a.1 == usize::MAX && b.1 == usize::MAX {
                b.2.total_cmp(&a.2)
            } else {
                std::cmp::Ordering::Equal
            })
            .then(a.3.total_cmp(&b.3))
            .then(a.4.cmp(&b.4))
            .then(a.5.cmp(b.5))
    });
    keyed.into_iter().map(|k| k.5.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::model_registry::EFFORT_LADDER;

    fn rung(e: &str) -> usize {
        EFFORT_LADDER.iter().position(|l| *l == e).unwrap()
    }

    #[test]
    fn every_row_is_on_the_ladder_and_floor_le_start_le_ceiling() {
        assert_eq!(CLASS_TABLE.len(), 19);
        for r in CLASS_TABLE {
            assert!(
                rung(r.floor) <= rung(r.start) && rung(r.start) <= rung(r.ceiling),
                "{}",
                r.class
            );
        }
        let hard = class_row("prepare:hard").unwrap();
        assert_eq!(
            (hard.start, hard.floor, hard.ceiling),
            ("high", "high", "xhigh")
        );
        assert_eq!(class_row("episode:step").unwrap().class, "desktop:soft");
        assert_eq!(class_row("eval:item"), None);
        const { assert!(POLICY_LIVE && MODEL_LIVE) };
    }

    #[test]
    fn families_and_fallback_chains_are_built_from_the_candidates_newest_first() {
        assert_eq!(family("grok-4.7"), Some(("grok-*".to_string(), vec![4, 7])));
        assert_eq!(
            family("grok-4-fast"),
            Some(("grok-*-fast".to_string(), vec![4]))
        );
        assert_eq!(
            family("grok-code-fast-1"),
            Some(("grok-code-fast-*".to_string(), vec![1]))
        );
        assert_eq!(family("mystery"), None);
        let c = [
            "grok-4.3",
            "grok-4.7",
            "grok-4.5",
            "grok-4-fast",
            "grok-4.6",
        ];
        assert_eq!(
            fallback_chain("grok-4.7", &c),
            vec!["grok-4.6", "grok-4.5", "grok-4.3"]
        );
        assert_eq!(
            fallback_chain("grok-4.6", &c),
            vec!["grok-4.7", "grok-4.5", "grok-4.3"]
        );
        assert_eq!(fallback_chain("grok-4-fast", &c), Vec::<String>::new());
        assert_eq!(fallback_chain("mystery", &c), Vec::<String>::new());
    }

    #[test]
    fn expected_cost_counts_the_cache_and_long_context_prices() {
        let p = Prices {
            prompt: Some(20_000),
            cached: Some(5_000),
            cache_write: None,
            completion: Some(100_000),
            prompt_long: Some(40_000),
            completion_long: Some(200_000),
            long_context_threshold: Some(200_000),
        };
        // chat:default: 8000 in (80% cached), 600 out.
        let want = (1_600.0 * 20_000.0 + 6_400.0 * 5_000.0 + 600.0 * 100_000.0) / 1e8 / 100.0;
        assert_eq!(expected_cost_usd(&p, "chat:default", 0), Some(want));
        let long = expected_cost_usd(&p, "chat:default", 250_000).unwrap();
        let short = expected_cost_usd(
            &Prices {
                long_context_threshold: None,
                ..p.clone()
            },
            "chat:default",
            250_000,
        )
        .unwrap();
        // 250k in, 80% cached: every token pays the long prices, the cached ones
        // at twice their rate too.
        assert_eq!(
            (long, short),
            (
                (50_000.0 * 40_000.0 + 200_000.0 * 10_000.0 + 600.0 * 200_000.0) / 1e8 / 100.0,
                (50_000.0 * 20_000.0 + 200_000.0 * 5_000.0 + 600.0 * 100_000.0) / 1e8 / 100.0
            )
        );
        assert_eq!(
            expected_cost_usd(&Prices::default(), "chat:default", 0),
            None
        );
    }

    fn tiered(v: u64) -> Prices {
        Prices {
            prompt: Some(10_000 * v),
            cached: Some(2_500 * v),
            completion: Some(50_000 * v),
            long_context_threshold: Some(200_000),
            ..Prices::default()
        }
    }

    #[test]
    fn a_pinned_chat_moves_down_only_when_the_saving_beats_the_uncached_resend() {
        assert_eq!(PIN_TURNS_CAP, 8);
        assert_eq!(
            [0, 1, 5, 8, 99].map(expected_remaining_turns),
            [1, 1, 5, 8, 8]
        );
        let (old, new) = (tiered(7), tiered(3));
        // chat:default: 1,600 new tokens and a 600-token reply a turn, 20,000 of history.
        // Old turn 672M ticks, new turn 288M, the re-send 20,000 × 30,000 = 600M.
        let one = pin_switch(&old, &new, "chat:default", 20_000, 1).unwrap();
        assert_eq!(
            one,
            PinSwitch {
                switch_ticks: 600_000_000,
                saving_ticks: 384_000_000,
                remaining_turns: 1,
            }
        );
        assert!(!one.pays());
        let two = pin_switch(&old, &new, "chat:default", 20_000, 2).unwrap();
        assert_eq!(
            (two.saving_ticks, two.remaining_turns, two.pays()),
            (768_000_000, 2, true)
        );
        let long = pin_switch(&old, &new, "chat:default", 20_000, 40).unwrap();
        assert_eq!(
            (long.saving_ticks, long.remaining_turns),
            (3_072_000_000, 8)
        );
        // Moving up never saves.
        let up = pin_switch(&new, &old, "chat:default", 20_000, 40).unwrap();
        assert_eq!(up.saving_ticks, -3_072_000_000);
        assert!(!up.pays());
        // No price, no move.
        assert_eq!(
            pin_switch(&old, &Prices::default(), "chat:default", 20_000, 4),
            None
        );
    }
}
