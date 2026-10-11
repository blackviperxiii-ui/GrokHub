//! Cabin pulse. Every 15s the organs that give the app autonomy run. The pulse
//! is also the background governor's clock (`crate::governor`).

pub const HEARTBEAT_MS: u64 = 15_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeartbeatAct {
    Housekeep,
    Inbox,
    /// The governor's queue (`crate::governor`): every background job that is
    /// due this beat, highest priority first. The governor holds the ones Halt,
    /// quiet hours or a spent budget stop.
    Background,
    Wall,
    MidThought,
}

impl HeartbeatAct {
    /// After Halt, only local upkeep with no model call and nothing new started:
    /// Housekeep (persist, day roll), MidThought, and the governor's queue,
    /// which runs only its jobs registered to run while halted.
    pub fn runs_while_halted(self) -> bool {
        matches!(self, Self::Housekeep | Self::Background | Self::MidThought)
    }
}

/// Every organ wakes each pulse. Background work runs only from the governor's
/// queue; the proactive acts in it (anticipate, the ideas ask, the nightly
/// review) also spend from `crate::heartbeat_throttle` before they act.
pub fn heartbeat_acts() -> Vec<HeartbeatAct> {
    vec![
        HeartbeatAct::Housekeep,
        HeartbeatAct::Inbox,
        HeartbeatAct::Background,
        HeartbeatAct::Wall,
        HeartbeatAct::MidThought,
    ]
}

pub fn heartbeat_due(elapsed_ms: u64, period_ms: u64) -> bool {
    elapsed_ms >= period_ms
}

pub fn next_heartbeat_wait_ms(elapsed_ms: u64, period_ms: u64) -> u64 {
    period_ms.saturating_sub(elapsed_ms).max(1)
}

pub fn heartbeat_repaint_ms(live: bool, hidden: bool, wait_ms: u64, hidden_ms: u64) -> u64 {
    if live {
        80
    } else if hidden {
        hidden_ms.min(wait_ms)
    } else {
        wait_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulse_is_fifteen_seconds() {
        assert_eq!(HEARTBEAT_MS, 15_000);
        assert!(!heartbeat_due(0, HEARTBEAT_MS));
        assert!(!heartbeat_due(14_999, HEARTBEAT_MS));
        assert!(heartbeat_due(15_000, HEARTBEAT_MS));
        assert!(heartbeat_due(16_000, HEARTBEAT_MS));
        assert_eq!(next_heartbeat_wait_ms(0, HEARTBEAT_MS), 15_000);
        assert_eq!(next_heartbeat_wait_ms(14_000, HEARTBEAT_MS), 1_000);
        assert_eq!(next_heartbeat_wait_ms(15_000, HEARTBEAT_MS), 1);
    }

    #[test]
    fn pulse_always_wakes_every_organ() {
        let acts = heartbeat_acts();
        assert_eq!(
            acts,
            vec![
                HeartbeatAct::Housekeep,
                HeartbeatAct::Inbox,
                HeartbeatAct::Background,
                HeartbeatAct::Wall,
                HeartbeatAct::MidThought,
            ]
        );
    }

    #[test]
    fn halt_keeps_only_local_upkeep() {
        let kept: Vec<HeartbeatAct> = heartbeat_acts()
            .into_iter()
            .filter(|a| a.runs_while_halted())
            .collect();
        assert_eq!(
            kept,
            vec![
                HeartbeatAct::Housekeep,
                HeartbeatAct::Background,
                HeartbeatAct::MidThought
            ]
        );
        assert!(!HeartbeatAct::Inbox.runs_while_halted());
        assert!(!HeartbeatAct::Wall.runs_while_halted());
    }

    #[test]
    fn idle_cabin_wakes_for_the_pulse() {
        assert_eq!(heartbeat_repaint_ms(true, false, 15_000, 400), 80);
        assert_eq!(heartbeat_repaint_ms(true, true, 15_000, 400), 80);
        assert_eq!(
            heartbeat_repaint_ms(false, true, 15_000, 400),
            400,
            "a hidden cabin must poll tray Quit / Show, not sleep the 15s pulse"
        );
        assert_eq!(heartbeat_repaint_ms(false, true, 200, 400), 200);
        assert_eq!(heartbeat_repaint_ms(false, false, 15_000, 400), 15_000);
    }
}
