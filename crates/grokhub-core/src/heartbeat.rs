//! Cabin pulse. Every 15s the organs that give the app autonomy run.

pub const HEARTBEAT_MS: u64 = 15_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeartbeatAct {
    Housekeep,
    Inbox,
    Night,
    Review,
    Wall,
    MidThought,
    Reflect,
    Anticipate,
}

/// Every organ runs. The cabin does not throttle the pulse.
pub fn heartbeat_acts() -> Vec<HeartbeatAct> {
    vec![
        HeartbeatAct::Housekeep,
        HeartbeatAct::Inbox,
        HeartbeatAct::Night,
        HeartbeatAct::Review,
        HeartbeatAct::Wall,
        HeartbeatAct::MidThought,
        HeartbeatAct::Reflect,
        HeartbeatAct::Anticipate,
    ]
}

pub fn heartbeat_due(elapsed_ms: u64, period_ms: u64) -> bool {
    elapsed_ms >= period_ms
}

pub fn next_heartbeat_wait_ms(elapsed_ms: u64, period_ms: u64) -> u64 {
    period_ms.saturating_sub(elapsed_ms).max(1)
}

pub fn heartbeat_repaint_ms(live: bool, hidden: bool, wait_ms: u64, hidden_ms: u64) -> u64 {
    let _ = (hidden, hidden_ms);
    if live {
        80
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
                HeartbeatAct::Night,
                HeartbeatAct::Review,
                HeartbeatAct::Wall,
                HeartbeatAct::MidThought,
                HeartbeatAct::Reflect,
                HeartbeatAct::Anticipate,
            ]
        );
    }

    #[test]
    fn idle_cabin_wakes_for_the_pulse() {
        assert_eq!(heartbeat_repaint_ms(true, false, 15_000, 400), 80);
        assert_eq!(heartbeat_repaint_ms(true, true, 15_000, 400), 80);
        assert_eq!(heartbeat_repaint_ms(false, true, 15_000, 400), 15_000);
        assert_eq!(heartbeat_repaint_ms(false, true, 200, 400), 200);
        assert_eq!(heartbeat_repaint_ms(false, false, 15_000, 400), 15_000);
    }
}
