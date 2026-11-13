//! The player's own goals: "99 Fishing in Old School RuneScape", "finish
//! Elden Ring", "100% achievements in Hades". Kept in `objectives.json`
//! next to the XP ledger and shared by the desktop app, the shell, web and
//! phone.
//!
//! A goal is linked to a game (by Steam app id or an integration's id when
//! one is known), starts at where the player really is, and has one target.
//! Progress pays XP at 25, 50 and 75% of the way from the start and when the
//! goal is done. Each payment is written on the goal itself with its time,
//! and the XP ledger folds them in on read, so a broken ledger rebuilds and
//! nothing is ever paid twice or taken back.
//!
//! Suggested goals (from chat or the quiet detector) wait for Track or
//! Not now; Not now hides one for 30 days and the same goal is never
//! suggested twice in a row. Done and dropped goals stay for the trophy
//! shelf and the season count.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::xp::{local_day, Earned, XpKind};

mod board;
mod chat;
mod follow_up;
mod game_data;
mod season;
mod typed;
mod week;

pub use board::{
    apply, ask_card, badge_line, board, goal_detail, goal_title, steps_by_one, track_chip, Board,
    GoalAct, GoalCard, Season, SHELF_MAX, TRACK_CHIP_PREFIX,
};
pub use chat::{
    detect, is_marker_line, markers, read_turn, wants_tracking, without_markers, worth_a_look,
    Marker, MarkerFilter, TurnGoals, GOAL_MARKER_RULE,
};
pub use follow_up::{
    due_follow_ups, follow_up_candidate, follow_up_candidates, followed_up, posted_follow_up,
    shown_follow_up, take_follow_up, FollowUp, FOLLOW_UPS_PER_WEEK, FOLLOW_UP_IDLE_MS,
    FOLLOW_UP_SHOWN_MS, IGNORED_BEFORE_STOP,
};
pub use game_data::{
    apply_game_data, goal_steam_games, match_finished, refresh_from_games, GameData,
};
pub use season::{
    badges, done_this_season, pick_up_line, season_bonus_note, season_firsts, season_label,
    short_date, today_picks, Pick, BADGE_GOALS, XP_SEASON_FIRST,
};
pub use typed::{parse_goal, KnownGame};
pub use week::{week_from_disk, your_week, YourWeek, WEEK_LINES};

pub const OBJECTIVES_FILE: &str = "objectives.json";

/// XP for each of the 25, 50 and 75% milestones.
pub const XP_PER_MILESTONE: u32 = 50;
/// XP for a goal done.
pub const XP_GOAL_DONE: u32 = 200;
/// XP for a big one: finishing a game, or a skill to 99 or more.
pub const XP_GOAL_DONE_BIG: u32 = 300;
/// A skill target at or past this counts as a big goal.
pub const BIG_SKILL_TARGET: u32 = 99;
/// Not now hides a suggestion this long.
pub const MUTE_MS: u64 = 30 * DAY_MS;
/// Progress marks kept per goal, newest last.
pub const HISTORY_MAX: usize = 30;
/// Longest goal text and game name kept.
pub const TEXT_MAX: usize = 120;
pub const GAME_MAX: usize = 80;
/// Milestones need at least this many steps between the start and the
/// target, so a one-step goal pays only when it's done.
pub const MILESTONE_MIN_SPAN: u32 = 4;

const DAY_MS: u64 = 86_400_000;
const MILESTONES: [(u32, &str); 3] = [(25, "m1"), (50, "m2"), (75, "m3")];

/// What kind of goal it is, which sets its words and its XP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalKind {
    /// Win 10 matches, play 5 sessions.
    Count,
    /// Finish the game.
    Finish,
    /// X of Y achievements, or 100%.
    Achievements,
    /// A skill or level: 99 Fishing, level 90.
    Skill,
    /// A rating or score: Mythic+ rating, item level.
    Rating,
    /// Anything else, in the player's words.
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalState {
    /// Waiting in the tray for Track or Not now.
    Suggested,
    Tracking,
    Done,
    Dropped,
}

/// Where the goal came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Typed in the "What do you want to do?" box.
    Typed,
    /// Said in chat.
    Chat,
    /// Picked up by the local detector; tray only.
    Detected,
}

/// The game a goal is about.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameRef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steam_app_id: Option<u32>,
    /// The integration that can read its progress (`osrs`, `battlenet`,
    /// `gw2`, `destiny`, `stardew`, `factorio`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// One progress reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub at: u64,
    pub value: u32,
}

/// One XP payment for this goal: `m1`, `m2`, `m3` or `done`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Paid {
    pub what: String,
    pub xp: u32,
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Goal {
    pub id: String,
    pub game: GameRef,
    pub text: String,
    pub kind: GoalKind,
    /// Where the player was when the goal was set.
    pub start: u32,
    pub current: u32,
    pub target: u32,
    pub state: GoalState,
    pub origin: Origin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat: Option<String>,
    pub created_ms: u64,
    pub updated_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muted_until: Option<u64>,
    /// The last reading came from allowed game data, not from the player.
    #[serde(default)]
    pub checked: bool,
    #[serde(default)]
    pub history: Vec<Mark>,
    #[serde(default)]
    pub paid: Vec<Paid>,
    /// Follow-ups that went out about this goal, newest last.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nudges: Vec<Nudge>,
}

/// One follow-up about a goal. `stop` is the last one: "I'll stop asking".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nudge {
    pub at: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stop: bool,
}

/// A goal to add or suggest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewGoal {
    pub game: GameRef,
    pub text: String,
    pub kind: GoalKind,
    pub current: u32,
    pub target: u32,
    pub origin: Origin,
    pub chat: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Objectives {
    #[serde(default)]
    pub goals: Vec<Goal>,
}

/// Why a goal can't be added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddError {
    NoText,
    /// The player is already at or past the target.
    AlreadyThere,
}

impl AddError {
    pub fn message(self) -> &'static str {
        match self {
            Self::NoText => "Say what you want to do first.",
            Self::AlreadyThere => "You're already there. Pick a target past where you are.",
        }
    }
}

/// Lowercase letters and digits only, for spotting the same goal twice.
fn same_key(game: &str, text: &str) -> String {
    format!("{game} {text}")
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn clip(s: &str, max: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    s.chars().take(max).collect()
}

impl Goal {
    /// How far along the bar is, from nothing to the target (the bar shows
    /// where the player really is, not only the progress since tracking).
    pub fn frac(&self) -> f32 {
        if self.state == GoalState::Done {
            return 1.0;
        }
        (self.current as f32 / self.target.max(1) as f32).clamp(0.0, 1.0)
    }

    /// Percent of the way from the start to the target, 0 to 100.
    fn pct_since_start(&self) -> u32 {
        let span = self.target.saturating_sub(self.start);
        if span == 0 {
            return 100;
        }
        let done = self.current.saturating_sub(self.start).min(span);
        (u64::from(done) * 100 / u64::from(span)) as u32
    }

    /// "2 levels to 99", "3 achievements to go", "Done".
    pub fn next_line(&self) -> String {
        if self.state == GoalState::Done {
            return "Done".into();
        }
        let left = self.target.saturating_sub(self.current);
        let s = if left == 1 { "" } else { "s" };
        match self.kind {
            GoalKind::Finish => "Not finished yet".into(),
            GoalKind::Custom if self.target <= 1 => "Not done yet".into(),
            GoalKind::Count | GoalKind::Custom => format!("{left} to go"),
            GoalKind::Achievements => format!("{left} achievement{s} to go"),
            GoalKind::Skill => format!("{left} level{s} to {}", self.target),
            GoalKind::Rating => format!("{left} to {}", self.target),
        }
    }

    /// The XP a goal pays when it's done.
    pub fn done_xp(&self) -> u32 {
        match self.kind {
            GoalKind::Finish => XP_GOAL_DONE_BIG,
            GoalKind::Skill if self.target >= BIG_SKILL_TARGET => XP_GOAL_DONE_BIG,
            _ => XP_GOAL_DONE,
        }
    }

    fn has_paid(&self, what: &str) -> bool {
        self.paid.iter().any(|p| p.what == what)
    }

    fn pay(&mut self, what: &str, xp: u32, now: u64) {
        if !self.has_paid(what) {
            self.paid.push(Paid {
                what: what.into(),
                xp,
                at: now,
            });
        }
    }

    fn mark(&mut self, value: u32, now: u64) {
        self.history.push(Mark { at: now, value });
        if self.history.len() > HISTORY_MAX {
            let extra = self.history.len() - HISTORY_MAX;
            self.history.drain(..extra);
        }
    }

    fn hidden(&self, now: u64) -> bool {
        self.muted_until.is_some_and(|until| now < until)
    }

    /// The ledger lines this goal earned, keyed `goal:<id>:<what>`.
    pub fn earned(&self, offset_min: i32) -> impl Iterator<Item = (String, Earned)> + '_ {
        self.paid.iter().map(move |p| {
            let kind = if p.what == "done" {
                XpKind::GoalDone
            } else {
                XpKind::Milestone
            };
            (
                format!("goal:{}:{}", self.id, p.what),
                Earned {
                    day: local_day(p.at, offset_min),
                    xp: p.xp,
                    kind,
                },
            )
        })
    }
}

impl Objectives {
    fn find(&mut self, id: &str) -> Option<&mut Goal> {
        self.goals.iter_mut().find(|g| g.id == id)
    }

    fn same(&mut self, game: &str, text: &str) -> Option<&mut Goal> {
        let key = same_key(game, text);
        self.goals
            .iter_mut()
            .rev()
            .find(|g| same_key(&g.game.name, &g.text) == key)
    }

    fn build(&self, new: NewGoal, state: GoalState, now: u64) -> Result<Goal, AddError> {
        let text = clip(&new.text, TEXT_MAX);
        if text.is_empty() {
            return Err(AddError::NoText);
        }
        let target = match new.kind {
            GoalKind::Finish => 1,
            _ => new.target.max(1),
        };
        let current = if new.kind == GoalKind::Finish {
            0
        } else {
            new.current
        };
        if current >= target {
            return Err(AddError::AlreadyThere);
        }
        Ok(Goal {
            id: format!("g{now:x}-{}", self.goals.len()),
            game: GameRef {
                name: clip(&new.game.name, GAME_MAX),
                ..new.game
            },
            text,
            kind: new.kind,
            start: current,
            current,
            target,
            state,
            origin: new.origin,
            chat: new.chat,
            created_ms: now,
            updated_ms: now,
            done_ms: None,
            muted_until: None,
            checked: false,
            history: vec![Mark {
                at: now,
                value: current,
            }],
            paid: Vec::new(),
            nudges: Vec::new(),
        })
    }

    /// Tracks a goal the player asked for. The same goal already here
    /// (suggested, muted or dropped) is tracked again instead of doubled.
    pub fn add(&mut self, new: NewGoal, now: u64) -> Result<String, AddError> {
        if let Some(g) = self.same(&new.game.name, &new.text) {
            if g.state != GoalState::Done {
                g.state = GoalState::Tracking;
                g.muted_until = None;
                g.updated_ms = now;
                return Ok(g.id.clone());
            }
        }
        let goal = self.build(new, GoalState::Tracking, now)?;
        let id = goal.id.clone();
        self.goals.push(goal);
        Ok(id)
    }

    /// Puts a goal in the tray. Nothing is suggested when the same goal is
    /// already here in any state, or this chat already asked once.
    pub fn suggest(&mut self, new: NewGoal, now: u64) -> Option<String> {
        if self.same(&new.game.name, &new.text).is_some() {
            return None;
        }
        if let Some(chat) = new.chat.as_deref() {
            if self.asked_in(chat) {
                return None;
            }
        }
        let goal = self.build(new, GoalState::Suggested, now).ok()?;
        let id = goal.id.clone();
        self.goals.push(goal);
        Some(id)
    }

    /// True when a goal was already suggested from this chat.
    pub fn asked_in(&self, chat: &str) -> bool {
        self.goals
            .iter()
            .any(|g| g.origin != Origin::Typed && g.chat.as_deref() == Some(chat))
    }

    /// Track a suggestion.
    pub fn track(&mut self, id: &str, now: u64) -> bool {
        let Some(g) = self.find(id) else { return false };
        if g.state != GoalState::Suggested {
            return false;
        }
        g.state = GoalState::Tracking;
        g.muted_until = None;
        g.updated_ms = now;
        true
    }

    /// Not now: hides a suggestion for 30 days.
    pub fn not_now(&mut self, id: &str, now: u64) -> bool {
        let Some(g) = self.find(id) else { return false };
        if g.state != GoalState::Suggested {
            return false;
        }
        g.muted_until = Some(now + MUTE_MS);
        g.updated_ms = now;
        true
    }

    /// A new reading for a tracked goal. Pays the milestones it passed and
    /// finishes the goal at its target. `checked` is true when the reading
    /// came from allowed game data. True when anything changed.
    pub fn set_progress(&mut self, id: &str, value: u32, checked: bool, now: u64) -> bool {
        let Some(g) = self.find(id) else { return false };
        if g.state != GoalState::Tracking || (value == g.current && checked == g.checked) {
            return false;
        }
        g.current = value;
        g.checked = checked;
        g.updated_ms = now;
        g.mark(value, now);
        if g.target.saturating_sub(g.start) >= MILESTONE_MIN_SPAN {
            let pct = g.pct_since_start();
            for (at, what) in MILESTONES {
                if pct >= at && pct < 100 {
                    g.pay(what, XP_PER_MILESTONE, now);
                }
            }
        }
        if g.current >= g.target {
            finish(g, now);
        }
        true
    }

    /// +1 on a tracked goal.
    pub fn bump(&mut self, id: &str, now: u64) -> bool {
        let Some(value) = self
            .goals
            .iter()
            .find(|g| g.id == id)
            .map(|g| g.current.saturating_add(1))
        else {
            return false;
        };
        self.set_progress(id, value, false, now)
    }

    /// Done, from the player.
    pub fn done(&mut self, id: &str, now: u64) -> bool {
        let Some(g) = self.find(id) else { return false };
        if g.state != GoalState::Tracking {
            return false;
        }
        g.current = g.current.max(g.target);
        g.updated_ms = now;
        g.mark(g.current, now);
        finish(g, now);
        true
    }

    /// Drop a goal. What it already paid stays paid.
    pub fn drop_goal(&mut self, id: &str, now: u64) -> bool {
        let Some(g) = self.find(id) else { return false };
        if matches!(g.state, GoalState::Done | GoalState::Dropped) {
            return false;
        }
        g.state = GoalState::Dropped;
        g.updated_ms = now;
        true
    }

    /// Tracked goals, most recently moved first.
    pub fn tracking(&self) -> Vec<&Goal> {
        let mut out: Vec<&Goal> = self
            .goals
            .iter()
            .filter(|g| g.state == GoalState::Tracking)
            .collect();
        out.sort_by(|a, b| b.updated_ms.cmp(&a.updated_ms).then(a.id.cmp(&b.id)));
        out
    }

    /// Suggestions not hidden by Not now, newest first.
    pub fn suggestions(&self, now: u64) -> Vec<&Goal> {
        let mut out: Vec<&Goal> = self
            .goals
            .iter()
            .filter(|g| g.state == GoalState::Suggested && !g.hidden(now))
            .collect();
        out.sort_by(|a, b| b.created_ms.cmp(&a.created_ms).then(a.id.cmp(&b.id)));
        out
    }

    /// Finished goals, newest first: the trophy shelf.
    pub fn trophies(&self) -> Vec<&Goal> {
        let mut out: Vec<&Goal> = self
            .goals
            .iter()
            .filter(|g| g.state == GoalState::Done)
            .collect();
        out.sort_by(|a, b| b.done_ms.cmp(&a.done_ms).then(a.id.cmp(&b.id)));
        out
    }
}

fn finish(g: &mut Goal, now: u64) {
    g.state = GoalState::Done;
    g.done_ms = Some(now);
    let xp = g.done_xp();
    g.pay("done", xp, now);
}

pub fn objectives_path(dir: &Path) -> PathBuf {
    dir.join(OBJECTIVES_FILE)
}

/// The saved goals; a missing or broken file reads as none.
pub fn load(dir: &Path) -> Objectives {
    std::fs::read_to_string(objectives_path(dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Saves through a temp file and a rename.
pub fn save(dir: &Path, o: &Objectives) -> Result<(), String> {
    let raw = serde_json::to_string(o).map_err(|e| e.to_string())?;
    crate::pulse_writer::write_atomic(&objectives_path(dir), &raw)
}

/// Reads the goals, applies one change and saves them when it changed
/// something. Reading right before the write keeps a change from the other
/// app safe. Returns the goals as saved and what the change returned.
pub fn change<T>(
    dir: &Path,
    f: impl FnOnce(&mut Objectives) -> T,
) -> Result<(Objectives, T), String> {
    let mut o = load(dir);
    let before = o.clone();
    let out = f(&mut o);
    if o != before {
        save(dir, &o)?;
    }
    Ok((o, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_793_620_800_000;

    fn new(game: &str, text: &str, kind: GoalKind, current: u32, target: u32) -> NewGoal {
        NewGoal {
            game: GameRef {
                name: game.into(),
                ..GameRef::default()
            },
            text: text.into(),
            kind,
            current,
            target,
            origin: Origin::Typed,
            chat: None,
        }
    }

    fn whats(g: &Goal) -> Vec<&str> {
        g.paid.iter().map(|p| p.what.as_str()).collect()
    }

    #[test]
    fn a_skill_goal_starts_where_you_are_and_pays_each_milestone_once() {
        let mut o = Objectives::default();
        let id = o
            .add(
                new(
                    "Old School RuneScape",
                    "99 Fishing",
                    GoalKind::Skill,
                    91,
                    99,
                ),
                NOW,
            )
            .unwrap();
        let g = &o.goals[0];
        assert_eq!((g.start, g.current, g.target), (91, 91, 99));
        assert!((g.frac() - 91.0 / 99.0).abs() < 1e-6);
        assert_eq!(g.next_line(), "8 levels to 99");

        assert!(o.set_progress(&id, 93, true, NOW + 1));
        assert_eq!(whats(&o.goals[0]), ["m1"]);
        assert!(o.goals[0].checked);
        // The same reading again changes nothing.
        assert!(!o.set_progress(&id, 93, true, NOW + 2));
        assert!(o.set_progress(&id, 97, false, NOW + 3));
        assert_eq!(whats(&o.goals[0]), ["m1", "m2", "m3"]);
        // Slipping back pays nothing again and takes nothing back.
        assert!(o.set_progress(&id, 95, false, NOW + 4));
        assert!(o.set_progress(&id, 97, false, NOW + 5));
        assert_eq!(o.goals[0].paid.len(), 3);
        assert_eq!(o.goals[0].next_line(), "2 levels to 99");

        assert!(o.set_progress(&id, 99, true, NOW + 6));
        let g = &o.goals[0];
        assert_eq!(g.state, GoalState::Done);
        assert_eq!(g.done_ms, Some(NOW + 6));
        assert_eq!(whats(g), ["m1", "m2", "m3", "done"]);
        assert_eq!(g.paid[3].xp, XP_GOAL_DONE_BIG);
        assert_eq!(g.next_line(), "Done");
        assert!(
            !o.set_progress(&id, 100, true, NOW + 7),
            "done goals stay done"
        );
    }

    #[test]
    fn a_jump_straight_to_the_target_pays_only_done() {
        let mut o = Objectives::default();
        let id = o
            .add(
                new("Hades", "All achievements", GoalKind::Achievements, 40, 49),
                NOW,
            )
            .unwrap();
        assert!(o.set_progress(&id, 49, true, NOW + 1));
        assert_eq!(whats(&o.goals[0]), ["done"]);
        assert_eq!(o.goals[0].paid[0].xp, XP_GOAL_DONE);
    }

    #[test]
    fn small_goals_skip_milestones_and_finish_pays_big() {
        let mut o = Objectives::default();
        let wins = o
            .add(
                new("Rocket League", "Win 3 matches", GoalKind::Count, 0, 3),
                NOW,
            )
            .unwrap();
        assert!(o.bump(&wins, NOW + 1));
        assert!(o.bump(&wins, NOW + 2));
        assert!(
            o.goals[0].paid.is_empty(),
            "a 3-step goal has no milestones"
        );
        assert_eq!(o.goals[0].next_line(), "1 to go");
        assert!(o.bump(&wins, NOW + 3));
        assert_eq!(o.goals[0].state, GoalState::Done);
        assert_eq!(o.goals[0].history.len(), 4);

        let ring = o
            .add(new("Elden Ring", "Finish it", GoalKind::Finish, 7, 40), NOW)
            .unwrap();
        let g = o.goals.iter().find(|g| g.id == ring).unwrap();
        assert_eq!((g.current, g.target), (0, 1), "finish goals are one step");
        assert_eq!(g.next_line(), "Not finished yet");
        assert!(o.done(&ring, NOW + 9));
        let g = o.goals.iter().find(|g| g.id == ring).unwrap();
        assert_eq!(
            g.paid,
            vec![Paid {
                what: "done".into(),
                xp: 300,
                at: NOW + 9
            }]
        );
        assert!(!o.done(&ring, NOW + 10));
    }

    #[test]
    fn adding_checks_the_words_and_never_doubles_a_goal() {
        let mut o = Objectives::default();
        assert_eq!(
            o.add(new("Hades", "   ", GoalKind::Custom, 0, 1), NOW),
            Err(AddError::NoText)
        );
        assert_eq!(
            o.add(
                new("Path of Exile", "Level 90", GoalKind::Skill, 92, 90),
                NOW
            ),
            Err(AddError::AlreadyThere)
        );
        let a = o
            .add(
                new("Path of Exile", "Level  90", GoalKind::Skill, 80, 90),
                NOW,
            )
            .unwrap();
        assert_eq!(o.goals[0].text, "Level 90");
        let b = o
            .add(
                new("path of exile", "level 90!", GoalKind::Skill, 80, 90),
                NOW + 1,
            )
            .unwrap();
        assert_eq!(a, b);
        assert_eq!(o.goals.len(), 1);

        assert!(o.drop_goal(&a, NOW + 2));
        assert_eq!(o.goals[0].state, GoalState::Dropped);
        assert!(o.tracking().is_empty());
        // Asking again tracks the dropped one again.
        assert_eq!(
            o.add(
                new("Path of Exile", "Level 90", GoalKind::Skill, 85, 90),
                NOW + 3
            ),
            Ok(a)
        );
        assert_eq!(o.goals[0].state, GoalState::Tracking);
        assert_eq!(o.goals.len(), 1);
    }

    #[test]
    fn suggestions_ask_once_per_chat_and_not_now_hides_one_for_30_days() {
        let mut o = Objectives::default();
        let chat = |text: &str| NewGoal {
            origin: Origin::Chat,
            chat: Some("chat-1".into()),
            ..new("VALORANT", text, GoalKind::Custom, 0, 1)
        };
        let id = o.suggest(chat("Reach Plat 2"), NOW).unwrap();
        assert!(o.asked_in("chat-1"));
        assert_eq!(
            o.suggest(chat("Win a match"), NOW + 1),
            None,
            "one ask per chat"
        );
        assert_eq!(o.suggestions(NOW).len(), 1);
        assert!(o.tracking().is_empty());

        assert!(o.not_now(&id, NOW + 2));
        assert!(o.suggestions(NOW + 3).is_empty());
        assert_eq!(o.suggestions(NOW + 2 + MUTE_MS).len(), 1);
        // The same goal from another chat isn't suggested again.
        let other = NewGoal {
            chat: Some("chat-2".into()),
            ..chat("reach plat 2")
        };
        assert_eq!(o.suggest(other, NOW + 4), None);

        assert!(o.track(&id, NOW + 5));
        assert_eq!(o.tracking()[0].id, id);
        assert!(!o.track(&id, NOW + 6));
        assert!(!o.not_now(&id, NOW + 6));
        assert!(!o.set_progress("missing", 1, false, NOW));
    }

    #[test]
    fn suggested_goals_pay_nothing_until_tracked() {
        let mut o = Objectives::default();
        let id = o
            .suggest(new("Hades", "Clear 10 runs", GoalKind::Count, 0, 10), NOW)
            .unwrap();
        assert!(!o.set_progress(&id, 5, true, NOW + 1));
        assert!(!o.done(&id, NOW + 1));
        assert!(o.goals[0].paid.is_empty());
    }

    #[test]
    fn the_trophy_shelf_lists_done_goals_newest_first() {
        let mut o = Objectives::default();
        let a = o.add(new("A", "one", GoalKind::Custom, 0, 1), NOW).unwrap();
        let b = o.add(new("B", "two", GoalKind::Custom, 0, 1), NOW).unwrap();
        o.done(&a, NOW + 1);
        o.done(&b, NOW + 2);
        let ids: Vec<&str> = o.trophies().iter().map(|g| g.id.as_str()).collect();
        assert_eq!(ids, [b.as_str(), a.as_str()]);
    }

    #[test]
    fn history_keeps_the_newest_readings() {
        let mut o = Objectives::default();
        let id = o
            .add(
                new("Factorio", "Launch 100 rockets", GoalKind::Count, 0, 100),
                NOW,
            )
            .unwrap();
        for i in 1..=40 {
            o.set_progress(&id, i, true, NOW + u64::from(i));
        }
        let h = &o.goals[0].history;
        assert_eq!(h.len(), HISTORY_MAX);
        assert_eq!(h.last().unwrap().value, 40);
        assert_eq!(h[0].value, 11);
    }

    #[test]
    fn earned_lines_carry_the_local_day_and_kind() {
        let mut o = Objectives::default();
        let id = o.add(new("A", "one", GoalKind::Custom, 0, 1), NOW).unwrap();
        o.done(&id, NOW);
        let lines: Vec<(String, Earned)> = o.goals[0].earned(0).collect();
        assert_eq!(
            lines,
            [(
                format!("goal:{id}:done"),
                Earned {
                    day: 20759,
                    xp: 200,
                    kind: XpKind::GoalDone
                }
            )]
        );
    }

    #[test]
    fn the_file_round_trips_and_change_saves_only_when_needed() {
        let dir = std::env::temp_dir().join(format!("objectives-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load(&dir), Objectives::default());
        let (o, id) = change(&dir, |o| {
            o.add(new("Hades", "Clear 10 runs", GoalKind::Count, 2, 10), NOW)
        })
        .unwrap();
        let id = id.unwrap();
        assert_eq!(load(&dir), o);
        let (_, moved) = change(&dir, |o| o.set_progress(&id, 2, false, NOW + 1)).unwrap();
        assert!(!moved);
        std::fs::write(objectives_path(&dir), "not json").unwrap();
        assert_eq!(load(&dir), Objectives::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
