//! Deliberate experiments: trying something the app is not sure of, on
//! purpose and rarely, so it can find a habit it would otherwise never test.
//!
//! The app learns from what happens to occur. A habit seen a few times stays
//! under the line for a suggestion, and nothing ever asks the user about it.
//! An experiment shows it once as a chip marked as a try. A tap runs the
//! skill, which counts for the habit like any run. A hide is a no to its
//! rule, which rests it.
//!
//! The limits, all in [`Tries::allow`]:
//! - one experiment a day, in the whole app ([`PER_DAY`]);
//! - the same thing is not tried again for [`REST_MS`];
//! - only where the area's dial is on act, and never while the app is
//!   paused, halted, busy or in quiet hours;
//! - only one-click things. Nothing here makes a change: a try is a chip.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::signal::Subject;

const DAY_MS: u64 = 24 * 60 * 60 * 1_000;
/// Experiments a day, across every area.
pub const PER_DAY: usize = 1;
/// How long before the same thing may be tried again.
pub const REST_MS: u64 = 14 * DAY_MS;
/// Evidence a belief needs before it is worth a try: seen more than once.
pub const MIN_EVIDENCE: f32 = 2.0;

/// When each rule was last tried. Part of the decision file.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Tries {
    at_ms: BTreeMap<Subject, u64>,
}

impl Tries {
    /// When `rule` was last tried, if it still counts.
    pub fn at(&self, rule: &Subject) -> Option<u64> {
        self.at_ms.get(rule).copied()
    }

    /// Whether `rule` is today's experiment. The first rule asked on a day
    /// takes the day, and keeps it for the day, so its chip doesn't come and
    /// go. `clear` is whether the app may try anything right now.
    pub fn allow(&mut self, rule: &Subject, clear: bool, now_ms: u64) -> bool {
        self.at_ms
            .retain(|_, at| now_ms.saturating_sub(*at) < REST_MS);
        let today = |at: u64| at / DAY_MS == now_ms / DAY_MS;
        if let Some(at) = self.at_ms.get(rule) {
            // Already today's try: it stays while the app may still try.
            return clear && today(*at);
        }
        if !clear || self.at_ms.values().filter(|at| today(**at)).count() >= PER_DAY {
            return false;
        }
        self.at_ms.insert(rule.clone(), now_ms);
        true
    }
}
