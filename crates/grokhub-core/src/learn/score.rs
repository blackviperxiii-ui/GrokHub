//! The scoreboard: whether what the app decides goes the user's way, week
//! by week, for each [`Area`].
//!
//! Every answer to an offer and every kept or undone change is one count,
//! good or bad, in the week it happened. An area is scored by its share of
//! good counts over the last [`WINDOW_WEEKS`] weeks against the same span
//! before. An area that slipped by [`SLIP`] or more, with at least
//! [`MIN_OUTCOMES`] counts on both sides, stops acting on its own for
//! [`SLIP_REST_MS`] and only offers. Counts only, no ids of what was changed.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::decide::Area;

const DAY_MS: u64 = 24 * 60 * 60 * 1_000;
pub const WEEK_MS: u64 = 7 * DAY_MS;
/// Weeks in each of the two spans compared.
pub const WINDOW_WEEKS: u64 = 4;
/// Counts a span needs before it is compared.
pub const MIN_OUTCOMES: u32 = 10;
/// How far the share must move to count as better or worse.
pub const SLIP: f32 = 0.15;
/// How long a slipped area only offers.
pub const SLIP_REST_MS: u64 = 30 * DAY_MS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Tally {
    week: u64,
    area: Area,
    good: u32,
    bad: u32,
}

/// How an area's last span compares with the one before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Trend {
    /// Not enough counts on both sides yet.
    TooEarly,
    Better,
    Steady,
    Worse,
}

/// One area's numbers. Shares are 0 to 100; `None` with nothing counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Score {
    pub now: Option<u8>,
    pub before: Option<u8>,
    pub trend: Trend,
}

/// The counts and which areas are resting. Part of the decision file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Board {
    tallies: Vec<Tally>,
    resting_until_ms: BTreeMap<Area, u64>,
}

fn share(good: u32, bad: u32) -> Option<f32> {
    (good + bad > 0).then(|| good as f32 / (good + bad) as f32)
}

impl Board {
    /// Counts one outcome in `area`, and drops weeks no span reads any more.
    pub fn note(&mut self, area: Area, good: bool, now_ms: u64) {
        let week = now_ms / WEEK_MS;
        self.tallies
            .retain(|t| week.saturating_sub(t.week) < 2 * WINDOW_WEEKS);
        let at = match self
            .tallies
            .iter()
            .position(|t| t.week == week && t.area == area)
        {
            Some(at) => at,
            None => {
                self.tallies.push(Tally {
                    week,
                    area,
                    good: 0,
                    bad: 0,
                });
                self.tallies.len() - 1
            }
        };
        if good {
            self.tallies[at].good += 1;
        } else {
            self.tallies[at].bad += 1;
        }
    }

    /// Good and bad counts for `area` in the span `back` spans ago (0 is
    /// the one that ends this week).
    fn span(&self, area: Area, now_ms: u64, back: u64) -> (u32, u32) {
        let week = now_ms / WEEK_MS;
        self.tallies
            .iter()
            .filter(|t| t.area == area && t.week <= week)
            .filter(|t| (week - t.week) / WINDOW_WEEKS == back)
            .fold((0, 0), |(g, b), t| (g + t.good, b + t.bad))
    }

    pub fn score(&self, area: Area, now_ms: u64) -> Score {
        let (now, before) = (self.span(area, now_ms, 0), self.span(area, now_ms, 1));
        let pct = |(g, b)| share(g, b).map(|s| (s * 100.0).round() as u8);
        let enough = |(g, b): (u32, u32)| g + b >= MIN_OUTCOMES;
        let trend = match (share(now.0, now.1), share(before.0, before.1)) {
            (Some(n), Some(b)) if enough(now) && enough(before) => {
                if n <= b - SLIP {
                    Trend::Worse
                } else if n >= b + SLIP {
                    Trend::Better
                } else {
                    Trend::Steady
                }
            }
            _ => Trend::TooEarly,
        };
        Score {
            now: pct(now),
            before: pct(before),
            trend,
        }
    }

    /// Whether `area` only offers for now because it slipped.
    pub fn resting(&self, area: Area, now_ms: u64) -> bool {
        self.resting_until_ms
            .get(&area)
            .is_some_and(|until| now_ms < *until)
    }

    /// Starts a rest for `area` when its numbers got worse. Returns whether
    /// it is resting after the check.
    pub fn check(&mut self, area: Area, now_ms: u64) -> bool {
        if !self.resting(area, now_ms) && self.score(area, now_ms).trend == Trend::Worse {
            self.resting_until_ms.insert(area, now_ms + SLIP_REST_MS);
        }
        self.resting(area, now_ms)
    }

    /// The user's "carry on": every area may act again.
    pub fn resume(&mut self) {
        self.resting_until_ms.clear();
    }
}
