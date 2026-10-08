//! Router R2b: what Auto may spend on one call (§14.4.5, §14.5).
//!
//! Every candidate route has a cost class (`grokhub_core::model_registry::cost_class`).
//! `included` routes are free to use. A Fast variant (`autonomous_premium`) is
//! taken on Auto's own only while you wait and speed clearly matters
//! ([`Latency`]), never for background, automation or proactive work, never
//! with the Settings toggle off, and it falls back to the plain model when the
//! week's budget is tight. A `premium` route needs your one-time click: a
//! grant in the ConsentLedger (`premium:<model>`), which `harness::decide`
//! asks for as hard class money. `extra_spend` has no path at all.
//!
//! The settings are the cabin's (`app.json`, written only from Settings). The
//! router reads them here and can't change them.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use grokhub_core::model_registry::cost_class::{CostClass, DEFAULT_CEILING_USD_PER_M};

use crate::harness::{consent_path, ConsentLedger, Origin};

/// Steps in one user turn after which a chain counts as latency-bound.
pub const CHAIN_LATENCY_STEPS: u32 = 8;
/// A time-boxed task with less than this left is at risk.
pub const TIME_BOX_RISK_MS: u64 = 5 * 60 * 1000;
/// The prefix of a premium grant's ledger key.
pub const PREMIUM_PREFIX: &str = "premium:";

/// The cabin's spend settings. Only a Settings click changes them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpendSettings {
    /// "Use Grok 4.7 Fast when you're waiting". On by default.
    pub fast_when_waiting: bool,
    /// Weekly $ cap for an API key. 0 is no cap.
    pub weekly_cap_usd: f64,
    /// A key route over this $/M output price is premium.
    pub ceiling_usd_per_m: f64,
}

impl Default for SpendSettings {
    fn default() -> Self {
        Self { fast_when_waiting: true, weekly_cap_usd: 0.0, ceiling_usd_per_m: DEFAULT_CEILING_USD_PER_M }
    }
}

static SETTINGS: Mutex<Option<SpendSettings>> = Mutex::new(None);

/// The cabin sets these on start and whenever Settings changes them.
pub fn set_spend(s: SpendSettings) {
    *SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = Some(s);
}

pub fn spend_settings() -> SpendSettings {
    SETTINGS.lock().unwrap_or_else(|e| e.into_inner()).unwrap_or_default()
}

/// The three times speed is worth paying for, as explicit signals.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Latency {
    /// You typed this turn and are watching it run.
    pub waiting_live: bool,
    /// A long multi-step chain you are waiting on ([`CHAIN_LATENCY_STEPS`] or more steps).
    pub chain_latency_bound: bool,
    /// A time-boxed task is close to its limit ([`TIME_BOX_RISK_MS`]).
    pub time_box_at_risk: bool,
}

impl Latency {
    /// The rule id for the strongest condition that holds.
    pub fn rule(self) -> Option<&'static str> {
        if self.time_box_at_risk {
            Some("fast:time_box")
        } else if self.chain_latency_bound {
            Some("fast:chain")
        } else if self.waiting_live {
            Some("fast:waiting")
        } else {
            None
        }
    }

    /// From a call's own facts: who started it, how many steps this turn has
    /// run, and the task's deadline. Nothing but a typed turn waits live.
    pub fn of(origin: Origin, user_facing: bool, steps: u32, deadline_ms: Option<u64>, now_ms: u64) -> Self {
        let live = origin == Origin::User && user_facing;
        Self {
            waiting_live: live,
            chain_latency_bound: live && steps >= CHAIN_LATENCY_STEPS,
            time_box_at_risk: live && deadline_ms.is_some_and(|d| d.saturating_sub(now_ms) < TIME_BOX_RISK_MS),
        }
    }
}

/// What one call may spend.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Spend {
    pub settings: SpendSettings,
    pub latency: Latency,
    /// A `background:*` class, or work that automation or a proactive card started.
    pub background: bool,
    /// The week's budget is at 80% or more.
    pub budget_tight: bool,
    /// Premium route keys you approved (`premium:grok-heavy`).
    pub grants: Vec<String>,
}

/// May Auto route to this class on its own? `included` always, `premium`
/// only under your grant for exactly `key`. A Fast variant goes through
/// [`fast_verdict`] instead, and the rest never.
pub fn allowed(class: CostClass, key: &str, spend: &Spend) -> bool {
    match class {
        CostClass::Included => true,
        CostClass::Premium => spend.grants.iter().any(|g| g == key),
        CostClass::AutonomousPremium | CostClass::ExtraSpend | CostClass::NewProvider | CostClass::Unknown => false,
    }
}

/// Should this call take the Fast variant? `Ok(rule)` names the latency
/// condition; `Err(rule)` says why not.
pub fn fast_verdict(spend: &Spend) -> Result<&'static str, &'static str> {
    if spend.background {
        return Err("fast:background");
    }
    if !spend.settings.fast_when_waiting {
        return Err("fast:off");
    }
    let Some(rule) = spend.latency.rule() else {
        return Err("fast:not_waiting");
    };
    if spend.budget_tight {
        return Err("fast:budget_tight");
    }
    Ok(rule)
}

/// Origins that are never you waiting: automation and proactive work.
pub fn background_origin(origin: Origin) -> bool {
    matches!(origin, Origin::Automation | Origin::Proactive)
}

type GrantCache = Option<((PathBuf, Option<SystemTime>, u64), Vec<String>)>;
static GRANTS: Mutex<GrantCache> = Mutex::new(None);

/// Active premium grant keys, re-read only when `consent.jsonl` changes.
/// Never waits on the keyring: until it answers there are none (fail closed).
pub fn premium_grants(config_dir: &Path) -> Vec<String> {
    let path = consent_path(config_dir);
    let meta = std::fs::metadata(&path).ok();
    let stamp = (config_dir.to_path_buf(), meta.as_ref().and_then(|m| m.modified().ok()), meta.map_or(0, |m| m.len()));
    let mut c = GRANTS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((s, keys)) = c.as_ref() {
        if *s == stamp {
            return keys.clone();
        }
    }
    let ledger = ConsentLedger::load_now(config_dir);
    let keys: Vec<String> = ledger.active().filter(|g| g.source.starts_with(PREMIUM_PREFIX)).map(|g| g.source.clone()).collect();
    if !ledger.pending() {
        *c = Some((stamp, keys.clone()));
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waiting() -> Spend {
        Spend { latency: Latency { waiting_live: true, ..Latency::default() }, ..Spend::default() }
    }

    #[test]
    fn fast_only_when_you_wait_with_the_toggle_on_and_room_in_the_budget() {
        assert_eq!(fast_verdict(&waiting()), Ok("fast:waiting"));
        assert_eq!(fast_verdict(&Spend::default()), Err("fast:not_waiting"));
        assert_eq!(fast_verdict(&Spend { background: true, ..waiting() }), Err("fast:background"));
        let off = Spend { settings: SpendSettings { fast_when_waiting: false, ..SpendSettings::default() }, ..waiting() };
        assert_eq!(fast_verdict(&off), Err("fast:off"));
        assert_eq!(fast_verdict(&Spend { budget_tight: true, ..waiting() }), Err("fast:budget_tight"));
        let chain = Spend { latency: Latency { waiting_live: true, chain_latency_bound: true, time_box_at_risk: false }, ..Spend::default() };
        assert_eq!(fast_verdict(&chain), Ok("fast:chain"));
    }

    #[test]
    fn latency_comes_only_from_a_typed_user_facing_turn() {
        let now = 1_000_000;
        assert_eq!(Latency::of(Origin::User, true, 1, None, now), Latency { waiting_live: true, ..Latency::default() });
        assert_eq!(Latency::of(Origin::User, false, 20, Some(now + 1), now), Latency::default());
        assert_eq!(Latency::of(Origin::Automation, true, 20, Some(now + 1), now), Latency::default());
        assert_eq!(Latency::of(Origin::Proactive, true, 1, None, now), Latency::default());
        let late = Latency::of(Origin::User, true, CHAIN_LATENCY_STEPS, Some(now + TIME_BOX_RISK_MS - 1), now);
        assert_eq!(late, Latency { waiting_live: true, chain_latency_bound: true, time_box_at_risk: true });
        assert_eq!(late.rule(), Some("fast:time_box"));
        assert!(!Latency::of(Origin::User, true, 1, Some(now + TIME_BOX_RISK_MS), now).time_box_at_risk);
        assert!(background_origin(Origin::Automation) && background_origin(Origin::Proactive) && !background_origin(Origin::User));
    }

    #[test]
    fn premium_needs_a_grant_for_exactly_its_key_and_extra_spend_never_goes() {
        let mut s = Spend::default();
        assert!(allowed(CostClass::Included, "", &s));
        assert!(!allowed(CostClass::Premium, "premium:grok-heavy", &s));
        s.grants = vec!["premium:grok-heavy".into()];
        assert!(allowed(CostClass::Premium, "premium:grok-heavy", &s));
        assert!(!allowed(CostClass::Premium, "premium:grok-heavy+priority", &s));
        for c in [CostClass::ExtraSpend, CostClass::NewProvider, CostClass::Unknown, CostClass::AutonomousPremium] {
            s.grants = vec![c.as_str().into(), "premium:grok-heavy".into()];
            assert!(!allowed(c, c.as_str(), &s), "{c:?}");
        }
    }
}
