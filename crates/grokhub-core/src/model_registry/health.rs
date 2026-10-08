//! Passive health: real calls move a model's state. No health probes.
//! 403 content-safety and 429 `free-usage-exhausted` are not health signals.
//!
//! R2a adds the breaker ([`breaker_observe`]): 3 hard failures in a row (5xx
//! or a timeout) or a ghost strike quarantine a model. It stays out of routing
//! for its open backoff ([`BREAKER_BACKOFF_MS`], full jitter), then goes
//! half-open: one probe or one low-risk background call decides whether it
//! closes or opens again on the next step.

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
/// Hard failures in a row (5xx or a timeout) that open the breaker.
pub const BREAKER_FAILS: u32 = 3;
/// The open backoff per open, before jitter: 1 m, 5 m, 30 m, 2 h, then 12 h.
pub const BREAKER_BACKOFF_MS: [u64; 5] = [60_000, 5 * 60_000, 30 * 60_000, 2 * 60 * 60_000, 12 * 60 * 60_000];

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
    /// The HTTP status (0 when there was none: a timeout or no connection).
    #[serde(default)]
    pub http: u16,
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

/// The cap of the open backoff for the `opens`-th open (1-based), before jitter.
pub fn backoff_cap_ms(opens: u32) -> u64 {
    let step = (opens.max(1) - 1) as usize;
    BREAKER_BACKOFF_MS[step.min(BREAKER_BACKOFF_MS.len() - 1)]
}

/// Full jitter: anywhere in `[0, cap]`, drawn from `seed` (FNV-1a, so the
/// fold stays deterministic and needs no random state).
pub fn backoff_ms(opens: u32, seed: u64) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for b in seed.to_le_bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    h % (backoff_cap_ms(opens) + 1)
}

fn seed_of(id: &str, opens: u32, now_ms: u64) -> u64 {
    id.bytes().fold(now_ms ^ ((opens as u64) << 48), |h, b| h.rotate_left(5) ^ b as u64)
}

/// A 5xx answer, a 503, or no answer at all (a timeout or no connection).
pub fn hard_failure(obs: &Observation) -> bool {
    match obs.status {
        CallStatus::Error => obs.http == 0 || (500..600).contains(&obs.http),
        CallStatus::Pressure => obs.http == 503,
        _ => false,
    }
}

fn open_breaker(rec: &mut ModelRecord, now_ms: u64, why: &str) {
    let b = &mut rec.breaker;
    b.opens = b.opens.saturating_add(1);
    b.fails = 0;
    b.half_open = false;
    b.open_until_ms = now_ms.saturating_add(backoff_ms(b.opens, seed_of(&rec.meta.id, b.opens, now_ms)));
    rec.state = ModelState::Quarantined;
    rec.reason = why.to_string();
}

/// Close the breaker: the model answers again.
pub fn close_breaker(rec: &mut ModelRecord) {
    rec.breaker = super::record::Breaker::default();
    rec.state = ModelState::Live;
    rec.reason = "Answering again.".into();
}

/// The half-open check came back: a pass closes the breaker, anything else
/// opens it again on the next step of the schedule. Returns true when the state changed.
pub fn half_open_result(rec: &mut ModelRecord, ok: bool, now_ms: u64) -> bool {
    if rec.state != ModelState::Quarantined {
        return false;
    }
    if ok {
        close_breaker(rec);
        return true;
    }
    open_breaker(rec, now_ms, "Its check after the pause failed, so it waits longer.");
    false
}

/// The breaker's half of a call, run after [`observe`]. `listed` is "in your
/// own fresh list". Returns true when the state changed.
pub fn breaker_observe(rec: &mut ModelRecord, listed: bool, obs: &Observation) -> bool {
    let before = rec.state;
    let now = obs.ts_ms;
    match obs.status {
        CallStatus::Ok => {
            rec.breaker.fails = 0;
            if rec.state == ModelState::Quarantined {
                close_breaker(rec);
            }
        }
        CallStatus::NotFound if listed && obs.endpoint_ok => {
            // The first strike quarantines; the second (inside a day) made it a ghost in `observe`.
            if matches!(rec.state, ModelState::Live | ModelState::Degraded | ModelState::Probing) {
                open_breaker(rec, now, "The API said not found for it once, so it rests until a check passes.");
            }
        }
        _ if hard_failure(obs) => {
            rec.breaker.fails = rec.breaker.fails.saturating_add(1);
            if rec.state == ModelState::Quarantined && rec.breaker.half_open {
                open_breaker(rec, now, "Its check after the pause failed, so it waits longer.");
            } else if rec.breaker.fails >= BREAKER_FAILS && rec.state.routable() {
                open_breaker(rec, now, "It failed 3 times in a row (server errors or timeouts), so it rests for a while.");
            }
        }
        _ => {}
    }
    rec.state != before
}

/// Quarantined and its open backoff has run out, with no check out yet.
pub fn half_open_due(rec: &ModelRecord, now_ms: u64) -> bool {
    rec.state == ModelState::Quarantined && !rec.breaker.half_open && now_ms >= rec.breaker.open_until_ms
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
            http: 0,
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

    fn hard(ts_ms: u64) -> Observation {
        Observation { http: 502, ..obs(CallStatus::Error, ts_ms) }
    }

    fn both(r: &mut ModelRecord, o: &Observation) {
        observe(r, true, o);
        breaker_observe(r, true, o);
    }

    #[test]
    fn three_hard_failures_in_a_row_quarantine_and_an_ok_in_between_resets() {
        let mut r = rec();
        both(&mut r, &hard(1));
        both(&mut r, &hard(2));
        both(&mut r, &obs(CallStatus::Ok, 3));
        both(&mut r, &hard(4));
        both(&mut r, &hard(5));
        assert_ne!(r.state, ModelState::Quarantined);
        assert_eq!(r.breaker.fails, 2);
        // A 400 or a 429 is not a hard failure; a timeout (no status) and a 503 are.
        both(&mut r, &Observation { http: 400, ..obs(CallStatus::Error, 6) });
        both(&mut r, &Observation { http: 429, ..obs(CallStatus::Pressure, 7) });
        assert_eq!(r.breaker.fails, 2);
        assert!(hard_failure(&Observation { http: 503, ..obs(CallStatus::Pressure, 0) }));
        assert!(hard_failure(&obs(CallStatus::Error, 0)));
        both(&mut r, &obs(CallStatus::Error, 8));
        assert_eq!((r.state, r.breaker.opens), (ModelState::Quarantined, 1));
        assert!(r.breaker.open_until_ms >= 8 && r.breaker.open_until_ms <= 8 + 60_000);
        assert!(!r.state.routable());
    }

    #[test]
    fn a_ghost_strike_quarantines_and_the_second_makes_a_ghost() {
        let mut r = rec();
        both(&mut r, &obs(CallStatus::NotFound, 10));
        assert_eq!(r.state, ModelState::Quarantined);
        both(&mut r, &obs(CallStatus::NotFound, 20));
        assert_eq!(r.state, ModelState::Ghost);
    }

    #[test]
    fn backoff_follows_the_schedule_with_bounded_full_jitter() {
        assert_eq!((1..=7).map(backoff_cap_ms).collect::<Vec<_>>(), vec![60_000, 300_000, 1_800_000, 7_200_000, 43_200_000, 43_200_000, 43_200_000]);
        for opens in 1..=6 {
            let cap = backoff_cap_ms(opens);
            let draws: Vec<u64> = (0..500).map(|s| backoff_ms(opens, s)).collect();
            assert!(draws.iter().all(|d| *d <= cap), "{opens}");
            // Full jitter spreads over the whole range, not one point.
            assert!(draws.iter().any(|d| *d < cap / 4) && draws.iter().any(|d| *d > cap * 3 / 4), "{opens}");
        }
        assert_eq!(backoff_ms(2, 77), backoff_ms(2, 77));
    }

    #[test]
    fn half_open_lets_one_check_through_then_closes_or_steps_the_backoff() {
        let mut r = rec();
        for t in 0..3 {
            both(&mut r, &hard(t));
        }
        let until = r.breaker.open_until_ms;
        assert!(!half_open_due(&r, until - 1));
        assert!(half_open_due(&r, until));
        r.breaker.half_open = true;
        assert!(!half_open_due(&r, until + 1), "one check at a time");
        // The check fails: open again on the next step (up to 5 m).
        assert!(!half_open_result(&mut r, false, until));
        assert_eq!((r.state, r.breaker.opens, r.breaker.half_open), (ModelState::Quarantined, 2, false));
        assert!(r.breaker.open_until_ms <= until + 300_000);
        // A failing background call while half-open also opens it again.
        r.breaker.half_open = true;
        both(&mut r, &hard(until + 10));
        assert_eq!(r.breaker.opens, 3);
        // A pass closes it.
        assert!(half_open_result(&mut r, true, until + 20));
        assert_eq!((r.state, r.breaker.clone()), (ModelState::Live, crate::model_registry::record::Breaker::default()));
    }
}
