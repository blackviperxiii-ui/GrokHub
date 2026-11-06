//! Seasons (calendar quarters) and the Today rows picked from the goals.

use super::{Goal, GoalState, Objectives};
use crate::xp::local_day;

const DAY_MS: u64 = 86_400_000;
/// A tracked goal untouched this long gets a "pick it up" row.
pub const STALE_MS: u64 = 7 * DAY_MS;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Days since 1 Jan 1970 → (year, month 1-12, day 1-31).
fn civil(day: i64) -> (i32, u32, u32) {
    // Howard Hinnant's civil_from_days.
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe as i64 + era * 400 + i64::from(m <= 2);
    (y as i32, m, d)
}

/// "Nov 2", for the trophy shelf.
pub fn short_date(ms: u64, offset_min: i32) -> String {
    let (_, m, d) = civil(local_day(ms, offset_min));
    format!("{} {d}", MONTHS[(m - 1) as usize])
}

/// The season a local day falls in: (year, quarter 0-3).
pub fn season_of(day: i64) -> (i32, u32) {
    let (y, m, _) = civil(day);
    (y, (m - 1) / 3)
}

/// "Oct to Dec 2026".
pub fn season_label(now_ms: u64, offset_min: i32) -> String {
    let (y, q) = season_of(local_day(now_ms, offset_min));
    let first = (q * 3) as usize;
    format!("{} to {} {y}", MONTHS[first], MONTHS[first + 2])
}

/// Goals done this season.
pub fn done_this_season(o: &Objectives, now_ms: u64, offset_min: i32) -> usize {
    let season = season_of(local_day(now_ms, offset_min));
    o.goals
        .iter()
        .filter(|g| g.state == GoalState::Done)
        .filter_map(|g| g.done_ms)
        .filter(|at| season_of(local_day(*at, offset_min)) == season)
        .count()
}

/// A Today row from the goals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// The tracked goal closest to done.
    Closest(String),
    /// A tracked goal untouched for a week: "Pick up Elden Ring tonight?"
    PickUp(String),
}

impl Pick {
    pub fn id(&self) -> &str {
        match self {
            Self::Closest(id) | Self::PickUp(id) => id,
        }
    }
}

/// "Pick up Elden Ring tonight?", or the goal's own words when it has no game.
pub fn pick_up_line(g: &Goal) -> String {
    let what = if g.game.name.is_empty() {
        g.text.as_str()
    } else {
        g.game.name.as_str()
    };
    format!("Pick up {what} tonight?")
}

/// Up to two rows: the goal closest to done, and the one left longest
/// (a week or more) if it's a different goal.
pub fn today_picks(o: &Objectives, now_ms: u64) -> Vec<Pick> {
    let tracking = o.tracking();
    let closest = tracking
        .iter()
        .max_by(|a, b| a.frac().total_cmp(&b.frac()).then(b.id.cmp(&a.id)));
    let stale = tracking
        .iter()
        .filter(|g| g.updated_ms + STALE_MS <= now_ms)
        .min_by_key(|g| (g.updated_ms, g.id.clone()));
    let mut out = Vec::new();
    if let Some(g) = closest {
        out.push(Pick::Closest(g.id.clone()));
    }
    if let Some(g) = stale {
        if closest.is_none_or(|c| c.id != g.id) {
            out.push(Pick::PickUp(g.id.clone()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::{GameRef, GoalKind, NewGoal, Origin};
    use super::*;

    /// Monday 2 Nov 2026, 12:00 UTC.
    const NOW: u64 = 1_793_620_800_000;

    fn goal(
        o: &mut Objectives,
        game: &str,
        text: &str,
        current: u32,
        target: u32,
        at: u64,
    ) -> String {
        o.add(
            NewGoal {
                game: GameRef {
                    name: game.into(),
                    ..GameRef::default()
                },
                text: text.into(),
                kind: GoalKind::Count,
                current,
                target,
                origin: Origin::Typed,
                chat: None,
            },
            at,
        )
        .unwrap()
    }

    #[test]
    fn seasons_are_calendar_quarters() {
        assert_eq!(season_label(NOW, 0), "Oct to Dec 2026");
        assert_eq!(season_of(0), (1970, 0));
        // 31 Dec 2026 and 1 Jan 2027.
        assert_eq!(season_of(20_818), (2026, 3));
        assert_eq!(season_of(20_819), (2027, 0));
        // 29 Feb 2024 sits in the first quarter.
        assert_eq!(season_of(19_782), (2024, 0));
        assert_eq!(season_label(NOW - 40 * DAY_MS, 0), "Jul to Sep 2026");
        assert_eq!(short_date(NOW, 0), "Nov 2");
        // 11pm in New York on 1 Nov is still the 1st there.
        assert_eq!(short_date(NOW - 14 * 3_600_000, -300), "Nov 1");
    }

    #[test]
    fn done_this_season_counts_only_this_quarter() {
        let mut o = Objectives::default();
        let old = goal(&mut o, "A", "old", 0, 1, NOW - 60 * DAY_MS);
        let new = goal(&mut o, "B", "new", 0, 1, NOW);
        o.done(&old, NOW - 60 * DAY_MS);
        o.done(&new, NOW);
        assert_eq!(done_this_season(&o, NOW, 0), 1);
    }

    #[test]
    fn today_picks_the_closest_goal_and_one_left_a_week() {
        let mut o = Objectives::default();
        assert!(today_picks(&o, NOW).is_empty());
        let near = goal(&mut o, "Hades", "Clear 10 runs", 8, 10, NOW);
        let old = goal(
            &mut o,
            "Elden Ring",
            "Win 20 duels",
            2,
            20,
            NOW - 8 * DAY_MS,
        );
        let picks = today_picks(&o, NOW);
        assert_eq!(
            picks,
            [Pick::Closest(near.clone()), Pick::PickUp(old.clone())]
        );
        let g = o.goals.iter().find(|g| g.id == old).unwrap();
        assert_eq!(pick_up_line(g), "Pick up Elden Ring tonight?");
        // Moving the old goal clears its pick-up row.
        o.bump(&old, NOW);
        assert_eq!(today_picks(&o, NOW), [Pick::Closest(near)]);
    }

    #[test]
    fn one_goal_is_never_picked_twice() {
        let mut o = Objectives::default();
        let only = goal(&mut o, "", "Learn to parry", 0, 5, NOW - 9 * DAY_MS);
        assert_eq!(today_picks(&o, NOW), [Pick::Closest(only.clone())]);
        let g = &o.goals[0];
        assert_eq!(pick_up_line(g), "Pick up Learn to parry tonight?");
        assert_eq!(Pick::PickUp(only.clone()).id(), only);
    }
}
