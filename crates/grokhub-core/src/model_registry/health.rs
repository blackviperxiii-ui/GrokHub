//! Passive health: real calls move a model's state. No health probes.
//! 403 content-safety and 429 `free-usage-exhausted` are not health signals.

use serde::{Deserialize, Serialize};

use super::record::{CallMark, ModelRecord, ModelState};

/// Calls the error-rate and burst rules look at.
pub const WINDOW_CALLS: usize = 10;
/// Over this share of errors in the window is degraded (strictly more than 20%).
pub const DEGRADED_ERROR_PCT: usize = 20;
/// The window needs this many calls before the error rule can fire.
pub const DEGRADED_MIN_CALLS: usize = 5;
/// 429/503 answers in the window that count as a burst.
pub const PRESSURE_BURST: usize = 3;
/// Latency baseline span.
pub const BASELINE_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// Baseline samples needed before the latency rule can fire.
pub const BASELINE_MIN_SAMPLES: usize = 20;
/// Latency samples kept per model.
pub const LATENCY_CAP: usize = 500;
/// 404 strikes that make a listed model a ghost, inside [`STRIKE_WINDOW_MS`].
pub const GHOST_STRIKES: usize = 2;
pub const STRIKE_WINDOW_MS: u64 = 24 * 60 * 60 * 1000;

/// What a call's answer means for health.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallStatus {
    Ok,
    /// 404 `not-found` for the model.
    NotFound,
    /// 403 "requires a Grok subscription".
    NoSubscription,
    /// 403 content-safety: about the prompt, not the model.
    ContentSafety,
    /// 429 `free-usage-exhausted`: about the account, not the model.
    FreeUsageExhausted,
    /// 429 or 503.
    Pressure,
    Error,
}

/// Map an HTTP status and error text to a [`CallStatus`].
pub fn classify_status(http: u16, message: &str) -> CallStatus {
    let m = message.to_ascii_lowercase();
    match http {
        200..=299 => CallStatus::Ok,
        404 => CallStatus::NotFound,
        403 if m.contains("subscription") => CallStatus::NoSubscription,
        403 if m.contains("content") && (m.contains("safety") || m.contains("policy") || m.contains("moderation")) => {
            CallStatus::ContentSafety
        }
        429 if m.contains("free-usage-exhausted") || m.contains("free usage") => CallStatus::FreeUsageExhausted,
        429 | 503 => CallStatus::Pressure,
        _ => CallStatus::Error,
    }
}

/// One real call, as the shadow log writes it (`models/health.jsonl`). No content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    pub model: String,
    pub ts_ms: u64,
    pub status: CallStatus,
    #[serde(default)]
    pub latency_ms: u64,
    /// The response's `model`, when it sent one.
    #[serde(default)]
    pub served_model: Option<String>,
    /// The call went to the right endpoint (a 404 from a wrong URL is not the model's).
    #[serde(default)]
    pub endpoint_ok: bool,
    #[serde(default)]
    pub reasoning_tokens: u64,
    #[serde(default)]
    pub cost_ticks: i64,
}

fn percentile(sorted: &[u64], pct: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = (sorted.len() * pct).div_ceil(100).saturating_sub(1).min(sorted.len() - 1);
    sorted[idx]
}

/// The degraded rule on a record's window, as of `now_ms`.
pub fn degraded_reason(rec: &ModelRecord, now_ms: u64) -> Option<&'static str> {
    let recent = &rec.health.recent;
    let errors = recent.iter().filter(|c| !c.ok).count();
    if recent.len() >= DEGRADED_MIN_CALLS && errors * 100 > recent.len() * DEGRADED_ERROR_PCT {
        return Some("More than 1 in 5 of its last calls failed.");
    }
    if recent.iter().filter(|c| c.pressure).count() >= PRESSURE_BURST {
        return Some("It answered busy (429 or 503) several times in a row.");
    }
    let mut last: Vec<u64> = recent.iter().filter(|c| c.ok).map(|c| c.latency_ms).collect();
    let window_start = recent.front().map(|c| c.ts_ms).unwrap_or(now_ms);
    let mut base: Vec<u64> = rec
        .health
        .latencies
        .iter()
        .filter(|(ts, _)| now_ms.saturating_sub(*ts) <= BASELINE_MS && *ts < window_start)
        .map(|(_, ms)| *ms)
        .collect();
    if base.len() >= BASELINE_MIN_SAMPLES && last.len() >= DEGRADED_MIN_CALLS {
        last.sort_unstable();
        base.sort_unstable();
        let (p95, base_p95) = (percentile(&last, 95), percentile(&base, 95));
        if base_p95 > 0 && p95 > base_p95.saturating_mul(2) {
            return Some("It is answering more than twice as slowly as its usual week.");
        }
    }
    None
}

/// Fold one call into the record. `listed` is "in your own fresh list".
/// Returns true when the state changed.
pub fn observe(rec: &mut ModelRecord, listed: bool, obs: &Observation) -> bool {
    let before = rec.state;
    let now = obs.ts_ms;
    match obs.status {
        CallStatus::ContentSafety | CallStatus::FreeUsageExhausted => return false,
        CallStatus::NoSubscription => {
            rec.state = ModelState::NotInPlan;
            rec.reason = "Your plan doesn't include it (the API said it needs a Grok subscription).".into();
            return rec.state != before;
        }
        CallStatus::NotFound => {
            if !listed {
                rec.state = ModelState::NotInPlan;
                rec.reason = "It isn't in your own model list.".into();
                return rec.state != before;
            }
            if obs.endpoint_ok {
                rec.not_found.retain(|t| now.saturating_sub(*t) <= STRIKE_WINDOW_MS);
                rec.not_found.push(now);
                if rec.not_found.len() >= GHOST_STRIKES {
                    rec.state = ModelState::Ghost;
                    rec.reason = "It is listed, but the API said not found twice in a day.".into();
                }
            }
        }
        CallStatus::Ok => {
            if let Some(served) = obs.served_model.as_deref().filter(|s| !rec.answers_as(s)) {
                rec.served_as = Some(served.to_string());
                rec.state = ModelState::Redirected;
                rec.reason = format!("It answered as {served}.");
            } else if !listed && matches!(rec.state, ModelState::NotInPlan | ModelState::Ghost) {
                rec.state = ModelState::Redirected;
                rec.reason = "It isn't listed, but it still answers.".into();
            }
            rec.health.latencies.push_back((now, obs.latency_ms));
            while rec.health.latencies.len() > LATENCY_CAP {
                rec.health.latencies.pop_front();
            }
        }
        CallStatus::Pressure | CallStatus::Error => {}
    }
    let pressure = obs.status == CallStatus::Pressure;
    rec.health.recent.push_back(CallMark { ts_ms: now, ok: obs.status == CallStatus::Ok, pressure, latency_ms: obs.latency_ms });
    while rec.health.recent.len() > WINDOW_CALLS {
        rec.health.recent.pop_front();
    }
    if matches!(rec.state, ModelState::Live | ModelState::Degraded) {
        match degraded_reason(rec, now) {
            Some(why) => {
                rec.state = ModelState::Degraded;
                rec.reason = why.into();
            }
            None if rec.state == ModelState::Degraded => {
                rec.state = ModelState::Live;
                rec.reason = "Healthy again.".into();
            }
            None => {}
        }
    }
    rec.state != before
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_registry::record::ModelMeta;

    fn rec() -> ModelRecord {
        ModelRecord { meta: ModelMeta::bare("grok-4.7"), ..ModelRecord::default() }
    }

    fn obs(status: CallStatus, ts_ms: u64) -> Observation {
        Observation {
            model: "grok-4.7".into(),
            ts_ms,
            status,
            latency_ms: 800,
            served_model: None,
            endpoint_ok: true,
            reasoning_tokens: 0,
            cost_ticks: 0,
        }
    }

    #[test]
    fn statuses_map_from_http_and_text() {
        assert_eq!(classify_status(200, ""), CallStatus::Ok);
        assert_eq!(classify_status(404, "Model not found"), CallStatus::NotFound);
        assert_eq!(classify_status(403, "This model requires a Grok subscription"), CallStatus::NoSubscription);
        assert_eq!(classify_status(403, "Blocked by content safety"), CallStatus::ContentSafety);
        assert_eq!(classify_status(429, "free-usage-exhausted"), CallStatus::FreeUsageExhausted);
        assert_eq!(classify_status(429, "slow down"), CallStatus::Pressure);
        assert_eq!(classify_status(503, ""), CallStatus::Pressure);
        assert_eq!(classify_status(500, ""), CallStatus::Error);
    }

    #[test]
    fn a_listed_404_is_a_ghost_only_after_two_strikes_in_a_day() {
        let mut r = rec();
        assert!(!observe(&mut r, true, &obs(CallStatus::NotFound, 1_000)));
        assert_eq!(r.state, ModelState::Live);
        // A strike older than 24 h does not count.
        let mut late = r.clone();
        assert!(!observe(&mut late, true, &obs(CallStatus::NotFound, 1_000 + STRIKE_WINDOW_MS + 1)));
        assert_eq!((late.state, late.not_found.len()), (ModelState::Live, 1));
        assert!(observe(&mut r, true, &obs(CallStatus::NotFound, 5_000)));
        assert_eq!(r.state, ModelState::Ghost);
        // A 404 from a wrong endpoint is not a strike.
        let mut wrong = rec();
        let mut o = obs(CallStatus::NotFound, 1);
        o.endpoint_ok = false;
        observe(&mut wrong, true, &o);
        observe(&mut wrong, true, &o);
        assert_eq!((wrong.state, wrong.not_found.len()), (ModelState::Live, 0));
    }

    #[test]
    fn unlisted_404_and_subscription_403_are_not_in_plan_with_no_strikes() {
        let mut r = rec();
        assert!(observe(&mut r, false, &obs(CallStatus::NotFound, 1)));
        assert_eq!((r.state, r.not_found.len()), (ModelState::NotInPlan, 0));
        let mut s = rec();
        assert!(observe(&mut s, true, &obs(CallStatus::NoSubscription, 1)));
        assert_eq!((s.state, s.not_found.len()), (ModelState::NotInPlan, 0));
    }

    #[test]
    fn content_safety_and_free_usage_change_nothing() {
        let mut r = rec();
        let before = r.clone();
        for _ in 0..10 {
            assert!(!observe(&mut r, true, &obs(CallStatus::ContentSafety, 1)));
            assert!(!observe(&mut r, true, &obs(CallStatus::FreeUsageExhausted, 1)));
        }
        assert_eq!(r, before);
    }

    #[test]
    fn a_different_response_model_is_redirected() {
        let mut r = rec();
        r.meta.aliases = vec!["grok-4.7-latest".into()];
        let mut alias = obs(CallStatus::Ok, 1);
        alias.served_model = Some("grok-4.7-latest".into());
        assert!(!observe(&mut r, true, &alias));
        let mut o = obs(CallStatus::Ok, 2);
        o.served_model = Some("grok-4.5".into());
        assert!(observe(&mut r, true, &o));
        assert_eq!((r.state, r.served_as.as_deref()), (ModelState::Redirected, Some("grok-4.5")));
        assert_eq!(r.reason, "It answered as grok-4.5.");
    }

    #[test]
    fn errors_over_a_fifth_or_a_busy_burst_degrade_and_recover() {
        let mut r = rec();
        for t in 0..8 {
            observe(&mut r, true, &obs(CallStatus::Ok, t));
        }
        observe(&mut r, true, &obs(CallStatus::Error, 8));
        observe(&mut r, true, &obs(CallStatus::Error, 9));
        // 2 of 10 is exactly 20%: not over.
        assert_eq!(r.state, ModelState::Live);
        observe(&mut r, true, &obs(CallStatus::Error, 10));
        assert_eq!(r.state, ModelState::Degraded);
        for t in 11..21 {
            observe(&mut r, true, &obs(CallStatus::Ok, t));
        }
        assert_eq!((r.state, r.reason.as_str()), (ModelState::Live, "Healthy again."));
        let mut b = rec();
        for t in 0..3 {
            observe(&mut b, true, &obs(CallStatus::Pressure, t));
        }
        assert_eq!(b.state, ModelState::Degraded);
    }

    #[test]
    fn p95_over_twice_the_week_degrades() {
        let mut r = rec();
        for t in 0..30 {
            observe(&mut r, true, &obs(CallStatus::Ok, t));
        }
        assert_eq!(r.state, ModelState::Live);
        for t in 30..40 {
            let mut o = obs(CallStatus::Ok, t);
            o.latency_ms = 2_000;
            observe(&mut r, true, &o);
        }
        assert_eq!(r.state, ModelState::Degraded);
        assert_eq!(r.reason, "It is answering more than twice as slowly as its usual week.");
    }
}
