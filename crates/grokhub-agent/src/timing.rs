//! Speed spans: how long the router, the harness guards and the send path
//! take, kept in memory (no disk write on the hot path). Each name keeps its
//! newest [`SAMPLES`] durations; [`stats`] reads p50 and p95 from them.
//! [`crate::route::live::decide`] also puts its own breakdown on the call's
//! route record (`timing_us`), so a real cabin's span file shows where a slow
//! send went.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Durations kept per name.
pub const SAMPLES: usize = 1_024;

static SPANS: Mutex<BTreeMap<&'static str, VecDeque<u32>>> = Mutex::new(BTreeMap::new());

/// Record one duration for `name`, in microseconds.
pub fn record(name: &'static str, took: Duration) {
    let us = u32::try_from(took.as_micros()).unwrap_or(u32::MAX);
    let mut spans = SPANS.lock().unwrap_or_else(|e| e.into_inner());
    let ring = spans.entry(name).or_default();
    if ring.len() >= SAMPLES {
        ring.pop_front();
    }
    ring.push_back(us);
}

/// A running span: its time is recorded when it drops (or on [`Lap::stop`]).
pub struct Lap {
    name: &'static str,
    start: Instant,
}

impl Lap {
    /// Stop now and return the time taken, in microseconds.
    pub fn stop(self) -> u32 {
        u32::try_from(self.start.elapsed().as_micros()).unwrap_or(u32::MAX)
    }
}

impl Drop for Lap {
    fn drop(&mut self) {
        record(self.name, self.start.elapsed());
    }
}

/// Start a span named `name`.
pub fn lap(name: &'static str) -> Lap {
    Lap { name, start: Instant::now() }
}

/// Run `f` inside a span named `name`.
pub fn time<T>(name: &'static str, f: impl FnOnce() -> T) -> T {
    let _lap = lap(name);
    f()
}

/// One span's numbers, in microseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stat {
    pub name: &'static str,
    pub n: usize,
    pub p50_us: u32,
    pub p95_us: u32,
    pub max_us: u32,
}

/// The `pct` percentile (nearest rank) of sorted samples.
fn percentile(sorted: &[u32], pct: usize) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (sorted.len() * pct).div_ceil(100).max(1);
    sorted[rank.min(sorted.len()) - 1]
}

/// Every span recorded so far, by name.
pub fn stats() -> Vec<Stat> {
    let spans = SPANS.lock().unwrap_or_else(|e| e.into_inner());
    spans
        .iter()
        .map(|(name, ring)| {
            let mut sorted: Vec<u32> = ring.iter().copied().collect();
            sorted.sort_unstable();
            Stat {
                name,
                n: sorted.len(),
                p50_us: percentile(&sorted, 50),
                p95_us: percentile(&sorted, 95),
                max_us: sorted.last().copied().unwrap_or(0),
            }
        })
        .collect()
}

static ENTER: Mutex<Option<Instant>> = Mutex::new(None);

/// You pressed Enter on a chat message: the next send measures from here.
pub fn note_enter() {
    *ENTER.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
}

/// An Enter older than this belongs to no send (it ended in a slash command, say).
const ENTER_STALE: Duration = Duration::from_secs(120);

/// When Enter was pressed for the send starting now, once.
pub fn take_enter() -> Option<Instant> {
    ENTER.lock().unwrap_or_else(|e| e.into_inner()).take().filter(|t| t.elapsed() < ENTER_STALE)
}

/// Forget every span (a bench run starts clean).
pub fn reset() {
    SPANS.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// One call's own breakdown, for its route record: `(step, µs)` in order.
#[derive(Debug, Default)]
pub struct Breakdown {
    start: Option<Instant>,
    last: Option<Instant>,
    steps: Vec<(&'static str, u32)>,
}

impl Breakdown {
    pub fn start() -> Self {
        let now = Instant::now();
        Self { start: Some(now), last: Some(now), steps: Vec::new() }
    }

    /// Close the step that ran since the last mark, as `name` (also a speed span).
    pub fn mark(&mut self, name: &'static str) {
        let now = Instant::now();
        let took = now.duration_since(self.last.unwrap_or(now));
        record(name, took);
        self.steps.push((name, u32::try_from(took.as_micros()).unwrap_or(u32::MAX)));
        self.last = Some(now);
    }

    /// The steps plus `total` (recorded as the span `total`), for the record.
    pub fn finish(mut self, total: &'static str) -> BTreeMap<String, u32> {
        let took = self.start.map(|s| s.elapsed()).unwrap_or_default();
        record(total, took);
        self.steps.push((total, u32::try_from(took.as_micros()).unwrap_or(u32::MAX)));
        self.steps.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_use_the_nearest_rank() {
        let s: Vec<u32> = (1..=100).collect();
        assert_eq!((percentile(&s, 50), percentile(&s, 95), percentile(&s, 100)), (50, 95, 100));
        assert_eq!((percentile(&[7], 50), percentile(&[7], 95)), (7, 7));
        assert_eq!(percentile(&[], 95), 0);
        assert_eq!((percentile(&[1, 2, 3], 50), percentile(&[1, 2, 3], 95)), (2, 3));
    }

    #[test]
    fn spans_keep_the_newest_samples_and_report_by_name() {
        let name = "test:ring";
        for us in 0..(SAMPLES as u64 + 10) {
            record(name, Duration::from_micros(us));
        }
        let s = stats().into_iter().find(|s| s.name == name).unwrap();
        assert_eq!((s.n, s.max_us), (SAMPLES, SAMPLES as u32 + 9));
        assert_eq!(s.p50_us, 10 + (SAMPLES as u32 / 2) - 1, "the 10 oldest samples were dropped");
    }

    #[test]
    fn a_breakdown_lists_its_steps_and_the_total() {
        let mut b = Breakdown::start();
        b.mark("test:step_a");
        b.mark("test:step_b");
        let steps = b.finish("test:total");
        assert_eq!(steps.keys().map(String::as_str).collect::<Vec<_>>(), ["test:step_a", "test:step_b", "test:total"]);
        assert!(steps["test:total"] >= steps["test:step_a"] + steps["test:step_b"]);
    }
}
