//! Who pays for a route, and how much extra (Router R2b, §14.4.5). A route is
//! a model plus how it is sent (service tier, endpoint, multi-agent). The
//! class comes from registry data only: Fast variants, price against the
//! user's $/M ceiling, the tier and the endpoint.
//!
//! - `included`: the signed-in plan's pool, the user's own API key at list
//!   price on the global endpoint, and local models. Used freely, inside caps.
//! - `autonomous_premium`: a Fast variant (same model, about 2× the price).
//!   Auto may take it on its own only while the user is waiting and speed
//!   clearly matters, and only when the registry says it's entitled.
//! - `premium`: `service_tier: "priority"`, the US regional endpoint, 16-agent
//!   `xhigh`, or a model over the $/M ceiling. Only after the user's one-time click.
//! - `extra_spend`: credits, top-ups and new plans. Never: GrokHub has no path to it.
//! - `new_provider`: anything not xAI (R3).
//!
//! The onboarding probe and the R2a evals only run on [`probe_class`]
//! `included` routes: the plan pool, never a key, a Fast variant or a premium route.

use serde::{Deserialize, Serialize};

use super::entitlement::Credential;
use super::record::{ModelRecord, SourceKind};
use super::Registry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostClass {
    Included,
    AutonomousPremium,
    Premium,
    /// Credits, top-ups, new subscriptions. Nothing routes here, ever.
    ExtraSpend,
    NewProvider,
    Unknown,
}

impl CostClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Included => "included",
            Self::AutonomousPremium => "autonomous_premium",
            Self::Premium => "premium",
            Self::ExtraSpend => "extra_spend",
            Self::NewProvider => "new_provider",
            Self::Unknown => "unknown",
        }
    }
}

/// The $/M output price a key route may cost before it is `premium`, unless
/// the user changes it in Settings.
pub const DEFAULT_CEILING_USD_PER_M: f64 = 15.0;
/// A `-fast` row priced at least this many times its base is a Fast variant.
pub const FAST_PRICE_RATIO: f64 = 1.5;
/// The US regional endpoint's markup (+10%), for the premium card.
pub const US_ENDPOINT_MARKUP_PCT: u32 = 10;

/// How a route is sent, beyond the model. All off is the plain global route.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteOpts {
    /// `service_tier: "priority"`.
    #[serde(default)]
    pub priority: bool,
    /// The US regional endpoint.
    #[serde(default)]
    pub us_endpoint: bool,
    /// Multi-agent with 16 agents at `xhigh`.
    #[serde(default)]
    pub multi_agent_16: bool,
}

impl RouteOpts {
    pub fn plain(self) -> bool {
        self == Self::default()
    }
}

/// The grant key for a premium route: `premium:<model>`, plus `+priority`,
/// `+us` and `+agents16` for its options. One click approves exactly this key.
pub fn route_key(model: &str, opts: RouteOpts) -> String {
    let mut key = format!("premium:{}", model.trim());
    for (on, tag) in [(opts.priority, "+priority"), (opts.us_endpoint, "+us"), (opts.multi_agent_16, "+agents16")] {
        if on {
            key.push_str(tag);
        }
    }
    key
}

/// USD per million output tokens. Prices are USD cents per 100M tokens.
pub fn usd_per_m(cents_per_100m: u64) -> f64 {
    cents_per_100m as f64 / 10_000.0
}

/// The plain model a `-fast` row speeds up, when the row is a Fast variant:
/// its base is in the registry, and it is priced at least [`FAST_PRICE_RATIO`]×
/// the base or (with no price) Grok Build lists it and the xAI API doesn't.
pub fn fast_base<'a>(reg: &'a Registry, id: &str) -> Option<&'a str> {
    let rec = reg.get(id)?;
    let base = rec.meta.id.strip_suffix("-fast")?;
    let (base_id, base_rec) = reg.models.get_key_value(base)?;
    let price = |r: &ModelRecord| r.meta.prices.completion.or(r.meta.prices.prompt);
    let fast = match (price(rec), price(base_rec)) {
        (Some(f), Some(b)) if b > 0 => f as f64 >= b as f64 * FAST_PRICE_RATIO,
        (None, _) => rec.sources.contains(&SourceKind::GrokBuild) && !rec.sources.contains(&SourceKind::XaiApi),
        _ => false,
    };
    fast.then_some(base_id.as_str())
}

/// The Fast variant of `base`, if the registry has one.
pub fn fast_of<'a>(reg: &'a Registry, base: &str) -> Option<&'a str> {
    let id = format!("{}-fast", base.trim());
    let (key, _) = reg.models.get_key_value(id.as_str())?;
    fast_base(reg, key).map(|_| key.as_str())
}

/// The registry says this Fast row may be used: Grok Build lists it in this
/// plan (so not GB's free tier) and it is in a routable state.
pub fn fast_entitled(reg: &Registry, id: &str) -> bool {
    reg.get(id).is_some_and(|r| r.state.routable() && r.sources.contains(&SourceKind::GrokBuild))
}

/// The cost class of one route. `grok_build` is the Grok Build path (GB's own
/// plan list decides what is in the pool). `ceiling` is the user's $/M output
/// ceiling for key routes. An unlisted model is `unknown`.
pub fn classify(reg: &Registry, id: &str, cred: Credential, grok_build: bool, opts: RouteOpts, ceiling_usd_per_m: f64) -> CostClass {
    let Some(rec) = reg.get(id) else {
        return CostClass::Unknown;
    };
    if !opts.plain() {
        return CostClass::Premium;
    }
    if !grok_build && cred == Credential::None {
        // Not signed in: nothing native is billed or included.
        return CostClass::Unknown;
    }
    if fast_base(reg, id).is_some() {
        return CostClass::AutonomousPremium;
    }
    if grok_build {
        return if rec.sources.contains(&SourceKind::GrokBuild) { CostClass::Included } else { CostClass::Unknown };
    }
    match cred {
        Credential::Plan if rec.state.routable() && rec.sources.contains(&SourceKind::XaiApi) => CostClass::Included,
        Credential::ApiKey => match rec.meta.prices.completion {
            Some(p) if usd_per_m(p) > ceiling_usd_per_m => CostClass::Premium,
            Some(_) => CostClass::Included,
            None => CostClass::Unknown,
        },
        _ => CostClass::Unknown,
    }
}

/// The class the onboarding probe and the R2a evals check: only the plan
/// pool's plain routes are `included` here, never a key, a Fast variant or a
/// premium route.
pub fn probe_class(reg: &Registry, cred: Credential, id: &str) -> CostClass {
    if cred != Credential::Plan {
        return CostClass::Unknown;
    }
    classify(reg, id, cred, false, RouteOpts::default(), DEFAULT_CEILING_USD_PER_M)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_registry::record::{ModelMeta, ModelState, Prices};

    fn row(id: &str, completion: Option<u64>, sources: &[SourceKind]) -> ModelRecord {
        let prices = Prices { completion, prompt: completion.map(|c| c / 5), ..Prices::default() };
        ModelRecord { meta: ModelMeta { id: id.into(), prices, ..ModelMeta::default() }, sources: sources.to_vec(), ..ModelRecord::default() }
    }

    fn reg(rows: Vec<ModelRecord>) -> Registry {
        let mut r = Registry::default();
        for m in rows {
            r.models.insert(m.meta.id.clone(), m);
        }
        r
    }

    const API: &[SourceKind] = &[SourceKind::XaiApi];
    const BOTH: &[SourceKind] = &[SourceKind::XaiApi, SourceKind::GrokBuild];
    const GB: &[SourceKind] = &[SourceKind::GrokBuild];

    #[test]
    fn plan_and_list_price_keys_are_included_and_the_ceiling_makes_premium() {
        // grok-4.7 at $15/M out, grok-heavy at $60/M out.
        let r = reg(vec![row("grok-4.7", Some(150_000), BOTH), row("grok-heavy", Some(600_000), API)]);
        let plain = RouteOpts::default();
        assert_eq!(classify(&r, "grok-4.7", Credential::Plan, false, plain, 15.0), CostClass::Included);
        assert_eq!(classify(&r, "grok-4.7", Credential::ApiKey, false, plain, 15.0), CostClass::Included);
        assert_eq!(classify(&r, "grok-heavy", Credential::ApiKey, false, plain, 15.0), CostClass::Premium);
        assert_eq!(classify(&r, "grok-heavy", Credential::ApiKey, false, plain, 60.0), CostClass::Included);
        // The plan pool doesn't bill per token, so the ceiling doesn't apply there.
        assert_eq!(classify(&r, "grok-heavy", Credential::Plan, false, plain, 15.0), CostClass::Included);
        assert_eq!(classify(&r, "grok-4.7", Credential::None, false, plain, 15.0), CostClass::Unknown);
        assert_eq!(classify(&r, "nope", Credential::Plan, false, plain, 15.0), CostClass::Unknown);
        let mut gone = r.clone();
        gone.models.get_mut("grok-4.7").unwrap().state = ModelState::NotInPlan;
        assert_eq!(classify(&gone, "grok-4.7", Credential::Plan, false, plain, 15.0), CostClass::Unknown);
    }

    #[test]
    fn priority_us_and_sixteen_agents_are_premium_on_any_model() {
        let r = reg(vec![row("grok-4.7", Some(150_000), BOTH)]);
        for opts in [
            RouteOpts { priority: true, ..RouteOpts::default() },
            RouteOpts { us_endpoint: true, ..RouteOpts::default() },
            RouteOpts { multi_agent_16: true, ..RouteOpts::default() },
        ] {
            assert_eq!(classify(&r, "grok-4.7", Credential::Plan, false, opts, 15.0), CostClass::Premium, "{opts:?}");
            assert_eq!(classify(&r, "grok-4.7", Credential::ApiKey, false, opts, 100.0), CostClass::Premium, "{opts:?}");
        }
        let all = RouteOpts { priority: true, us_endpoint: true, multi_agent_16: true };
        assert_eq!(route_key("grok-4.7", all), "premium:grok-4.7+priority+us+agents16");
        assert_eq!(route_key(" grok-heavy ", RouteOpts::default()), "premium:grok-heavy");
    }

    #[test]
    fn a_fast_variant_is_autonomous_premium_and_old_cheap_fast_models_are_not() {
        let r = reg(vec![
            row("grok-4.7", Some(150_000), BOTH),
            // GB-only, no price: the Grok 4.7 Fast shape.
            row("grok-4.7-fast", None, GB),
            // An API-listed `-fast` priced at 2× its base.
            row("grok-4.6", Some(100_000), API),
            row("grok-4.6-fast", Some(200_000), API),
            // grok-4-fast is cheaper than grok-4: not a Fast variant.
            row("grok-4", Some(150_000), API),
            row("grok-4-fast", Some(5_000), API),
            // No base in the registry: not a Fast variant either.
            row("grok-code-fast", Some(15_000), API),
        ]);
        assert_eq!(fast_base(&r, "grok-4.7-fast"), Some("grok-4.7"));
        assert_eq!(fast_of(&r, "grok-4.7"), Some("grok-4.7-fast"));
        assert_eq!(fast_of(&r, "grok-4.6"), Some("grok-4.6-fast"));
        assert_eq!(fast_of(&r, "grok-4"), None);
        assert_eq!(fast_base(&r, "grok-code-fast"), None);
        for (id, gb) in [("grok-4.7-fast", true), ("grok-4.6-fast", false)] {
            assert_eq!(classify(&r, id, Credential::Plan, gb, RouteOpts::default(), 15.0), CostClass::AutonomousPremium, "{id}");
        }
        assert_eq!(classify(&r, "grok-4-fast", Credential::Plan, false, RouteOpts::default(), 15.0), CostClass::Included);
        assert!(fast_entitled(&r, "grok-4.7-fast"));
        assert!(!fast_entitled(&r, "grok-4.6-fast"), "the xAI API alone doesn't say the plan includes it");
        let mut free = r.clone();
        free.models.get_mut("grok-4.7-fast").unwrap().state = ModelState::NotInPlan;
        assert!(!fast_entitled(&free, "grok-4.7-fast"));
    }

    #[test]
    fn probes_and_evals_only_see_the_plan_pool() {
        let r = reg(vec![row("grok-4.7", Some(150_000), BOTH), row("grok-4.7-fast", Some(300_000), BOTH)]);
        assert_eq!(probe_class(&r, Credential::Plan, "grok-4.7"), CostClass::Included);
        assert_eq!(probe_class(&r, Credential::ApiKey, "grok-4.7"), CostClass::Unknown);
        assert_eq!(probe_class(&r, Credential::Plan, "grok-4.7-fast"), CostClass::AutonomousPremium);
        assert_eq!(probe_class(&r, Credential::None, "grok-4.7"), CostClass::Unknown);
    }
}
