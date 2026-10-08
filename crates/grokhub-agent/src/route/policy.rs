//! The §14.3 class table, as data. Router R1 turns it on for effort: every
//! listed class's calls send the effort the router picks. Effort rungs are
//! names on [`grokhub_core::model_registry::EFFORT_LADDER`].
//!
//! R2a adds the model side: which models may take a call ([`fits`]), how they
//! rank ([`rank`]: the routing table's order, then the lowest expected cost,
//! then the session's own model), and the fallback chain a pinned or failing
//! model falls back along ([`fallback_chain`]).

use std::collections::BTreeMap;

use grokhub_core::model_registry::cost_class::{cost_class, CostClass};
use grokhub_core::model_registry::profile::ModelProfile;
use grokhub_core::model_registry::{health, ModelRecord, ModelState, Prices, Registry, SourceKind};

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

const fn row(class: &'static str, start: &'static str, floor: &'static str, ceiling: &'static str, plain: &'static str) -> ClassRow {
    ClassRow { class, start, floor, ceiling, plain }
}

/// start / floor / ceiling per class (§14.3).
pub const CLASS_TABLE: &[ClassRow] = &[
    row("background:classify", "low", "none", "medium", "background sorting"),
    row("background:triage", "low", "none", "medium", "background triage"),
    row("background:redact", "low", "none", "medium", "background redaction"),
    row("background:summarize", "low", "none", "medium", "a background summary"),
    row("background:judge", "low", "low", "high", "an independent check"),
    row("background:compact", "low", "low", "medium", "folding old context"),
    row("background:memory", "low", "low", "medium", "memory upkeep"),
    row("background:dream", "low", "low", "medium", "the nightly memory tidy"),
    row("chat:quick", "low", "none", "high", "a quick answer"),
    row("chat:default", "medium", "low", "xhigh", "everyday chat"),
    row("code:edit-small", "medium", "low", "xhigh", "a small code edit"),
    row("plan", "high", "medium", "xhigh", "planning"),
    row("design", "high", "medium", "xhigh", "design work"),
    row("code:multi-file", "high", "medium", "xhigh", "a multi-file change"),
    row("debug", "high", "medium", "xhigh", "debugging"),
    row("desktop:soft", "medium", "low", "high", "a desktop step"),
    row("prepare:hard", "high", "high", "xhigh", "preparing a hard action"),
    row("repair:diagnose", "medium", "low", "high", "a computer check"),
];

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
    let c = CLASS_ALIASES.iter().find(|(from, _)| *from == c).map(|(_, to)| *to).unwrap_or(c);
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
        "code:edit-small" => (12_000, 1_000, 80),
        "plan" | "design" => (16_000, 2_000, 70),
        "code:multi-file" | "debug" => (40_000, 3_000, 80),
        "desktop:soft" => (20_000, 400, 85),
        "prepare:hard" => (6_000, 500, 50),
        _ => (8_000, 800, 60),
    }
}

/// List price × predicted tokens with the cache hit in, in USD. A call over
/// the model's long-context threshold pays the long prices. `None` when the
/// listing has no prices.
pub fn expected_cost_usd(prices: &Prices, class: &str, ctx_tokens: u64) -> Option<f64> {
    let (mut input, out, hit_pct) = predicted_tokens(class);
    input = input.max(ctx_tokens);
    let long = prices.long_context_threshold.is_some_and(|t| t > 0 && input >= t);
    let prompt = if long { prices.prompt_long.or(prices.prompt) } else { prices.prompt }?;
    let completion = if long { prices.completion_long.or(prices.completion) } else { prices.completion }?;
    let cached = prices.cached.unwrap_or(prompt);
    let hit = input * hit_pct / 100;
    // Prices are USD cents per 100M tokens.
    let cents = ((input - hit) * prompt + hit * cached + out * completion) as f64 / 100_000_000.0;
    Some(cents / 100.0)
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
/// ghost strike, a usable profile (none yet keeps today's behavior), on the
/// approved route (only `included` until R2b; GB's own plan list on the GB
/// path), and it fits the step (context, image input, tools, the Responses API).
pub fn fits(reg: &Registry, profiles: &BTreeMap<String, ModelProfile>, id: &str, fit: Fit, now_ms: u64) -> bool {
    let Some(rec) = reg.get(id) else {
        return false;
    };
    let strikes = rec.not_found.iter().filter(|t| now_ms.saturating_sub(**t) <= health::STRIKE_WINDOW_MS).count();
    if !rec.state.routable() || strikes >= health::GHOST_STRIKES {
        return false;
    }
    if !approved(reg, rec, fit.grok_build) {
        return false;
    }
    let profile = profiles.get(id);
    if profile.is_some_and(|p| !p.usable) {
        return false;
    }
    let modalities = profile.and_then(|p| p.metadata.input_modalities.as_ref()).or(rec.meta.input_modalities.as_ref());
    if fit.needs_image && modalities.is_some_and(|m| !m.iter().any(|x| x == "image")) {
        return false;
    }
    if fit.needs_tools && (rec.meta.tool_calling == Some(false) || profile.is_some_and(|p| p.runtime.tool_adapter == "none")) {
        return false;
    }
    if !fit.grok_build && rec.meta.api_shape.as_deref().is_some_and(|s| s != "responses") {
        return false;
    }
    !rec.meta.context_length.is_some_and(|c| fit.ctx_tokens > c)
}

/// Only `included` routes are approved until R2b (cost classes). On the GB
/// path the model must be in Grok Build's own plan list.
pub fn approved(reg: &Registry, rec: &ModelRecord, grok_build: bool) -> bool {
    if grok_build {
        return rec.sources.contains(&SourceKind::GrokBuild);
    }
    cost_class(reg.entitlement.credential, Some(rec)) == CostClass::Included
}

/// A model id's family and version: `grok-4.7` is (`grok-*`, [4, 7]),
/// `grok-4-fast` is (`grok-*-fast`, [4]). `None` without a version.
pub fn family(id: &str) -> Option<(String, Vec<u32>)> {
    let parts: Vec<&str> = id.trim().split('-').collect();
    let at = parts.iter().position(|p| !p.is_empty() && p.split('.').all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))?;
    let version = parts[at].split('.').filter_map(|n| n.parse().ok()).collect();
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

/// Rank the candidates for `class`: the routing table's order first, then
/// the lowest expected cost, then the session's own model, then the id.
/// Degraded models stay eligible but go last.
pub fn rank(class: &str, candidates: &[&str], table: &RoutingTable, reg: &Registry, current: &str, ctx_tokens: u64) -> Vec<String> {
    let rows = table.rows(class);
    let pos = |id: &str| rows.iter().position(|r| r.model == id).unwrap_or(usize::MAX);
    let cost = |id: &str| reg.get(id).and_then(|r| expected_cost_usd(&r.meta.prices, class, ctx_tokens)).unwrap_or(f64::MAX);
    let degraded = |id: &str| reg.get(id).is_some_and(|r| r.state == ModelState::Degraded);
    let mut out: Vec<&str> = candidates.to_vec();
    out.sort_by(|a, b| {
        degraded(a)
            .cmp(&degraded(b))
            .then(pos(a).cmp(&pos(b)))
            .then(cost(a).total_cmp(&cost(b)))
            .then((a.trim() != current).cmp(&(b.trim() != current)))
            .then(a.cmp(b))
    });
    out.into_iter().map(str::to_string).collect()
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
        assert_eq!(CLASS_TABLE.len(), 18);
        for r in CLASS_TABLE {
            assert!(rung(r.floor) <= rung(r.start) && rung(r.start) <= rung(r.ceiling), "{}", r.class);
        }
        let hard = class_row("prepare:hard").unwrap();
        assert_eq!((hard.start, hard.floor, hard.ceiling), ("high", "high", "xhigh"));
        assert_eq!(class_row("episode:step").unwrap().class, "desktop:soft");
        assert_eq!(class_row("eval:item"), None);
        const { assert!(POLICY_LIVE && MODEL_LIVE) };
    }

    #[test]
    fn families_and_fallback_chains_are_built_from_the_candidates_newest_first() {
        assert_eq!(family("grok-4.7"), Some(("grok-*".to_string(), vec![4, 7])));
        assert_eq!(family("grok-4-fast"), Some(("grok-*-fast".to_string(), vec![4])));
        assert_eq!(family("grok-code-fast-1"), Some(("grok-code-fast-*".to_string(), vec![1])));
        assert_eq!(family("mystery"), None);
        let c = ["grok-4.3", "grok-4.7", "grok-4.5", "grok-4-fast", "grok-4.6"];
        assert_eq!(fallback_chain("grok-4.7", &c), vec!["grok-4.6", "grok-4.5", "grok-4.3"]);
        assert_eq!(fallback_chain("grok-4.6", &c), vec!["grok-4.7", "grok-4.5", "grok-4.3"]);
        assert_eq!(fallback_chain("grok-4-fast", &c), Vec::<String>::new());
        assert_eq!(fallback_chain("mystery", &c), Vec::<String>::new());
    }

    #[test]
    fn expected_cost_counts_the_cache_and_long_context_prices() {
        let p = Prices { prompt: Some(20_000), cached: Some(5_000), completion: Some(100_000), prompt_long: Some(40_000), completion_long: Some(200_000), long_context_threshold: Some(200_000) };
        // chat:default: 8000 in (80% cached), 600 out.
        let want = (1_600.0 * 20_000.0 + 6_400.0 * 5_000.0 + 600.0 * 100_000.0) / 1e8 / 100.0;
        assert_eq!(expected_cost_usd(&p, "chat:default", 0), Some(want));
        let long = expected_cost_usd(&p, "chat:default", 250_000).unwrap();
        let short = expected_cost_usd(&Prices { long_context_threshold: None, ..p.clone() }, "chat:default", 250_000).unwrap();
        // 250k in, 80% cached: the uncached prompt and the output pay the long prices.
        assert_eq!((long, short), ((50_000.0 * 40_000.0 + 200_000.0 * 5_000.0 + 600.0 * 200_000.0) / 1e8 / 100.0, (50_000.0 * 20_000.0 + 200_000.0 * 5_000.0 + 600.0 * 100_000.0) / 1e8 / 100.0));
        assert_eq!(expected_cost_usd(&Prices::default(), "chat:default", 0), None);
    }
}
