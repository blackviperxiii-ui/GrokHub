//! The two-week tryout a new provider model runs before it takes real
//! work (router failover step 9). A model a source starts listing after
//! that source's first listing — a new OpenRouter release, a new Claude —
//! and any model the bundled card table marks `probation` starts a
//! [`Stage::Trial`]: the scorer keeps it to tiny and short turns, a 5%
//! canary of low-risk turns hands it real ones, and free models also get
//! a few small probes a day. Real calls count toward its tally as they
//! fold in from the health log. The refresh judges it: it graduates on
//! evidence (enough calls, enough of them answered, and its answers
//! passing checks no less often than its peers'), fails on evidence, and
//! a failed model is barred until its retry two weeks on.

use serde::{Deserialize, Serialize};

use super::health::{CallStatus, Observation};

const DAY_MS: u64 = 24 * 60 * 60 * 1000;
/// The tryout lasts at least this long before it can graduate on time.
pub const TRIAL_DAYS: u64 = 14;
/// Counted calls a graduation needs.
pub const TRIAL_MIN_CALLS: u32 = 20;
/// Counted calls that let a model graduate before its two weeks are up,
/// when its answers held up too.
pub const TRIAL_EARLY_CALLS: u32 = 200;
/// The share of counted calls that must answer, percent.
pub const TRIAL_MIN_OK_PCT: u32 = 90;
/// Counted calls with not one answer that fail a model outright.
pub const TRIAL_DEAD_CALLS: u32 = 5;
/// A failed model gets a fresh tryout this long after it failed.
pub const RETRY_DAYS: u64 = 14;

/// Where a model's tryout stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// On tryout: tiny and short turns, the canary and probes only.
    #[default]
    Trial,
    /// Passed: an ordinary model from here on.
    Graduated,
    /// Failed: out of routing until its retry.
    Failed,
}

/// How a model's answers compared with its peers' over the tryout, as
/// the learner's scorecards tell it. The router's crate decides it; this
/// crate only weighs it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Quality {
    /// Too few checked answers to say.
    #[default]
    Unknown,
    /// Passed its checks no less often than its peers (within the margin).
    Held,
    /// Passed its checks less often than its peers by more than the margin.
    Fell,
}

/// One model's tryout, kept on its registry record.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Probation {
    #[serde(default)]
    pub stage: Stage,
    /// When this tryout (or its retry) began.
    #[serde(default)]
    pub since_ms: u64,
    /// Calls that say something about the model (an answer, an error, a
    /// limit or an outage); calls refused for the prompt or the account
    /// never count.
    #[serde(default)]
    pub calls: u32,
    /// Of those, the ones it answered.
    #[serde(default)]
    pub ok: u32,
    /// Of `calls`, the probes (the rest were real turns).
    #[serde(default)]
    pub probes: u32,
    /// The day key (`YYYY-MM-DD`) of `probes_today`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub probe_day: String,
    #[serde(default)]
    pub probes_today: u32,
    /// When it graduated or failed (0 on trial).
    #[serde(default)]
    pub decided_ms: u64,
    /// One plain sentence: why it stands where it does.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
}

/// What a judgment changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub to: Stage,
    pub reason: String,
}

impl Probation {
    pub fn start(now_ms: u64) -> Self {
        Self {
            since_ms: now_ms,
            reason: "It's new: a two-week tryout on small turns first.".into(),
            ..Self::default()
        }
    }

    /// Still being tried, or failed: the scorer treats both as probation
    /// (a failed one is barred outright on top).
    pub fn active(&self) -> bool {
        self.stage != Stage::Graduated
    }

    pub fn ok_pct(&self) -> u32 {
        if self.calls == 0 {
            0
        } else {
            (u64::from(self.ok) * 100 / u64::from(self.calls)) as u32
        }
    }

    /// Count one real call from the health log. Only a trial counts.
    pub fn observe(&mut self, obs: &Observation) {
        if self.stage != Stage::Trial {
            return;
        }
        match obs.status {
            CallStatus::Ok => {
                self.calls += 1;
                self.ok += 1;
            }
            CallStatus::Error | CallStatus::Pressure | CallStatus::NotFound => self.calls += 1,
            // About the prompt or the account, not the model.
            CallStatus::ContentSafety
            | CallStatus::FreeUsageExhausted
            | CallStatus::QuotaExhausted
            | CallStatus::NoSubscription => {}
        }
    }

    /// Count one probe's outcome (`ok`: it answered and passed its check).
    pub fn probed(&mut self, ok: bool, day: &str) {
        if self.stage != Stage::Trial {
            return;
        }
        self.calls += 1;
        self.probes += 1;
        if ok {
            self.ok += 1;
        }
        if self.probe_day != day {
            self.probe_day = day.to_string();
            self.probes_today = 0;
        }
        self.probes_today += 1;
    }

    /// Probes already sent on `day`.
    pub fn probes_on(&self, day: &str) -> u32 {
        if self.probe_day == day {
            self.probes_today
        } else {
            0
        }
    }

    /// Judge the tryout at `now_ms` with the learner's word on quality.
    /// `None` leaves it where it stands.
    pub fn judge(&mut self, quality: Quality, now_ms: u64) -> Option<Verdict> {
        let verdict = match self.stage {
            Stage::Graduated => None,
            Stage::Failed => {
                (now_ms.saturating_sub(self.decided_ms) >= RETRY_DAYS * DAY_MS).then(|| Verdict {
                    to: Stage::Trial,
                    reason: "Its two weeks out are up: a fresh tryout on small turns.".into(),
                })
            }
            Stage::Trial => self.trial_verdict(quality, now_ms),
        }?;
        match verdict.to {
            Stage::Trial => {
                *self = Self {
                    reason: verdict.reason.clone(),
                    ..Self::start(now_ms)
                };
            }
            to => {
                self.stage = to;
                self.decided_ms = now_ms;
                self.reason.clone_from(&verdict.reason);
            }
        }
        Some(verdict)
    }

    fn trial_verdict(&self, quality: Quality, now_ms: u64) -> Option<Verdict> {
        let fail = |reason: String| {
            Some(Verdict {
                to: Stage::Failed,
                reason,
            })
        };
        if self.calls >= TRIAL_DEAD_CALLS && self.ok == 0 {
            return fail(format!(
                "None of its first {} tryout calls answered.",
                self.calls
            ));
        }
        if self.calls >= TRIAL_MIN_CALLS && self.ok_pct() < TRIAL_MIN_OK_PCT {
            return fail(format!(
                "It answered {} of {} tryout calls, under {TRIAL_MIN_OK_PCT}%.",
                self.ok, self.calls
            ));
        }
        if quality == Quality::Fell {
            return fail(
                "Its answers passed their checks less often than the models it would stand in for."
                    .into(),
            );
        }
        let reliable = self.calls >= TRIAL_MIN_CALLS && self.ok_pct() >= TRIAL_MIN_OK_PCT;
        let on_time = now_ms.saturating_sub(self.since_ms) >= TRIAL_DAYS * DAY_MS;
        let early = self.calls >= TRIAL_EARLY_CALLS && quality == Quality::Held;
        (reliable && (on_time || early)).then(|| Verdict {
            to: Stage::Graduated,
            reason: format!(
                "It passed its tryout: {} of {} calls answered.",
                self.ok, self.calls
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(status: CallStatus) -> Observation {
        Observation {
            model: "openrouter/vendor/new".into(),
            ts_ms: 1,
            status,
            latency_ms: 10,
            served_model: None,
            endpoint_ok: true,
            reasoning_tokens: 0,
            cost_ticks: 0,
            http: 200,
        }
    }

    fn tally(p: &mut Probation, ok: u32, bad: u32) {
        for _ in 0..ok {
            p.observe(&obs(CallStatus::Ok));
        }
        for _ in 0..bad {
            p.observe(&obs(CallStatus::Error));
        }
    }

    #[test]
    fn only_calls_about_the_model_count() {
        let mut p = Probation::start(0);
        for s in [
            CallStatus::ContentSafety,
            CallStatus::FreeUsageExhausted,
            CallStatus::QuotaExhausted,
            CallStatus::NoSubscription,
        ] {
            p.observe(&obs(s));
        }
        assert_eq!(p.calls, 0);
        p.observe(&obs(CallStatus::Pressure));
        p.observe(&obs(CallStatus::Ok));
        assert_eq!((p.calls, p.ok), (2, 1));
    }

    #[test]
    fn two_weeks_and_enough_good_calls_graduate_it() {
        let mut p = Probation::start(0);
        tally(&mut p, 19, 1);
        // Enough calls, but the two weeks aren't up.
        assert_eq!(p.judge(Quality::Unknown, 13 * DAY_MS), None);
        // Up, with no word on quality against it: it graduates.
        let v = p.judge(Quality::Unknown, 14 * DAY_MS).unwrap();
        assert_eq!(v.to, Stage::Graduated);
        assert!(!p.active());
        // Graduated stays graduated, and stops counting.
        tally(&mut p, 0, 50);
        assert_eq!((p.judge(Quality::Fell, 30 * DAY_MS), p.calls), (None, 20));
    }

    #[test]
    fn too_few_calls_keep_it_on_trial_however_long() {
        let mut p = Probation::start(0);
        tally(&mut p, 10, 0);
        assert_eq!(p.judge(Quality::Held, 60 * DAY_MS), None);
        assert_eq!(p.stage, Stage::Trial);
    }

    #[test]
    fn two_hundred_calls_that_held_up_graduate_it_early() {
        let mut p = Probation::start(0);
        tally(&mut p, 195, 5);
        // Without the learner's word that its answers held, it waits.
        assert_eq!(p.judge(Quality::Unknown, 3 * DAY_MS), None);
        assert_eq!(
            p.judge(Quality::Held, 3 * DAY_MS).map(|v| v.to),
            Some(Stage::Graduated)
        );
    }

    #[test]
    fn a_dead_an_unreliable_or_a_worse_model_fails_and_retries_two_weeks_on() {
        let mut dead = Probation::start(0);
        tally(&mut dead, 0, 5);
        assert_eq!(
            dead.judge(Quality::Unknown, DAY_MS).map(|v| v.to),
            Some(Stage::Failed)
        );
        let mut shaky = Probation::start(0);
        tally(&mut shaky, 17, 3);
        let v = shaky.judge(Quality::Unknown, DAY_MS).unwrap();
        assert_eq!(v.to, Stage::Failed);
        assert!(v.reason.contains("17 of 20"), "{}", v.reason);
        let mut worse = Probation::start(0);
        tally(&mut worse, 30, 0);
        assert_eq!(
            worse.judge(Quality::Fell, DAY_MS).map(|v| v.to),
            Some(Stage::Failed)
        );
        // Failed: still active (barred), and its retry comes two weeks on.
        assert!(worse.active());
        assert_eq!(worse.judge(Quality::Unknown, 14 * DAY_MS), None);
        let v = worse.judge(Quality::Unknown, 15 * DAY_MS).unwrap();
        assert_eq!(v.to, Stage::Trial);
        assert_eq!((worse.calls, worse.since_ms), (0, 15 * DAY_MS));
    }

    #[test]
    fn probes_count_and_are_tallied_per_day() {
        let mut p = Probation::start(0);
        p.probed(true, "2026-11-08");
        p.probed(false, "2026-11-08");
        assert_eq!((p.calls, p.ok, p.probes), (2, 1, 2));
        assert_eq!(p.probes_on("2026-11-08"), 2);
        assert_eq!(p.probes_on("2026-11-09"), 0);
        p.probed(true, "2026-11-09");
        assert_eq!(p.probes_on("2026-11-09"), 1);
    }
}
