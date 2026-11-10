//! Beliefs: what the app holds to be true, with how sure it is.
//!
//! A belief is a pair of counters (how often something went for and against
//! its subject) or a running average of a number the app measured. Both
//! update in constant time. Counts fade by half every [`HALF_LIFE_MS`], worked
//! out from the time of the last update, so an old habit loses weight without
//! a timer.
//!
//! A habit can also flip overnight, and a 30-day fade is too slow for that.
//! A belief with at least [`BREAK_MIN_EVIDENCE`] behind it counts votes in a
//! row against the side it leans to; at [`BREAK_RUN`] of them the old side
//! keeps only [`BREAK_KEEP`] of its count, so the new habit shows at once.
//!
//! Confidence is the lower end of the Wilson interval on the side the counts
//! lean to: twelve of twelve gives 0.82, three of three gives 0.53, and six
//! against six gives 0.29.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::signal::{Kind, Signal, Subject};

/// Counts are worth half after this long without a refresh.
pub const HALF_LIFE_MS: u64 = 30 * 24 * 60 * 60 * 1_000;
/// Beliefs kept. One more drops the weakest.
pub const BELIEF_CAP: usize = 2_000;
/// z² for a 95% one-sided interval (z = 1.645).
const Z2: f32 = 2.706;
/// Evidence a belief needs before a run against it counts as a break.
pub const BREAK_MIN_EVIDENCE: f32 = 6.0;
/// Votes in a row against the lean that make a break.
pub const BREAK_RUN: u8 = 4;
/// The share of its count the old side keeps after a break.
pub const BREAK_KEEP: f32 = 0.25;
/// The evidence a pinned belief reads as: enough for every rung of the ladder.
pub const PINNED_EVIDENCE: f32 = 100.0;

fn fade(from_ms: u64, to_ms: u64) -> f32 {
    let dt = to_ms.saturating_sub(from_ms);
    0.5f64.powf(dt as f64 / HALF_LIFE_MS as f64) as f32
}

/// What a belief counts.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    /// How often it went for (`yes`) and against (`no`) the subject.
    Rate { yes: f32, no: f32 },
    /// The running average of a measured number over `n` samples.
    Level { mean: f32, n: f32 },
}

/// Whether the user has overruled the belief.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Standing {
    Live,
    /// The user forgot it at `at_ms`. Signals from before then never count
    /// again; later ones start it fresh.
    Forgotten {
        at_ms: u64,
    },
    /// The user said it is wrong. It never learns and never acts again.
    Wrong,
    /// The user said it is always so. Nothing counts against it and it
    /// never fades, until the user forgets it or marks it wrong.
    Pinned,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Belief {
    pub shape: Shape,
    pub first_ms: u64,
    pub last_ms: u64,
    /// The learner that wrote it, e.g. `signals`.
    pub source: String,
    pub standing: Standing,
    /// Votes in a row against the side it leans to.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub against: u8,
}

fn is_zero(n: &u8) -> bool {
    *n == 0
}

impl Belief {
    fn new(shape: Shape, at_ms: u64, source: &str) -> Self {
        Self {
            shape,
            first_ms: at_ms,
            last_ms: at_ms,
            source: source.to_string(),
            standing: Standing::Live,
            against: 0,
        }
    }

    fn live(&self) -> bool {
        matches!(self.standing, Standing::Live | Standing::Pinned)
    }

    fn pinned(&self) -> bool {
        self.standing == Standing::Pinned
    }

    /// How much evidence stands behind it at `now_ms`, after fading.
    pub fn evidence(&self, now_ms: u64) -> f32 {
        if !self.live() {
            return 0.0;
        }
        if self.pinned() {
            return PINNED_EVIDENCE;
        }
        let raw = match self.shape {
            Shape::Rate { yes, no } => yes + no,
            Shape::Level { n, .. } => n,
        };
        raw * fade(self.last_ms, now_ms)
    }

    /// The share of the evidence that went for the subject. `None` for a
    /// level, and when there is no evidence.
    pub fn rate(&self) -> Option<f32> {
        match self.shape {
            Shape::Rate { .. } if self.pinned() => Some(1.0),
            Shape::Rate { yes, no } if self.live() && yes + no > 0.0 => Some(yes / (yes + no)),
            _ => None,
        }
    }

    /// The measured average. `None` for a rate.
    pub fn level(&self) -> Option<f32> {
        match self.shape {
            Shape::Level { mean, n } if self.live() && n > 0.0 => Some(mean),
            _ => None,
        }
    }

    /// Whether the evidence leans for the subject. `None` when it has none,
    /// or for a level.
    pub fn leans_yes(&self) -> Option<bool> {
        self.rate().map(|r| r >= 0.5)
    }

    /// 0 to 1: how sure the app is of the side the belief leans to (for a
    /// level, of the average) at `now_ms`.
    pub fn confidence(&self, now_ms: u64) -> f32 {
        if self.pinned() {
            return 1.0;
        }
        let n = self.evidence(now_ms);
        if n <= 0.0 {
            return 0.0;
        }
        let p = match self.rate() {
            Some(r) => r.max(1.0 - r),
            None => 1.0,
        };
        let centre = p + Z2 / (2.0 * n);
        let spread = (Z2 * (p * (1.0 - p) / n + Z2 / (4.0 * n * n))).sqrt();
        ((centre - spread) / (1.0 + Z2 / n)).clamp(0.0, 1.0)
    }

    /// Fades the counts up to `at_ms`. A signal older than the last update
    /// fades nothing and does not move the clock back.
    fn advance(&mut self, at_ms: u64) {
        if at_ms <= self.last_ms {
            return;
        }
        let f = fade(self.last_ms, at_ms);
        match &mut self.shape {
            Shape::Rate { yes, no } => {
                *yes *= f;
                *no *= f;
            }
            Shape::Level { n, .. } => *n *= f,
        }
        self.last_ms = at_ms;
    }
}

/// Every belief, by subject.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Beliefs {
    map: BTreeMap<Subject, Belief>,
}

impl Beliefs {
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn get(&self, key: &Subject) -> Option<&Belief> {
        self.map.get(key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Subject, &Belief)> {
        self.map.iter()
    }

    /// Learns from one signal under the signal's own subject. Says whether
    /// a belief changed.
    pub fn apply(&mut self, s: &Signal) -> bool {
        match (s.kind, s.kind.vote()) {
            (Kind::Measured, _) => self.measure(&s.subject, s.value, s.at_ms, "signals"),
            (_, Some(yes)) => self.vote(&s.subject, yes, s.at_ms, "signals"),
            (_, None) => false,
        }
    }

    /// Counts one thing that went for (`yes`) or against the subject.
    pub fn vote(&mut self, key: &Subject, yes: bool, at_ms: u64, source: &str) -> bool {
        let fresh = Shape::Rate { yes: 0.0, no: 0.0 };
        let Some(b) = self.slot(key, fresh, at_ms, source) else {
            return false;
        };
        b.advance(at_ms);
        // The side it leaned to before this vote, once there is enough
        // behind it for a run the other way to mean something.
        let lean = match b.shape {
            Shape::Rate { yes: y, no: n } if y + n >= BREAK_MIN_EVIDENCE && y != n => Some(y > n),
            _ => None,
        };
        b.against = match lean {
            Some(side) if side != yes => b.against.saturating_add(1),
            _ => 0,
        };
        let broke = b.against >= BREAK_RUN;
        match &mut b.shape {
            Shape::Rate { yes: y, no: n } => {
                if broke {
                    // The old habit ended: most of its count goes at once.
                    *(if yes { &mut *n } else { &mut *y }) *= BREAK_KEEP;
                }
                *(if yes { y } else { n }) += 1.0;
            }
            // A subject holds one shape: a vote can't turn a level into a rate.
            Shape::Level { .. } => return false,
        }
        if broke {
            b.against = 0;
        }
        true
    }

    /// Folds one measured number into the subject's running average.
    pub fn measure(&mut self, key: &Subject, x: f32, at_ms: u64, source: &str) -> bool {
        if !x.is_finite() {
            return false;
        }
        let fresh = Shape::Level { mean: 0.0, n: 0.0 };
        let Some(b) = self.slot(key, fresh, at_ms, source) else {
            return false;
        };
        b.advance(at_ms);
        match &mut b.shape {
            Shape::Level { mean, n } => {
                *n += 1.0;
                *mean += (x - *mean) / *n;
                true
            }
            Shape::Rate { .. } => false,
        }
    }

    /// The user's Forget. Says whether there was anything to forget.
    pub fn forget(&mut self, key: &Subject, now_ms: u64) -> bool {
        match self.map.get_mut(key) {
            Some(b) if b.standing != Standing::Wrong => {
                b.standing = Standing::Forgotten { at_ms: now_ms };
                b.shape = match b.shape {
                    Shape::Rate { .. } => Shape::Rate { yes: 0.0, no: 0.0 },
                    Shape::Level { .. } => Shape::Level { mean: 0.0, n: 0.0 },
                };
                true
            }
            _ => false,
        }
    }

    /// The user's "That's wrong". Works on a subject nothing was learned
    /// about yet, so it can be ruled out ahead of time.
    pub fn mark_wrong(&mut self, key: &Subject, now_ms: u64) {
        let fresh = Shape::Rate { yes: 0.0, no: 0.0 };
        let b = self
            .map
            .entry(key.clone())
            .or_insert_with(|| Belief::new(fresh, now_ms, "user"));
        b.standing = Standing::Wrong;
        b.shape = match b.shape {
            Shape::Rate { .. } => Shape::Rate { yes: 0.0, no: 0.0 },
            Shape::Level { .. } => Shape::Level { mean: 0.0, n: 0.0 },
        };
    }

    /// The user's "always": the subject holds from now on, whatever was or
    /// will be counted. Forget and "That's wrong" still overrule it. A
    /// measured number can't be pinned; says whether it was.
    pub fn pin(&mut self, key: &Subject, now_ms: u64) -> bool {
        let fresh = Shape::Rate { yes: 0.0, no: 0.0 };
        let b = self
            .map
            .entry(key.clone())
            .or_insert_with(|| Belief::new(fresh, now_ms, "user"));
        if matches!(b.shape, Shape::Level { .. }) {
            return false;
        }
        b.standing = Standing::Pinned;
        b.last_ms = now_ms;
        true
    }

    /// The belief a new fact at `at_ms` may change: `None` when the user
    /// ruled it out or pinned it, forgot it after that time, or the store is
    /// full of lines the user locked.
    fn slot(
        &mut self,
        key: &Subject,
        fresh: Shape,
        at_ms: u64,
        source: &str,
    ) -> Option<&mut Belief> {
        match self.map.get(key).map(|b| b.standing) {
            Some(Standing::Wrong | Standing::Pinned) => return None,
            Some(Standing::Forgotten { at_ms: gone }) if at_ms <= gone => return None,
            Some(Standing::Forgotten { .. }) => {
                self.map
                    .insert(key.clone(), Belief::new(fresh, at_ms, source));
            }
            Some(Standing::Live) => {}
            None => {
                if self.map.len() >= BELIEF_CAP && !self.drop_weakest(at_ms) {
                    return None;
                }
                self.map
                    .insert(key.clone(), Belief::new(fresh, at_ms, source));
            }
        }
        self.map.get_mut(key)
    }

    /// Drops the belief with the least evidence, the oldest on a tie. A line
    /// the user marked wrong is never dropped.
    fn drop_weakest(&mut self, now_ms: u64) -> bool {
        let weakest = self
            .map
            .iter()
            .filter(|(_, b)| !matches!(b.standing, Standing::Wrong | Standing::Pinned))
            .min_by(|(_, a), (_, b)| {
                a.evidence(now_ms)
                    .total_cmp(&b.evidence(now_ms))
                    .then(a.last_ms.cmp(&b.last_ms))
            })
            .map(|(k, _)| k.clone());
        match weakest {
            Some(k) => self.map.remove(&k).is_some(),
            None => false,
        }
    }
}
