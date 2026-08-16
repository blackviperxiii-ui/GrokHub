//! Cabin pulse. Every 15s the organs that give the app autonomy run.

pub const HEARTBEAT_MS: u64 = 15_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeartbeatAct {
    Housekeep,
    Inbox,
    Night,
    Wall,
    MidThought,
    Reflect,
    Anticipate,
}

/// User-owned work (inbox, night) always moves. Higher autonomy wakes more organs.
pub fn heartbeat_acts(autonomy: u8) -> Vec<HeartbeatAct> {
    let mut out = vec![
        HeartbeatAct::Housekeep,
        HeartbeatAct::Inbox,
        HeartbeatAct::Night,
    ];
    if autonomy >= 2 {
        out.push(HeartbeatAct::Wall);
        out.push(HeartbeatAct::MidThought);
    }
    if autonomy >= 3 {
        out.push(HeartbeatAct::Reflect);
    }
    if autonomy >= 4 {
        out.push(HeartbeatAct::Anticipate);
    }
    out
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
        wait_ms.min(hidden_ms)
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
    fn autonomy_wakes_organs_in_order() {
        assert_eq!(
            heartbeat_acts(0),
            vec![
                HeartbeatAct::Housekeep,
                HeartbeatAct::Inbox,
                HeartbeatAct::Night
            ]
        );
        assert_eq!(heartbeat_acts(1), heartbeat_acts(0));
        let two = heartbeat_acts(2);
        assert!(two.contains(&HeartbeatAct::Wall));
        assert!(two.contains(&HeartbeatAct::MidThought));
        assert!(!two.contains(&HeartbeatAct::Reflect));
        let three = heartbeat_acts(3);
        assert!(three.contains(&HeartbeatAct::Reflect));
        assert!(!three.contains(&HeartbeatAct::Anticipate));
        let four = heartbeat_acts(4);
        assert!(four.contains(&HeartbeatAct::Reflect));
        assert!(four.contains(&HeartbeatAct::Anticipate));
    }

    #[test]
    fn idle_cabin_wakes_for_the_pulse() {
        assert_eq!(heartbeat_repaint_ms(true, false, 15_000, 400), 80);
        assert_eq!(heartbeat_repaint_ms(false, true, 15_000, 400), 400);
        assert_eq!(heartbeat_repaint_ms(false, true, 200, 400), 200);
        assert_eq!(heartbeat_repaint_ms(false, false, 15_000, 400), 15_000);
    }
}
