//! What the Objectives page shows and what a press on it does, shared by the
//! desktop app and the shell (desktop, web and phone): the goal rows' words,
//! the Today, My goals and This season lists, and [`apply`] for the buttons.

use serde::{Deserialize, Serialize};

use super::{
    badges, done_this_season, parse_goal, pick_up_line, season_bonus_note, season_label,
    short_date, shown_follow_up, today_picks, FollowUp, Goal, GoalKind, KnownGame, Objectives,
    Pick, TurnGoals, YourWeek, BADGE_GOALS,
};

/// How many finished goals the trophy shelf shows.
pub const SHELF_MAX: usize = 8;

/// A press on the page. `{"act":"bump","id":"g1"}`, `{"act":"add","text":"…"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "act", rename_all = "camelCase")]
pub enum GoalAct {
    Add { text: String },
    Bump { id: String },
    Done { id: String },
    Drop { id: String },
    Track { id: String },
    NotNow { id: String },
}

/// Apply presses in order. Returns the words to show when an Add was
/// refused (empty text, or already there).
pub fn apply(
    o: &mut Objectives,
    acts: &[GoalAct],
    games: &[KnownGame<'_>],
    now: u64,
) -> Option<&'static str> {
    let mut error = None;
    for act in acts {
        match act {
            GoalAct::Add { text } => {
                if let Err(e) = o.add(parse_goal(text, games), now) {
                    error = Some(e.message());
                }
            }
            GoalAct::Bump { id } => {
                o.bump(id, now);
            }
            GoalAct::Done { id } => {
                o.done(id, now);
            }
            GoalAct::Drop { id } => {
                o.drop_goal(id, now);
            }
            GoalAct::Track { id } => {
                o.track(id, now);
            }
            GoalAct::NotNow { id } => {
                o.not_now(id, now);
            }
        }
    }
    error
}

/// True for goals that move one step at a time (they get a +1).
pub fn steps_by_one(g: &Goal) -> bool {
    !matches!(g.kind, GoalKind::Finish) && g.target > 1
}

/// "Old School RuneScape · 99 fishing", or the goal alone.
pub fn goal_title(g: &Goal) -> String {
    if g.game.name.is_empty() {
        g.text.clone()
    } else {
        format!("{} · {}", g.game.name, g.text)
    }
}

/// "93 / 99 · 6 levels to 99", with "· Checked" when game data said so.
pub fn goal_detail(g: &Goal) -> String {
    let mut s = if g.target > 1 {
        format!("{} / {} · {}", g.current, g.target, g.next_line())
    } else {
        g.next_line()
    };
    if g.checked {
        s.push_str(" · Checked");
    }
    s
}

/// One goal as a row on the page.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalCard {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub frac: f32,
    pub plus_one: bool,
    /// "Pick up Elden Ring tonight?" on a Today row for a goal left a week.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// "Nov 2" on a trophy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done_on: Option<String>,
}

impl GoalCard {
    pub fn of(g: &Goal) -> Self {
        Self {
            id: g.id.clone(),
            title: goal_title(g),
            detail: goal_detail(g),
            frac: g.frac(),
            plus_one: steps_by_one(g),
            prompt: None,
            done_on: None,
        }
    }
}

/// The id start of the one-tap chip that tracks a goal the reply noticed.
pub const TRACK_CHIP_PREFIX: &str = "goal:track:";

/// The goal a finished turn asks about, as a row, from the goals after it.
pub fn ask_card(o: &Objectives, turn: &TurnGoals) -> Option<GoalCard> {
    let id = turn.ask.as_deref()?;
    o.goals.iter().find(|g| g.id == id).map(GoalCard::of)
}

/// "Track goal: 99 fishing", the one-tap under the reply in the desktop app.
pub fn track_chip(g: &Goal) -> crate::QuickChip {
    crate::QuickChip {
        id: format!("{TRACK_CHIP_PREFIX}{}", g.id),
        label: format!("Track goal: {}", g.text),
        value: format!("{TRACK_CHIP_PREFIX}{}", g.id),
        kind: crate::ChipKind::Chat,
        score: 1.0,
        hint: format!("Adds {} to My goals on Objectives", goal_title(g)),
        primary: false,
    }
}

/// This season: its months, goals done in it and the trophy shelf.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Season {
    pub label: String,
    pub done: usize,
    pub trophies: Vec<GoalCard>,
    /// Seasons whose badge was earned (three goals done), newest first.
    pub badges: Vec<String>,
    /// "1 more goal for the Oct to Dec 2026 badge"; None once it's earned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub badge_line: Option<String>,
    /// "First goal this season: +50 bonus XP", for a week after it landed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bonus: Option<String>,
}

/// "2 more goals for the Oct to Dec 2026 badge", or None once earned.
pub fn badge_line(done: usize, label: &str) -> Option<String> {
    let left = BADGE_GOALS.checked_sub(done).filter(|n| *n > 0)?;
    let goals = if left == 1 { "goal" } else { "goals" };
    Some(format!("{left} more {goals} for the {label} badge"))
}

/// The page's goals: Today's picks, tracked goals, suggestions and the season.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Board {
    pub today: Vec<GoalCard>,
    pub mine: Vec<GoalCard>,
    pub suggested: Vec<GoalCard>,
    pub season: Season,
    /// The follow-up that went out in the last day, while its goal hasn't moved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follow_up: Option<FollowUp>,
    /// Monday's "Your week": filled by the caller, which has the XP ledger.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub week: Option<YourWeek>,
}

pub fn board(o: &Objectives, now_ms: u64, offset_min: i32) -> Board {
    let find = |id: &str| o.goals.iter().find(|g| g.id == id);
    let today = today_picks(o, now_ms)
        .into_iter()
        .filter_map(|p| {
            let g = find(p.id())?;
            let mut card = GoalCard::of(g);
            if matches!(p, Pick::PickUp(_)) {
                card.prompt = Some(pick_up_line(g));
            }
            Some(card)
        })
        .collect();
    let trophies = o
        .trophies()
        .into_iter()
        .take(SHELF_MAX)
        .map(|g| GoalCard {
            done_on: g.done_ms.map(|at| short_date(at, offset_min)),
            ..GoalCard::of(g)
        })
        .collect();
    Board {
        today,
        mine: o.tracking().into_iter().map(GoalCard::of).collect(),
        suggested: o
            .suggestions(now_ms)
            .into_iter()
            .map(GoalCard::of)
            .collect(),
        season: {
            let label = season_label(now_ms, offset_min);
            let done = done_this_season(o, now_ms, offset_min);
            Season {
                badge_line: badge_line(done, &label),
                label,
                done,
                trophies,
                badges: badges(o, offset_min),
                bonus: season_bonus_note(o, now_ms, offset_min),
            }
        },
        follow_up: shown_follow_up(o, now_ms),
        week: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Monday 2 Nov 2026, 12:00 UTC.
    const NOW: u64 = 1_793_620_800_000;
    const DAY: u64 = 86_400_000;

    fn add(o: &mut Objectives, text: &str, at: u64) -> Option<&'static str> {
        apply(o, &[GoalAct::Add { text: text.into() }], &[], at)
    }

    #[test]
    fn presses_parse_add_bump_finish_and_refuse_a_repeat() {
        let mut o = Objectives::default();
        assert_eq!(add(&mut o, "99 fishing in osrs", NOW), None);
        // The same goal again keeps the one already there.
        assert_eq!(add(&mut o, "99 Fishing in OSRS", NOW), None);
        assert_eq!(o.goals.len(), 1);
        assert_eq!(
            add(&mut o, "   ", NOW),
            Some("Say what you want to do first.")
        );
        let id = o.goals[0].id.clone();
        let g = &o.goals[0];
        assert_eq!(goal_title(g), "Old School RuneScape · 99 fishing");
        assert!(steps_by_one(g));
        apply(&mut o, &[GoalAct::Bump { id: id.clone() }], &[], NOW + 1);
        assert_eq!(o.goals[0].current, 1);
        apply(&mut o, &[GoalAct::Done { id }], &[], NOW + 2);
        let b = board(&o, NOW + 3, 0);
        assert!(b.mine.is_empty());
        assert_eq!(b.season.done, 1);
        assert_eq!(b.season.label, "Oct to Dec 2026");
        assert_eq!(b.season.trophies[0].done_on.as_deref(), Some("Nov 2"));
        assert_eq!(b.season.trophies[0].detail, "99 / 99 · Done");
    }

    #[test]
    fn today_has_the_closest_goal_and_a_pick_up_for_one_left_a_week() {
        let mut o = Objectives::default();
        let library = [KnownGame {
            name: "Elden Ring",
            steam_app_id: Some(1_245_620),
            source: None,
        }];
        let finish = GoalAct::Add {
            text: "finish elden ring".into(),
        };
        apply(&mut o, &[finish], &library, NOW - 10 * DAY);
        add(&mut o, "99 fishing in osrs", NOW);
        let osrs = o.goals[1].id.clone();
        for _ in 0..50 {
            apply(&mut o, &[GoalAct::Bump { id: osrs.clone() }], &[], NOW);
        }
        let b = board(&o, NOW, 0);
        assert_eq!(b.today.len(), 2);
        assert_eq!(b.today[0].title, "Old School RuneScape · 99 fishing");
        assert_eq!(b.today[0].prompt, None);
        assert_eq!(b.today[1].title, "Elden Ring · Finish");
        assert_eq!(
            b.today[1].prompt.as_deref(),
            Some("Pick up Elden Ring tonight?")
        );
        assert!(!b.today[1].plus_one);
        assert_eq!(b.mine.len(), 2);
    }

    #[test]
    fn an_act_reads_and_writes_as_small_json() {
        let a: GoalAct = serde_json::from_str(r#"{"act":"notNow","id":"g1"}"#).unwrap();
        assert_eq!(a, GoalAct::NotNow { id: "g1".into() });
        let a: GoalAct = serde_json::from_str(r#"{"act":"add","text":"x"}"#).unwrap();
        assert_eq!(a, GoalAct::Add { text: "x".into() });
        let card = GoalCard::of(&{
            let mut o = Objectives::default();
            add(&mut o, "finish hades", NOW);
            o.goals.remove(0)
        });
        let json = serde_json::to_value(&card).unwrap();
        assert_eq!(json["plusOne"], false);
        assert!(json.get("prompt").is_none());
    }

    #[test]
    fn a_noticed_goal_becomes_a_one_tap_track_chip() {
        let mut o = Objectives::default();
        let reply = "Nice!\nGAME_GOAL: osrs | 99 fishing | skill | 99";
        let t = super::super::read_turn(&mut o, "c1", "i'm on fishing", reply, &[], NOW);
        let card = ask_card(&o, &t).expect("a goal to ask about");
        assert_eq!(card.title, "Old School RuneScape · 99 fishing");
        let chip = track_chip(&o.goals[0]);
        assert_eq!(chip.label, "Track goal: 99 fishing");
        assert!(chip.id.starts_with(TRACK_CHIP_PREFIX));
        assert_eq!(crate::chip_action(&chip), crate::ChipAction::Do);
        let quiet = super::super::TurnGoals::default();
        assert_eq!(ask_card(&o, &quiet), None);
    }
}
