//! What auto setup reads from the beliefs.
//!
//! Auto setup's rules take a [`Usage`]: a few counts of how the app has been
//! used. Both apps fill it from here, so they offer the same changes from
//! the same evidence. A count the engine has no signals for stays at zero
//! and its rule stays quiet.

use super::belief::{Beliefs, Shape, Standing};
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
