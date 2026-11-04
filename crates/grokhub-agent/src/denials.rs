//! Denial circuit breaker (after Codex auto-review): when the person, a rule
//! or the Auto judge keeps saying no, the model is likely trying small
//! variations of the same step. It counts the model's replies, since every
//! call in one reply was written before any answer came back. Three replies
//! in a row with a denied step, or six of the last twenty, end the turn with
//! a note not to work around them.

use std::collections::VecDeque;

const IN_A_ROW: usize = 3;
const WINDOW: usize = 20;
const IN_WINDOW: usize = 6;

/// The note the model sees on the denial that trips the breaker.
pub const DONT_WORK_AROUND: &str = "Several of your steps were denied, so this turn ends here. \
Don't try to get around a denial with a different command, tool or path. Say what you wanted \
to do and why, and let the person decide.";

/// What the person reads when the breaker ends the turn.
pub fn stop_note(in_a_row: bool) -> String {
    if in_a_row {
        "I stopped here: my steps were denied three times in a row, so I won't look for another \
         way around them. Tell me how you'd like to go on."
            .to_string()
    } else {
        "I stopped here: too many of my steps this turn were denied, so I won't look for \
         another way around them. Tell me how you'd like to go on."
            .to_string()
    }
}

/// Whether a refusal counts as a denial: a person's no, a deny rule, the
/// judge, or a hard-class or floor stop. A read-only phase or an unknown
/// tool name is a mistake to correct, not a no.
pub fn is_denial(refusal: &str) -> bool {
    !(refusal.contains(crate::gate::READ_ONLY_PHASE) || refusal.starts_with("unknown tool `"))
}

#[derive(Debug, Default)]
pub struct Breaker {
    streak: usize,
    recent: VecDeque<bool>,
}

/// Why the breaker tripped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trip {
    InARow,
    InWindow,
}

impl Breaker {
    /// Record one reply: `denied` when any of its calls was refused. Returns a trip
    /// the first time a limit is reached.
    pub fn record(&mut self, denied: bool) -> Option<Trip> {
        self.streak = if denied { self.streak + 1 } else { 0 };
        self.recent.push_back(denied);
        if self.recent.len() > WINDOW {
            self.recent.pop_front();
        }
        if !denied {
            return None;
        }
        if self.streak >= IN_A_ROW {
            return Some(Trip::InARow);
        }
        if self.recent.iter().filter(|d| **d).count() >= IN_WINDOW {
            return Some(Trip::InWindow);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_in_a_row_or_ten_in_fifty_trip() {
        let mut b = Breaker::default();
        assert_eq!(b.record(true), None);
        assert_eq!(b.record(true), None);
        assert_eq!(b.record(false), None, "a run step resets the streak");
        assert_eq!(b.record(true), None);
        assert_eq!(b.record(true), None);
        assert_eq!(b.record(true), Some(Trip::InARow));

        let mut b = Breaker::default();
        let mut trip = None;
        for i in 0..20 {
            trip = b.record(i % 3 == 0).or(trip);
        }
        assert_eq!(trip, Some(Trip::InWindow), "every third reply denied");
    }

    #[test]
    fn mistakes_are_not_denials() {
        assert!(!is_denial(&crate::gate::readonly_refusal("write")));
        assert!(!is_denial("unknown tool `wrtie`"));
        assert!(is_denial(&crate::gate::user_rejected("write")));
        assert!(is_denial(&crate::gate::unattended_deny("write")));
    }
}
