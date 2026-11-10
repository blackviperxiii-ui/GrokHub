//! What auto setup reads from the beliefs.
//!
//! Auto setup's rules take a [`Usage`]: a few counts of how the app has been
//! used. Both apps fill it from here, so they offer the same changes from
//! the same evidence. A count the engine has no signals for stays at zero
//! and its rule stays quiet.
//!
//! It is also where auto setup's plan meets the decision engine
//! ([`weigh`]): the Settings trust dial, Halt, quiet hours, a busy moment
//! and the watchdog's pause hold a change auto setup would make on its own.

use super::belief::{Beliefs, Shape, Standing};
use super::decide::{Area, Decision, Proposal, Verdict};
use super::ladder::{Undo, ACT_AT, SUGGEST_AT};
use super::signal::Subject;
use crate::autotune::plan::{Change, Moment, Plan};
use crate::autotune::rules::Usage;

/// Picks below this, after fading, no longer break a run of passed-over rows.
const PICK_FLOOR: f32 = 1.0;

/// Rows of quick chips passed over with no pick to speak of, across every
/// place chips show (`chips:composer`, `chips:home`, …). One pick resets it
/// until that pick has faded, about a month. A place the user forgot or
/// marked wrong counts for nothing.
pub fn chips_ignored_streak(beliefs: &Beliefs, now_ms: u64) -> u32 {
    let (mut picked, mut passed) = (0.0f32, 0.0f32);
    for (subject, belief) in beliefs.iter() {
        if subject.area() != "chips" || belief.standing != Standing::Live {
            continue;
        }
        if let Shape::Rate { yes, no } = belief.shape {
            let total = yes + no;
            if total > 0.0 {
                let left = belief.evidence(now_ms);
                picked += left * yes / total;
                passed += left * no / total;
            }
        }
    }
    if picked >= PICK_FLOOR {
        0
    } else {
        passed as u32
    }
}

/// The usage counts the engine can vouch for. The caller adds the ones it
/// owns (the rhythm's quiet hours).
pub fn usage(beliefs: &Beliefs, now_ms: u64) -> Usage {
    Usage {
        chips_ignored_streak: chips_ignored_streak(beliefs, now_ms),
        ..Usage::default()
    }
}

/// The decision engine's rule for one of auto setup's rules
/// (`rule:settings/os_text_size`).
pub fn rule(rule: &str) -> Option<Subject> {
    Subject::new("rule", &format!("settings/{rule}"))
}

fn proposal(c: &Change, confidence: f32) -> Option<Proposal> {
    Some(Proposal {
        area: Area::Settings,
        rule: rule(c.rule)?,
        confidence,
        evidence: 1.0,
        // Auto setup keeps what was there before and puts it back.
        undo: Undo::OneClick,
        value: confidence,
        hard: false,
        helps: None,
        hurts: None,
        // A built-in rule that reads the PC, not a habit that is still
        // being learned: it does not wait for three yes answers.
        tested: true,
    })
}

/// Auto setup's plan after the decision engine weighed it. A change auto
/// setup would make on its own stays (act), becomes a suggestion (offer) or
/// is left for a later pass (wait, never); a suggestion is dropped while its
/// area only watches. The first-run setup and the user's own answers are not
/// weighed. `weigh` returns `None` when no decision engine is running, and
/// the plan stands.
pub fn weigh(
    plan: Plan,
    moment: Moment,
    weigh: &mut dyn FnMut(&Proposal) -> Option<Decision>,
) -> Plan {
    if moment == Moment::FirstRun {
        return plan;
    }
    let mut out = Plan {
        took_over: plan.took_over,
        ..Plan::default()
    };
    let mut verdict = |c: &Change, confidence: f32| {
        if c.asked {
            return None;
        }
        proposal(c, confidence)
            .and_then(|p| weigh(&p))
            .map(|d| d.verdict)
    };
    for c in plan.apply {
        match verdict(&c, ACT_AT) {
            None | Some(Verdict::Act) => out.apply.push(c),
            Some(Verdict::Offer) => out.suggest.push(c),
            Some(Verdict::Wait | Verdict::Never) => {}
        }
    }
    for c in plan.suggest {
        match verdict(&c, SUGGEST_AT) {
            Some(Verdict::Wait | Verdict::Never) => {}
            _ => out.suggest.push(c),
        }
    }
    out
}
