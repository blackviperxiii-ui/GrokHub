//! Client-side pacing for xAI calls. xAI's per-second limit is its per-minute
//! limit over 48, at least 2, and it sends no rate-limit headers, so parallel
//! tools and subagents pace themselves here instead of bunching into 429s.
//! Each model gets a token bucket that starts at the lowest tier's limit,
//! halves after a 429 and creeps back up as calls succeed.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use crate::CancelToken;

/// The documented floor: no tier allows fewer calls per second.
const FLOOR_RPS: f64 = 2.0;

/// The lowest tier's calls per second for a model (RPM / 48).
fn ceiling_rps(model: &str) -> f64 {
    if model.contains("grok-4.3") {
        37.0
    } else {
        150.0
    }
}

struct Bucket {
    rate: f64,
    ceiling: f64,
    tokens: f64,
    at: Instant,
}

impl Bucket {
    fn new(model: &str, now: Instant) -> Self {
        let ceiling = ceiling_rps(model);
        Self {
            rate: ceiling,
            ceiling,
            tokens: ceiling,
            at: now,
        }
    }

    fn refill(&mut self, now: Instant) {
        let gained = now.saturating_duration_since(self.at).as_secs_f64() * self.rate;
        self.tokens = (self.tokens + gained).min(self.rate.max(1.0));
        self.at = now;
    }

    /// Takes a token, or says how long until one is free.
    fn take(&mut self, now: Instant) -> Option<Duration> {
        self.refill(now);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            None
        } else {
            Some(Duration::from_secs_f64((1.0 - self.tokens) / self.rate))
        }
    }

    fn limited(&mut self) {
        self.rate = (self.rate / 2.0).max(FLOOR_RPS);
        self.tokens = self.tokens.min(self.rate);
    }

    fn ok(&mut self) {
        self.rate = (self.rate + 1.0).min(self.ceiling);
    }
}

static BUCKETS: LazyLock<Mutex<HashMap<String, Bucket>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn with_bucket<T>(model: &str, f: impl FnOnce(&mut Bucket, Instant) -> T) -> T {
    let now = Instant::now();
    let mut buckets = BUCKETS.lock().unwrap_or_else(|e| e.into_inner());
    let bucket = buckets
        .entry(model.to_string())
        .or_insert_with(|| Bucket::new(model, now));
    f(bucket, now)
}

/// Waits for a free slot for `model`. False when Stop came first.
pub(crate) fn take(model: &str, cancel: &CancelToken) -> bool {
    loop {
        if cancel.is_cancelled() {
            return false;
        }
        match with_bucket(model, |b, now| b.take(now)) {
            None => return true,
            Some(wait) => std::thread::sleep(wait.min(Duration::from_millis(100))),
        }
    }
}

/// xAI answered 429 for `model`: halve its pace.
pub(crate) fn note_limited(model: &str) {
    with_bucket(model, |b, _| b.limited());
}

/// A call for `model` went through: ease its pace back up.
pub(crate) fn note_ok(model: &str) {
    with_bucket(model, |b, _| b.ok());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_past_the_limit_waits_and_a_429_halves_the_pace() {
        let start = Instant::now();
        let mut b = Bucket::new("grok-4.3", start);
        assert_eq!(b.rate, 37.0);
        for _ in 0..37 {
            assert_eq!(b.take(start), None);
        }
        let wait = b.take(start).expect("the 38th call in one instant waits");
        assert!(
            wait > Duration::ZERO && wait <= Duration::from_millis(30),
            "{wait:?}"
        );
        assert_eq!(b.take(start + Duration::from_millis(30)), None);

        b.limited();
        assert_eq!(b.rate, 18.5);
        for _ in 0..10 {
            b.limited();
        }
        assert_eq!(b.rate, FLOOR_RPS, "never below the documented floor");
        for _ in 0..1000 {
            b.ok();
        }
        assert_eq!(b.rate, 37.0, "never above the model's ceiling");
        assert_eq!(Bucket::new("grok-4.7", start).rate, 150.0);
    }

    #[test]
    fn stop_ends_a_wait_for_a_slot() {
        let cancel = CancelToken::new();
        cancel.cancel();
        assert!(!take("pace-test-model", &cancel));
        assert!(take("pace-test-model", &CancelToken::new()));
    }
}
