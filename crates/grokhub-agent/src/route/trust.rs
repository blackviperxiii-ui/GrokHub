//! The router's tuner under the decision engine.
//!
//! The tuner keeps its own trial: shadow, then a canary on a share of real
//! steps ([`super::tune::judge`]). When the canary says promote, the change
//! is one [`Proposal`] for the Router area, so the user's trust dial, Halt,
//! quiet hours, the watchdog's pause and the day's budget hold it like any
//! other change the app makes on its own. What became of it (kept, undone,
//! rolled back) goes back as an [`Outcome`], which feeds the scoreboard.

use grokhub_core::learn::decide::{Alarm, Area, Decision, Goal, Outcome, Proposal};
use grokhub_core::learn::{hub, Subject, Undo};

use super::tune::{Candidate, Change, Evidence};

/// Who weighs the tuner's changes. The running app passes [`Hub`].
pub trait Trust {
    /// `None` when no decision engine is running: the tuner's own bar stands.
    fn weigh(&self, p: &Proposal, now_ms: u64) -> Option<Decision>;
    fn outcome(&self, rule: &Subject, outcome: Outcome, now_ms: u64) -> Option<Alarm>;
}

/// The decision engine the running app shares.
pub struct Hub;

impl Trust for Hub {
    fn weigh(&self, p: &Proposal, now_ms: u64) -> Option<Decision> {
        hub::decide_all(vec![p.clone()], hub::now().situation(), now_ms)
            .into_iter()
            .next()
    }

    fn outcome(&self, rule: &Subject, outcome: Outcome, now_ms: u64) -> Option<Alarm> {
        hub::decision_outcome(rule, outcome, now_ms)
    }
}

/// The rule a change belongs to: one per class and kind of change
/// (`rule:router/chat:default/order`), so two undos turn off that kind of
/// change for that class and no other.
pub fn rule(class: &str, change: &Change) -> Option<Subject> {
    let kind = match change {
        Change::Order { .. } => "order",
        Change::Start { .. } => "start",
    };
    Subject::new("rule", &format!("router/{class}/{kind}"))
}

/// A promotion the canary passed, as the decision engine weighs it. The
/// ledger's Undo puts the tuning file back, so it is a one-click change, and
/// it stays inside the spend rules the candidate was filtered by, so it is
/// not on the always-ask list.
pub fn promotion(
    c: &Candidate,
    ev: &Evidence,
    cost_gain_pct: f64,
    latency_gain_pct: f64,
) -> Option<Proposal> {
    let rule = rule(&c.class, c.change.as_ref()?)?;
    let (gain, helps, hurts) = if c.cost_rise_pct.is_some() {
        (
            ev.canary.pass_pct - ev.live.pass_pct,
            Goal::BestQuality,
            Some(Goal::Cheapest),
        )
    } else if cost_gain_pct >= latency_gain_pct {
        (cost_gain_pct, Goal::Cheapest, None)
    } else {
        (latency_gain_pct, Goal::Fastest, None)
    };
    Some(Proposal {
        area: Area::Router,
        rule,
        // The canary's verdict is the tuner's own bar: enough real steps,
        // a clear gain, quality held.
        confidence: 1.0,
        evidence: ev.canary.n as f32,
        undo: Undo::OneClick,
        value: (gain / 100.0).clamp(0.0, 1.0) as f32,
        hard: false,
        helps: Some(helps),
        hurts,
        tested: true,
    })
}
