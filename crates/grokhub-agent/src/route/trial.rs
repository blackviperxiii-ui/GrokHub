//! Failover step 9, the router's side of a new model's two-week tryout
//! ([`grokhub_core::model_registry::probation`] keeps the tally and the
//! rules). Each registry refresh:
//!
//! - sends a few small probes to free OpenRouter models on tryout (a tool
//!   call and a one-word answer, alternating), under a grant you already
//!   gave and never into a low free allowance — free models first, as the
//!   plan says; a paid model's evidence comes from its canary turns;
//! - reads the learner's steps for how each tryout model's checked
//!   answers compare with its peers' in the same classes ([`quality_of`]);
//! - judges every tryout (graduate, fail, retry two weeks on) and logs
//!   any model whose announced expiry is now inside the scorer's stop
//!   window.
//!
//! The canary itself — 5% of low-risk, non-sensitive, ordinary turns that
//! hand a tryout model a real tiny or short turn — is the scorer's
//! ([`super::score::order`]); [`CANARY`] is its draw's name.

use std::collections::BTreeMap;
use std::path::Path;

use grokhub_core::model_registry::probation::{Quality, Stage};
use grokhub_core::model_registry::quota::utc_day_key;
use grokhub_core::model_registry::store::load_quota;
use grokhub_core::model_registry::{split_provider_model, Registry, RegistryEvent, Route};
use grokhub_core::outcome::read_outcomes;

use super::providers::{self, TrialProbe, PROBE_DATA};
use super::tune::{Step, CANARY_MIN_N, CARD_MIN_N, DAY_MS};
use crate::client::ClientError;

/// The canary draw's name ([`super::tune::in_canary`]): one episode in
/// twenty that is eligible lands a tryout model up front.
pub const CANARY: &str = "probation";
/// Probes one refresh sends, at most (a refresh runs every six hours).
pub const PROBES_PER_RUN: usize = 2;
/// Probes one model gets a day, at most.
pub const PROBES_PER_MODEL_DAY: u32 = 2;
/// A free allowance under this many requests left today gets no probes:
/// your own turns come first.
pub const PROBE_FREE_FLOOR: u64 = 25;
/// How far a tryout model's checked-answer pass rate may sit under its
/// peers' before it counts as worse, in points: the tuner's own
/// margin once there are canary-sized numbers, a wider one before.
pub const QUALITY_MARGIN_PTS: f64 = 2.0;
pub const QUALITY_MARGIN_WIDE_PTS: f64 = 5.0;
/// The scorer's expiry stop window, in days.
pub const EXPIRY_STOP_DAYS: u64 = 14;

/// Checked answers (a VerifyGate pass or reject, or a task outcome) and
/// how many passed, as a percent.
fn pass_rate<'a>(steps: impl Iterator<Item = &'a Step>) -> (u32, f64) {
    let judged: Vec<bool> = steps.filter_map(|s| s.pass).collect();
    let n = judged.len() as u32;
    let pct = if n == 0 {
        0.0
    } else {
        judged.iter().filter(|p| **p).count() as f64 * 100.0 / f64::from(n)
    };
    (n, pct)
}

/// How `model`'s checked answers since its tryout began compare with its
/// peers' — every other model's in the classes it served, over the same
/// span. Too few checked answers on either side says nothing.
pub fn quality_of(steps: &[Step], model: &str, since_ms: u64) -> Quality {
    let window = steps.iter().filter(|s| s.at >= since_ms);
    let mine: Vec<&Step> = window.clone().filter(|s| s.model == model).collect();
    let (n, pct) = pass_rate(mine.iter().copied());
    if n < CARD_MIN_N {
        return Quality::Unknown;
    }
    let classes: Vec<&str> = mine.iter().map(|s| s.class.as_str()).collect();
    let (peer_n, peer_pct) =
        pass_rate(window.filter(|s| s.model != model && classes.contains(&s.class.as_str())));
    if peer_n < CARD_MIN_N {
        return Quality::Unknown;
    }
    let margin = if n >= CANARY_MIN_N {
        QUALITY_MARGIN_PTS
    } else {
        QUALITY_MARGIN_WIDE_PTS
    };
    if peer_pct - pct > margin {
        Quality::Fell
    } else {
        Quality::Held
    }
}

/// The learner's word on every model on tryout.
fn trial_quality(config_dir: &Path, reg: &Registry, now_ms: u64) -> BTreeMap<String, Quality> {
    let on_trial: Vec<(&String, u64)> = reg
        .models
        .iter()
        .filter_map(|(id, r)| {
            r.probation
                .as_ref()
                .filter(|p| p.stage == Stage::Trial)
                .map(|p| (id, p.since_ms))
        })
        .collect();
    if on_trial.is_empty() {
        return BTreeMap::new();
    }
    let log = super::learn::steps_from(
        &super::log::route_records(config_dir, super::learn::TUNE_SCAN_LINES),
        &read_outcomes(config_dir),
        reg,
        now_ms,
    );
    on_trial
        .into_iter()
        .map(|(id, since)| (id.clone(), quality_of(&log.steps, id, since)))
        .collect()
}

/// Judge every tryout and log close expiries. The events, for spans.
pub fn judge(config_dir: &Path, reg: &mut Registry, now_ms: u64) -> Vec<RegistryEvent> {
    let quality = trial_quality(config_dir, reg, now_ms);
    let mut events = reg.judge_probation(
        &|id: &str| quality.get(id).copied().unwrap_or_default(),
        now_ms,
    );
    events.extend(reg.note_expiring(&utc_day_key(now_ms + EXPIRY_STOP_DAYS * DAY_MS)));
    events
}

/// Free OpenRouter models on tryout that may take a probe now, fewest
/// counted calls first.
fn probe_queue(config_dir: &Path, reg: &Registry, today: &str) -> Vec<String> {
    let quota = load_quota(config_dir);
    if !quota.openrouter.permits(true)
        || quota
            .openrouter_free_remaining()
            .is_some_and(|r| r < PROBE_FREE_FLOOR)
    {
        return Vec::new();
    }
    let granted = providers::usable(config_dir, PROBE_DATA);
    let mut queue: Vec<(u32, String)> = reg
        .models
        .iter()
        .filter(|(id, r)| {
            r.state.routable()
                && r.route() == Route::OpenRouterFree
                && split_provider_model(id).is_some_and(|(p, _)| granted.iter().any(|g| g == p))
                && r.probation.as_ref().is_some_and(|p| {
                    p.stage == Stage::Trial && p.probes_on(today) < PROBES_PER_MODEL_DAY
                })
        })
        .map(|(id, r)| (r.probation.as_ref().map_or(0, |p| p.calls), id.clone()))
        .collect();
    queue.sort();
    queue.into_iter().map(|(_, id)| id).collect()
}

/// A probe failure that is about the account or the setup, not the
/// model: it doesn't count, and the run stops probing on a limit.
fn not_the_models(err: &ClientError) -> bool {
    matches!(
        err,
        ClientError::RateLimited { .. }
            | ClientError::PaymentRequired { .. }
            | ClientError::Auth { .. }
            | ClientError::KeyOffer { .. }
            | ClientError::Cancelled
            | ClientError::Protocol(_)
    )
}

/// Send this refresh's probes and count them on each tryout. How many went.
pub fn run_probes(config_dir: &Path, reg: &mut Registry, now_ms: u64) -> usize {
    let today = utc_day_key(now_ms);
    let mut sent = 0;
    for id in probe_queue(config_dir, reg, &today) {
        if sent >= PROBES_PER_RUN {
            break;
        }
        let Some(rec) = reg.models.get(&id) else {
            continue;
        };
        let probes = rec.probation.as_ref().map_or(0, |p| p.probes);
        let kind = if rec.meta.tool_calling != Some(false) && probes % 2 == 0 {
            TrialProbe::Tool
        } else {
            TrialProbe::Text
        };
        let span_id = format!("probation:{now_ms}:{sent}");
        let outcome = providers::probe_model(config_dir, &id, kind, &span_id);
        sent += 1;
        let passed = match outcome {
            Ok(passed) => passed,
            Err(e) if not_the_models(&e) => {
                if matches!(
                    e,
                    ClientError::RateLimited { .. } | ClientError::PaymentRequired { .. }
                ) {
                    break;
                }
                continue;
            }
            Err(_) => false,
        };
        if let Some(p) = reg.models.get_mut(&id).and_then(|r| r.probation.as_mut()) {
            p.probed(passed, &today);
        }
    }
    sent
}

#[cfg(test)]
mod tests {
    use super::super::tune::Arm;
    use super::*;

    fn step(model: &str, class: &str, at: u64, pass: Option<bool>) -> Step {
        Step {
            at,
            class: class.into(),
            model: model.into(),
            arm: Arm::Live,
            pass,
            ..Step::default()
        }
    }

    fn many(model: &str, class: &str, ok: usize, bad: usize, at: u64) -> Vec<Step> {
        (0..ok)
            .map(|_| step(model, class, at, Some(true)))
            .chain((0..bad).map(|_| step(model, class, at, Some(false))))
            .collect()
    }

    #[test]
    fn quality_compares_checked_answers_with_peers_in_the_same_classes() {
        let new = "openrouter/vendor/new";
        let mut steps = many("grok-4.3", "chat:default", 45, 5, 10);
        // Too few checked answers of its own: nothing to say.
        steps.extend(many(new, "chat:default", 10, 0, 10));
        assert_eq!(quality_of(&steps, new, 0), Quality::Unknown);
        // 20 checked, 85% against the peers' 90%: inside the wide margin.
        steps.extend(many(new, "chat:default", 7, 3, 10));
        assert_eq!(quality_of(&steps, new, 0), Quality::Held);
        // Unjudged steps count for nothing either way.
        steps.extend((0..40).map(|_| step(new, "chat:default", 10, None)));
        assert_eq!(quality_of(&steps, new, 0), Quality::Held);
        // 30 checked at 70%: worse than its peers by more than the margin.
        steps.extend(many(new, "chat:default", 4, 6, 10));
        assert_eq!(quality_of(&steps, new, 0), Quality::Fell);
        // Only steps since the tryout began count.
        assert_eq!(quality_of(&steps, new, 11), Quality::Unknown);
    }

    #[test]
    fn peers_are_only_the_classes_it_served() {
        let new = "openrouter/vendor/new";
        let mut steps = many(new, "chat:default", 20, 0, 10);
        steps.extend(many("grok-4.3", "code:edit", 50, 0, 10));
        assert_eq!(quality_of(&steps, new, 0), Quality::Unknown);
        steps.extend(many("grok-4.3", "chat:default", 20, 0, 10));
        assert_eq!(quality_of(&steps, new, 0), Quality::Held);
    }

    #[test]
    fn account_and_setup_failures_are_not_the_models() {
        assert!(not_the_models(&ClientError::RateLimited {
            retry_after_secs: None,
            message: String::new(),
        }));
        assert!(not_the_models(&ClientError::Protocol("no grant".into())));
        assert!(!not_the_models(&ClientError::Disconnect));
        assert!(!not_the_models(&ClientError::Server {
            status: 502,
            message: String::new(),
            retry_after_secs: None,
        }));
    }
}
