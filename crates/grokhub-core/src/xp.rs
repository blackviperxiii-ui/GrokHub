//! Home's level and XP, worked out from what the user already did. Nothing
//! here writes a file: XP is read back from the outcome log (finished tasks)
//! and the card signal log (Home cards the user ran or saw through), so it
//! can't drift from what really happened.
//!
//! Card and active-day XP is capped per day so tapping cards over and over
//! can't farm levels.

use std::collections::{BTreeMap, BTreeSet};

use crate::card_signals::{CardEvent, CardSignal};
use crate::outcome::{OutcomeResult, TaskOutcome};

pub const XP_PER_TASK: u32 = 100;
pub const XP_PER_STEP: u32 = 25;
pub const XP_PER_LEVEL: u32 = 500;
/// A Home card whose action the user ran (an idea filed, an offer automated).
pub const XP_PER_CARD_RAN: u32 = 25;
/// A Home card whose work went on to finish.
pub const XP_PER_CARD_DONE: u32 = 50;
/// Each day the user did any of the above.
pub const XP_PER_ACTIVE_DAY: u32 = 10;
/// Most card and active-day XP one day can add.
pub const DAILY_BONUS_CAP: u32 = 150;

const DAY_MS: u64 = 86_400_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct XpTally {
    /// Tasks that finished and weren't undone or corrected.
    pub finished: usize,
    /// Card and active-day XP after the daily cap.
    pub bonus: u32,
    /// XP earned today (tasks, cards, the day itself), after the cap.
    pub today: u32,
}

/// A task counts once it finished, unless it was later undone or corrected
/// (that writes a newer failure line, and `read_outcomes` keeps the newest).
/// A pending task counts right away instead of a day later.
pub fn finished_tasks(outcomes: &[TaskOutcome]) -> usize {
    outcomes
        .iter()
        .filter(|o| o.result != OutcomeResult::Failure)
        .count()
}

pub fn tally(outcomes: &[TaskOutcome], signals: &[CardSignal], now_ms: u64) -> XpTally {
    let today = now_ms / DAY_MS;
    let finished = finished_tasks(outcomes);
    let tasks_today = outcomes
        .iter()
        .filter(|o| o.result != OutcomeResult::Failure && o.finished_at / DAY_MS == today)
        .count() as u32;

    let mut ran = BTreeSet::new();
    let mut done = BTreeSet::new();
    let mut per_day: BTreeMap<u64, u32> = BTreeMap::new();
    for s in signals {
        let points = match s.event {
            CardEvent::Ran if ran.insert(s.card_id.as_str()) => XP_PER_CARD_RAN,
            CardEvent::Completed if done.insert(s.card_id.as_str()) => XP_PER_CARD_DONE,
            _ => continue,
        };
        *per_day.entry(s.ts / DAY_MS).or_default() += points;
    }
    for o in outcomes
        .iter()
        .filter(|o| o.result != OutcomeResult::Failure)
    {
        per_day.entry(o.finished_at / DAY_MS).or_default();
    }

    let mut bonus = 0;
    let mut bonus_today = 0;
    for (day, cards) in per_day {
        let capped = (cards + XP_PER_ACTIVE_DAY).min(DAILY_BONUS_CAP);
        bonus += capped;
        if day == today {
            bonus_today = capped;
        }
    }
    XpTally {
        finished,
        bonus,
        today: tasks_today * XP_PER_TASK + bonus_today,
    }
}

/// Level, total XP and progress into the level.
pub fn level_xp(finished: usize, checked: usize, bonus: u32) -> (u32, u32, f32) {
    let xp = finished as u32 * XP_PER_TASK + checked as u32 * XP_PER_STEP + bonus;
    let level = xp / XP_PER_LEVEL + 1;
    let frac = (xp % XP_PER_LEVEL) as f32 / XP_PER_LEVEL as f32;
    (level, xp, frac)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = DAY_MS;

    fn task(id: &str, result: OutcomeResult, finished_at: u64) -> TaskOutcome {
        TaskOutcome {
            task: id.into(),
            result,
            finished_at,
            ..TaskOutcome::default()
        }
    }

    fn signal(card: &str, event: CardEvent, ts: u64) -> CardSignal {
        CardSignal {
            ts,
            card_id: card.into(),
            kind: "suggestion".into(),
            group: String::new(),
            source_id: String::new(),
            event,
            after_open: None,
        }
    }

    #[test]
    fn level_counts_tasks_steps_and_bonus() {
        assert_eq!(level_xp(0, 0, 0), (1, 0, 0.0));
        assert_eq!(level_xp(23, 2, 0), (5, 2350, 0.7));
        assert_eq!(level_xp(4, 0, 100), (2, 500, 0.0));
    }

    #[test]
    fn a_task_counts_at_once_and_an_undone_one_never() {
        let now = 10 * DAY + 5;
        let t = [
            task("a:1", OutcomeResult::Pending, now - 1),
            task("a:2", OutcomeResult::Success, now - 2),
            task("a:3", OutcomeResult::Failure, now - 3),
        ];
        let x = tally(&t, &[], now);
        assert_eq!(x.finished, 2);
        assert_eq!(x.bonus, XP_PER_ACTIVE_DAY);
        assert_eq!(x.today, 2 * XP_PER_TASK + XP_PER_ACTIVE_DAY);
    }

    #[test]
    fn cards_pay_once_each_and_a_day_is_capped() {
        let now = 3 * DAY + 100;
        let mut s = vec![
            signal("c1", CardEvent::Ran, now - 10),
            signal("c1", CardEvent::Ran, now - 9),
            signal("c1", CardEvent::Completed, now - 8),
            signal("c2", CardEvent::Opened, now - 7),
        ];
        assert_eq!(tally(&[], &s, now).bonus, 25 + 50 + 10);
        for i in 0..20 {
            s.push(signal(&format!("x{i}"), CardEvent::Ran, now - 5));
        }
        let x = tally(&[], &s, now);
        assert_eq!(x.bonus, DAILY_BONUS_CAP);
        assert_eq!(x.today, DAILY_BONUS_CAP);
    }

    #[test]
    fn each_active_day_adds_its_own_bonus() {
        let now = 9 * DAY;
        let t = [
            task("a:1", OutcomeResult::Success, 2 * DAY),
            task("a:2", OutcomeResult::Success, 2 * DAY + 1),
            task("a:3", OutcomeResult::Success, 5 * DAY),
        ];
        let x = tally(&t, &[], now);
        assert_eq!(x.bonus, 2 * XP_PER_ACTIVE_DAY);
        assert_eq!(x.today, 0);
    }
}
