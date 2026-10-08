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
}
