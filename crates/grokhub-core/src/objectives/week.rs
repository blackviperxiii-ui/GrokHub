//! Monday's "Your week" on Objectives: one to three real changes from last
//! week (levels gained, goals done, the goal that moved most, weeks in a
//! row) and one suggested goal. A week with nothing to say shows nothing.

use serde::Serialize;

use super::{goal_title, Goal, GoalCard, GoalState, Objectives};
use crate::xp::{local_day, summarize, weekday_mon, XpLedger};

const MINUTE_MS: u64 = 60_000;
const DAY_MIN: i64 = 1440;
/// Lines the recap shows at most.
pub const WEEK_LINES: usize = 3;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct YourWeek {
    /// "Reached level 5 · Regular", "Goal done: Elden Ring · Finish",
    /// "Old School RuneScape · 99 fishing: 93 → 96", "3 weeks in a row".
    pub lines: Vec<String>,
    /// One goal from the suggested tray, to Track or leave.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<GoalCard>,
}

/// The last millisecond of a local day.
fn day_end_ms(day: i64, offset_min: i32) -> u64 {
    let minutes = (day + 1) * DAY_MIN - i64::from(offset_min);
    (minutes.max(0) as u64 * MINUTE_MS).saturating_sub(1)
}

/// The goal's reading at `at`, from its history.
fn value_at(g: &Goal, at: u64) -> Option<u32> {
    if g.created_ms > at {
        return None;
    }
    let read = g
        .history
        .iter()
        .rfind(|m| m.at <= at)
        .map_or(g.start, |m| m.value);
    // A first reading from game data moves the start up: count from there.
    Some(read.max(g.start))
}

/// Last week's recap, on Mondays only.
pub fn your_week(
    o: &Objectives,
    ledger: &XpLedger,
    now_ms: u64,
    offset_min: i32,
) -> Option<YourWeek> {
    let today = local_day(now_ms, offset_min);
    if weekday_mon(today) != 0 {
        return None;
    }
    let (first, last) = (today - 7, today - 1);
    let start_ms = day_end_ms(first - 1, offset_min) + 1;
    let end_ms = day_end_ms(last, offset_min);
    let before = summarize(ledger, start_ms - 1, offset_min);
    let after = summarize(ledger, end_ms, offset_min);
    let mut lines = Vec::new();

    let gained = after.level.saturating_sub(before.level);
    if gained == 1 {
        lines.push(format!("Reached level {} · {}", after.level, after.title));
    } else if gained > 1 {
        lines.push(format!("Up {gained} levels to level {}", after.level));
    }

    let done: Vec<&Goal> = o
        .goals
        .iter()
        .filter(|g| g.state == GoalState::Done)
        .filter(|g| {
            g.done_ms
                .is_some_and(|at| (start_ms..=end_ms).contains(&at))
        })
        .collect();
    match done.as_slice() {
        [] => {}
        [g] => lines.push(format!("Goal done: {}", goal_title(g))),
        many => lines.push(format!("{} goals done", many.len())),
    }

    let moved = o
        .goals
        .iter()
        .filter(|g| g.target > 1 && !done.iter().any(|d| d.id == g.id))
        .filter_map(|g| {
            let a = value_at(g, start_ms - 1).unwrap_or(g.start);
            let b = value_at(g, end_ms)?;
            let span = g.target.saturating_sub(g.start).max(1) as f32;
            (b > a).then(|| ((b - a) as f32 / span, g, a, b))
        })
        .max_by(|x, y| x.0.total_cmp(&y.0).then(y.1.id.cmp(&x.1.id)));
    if let Some((_, g, a, b)) = moved {
        lines.push(format!("{}: {a} → {b}", goal_title(g)));
    }

    if after.week.in_a_row >= 2 {
        lines.push(format!("{} weeks in a row", after.week.in_a_row));
    }

    if lines.is_empty() {
        return None;
    }
    lines.truncate(WEEK_LINES);
    Some(YourWeek {
        lines,
        suggestion: o.suggestions(now_ms).first().map(|g| GoalCard::of(g)),
    })
}

/// "Your week" from the saved goals and XP ledger, with the goals' own
/// payments folded in. Reads only.
pub fn week_from_disk(dir: &std::path::Path, now_ms: u64, offset_min: i32) -> Option<YourWeek> {
    let o = super::load(dir);
    let mut ledger = crate::xp::load_ledger(dir);
    ledger.ingest_goals(&o.goals, offset_min);
    your_week(&o, &ledger, now_ms, offset_min)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objectives::{parse_goal, KnownGame};

    /// Monday 2 Nov 2026, 12:00 UTC.
    const MON: u64 = 1_793_620_800_000;
    const DAY: u64 = 86_400_000;

    fn osrs() -> [KnownGame<'static>; 1] {
        [KnownGame {
            name: "Old School RuneScape",
            steam_app_id: None,
            source: Some("osrs"),
        }]
    }

    #[test]
    fn monday_shows_last_weeks_real_changes_and_one_suggestion() {
        let mut o = Objectives::default();
        let fish = o
            .add(parse_goal("99 fishing in osrs", &osrs()), MON - 20 * DAY)
            .unwrap();
        o.set_progress(&fish, 93, true, MON - 10 * DAY);
        let hades = o
            .add(parse_goal("finish hades", &[]), MON - 20 * DAY)
            .unwrap();
        // Last week: a goal done and four levels of fishing.
        o.done(&hades, MON - 3 * DAY);
        o.set_progress(&fish, 97, true, MON - 2 * DAY);
        // This week's progress isn't last week's.
        o.set_progress(&fish, 98, true, MON + 1);
        o.suggest(parse_goal("100% achievements in celeste", &[]), MON - DAY);
        let mut ledger = XpLedger::default();
        ledger.ingest_goals(&o.goals, 0);

        let w = your_week(&o, &ledger, MON, 0).expect("a recap");
        assert!(
            w.lines.contains(&"Goal done: Finish hades".to_string()),
            "{:?}",
            w.lines
        );
        assert!(w
            .lines
            .contains(&"Old School RuneScape · 99 fishing: 93 → 97".to_string()));
        assert!(w.lines.len() <= WEEK_LINES);
        assert!(w.suggestion.is_some());
        assert_eq!(your_week(&o, &ledger, MON + DAY, 0), None, "Mondays only");
    }

    #[test]
    fn a_quiet_week_shows_nothing() {
        let mut o = Objectives::default();
        o.add(parse_goal("finish hades", &[]), MON - 20 * DAY)
            .unwrap();
        assert_eq!(your_week(&o, &XpLedger::default(), MON, 0), None);
    }

    #[test]
    fn levels_gained_last_week_read_as_one_line() {
        let mut ledger = XpLedger::default();
        let day = local_day(MON - 3 * DAY, 0);
        ledger.earned.insert(
            "task:a".into(),
            crate::xp::Earned {
                day,
                xp: 300,
                kind: crate::xp::XpKind::Task,
            },
        );
        let w = your_week(&Objectives::default(), &ledger, MON, 0).expect("a recap");
        assert_eq!(w.lines[0], "Reached level 2 · Rookie");
    }
}
