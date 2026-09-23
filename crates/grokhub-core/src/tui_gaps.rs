//! Cabin chrome for the TUI gaps lock (v1.1).
//!
//! The btw pill keeps the persisted session id `ask`. Behavior is a side ask:
//! a live run is not cancelled; the question waits, then sends look-safe.

/// Composer label for `SessionMode::Ask`. `app.json` still stores `ask`.
pub const BTW_LABEL: &str = "btw";

pub const BTW_TIP_TITLE: &str = "btw";

/// Hover body. Kept under the composer tip cap (160).
pub const BTW_TIP_BODY: &str = "Side ask while a run is live — it waits and does not stop that run. Idle sends the same look-safe ask. /btw.";

/// Queue a side ask instead of halting when btw is selected and a turn is running.
pub fn btw_queues_without_interrupt(session_is_ask: bool, running: bool) -> bool {
    session_is_ask && running
}

/// User/assistant turns before a fork is offered for length.
pub const FORK_TURN_MIN: usize = 12;

/// Offer a fork when the thread is long, or context use is at least half `budget`.
///
/// Divergent-topic detection is omitted (weak signal). No stream event names a
/// fork, so that cue is omitted too.
pub fn fork_offer_why(turns: usize, tokens: u32, budget: u32) -> Option<&'static str> {
    if turns >= FORK_TURN_MIN {
        return Some("Long thread — fork keeps this line of work");
    }
    if budget > 0 && u64::from(tokens) * 2 >= u64::from(budget) {
        return Some("Context is half full — fork keeps this line of work");
    }
    None
}

pub const FORK_EXPLAINER: &str = "Fork copies this Grok session into a new chat. The next send starts from this history and leaves the original alone.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn btw_queues_only_while_a_run_is_live() {
        assert!(btw_queues_without_interrupt(true, true));
        assert!(!btw_queues_without_interrupt(true, false));
        assert!(!btw_queues_without_interrupt(false, true));
        assert!(!btw_queues_without_interrupt(false, false));
        assert!(BTW_TIP_BODY.len() < 160, "{}", BTW_TIP_BODY.len());
        assert!(!BTW_TIP_BODY.contains("Ask"));
        assert_eq!(BTW_LABEL, "btw");
    }

    #[test]
    fn fork_offer_uses_length_then_half_context() {
        assert!(fork_offer_why(11, 100, 96_000).is_none());
        let long = fork_offer_why(FORK_TURN_MIN, 100, 96_000).unwrap();
        assert!(long.contains("Long thread"), "{long}");
        let half = fork_offer_why(3, 48_000, 96_000).unwrap();
        assert!(half.contains("half full"), "{half}");
        assert!(fork_offer_why(3, 47_999, 96_000).is_none());
        assert!(fork_offer_why(0, 0, 0).is_none());
        assert!(!FORK_EXPLAINER.is_empty());
    }
}
