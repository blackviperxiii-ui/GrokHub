//! Heartbeat throttle: how often the pulse may start a proactive act on its own.
//!
//! The pulse still wakes every organ every 15s (`crate::heartbeat`). Only the
//! acts that spend tokens or put something in front of the user (`ProactiveAct`)
//! ask this gate first. Pure and clock-injected: every call takes `now_ms`, so
//! tests drive a fake clock. Scheduled automations and loops are not budgeted
//! here; they run as background runs on their own clock. Halt holds them too.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const MINUTE_MS: u64 = 60_000;
const HOUR_MS: u64 = 60 * MINUTE_MS;
const DAY_MS: u64 = 24 * HOUR_MS;
/// An act with no outcome after this long counts as empty: an anticipate turn
/// the user neither answered nor stopped, or an ideas ask that never came back.
pub const ENGAGE_WINDOW_MS: u64 = 15 * MINUTE_MS;
/// Backoff doubling stops here so a long streak cannot overflow the shift.
const MAX_BACKOFF_STEPS: u32 = 16;

/// What the pulse starts on its own that costs tokens or the user's attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProactiveAct {
    /// `tick_anticipate`: a follow-skill turn in the chat.
    Anticipate,
    /// The automatic ideas ask from the feed pulse (not the Suggest ideas button).
    Ideas,
    /// The nightly review model call.
    Review,
}

impl ProactiveAct {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anticipate => "anticipate",
            Self::Ideas => "ideas",
            Self::Review => "review",
        }
    }
}

/// Why the gate held an act. `reason` is the only thing traced: no content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaceHold {
    Halted,
    Busy,
    Off,
    DayCap,
    HourCap,
    Backoff,
    MinInterval,
}

impl PaceHold {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Halted => "halted",
            Self::Busy => "busy",
            Self::Off => "off",
            Self::DayCap => "day_cap",
            Self::HourCap => "hour_cap",
            Self::Backoff => "backoff",
            Self::MinInterval => "min_interval",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaceGate {
    Act,
    Hold(PaceHold),
}

impl PaceGate {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Act => "under_budget",
            Self::Hold(h) => h.reason(),
        }
    }
}

/// How an act went. Empty and dismissed both count toward backoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActOutcome {
    Useful,
    Empty,
    Dismissed,
}

/// `app.json` → `"heartbeat"`. Minutes and counts. A missing key keeps its default;
/// `maxPerHour` or `maxPerDay` at 0 turns proactive acts off; `backoffAfter` at 0
/// turns backoff off; `haltHoldMin` at 0 holds after Halt until your next message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatPace {
    #[serde(default = "default_min_interval_min")]
    pub min_interval_min: u32,
    #[serde(default = "default_max_per_hour")]
    pub max_per_hour: u32,
    #[serde(default = "default_max_per_day")]
    pub max_per_day: u32,
    #[serde(default = "default_backoff_after")]
    pub backoff_after: u32,
    #[serde(default = "default_backoff_max_min")]
    pub backoff_max_min: u32,
    #[serde(default = "default_halt_hold_min")]
    pub halt_hold_min: u32,
}

fn default_min_interval_min() -> u32 {
    PACE_NORMAL.min_interval_min
}
fn default_max_per_hour() -> u32 {
    PACE_NORMAL.max_per_hour
}
fn default_max_per_day() -> u32 {
    PACE_NORMAL.max_per_day
}
fn default_backoff_after() -> u32 {
    PACE_NORMAL.backoff_after
}
fn default_backoff_max_min() -> u32 {
    PACE_NORMAL.backoff_max_min
}
fn default_halt_hold_min() -> u32 {
    PACE_NORMAL.halt_hold_min
}

/// The default (config only, no Settings control): at most one act per 15 min, 3 an hour, 8 a day (harness design §12 P2).
pub const PACE_NORMAL: HeartbeatPace = HeartbeatPace {
    min_interval_min: 15,
    max_per_hour: 3,
    max_per_day: 8,
    backoff_after: 3,
    backoff_max_min: 240,
    halt_hold_min: 15,
};

impl Default for HeartbeatPace {
    fn default() -> Self {
        PACE_NORMAL
    }
}

impl HeartbeatPace {
    /// Omitted from `app.json` while it stays default, so old files do not grow a key.
    pub fn is_default(&self) -> bool {
        *self == PACE_NORMAL
    }

    pub fn is_off(&self) -> bool {
        self.max_per_day == 0 || self.max_per_hour == 0
    }

    fn min_interval_ms(&self) -> u64 {
        u64::from(self.min_interval_min) * MINUTE_MS
    }

    /// The gap the next act needs after `streak` empty or dismissed acts in a row.
    /// 0 until `backoff_after`; then the interval doubles per extra miss, capped.
    pub fn backoff_ms(&self, streak: u32) -> u64 {
        if self.backoff_after == 0 || streak < self.backoff_after {
            return 0;
        }
        let base = u64::from(self.min_interval_min.max(1)) * MINUTE_MS;
        let steps = (streak - self.backoff_after + 1).min(MAX_BACKOFF_STEPS);
        let cap = (u64::from(self.backoff_max_min) * MINUTE_MS).max(base);
        (base << steps).min(cap)
    }
}

/// Runtime state for the gate. In memory only: a relaunch starts a fresh budget.
#[derive(Debug, Clone, Default)]
pub struct HeartbeatThrottle {
    /// Start times of acts in the last 24h, oldest first.
    acts: VecDeque<u64>,
    last_act_ms: Option<u64>,
    /// Empty or dismissed outcomes in a row. A useful one resets it.
    quiet_streak: u32,
    halted_at_ms: Option<u64>,
    /// The last act, until its outcome lands.
    pending: Option<(ProactiveAct, u64)>,
}

impl HeartbeatThrottle {
    /// May a proactive act start now? Checks, in order: Halt hold, busy user,
    /// off, day cap, hour cap, backoff, min interval.
    pub fn gate(&mut self, pace: &HeartbeatPace, now_ms: u64, busy: bool) -> PaceGate {
        self.prune(now_ms);
        self.settle(now_ms);
        if self.halted(pace, now_ms) {
            return PaceGate::Hold(PaceHold::Halted);
        }
        if busy {
            return PaceGate::Hold(PaceHold::Busy);
        }
        if pace.is_off() {
            return PaceGate::Hold(PaceHold::Off);
        }
        if self.acts.len() >= pace.max_per_day as usize {
            return PaceGate::Hold(PaceHold::DayCap);
        }
        let hour_ago = now_ms.saturating_sub(HOUR_MS);
        if self.acts.iter().filter(|t| **t > hour_ago).count() >= pace.max_per_hour as usize {
            return PaceGate::Hold(PaceHold::HourCap);
        }
        if let Some(last) = self.last_act_ms {
            let gap = now_ms.saturating_sub(last);
            if gap < pace.backoff_ms(self.quiet_streak) {
                return PaceGate::Hold(PaceHold::Backoff);
            }
            if gap < pace.min_interval_ms() {
                return PaceGate::Hold(PaceHold::MinInterval);
            }
        }
        PaceGate::Act
    }

    /// An act started. A previous act still waiting on its outcome counts as empty.
    pub fn record_act(&mut self, act: ProactiveAct, now_ms: u64) {
        if self.pending.take().is_some() {
            self.apply(ActOutcome::Empty);
        }
        self.acts.push_back(now_ms);
        self.last_act_ms = Some(now_ms);
        self.pending = Some((act, now_ms));
    }

    /// The outcome of the pending act. Ignored (false) when `act` is not the one pending,
    /// so the Suggest ideas button or a manual review never moves the streak.
    pub fn record_outcome(&mut self, act: ProactiveAct, outcome: ActOutcome) -> bool {
        match self.pending {
            Some((p, _)) if p == act => {
                self.pending = None;
                self.apply(outcome);
                true
            }
            _ => false,
        }
    }

    /// The user dismissed a card the pulse put up, after its act already landed.
    pub fn note_dismissed(&mut self) {
        self.apply(ActOutcome::Dismissed);
    }

    /// Halt: nothing new starts until `halt_hold_min` passes or the user sends.
    /// A pending act was stopped by the user, so it counts as dismissed.
    pub fn halt(&mut self, now_ms: u64) {
        self.halted_at_ms = Some(now_ms);
        if self.pending.take().is_some() {
            self.apply(ActOutcome::Dismissed);
        }
    }

    /// The user sent their own message: the Halt hold ends.
    pub fn resume(&mut self) {
        self.halted_at_ms = None;
    }

    pub fn halted(&mut self, pace: &HeartbeatPace, now_ms: u64) -> bool {
        let Some(at) = self.halted_at_ms else {
            return false;
        };
        if pace.halt_hold_min == 0 {
            return true;
        }
        let until = at.saturating_add(u64::from(pace.halt_hold_min) * MINUTE_MS);
        if now_ms < until {
            return true;
        }
        self.halted_at_ms = None;
        false
    }

    pub fn quiet_streak(&self) -> u32 {
        self.quiet_streak
    }

    pub fn pending(&self) -> Option<ProactiveAct> {
        self.pending.map(|(a, _)| a)
    }

    fn apply(&mut self, outcome: ActOutcome) {
        match outcome {
            ActOutcome::Useful => self.quiet_streak = 0,
            ActOutcome::Empty | ActOutcome::Dismissed => {
                self.quiet_streak = self.quiet_streak.saturating_add(1)
            }
        }
    }

    fn prune(&mut self, now_ms: u64) {
        let day_ago = now_ms.saturating_sub(DAY_MS);
        while self.acts.front().is_some_and(|t| *t <= day_ago) {
            self.acts.pop_front();
        }
    }

    fn settle(&mut self, now_ms: u64) {
        if let Some((_, at)) = self.pending {
            if now_ms.saturating_sub(at) >= ENGAGE_WINDOW_MS {
                self.pending = None;
                self.apply(ActOutcome::Empty);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_800_000_000_000;
    const MIN: u64 = MINUTE_MS;

    fn act(t: &mut HeartbeatThrottle, pace: &HeartbeatPace, now: u64) -> PaceGate {
        let g = t.gate(pace, now, false);
        if g == PaceGate::Act {
            t.record_act(ProactiveAct::Anticipate, now);
        }
        g
    }

    #[test]
    fn defaults_are_fifteen_minutes_three_an_hour_eight_a_day() {
        let p = HeartbeatPace::default();
        assert_eq!(p.min_interval_min, 15);
        assert_eq!(p.max_per_hour, 3);
        assert_eq!(p.max_per_day, 8);
        assert_eq!(p.backoff_after, 3);
        assert_eq!(p.backoff_max_min, 240);
        assert_eq!(p.halt_hold_min, 15);
        assert!(p.is_default());
        assert!(!p.is_off());
    }

    #[test]
    fn min_interval_holds_the_next_act_until_it_passes() {
        let pace = PACE_NORMAL;
        let mut t = HeartbeatThrottle::default();
        assert_eq!(act(&mut t, &pace, T0), PaceGate::Act);
        assert_eq!(
            t.gate(&pace, T0 + 14 * MIN, false),
            PaceGate::Hold(PaceHold::MinInterval)
        );
        assert_eq!(t.gate(&pace, T0 + 15 * MIN, false), PaceGate::Act);
        assert_eq!(PaceGate::Hold(PaceHold::MinInterval).reason(), "min_interval");
    }

    #[test]
    fn hour_cap_then_day_cap() {
        // No interval, so only the caps bind.
        let pace = HeartbeatPace {
            min_interval_min: 0,
            max_per_hour: 2,
            max_per_day: 3,
            backoff_after: 0,
            ..PACE_NORMAL
        };
        let mut t = HeartbeatThrottle::default();
        assert_eq!(act(&mut t, &pace, T0), PaceGate::Act);
        assert_eq!(act(&mut t, &pace, T0 + MIN), PaceGate::Act);
        assert_eq!(
            act(&mut t, &pace, T0 + 2 * MIN),
            PaceGate::Hold(PaceHold::HourCap)
        );
        // The first act leaves the hour window.
        assert_eq!(act(&mut t, &pace, T0 + 61 * MIN), PaceGate::Act);
        assert_eq!(
            act(&mut t, &pace, T0 + 3 * 60 * MIN),
            PaceGate::Hold(PaceHold::DayCap)
        );
        // A day after the first act, one slot frees up.
        assert_eq!(act(&mut t, &pace, T0 + 24 * 60 * MIN + 1), PaceGate::Act);
        assert_eq!(
            act(&mut t, &pace, T0 + 24 * 60 * MIN + 2),
            PaceGate::Hold(PaceHold::DayCap)
        );
    }

    #[test]
    fn default_pace_allows_eight_a_day_and_three_an_hour() {
        let pace = PACE_NORMAL;
        let mut t = HeartbeatThrottle::default();
        let mut n = 0;
        // Ask every 15s pulse for a day.
        let mut now = T0;
        while now < T0 + 24 * 60 * MIN {
            if act(&mut t, &pace, now) == PaceGate::Act {
                t.record_outcome(ProactiveAct::Anticipate, ActOutcome::Useful);
                n += 1;
            }
            now += 15_000;
        }
        assert_eq!(n, 8);
        let mut t = HeartbeatThrottle::default();
        let mut in_hour = 0;
        let mut now = T0;
        while now < T0 + 60 * MIN {
            if act(&mut t, &pace, now) == PaceGate::Act {
                t.record_outcome(ProactiveAct::Anticipate, ActOutcome::Useful);
                in_hour += 1;
            }
            now += 15_000;
        }
        assert_eq!(in_hour, 3, "15-min interval and the hour cap of 3");
    }

    #[test]
    fn backoff_doubles_after_empty_or_dismissed_acts_and_resets_on_useful() {
        let pace = PACE_NORMAL;
        assert_eq!(pace.backoff_ms(2), 0);
        assert_eq!(pace.backoff_ms(3), 30 * MIN);
        assert_eq!(pace.backoff_ms(4), 60 * MIN);
        assert_eq!(pace.backoff_ms(5), 120 * MIN);
        assert_eq!(pace.backoff_ms(6), 240 * MIN);
        assert_eq!(pace.backoff_ms(60), 240 * MIN, "capped at backoffMaxMin");

        let mut t = HeartbeatThrottle::default();
        let mut now = T0;
        for outcome in [ActOutcome::Empty, ActOutcome::Dismissed, ActOutcome::Empty] {
            assert_eq!(act(&mut t, &pace, now), PaceGate::Act);
            assert!(t.record_outcome(ProactiveAct::Anticipate, outcome));
            now += 20 * MIN;
        }
        assert_eq!(t.quiet_streak(), 3);
        // Last act at T0+40; 20 min later the 15-min interval has passed, backoff has not.
        assert_eq!(t.gate(&pace, now, false), PaceGate::Hold(PaceHold::Backoff));
        assert_eq!(t.gate(&pace, T0 + 40 * MIN + 30 * MIN, false), PaceGate::Act);
        t.record_act(ProactiveAct::Anticipate, T0 + 70 * MIN);
        t.record_outcome(ProactiveAct::Anticipate, ActOutcome::Useful);
        assert_eq!(t.quiet_streak(), 0);
        assert_eq!(
            t.gate(&pace, T0 + 85 * MIN, false),
            PaceGate::Act,
            "a useful act brings back the plain interval"
        );
    }

    #[test]
    fn an_act_nobody_answers_counts_as_empty() {
        let pace = PACE_NORMAL;
        let mut t = HeartbeatThrottle::default();
        t.record_act(ProactiveAct::Anticipate, T0);
        assert_eq!(t.pending(), Some(ProactiveAct::Anticipate));
        let _ = t.gate(&pace, T0 + ENGAGE_WINDOW_MS - 1, false);
        assert_eq!(t.quiet_streak(), 0);
        let _ = t.gate(&pace, T0 + ENGAGE_WINDOW_MS, false);
        assert_eq!(t.quiet_streak(), 1);
        assert_eq!(t.pending(), None);
        // A second act while the first still waits settles the first as empty.
        t.record_act(ProactiveAct::Ideas, T0 + 20 * MIN);
        t.record_act(ProactiveAct::Review, T0 + 21 * MIN);
        assert_eq!(t.quiet_streak(), 2);
        assert_eq!(t.pending(), Some(ProactiveAct::Review));
    }

    #[test]
    fn a_later_card_dismissal_counts_toward_backoff() {
        let mut t = HeartbeatThrottle::default();
        t.note_dismissed();
        t.note_dismissed();
        assert_eq!(t.quiet_streak(), 2);
    }

    #[test]
    fn outcome_for_another_act_is_ignored() {
        let mut t = HeartbeatThrottle::default();
        t.record_act(ProactiveAct::Ideas, T0);
        assert!(!t.record_outcome(ProactiveAct::Review, ActOutcome::Empty));
        assert_eq!(t.quiet_streak(), 0);
        assert_eq!(t.pending(), Some(ProactiveAct::Ideas));
        assert!(!HeartbeatThrottle::default().record_outcome(ProactiveAct::Ideas, ActOutcome::Empty));
    }

    #[test]
    fn busy_user_holds_every_act_and_spends_nothing() {
        let pace = PACE_NORMAL;
        let mut t = HeartbeatThrottle::default();
        assert_eq!(t.gate(&pace, T0, true), PaceGate::Hold(PaceHold::Busy));
        assert_eq!(PaceHold::Busy.reason(), "busy");
        assert_eq!(t.pending(), None);
        // Not busy a beat later: nothing was spent, so it acts.
        assert_eq!(t.gate(&pace, T0 + 15_000, false), PaceGate::Act);
    }

    #[test]
    fn halt_holds_at_once_until_the_hold_ends_or_the_user_sends() {
        let pace = PACE_NORMAL;
        let mut t = HeartbeatThrottle::default();
        t.record_act(ProactiveAct::Anticipate, T0);
        t.halt(T0 + MIN);
        assert_eq!(t.quiet_streak(), 1, "the halted act counts as dismissed");
        assert_eq!(t.pending(), None);
        // Halt beats every other reason, even a free budget.
        assert_eq!(t.gate(&pace, T0 + MIN, false), PaceGate::Hold(PaceHold::Halted));
        assert_eq!(t.gate(&pace, T0 + MIN, true), PaceGate::Hold(PaceHold::Halted));
        assert_eq!(
            t.gate(&pace, T0 + 16 * MIN - 1, false),
            PaceGate::Hold(PaceHold::Halted)
        );
        assert_eq!(t.gate(&pace, T0 + 16 * MIN, false), PaceGate::Act);
        t.halt(T0 + 20 * MIN);
        assert!(t.halted(&pace, T0 + 21 * MIN));
        t.resume();
        assert!(!t.halted(&pace, T0 + 21 * MIN));
        // haltHoldMin 0 holds until the user sends.
        let forever = HeartbeatPace {
            halt_hold_min: 0,
            ..PACE_NORMAL
        };
        t.halt(T0);
        assert!(t.halted(&forever, T0 + 48 * 60 * MIN));
        t.resume();
        assert!(!t.halted(&forever, T0 + 48 * 60 * MIN));
    }

    #[test]
    fn off_holds_everything() {
        let mut t = HeartbeatThrottle::default();
        let no_day = HeartbeatPace {
            max_per_day: 0,
            ..PACE_NORMAL
        };
        assert_eq!(t.gate(&no_day, T0, false), PaceGate::Hold(PaceHold::Off));
        let no_hour = HeartbeatPace {
            max_per_hour: 0,
            ..PACE_NORMAL
        };
        assert_eq!(t.gate(&no_hour, T0, false), PaceGate::Hold(PaceHold::Off));
    }

    #[test]
    fn config_overrides_parse_and_missing_keys_keep_defaults() {
        let empty: HeartbeatPace = serde_json::from_str("{}").expect("empty");
        assert_eq!(empty, PACE_NORMAL);
        let p: HeartbeatPace =
            serde_json::from_str(r#"{"minIntervalMin":5,"maxPerDay":20,"haltHoldMin":0}"#)
                .expect("partial");
        assert_eq!(p.min_interval_min, 5);
        assert_eq!(p.max_per_day, 20);
        assert_eq!(p.halt_hold_min, 0);
        assert_eq!(p.max_per_hour, 3);
        assert_eq!(p.backoff_after, 3);
        assert_eq!(p.backoff_max_min, 240);
        let out = serde_json::to_string(&p).expect("ser");
        assert_eq!(
            out,
            r#"{"minIntervalMin":5,"maxPerHour":3,"maxPerDay":20,"backoffAfter":3,"backoffMaxMin":240,"haltHoldMin":0}"#
        );
        // The override changes the gate: 5-min interval instead of 15.
        let mut t = HeartbeatThrottle::default();
        t.record_act(ProactiveAct::Ideas, T0);
        t.record_outcome(ProactiveAct::Ideas, ActOutcome::Useful);
        assert_eq!(t.gate(&p, T0 + 5 * MIN, false), PaceGate::Act);
        assert_eq!(
            t.gate(&PACE_NORMAL, T0 + 5 * MIN, false),
            PaceGate::Hold(PaceHold::MinInterval)
        );
    }

    #[test]
    fn reasons_are_plain_tags() {
        let all = [
            PaceHold::Halted,
            PaceHold::Busy,
            PaceHold::Off,
            PaceHold::DayCap,
            PaceHold::HourCap,
            PaceHold::Backoff,
            PaceHold::MinInterval,
        ];
        let tags: Vec<&str> = all.iter().map(|h| h.reason()).collect();
        assert_eq!(
            tags,
            ["halted", "busy", "off", "day_cap", "hour_cap", "backoff", "min_interval"]
        );
        assert_eq!(PaceGate::Act.reason(), "under_budget");
        assert_eq!(ProactiveAct::Anticipate.as_str(), "anticipate");
        assert_eq!(ProactiveAct::Ideas.as_str(), "ideas");
        assert_eq!(ProactiveAct::Review.as_str(), "review");
    }
}
