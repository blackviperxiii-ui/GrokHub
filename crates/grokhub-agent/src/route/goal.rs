//! Brain 2: the goal the Brain hands the router for a turn (speed, quality,
//! cost or balanced) and the one-line reason from the policy row. The goal
//! moves the starting effort inside the class band only; it never leaves the
//! band, never prompts, and the user's own words ("think hard", "keep it
//! quick") win over it. Escalation on failure (E1/E2) and drop-back (DE1)
//! still move the rung from there.

use std::collections::BTreeMap;
use std::sync::Mutex;

use super::ladder::{high, Band, Steer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Goal {
    Speed,
    Quality,
    Cost,
    Balanced,
}

impl Goal {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "speed" => Some(Self::Speed),
            "quality" => Some(Self::Quality),
            "cost" => Some(Self::Cost),
            "balanced" => Some(Self::Balanced),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Speed => "speed",
            Self::Quality => "quality",
            Self::Cost => "cost",
            Self::Balanced => "balanced",
        }
    }
}

/// The goal and the policy row's reason ("mistakes can break the system").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteGoal {
    pub goal: Goal,
    pub reason: String,
}

impl RouteGoal {
    /// From a policy row's goal word and reason; `None` for an unknown goal.
    pub fn of(goal: &str, reason: &str) -> Option<Self> {
        Goal::parse(goal).map(|goal| Self { goal, reason: reason.trim().to_string() })
    }

    /// "goal quality (mistakes can break the system)"
    pub fn label(&self) -> String {
        if self.reason.is_empty() {
            format!("goal {}", self.goal.as_str())
        } else {
            format!("goal {} ({})", self.goal.as_str(), self.reason)
        }
    }
}

/// Sessions remembered at once; past this the map starts over.
const GOALS_CAP: usize = 256;

static GOALS: Mutex<BTreeMap<String, RouteGoal>> = Mutex::new(BTreeMap::new());

/// Brain 4: goals the cabin pins on a background run's session.
static PINNED: Mutex<BTreeMap<String, RouteGoal>> = Mutex::new(BTreeMap::new());

fn put(map: &Mutex<BTreeMap<String, RouteGoal>>, session: &str, goal: Option<RouteGoal>) {
    if session.is_empty() {
        return;
    }
    let mut g = map.lock().unwrap_or_else(|e| e.into_inner());
    let Some(goal) = goal else {
        g.remove(session);
        return;
    };
    if g.len() >= GOALS_CAP && !g.contains_key(session) {
        g.clear();
    }
    g.insert(session.to_string(), goal);
}

/// The turn's goal for a chat session, set when its Brief is written. `None`
/// (no policy row matched) clears the last turn's goal.
pub fn set_goal(session: &str, goal: Option<RouteGoal>) {
    put(&GOALS, session, goal);
}

/// Brain 4: the governor's goal for one background run (cost, "user active",
/// while you're active). It wins over the Brief's row for that session until
/// the run clears it with `None`.
pub fn pin_goal(session: &str, goal: Option<RouteGoal>) {
    put(&PINNED, session, goal);
}

/// The pinned goal of the first of `keys` that has one (Brain 4 background runs).
pub fn pinned_for(keys: &[&str]) -> Option<RouteGoal> {
    let pinned = PINNED.lock().unwrap_or_else(|e| e.into_inner());
    keys.iter().filter(|k| !k.is_empty()).find_map(|k| pinned.get(*k).cloned())
}

/// The goal of the first of `keys` that has one; a pinned goal wins for its key.
pub fn goal_for(keys: &[&str]) -> Option<RouteGoal> {
    let pinned = PINNED.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let g = GOALS.lock().unwrap_or_else(|e| e.into_inner());
    keys.iter()
        .filter(|k| !k.is_empty())
        .find_map(|k| pinned.get(*k).or_else(|| g.get(*k)).cloned())
}

/// Move a user-facing step's starting rung for the goal. `start` is
/// [`super::ladder::start_rung`]'s pick; the user's steer leaves it alone.
/// Quality starts one rung above the class start (capped at High and the
/// ceiling), speed one below (never under the floor), cost at the floor.
pub fn goal_start(band: Band, start: (usize, Vec<String>), steer: Steer, goal: Option<&RouteGoal>) -> (usize, Vec<String>) {
    let (rung, mut rules) = start;
    let Some(g) = goal.filter(|_| steer == Steer::None) else {
        return (rung, rules);
    };
    let moved = match g.goal {
        Goal::Quality => rung.max((band.start + 1).min(band.ceiling.min(high()).max(band.start))),
        Goal::Speed => rung.min(band.start.saturating_sub(1).max(band.floor)),
        Goal::Cost => band.floor,
        Goal::Balanced => rung,
    };
    if moved != rung {
        rules.push(format!("goal:{}", g.goal.as_str()));
    }
    (moved, rules)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn band() -> Band {
        Band { floor: 1, start: 3, ceiling: 5 }
    }

    fn goal(g: &str) -> RouteGoal {
        RouteGoal::of(g, "why").unwrap()
    }

    #[test]
    fn each_goal_moves_the_start_inside_the_band_and_the_users_words_win() {
        let b = band();
        assert_eq!(goal_start(b, (3, vec![]), Steer::None, Some(&goal("quality"))), (4, vec!["goal:quality".to_string()]));
        assert_eq!(goal_start(b, (3, vec![]), Steer::None, Some(&goal("speed"))), (2, vec!["goal:speed".to_string()]));
        assert_eq!(goal_start(b, (3, vec![]), Steer::None, Some(&goal("cost"))), (1, vec!["goal:cost".to_string()]));
        assert_eq!(goal_start(b, (3, vec![]), Steer::None, Some(&goal("balanced"))), (3, vec![]));
        assert_eq!(goal_start(b, (3, vec![]), Steer::None, None), (3, vec![]));
        // d>0.7 already raised it: quality doesn't stack a second rung.
        assert_eq!(goal_start(b, (4, vec!["d>0.7".into()]), Steer::None, Some(&goal("quality"))), (4, vec!["d>0.7".to_string()]));
        // "think hard" / "keep it quick" decide on their own.
        assert_eq!(goal_start(b, (5, vec!["steer:harder".into()]), Steer::Harder, Some(&goal("cost"))), (5, vec!["steer:harder".to_string()]));
        assert_eq!(goal_start(b, (1, vec!["steer:quicker".into()]), Steer::Quicker, Some(&goal("quality"))), (1, vec!["steer:quicker".to_string()]));
        // A narrow band: never past the ceiling or under the floor.
        let narrow = Band { floor: 2, start: 2, ceiling: 2 };
        assert_eq!(goal_start(narrow, (2, vec![]), Steer::None, Some(&goal("quality"))), (2, vec![]));
        assert_eq!(goal_start(narrow, (2, vec![]), Steer::None, Some(&goal("speed"))), (2, vec![]));
    }

    #[test]
    fn goals_are_kept_per_session_and_unknown_words_are_refused() {
        assert_eq!(RouteGoal::of("fastest", "x"), None);
        let g = RouteGoal::of("Quality", " mistakes can break the system ").unwrap();
        assert_eq!(g.label(), "goal quality (mistakes can break the system)");
        set_goal("goal-test-a", Some(g.clone()));
        set_goal("", Some(goal("cost")));
        assert_eq!(goal_for(&["", "goal-test-missing", "goal-test-a"]), Some(g));
        assert_eq!(goal_for(&["goal-test-missing"]), None);
        // A turn with no policy row clears the last turn's goal.
        set_goal("goal-test-a", None);
        assert_eq!(goal_for(&["goal-test-a"]), None);
    }

    #[test]
    fn a_pinned_goal_wins_over_the_briefs_row_until_it_is_cleared() {
        let active = RouteGoal::of("cost", "user active").unwrap();
        let row = RouteGoal::of("cost", "user isn't waiting").unwrap();
        pin_goal("goal-test-pin", Some(active.clone()));
        set_goal("goal-test-pin", Some(row.clone()));
        assert_eq!(goal_for(&["goal-test-pin"]).map(|g| g.label()), Some("goal cost (user active)".to_string()));
        pin_goal("", Some(active));
        assert_eq!(pinned_for(&["", "goal-test-pin"]).map(|g| g.reason), Some("user active".to_string()));
        pin_goal("goal-test-pin", None);
        assert_eq!(pinned_for(&["goal-test-pin"]), None);
        assert_eq!(goal_for(&["goal-test-pin"]), Some(row));
        set_goal("goal-test-pin", None);
        assert_eq!(goal_for(&["goal-test-pin"]), None);
    }
}
