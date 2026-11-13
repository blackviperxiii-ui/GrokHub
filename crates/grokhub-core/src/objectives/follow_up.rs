//! Gentle follow-ups on goals left for a few days: "Want to pick a night for
//! your next Elden Ring session?". At most one per goal a week and two a week
//! in all. When two in a row go by with the goal untouched, the next is the
//! last: "I'll stop asking about Elden Ring, just say the word." Any progress
//! on the goal (a +1, a reading from game data, a line in chat) starts it over.
//!
//! Both apps send follow-ups through the proactive engine
//! ([`follow_up_candidates`], then [`posted_follow_up`] once a card is up), so
//! its budget, quiet hours, busy (a game running counts) and Not this all
//! apply. The shell also shows the follow-up on Home and Objectives.
//! The words never mention how long it has been.

use serde::Serialize;

use super::{goal_title, Goal, GoalState, Nudge, Objectives};
use crate::proactive::{Candidate, CandidateSource};

const DAY_MS: u64 = 86_400_000;
const WEEK_MS: u64 = 7 * DAY_MS;
/// A goal untouched this long may get a follow-up.
pub const FOLLOW_UP_IDLE_MS: u64 = 3 * DAY_MS;
/// Follow-ups a week, across every goal.
pub const FOLLOW_UPS_PER_WEEK: usize = 2;
/// Follow-ups in a row with no progress before the last one.
pub const IGNORED_BEFORE_STOP: usize = 2;
/// How long a follow-up stays on Home and Objectives in the shell.
pub const FOLLOW_UP_SHOWN_MS: u64 = DAY_MS;
const NUDGES_KEPT: usize = 8;

/// One follow-up, ready to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowUp {
    pub id: String,
    /// "Elden Ring · Finish"
    pub title: String,
    /// The question, or the last line.
    pub line: String,
    /// What a tap asks in chat. Empty on the last line.
    pub prompt: String,
    /// "I'll stop asking": the last one for now.
    pub stop: bool,
}

/// Follow-ups since the goal last moved.
fn since_touch(g: &Goal) -> impl Iterator<Item = &Nudge> {
    g.nudges.iter().filter(move |n| n.at > g.updated_ms)
}

fn what(g: &Goal) -> &str {
    if g.game.name.is_empty() {
        g.text.as_str()
    } else {
        g.game.name.as_str()
    }
}

fn follow_up(g: &Goal, stop: bool) -> FollowUp {
    let what = what(g);
    let has_game = !g.game.name.is_empty();
    let line = if stop {
        format!("I'll stop asking about {what}, just say the word.")
    } else if has_game {
        format!("Want to pick a night for your next {what} session?")
    } else {
        format!("Want to pick a night to work on {what}?")
    };
    let prompt = if stop {
        String::new()
    } else if has_game {
        format!(
            "Help me pick a night this week for my next {what} session. My goal there: {}.",
            g.text
        )
    } else {
        format!("Help me pick a night this week to work on my goal: {what}.")
    };
    FollowUp {
        id: g.id.clone(),
        title: goal_title(g),
        line,
        prompt,
        stop,
    }
}

/// Follow-ups that may go out now, the goal left longest first. Empty once
/// two went out this week.
pub fn due_follow_ups(o: &Objectives, now: u64) -> Vec<FollowUp> {
    let this_week = o
        .goals
        .iter()
        .flat_map(|g| g.nudges.iter())
        .filter(|n| n.at + WEEK_MS > now)
        .count();
    let room = FOLLOW_UPS_PER_WEEK.saturating_sub(this_week);
    let mut goals: Vec<&Goal> = o
        .goals
        .iter()
        .filter(|g| g.state == GoalState::Tracking)
        .filter(|g| g.updated_ms + FOLLOW_UP_IDLE_MS <= now)
        .filter(|g| !since_touch(g).any(|n| n.stop))
        .filter(|g| g.nudges.last().is_none_or(|n| n.at + WEEK_MS <= now))
        .collect();
    goals.sort_by(|a, b| a.updated_ms.cmp(&b.updated_ms).then(a.id.cmp(&b.id)));
    goals
        .into_iter()
        .take(room)
        .map(|g| follow_up(g, since_touch(g).count() >= IGNORED_BEFORE_STOP))
        .collect()
}

/// Writes that a follow-up went out. False for a goal that isn't tracked.
pub fn followed_up(o: &mut Objectives, id: &str, stop: bool, now: u64) -> bool {
    let Some(g) = o
        .goals
        .iter_mut()
        .find(|g| g.id == id && g.state == GoalState::Tracking)
    else {
        return false;
    };
    g.nudges.push(Nudge { at: now, stop });
    if g.nudges.len() > NUDGES_KEPT {
        let extra = g.nudges.len() - NUDGES_KEPT;
        g.nudges.drain(..extra);
    }
    true
}

/// The follow-up that went out in the last day, while its goal hasn't moved.
pub fn shown_follow_up(o: &Objectives, now: u64) -> Option<FollowUp> {
    o.goals
        .iter()
        .filter(|g| g.state == GoalState::Tracking)
        .filter_map(|g| {
            let n = g.nudges.last()?;
            (n.at > g.updated_ms && now < n.at + FOLLOW_UP_SHOWN_MS).then_some((n.at, g, n.stop))
        })
        .max_by(|a, b| a.0.cmp(&b.0).then(b.1.id.cmp(&a.1.id)))
        .map(|(_, g, stop)| follow_up(g, stop))
}

/// For the shell, which shows instead of sending: the one showing now, or
/// the first one due, written as gone out.
pub fn take_follow_up(o: &mut Objectives, now: u64) -> Option<FollowUp> {
    if let Some(shown) = shown_follow_up(o, now) {
        return Some(shown);
    }
    let next = due_follow_ups(o, now).into_iter().next()?;
    followed_up(o, &next.id, next.stop, now).then_some(next)
}

/// The proactive card for a follow-up: a one-tap question whose tap asks
/// for help in chat. None for the last line, which is a note, not an offer.
pub fn follow_up_candidate(f: &FollowUp) -> Option<Candidate> {
    if f.stop {
        return None;
    }
    let mut c = Candidate::soft(
        CandidateSource::Objectives,
        &f.prompt,
        &f.title,
        0.6,
        0.7,
        "One of your goals on Objectives.",
    );
    c.ask = Some(f.line.clone());
    Some(c)
}

/// Follow-ups due now, as proactive candidates. A last "I'll stop asking"
/// line is no offer: it is written as gone out here instead, and shows on
/// Home and Objectives.
pub fn follow_up_candidates(o: &mut Objectives, now: u64) -> Vec<Candidate> {
    let mut out = Vec::new();
    for f in due_follow_ups(o, now) {
        match follow_up_candidate(&f) {
            Some(c) => out.push(c),
            None => {
                followed_up(o, &f.id, true, now);
            }
        }
    }
    out
}

/// A follow-up card about the tracked goal titled `topic` went up: written
/// as gone out. False when no tracked goal has that title.
pub fn posted_follow_up(o: &mut Objectives, topic: &str, now: u64) -> bool {
    let Some(id) = o
        .tracking()
        .into_iter()
        .find(|g| goal_title(g) == topic)
        .map(|g| g.id.clone())
    else {
        return false;
    };
    followed_up(o, &id, false, now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::objectives::{GameRef, GoalKind, NewGoal, Origin};
    use crate::proactive::{proactive_card, route, ProactiveRoute};

    /// Monday 2 Nov 2026, 12:00 UTC.
    const NOW: u64 = 1_793_620_800_000;

    fn goal(o: &mut Objectives, game: &str, text: &str, at: u64) -> String {
        o.add(
            NewGoal {
                game: GameRef {
                    name: game.into(),
                    ..GameRef::default()
                },
                text: text.into(),
                kind: GoalKind::Finish,
                current: 0,
                target: 1,
                origin: Origin::Typed,
                chat: None,
            },
            at,
        )
        .unwrap()
    }

    #[test]
    fn a_goal_left_a_few_days_gets_one_kind_question_a_week() {
        let mut o = Objectives::default();
        let id = goal(&mut o, "Elden Ring", "Finish", NOW);
        assert!(due_follow_ups(&o, NOW + DAY_MS).is_empty(), "too soon");
        let due = due_follow_ups(&o, NOW + 3 * DAY_MS);
        assert_eq!(due.len(), 1);
        assert_eq!(
            due[0].line,
            "Want to pick a night for your next Elden Ring session?"
        );
        assert!(!due[0].line.contains("day"), "never about the gap");
        assert!(due[0].prompt.starts_with("Help me pick a night"));
        assert!(followed_up(&mut o, &id, false, NOW + 3 * DAY_MS));
        assert!(
            due_follow_ups(&o, NOW + 9 * DAY_MS).is_empty(),
            "one a week"
        );
        assert_eq!(due_follow_ups(&o, NOW + 10 * DAY_MS).len(), 1);
    }

    #[test]
    fn two_a_week_in_all_the_one_left_longest_first() {
        let mut o = Objectives::default();
        let a = goal(&mut o, "Elden Ring", "Finish", NOW);
        goal(&mut o, "Hades", "Finish", NOW + 1);
        goal(&mut o, "Celeste", "Finish", NOW + 2);
        let at = NOW + 4 * DAY_MS;
        let due = due_follow_ups(&o, at);
        assert_eq!(due.len(), 2);
        assert_eq!(due[0].id, a);
        for f in &due {
            followed_up(&mut o, &f.id, false, at);
        }
        assert!(due_follow_ups(&o, at + DAY_MS).is_empty());
    }

    #[test]
    fn two_ignored_bring_the_stop_line_and_progress_starts_it_over() {
        let mut o = Objectives::default();
        let id = goal(&mut o, "Elden Ring", "Finish", NOW);
        let mut at = NOW + 3 * DAY_MS;
        for _ in 0..2 {
            let f = due_follow_ups(&o, at).remove(0);
            assert!(!f.stop);
            followed_up(&mut o, &id, false, at);
            at += 7 * DAY_MS;
        }
        let last = due_follow_ups(&o, at).remove(0);
        assert!(last.stop);
        assert_eq!(
            last.line,
            "I'll stop asking about Elden Ring, just say the word."
        );
        assert_eq!(follow_up_candidate(&last), None, "a note, not an offer");
        followed_up(&mut o, &id, true, at);
        assert!(due_follow_ups(&o, at + 30 * DAY_MS).is_empty(), "stopped");
        // The player says the word: any progress starts it over.
        o.goals[0].updated_ms = at + 31 * DAY_MS;
        let back = due_follow_ups(&o, at + 35 * DAY_MS);
        assert_eq!(back.len(), 1);
        assert!(!back[0].stop);
    }

    #[test]
    fn the_shell_shows_one_for_a_day_and_writes_it_once() {
        let mut o = Objectives::default();
        goal(&mut o, "Hades", "Finish", NOW);
        let at = NOW + 3 * DAY_MS;
        let f = take_follow_up(&mut o, at).expect("one due");
        assert_eq!(o.goals[0].nudges.len(), 1);
        assert_eq!(take_follow_up(&mut o, at + HOUR).as_ref(), Some(&f));
        assert_eq!(
            o.goals[0].nudges.len(),
            1,
            "showing it again writes nothing"
        );
        assert_eq!(shown_follow_up(&o, at + DAY_MS), None, "gone after a day");
        o.goals[0].updated_ms = at + HOUR;
        assert_eq!(
            shown_follow_up(&o, at + 2 * HOUR),
            None,
            "gone once it moves"
        );
    }

    const HOUR: u64 = 3_600_000;

    #[test]
    fn the_card_is_the_question_and_its_tap_asks_for_help() {
        let mut o = Objectives::default();
        goal(&mut o, "Elden Ring", "Finish", NOW);
        let f = due_follow_ups(&o, NOW + 3 * DAY_MS).remove(0);
        let c = follow_up_candidate(&f).expect("a card");
        assert_eq!(c.source, CandidateSource::Objectives);
        let r = route(&c, false).expect("over the line");
        assert_eq!(r, ProactiveRoute::Suggest);
        let card = proactive_card(&c, r, NOW);
        assert_eq!(
            card.title,
            "Want to pick a night for your next Elden Ring session?"
        );
        assert_eq!(card.idea_action(), f.prompt);
        assert_eq!(crate::pulse::i_can_title(&card), card.title);
    }

    #[test]
    fn the_engine_gets_offers_and_the_stop_line_is_written_not_offered() {
        let mut o = Objectives::default();
        let id = goal(&mut o, "Elden Ring", "Finish", NOW);
        let at = NOW + 3 * DAY_MS;
        let cands = follow_up_candidates(&mut o, at);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].topic, "Elden Ring · Finish");
        assert!(
            o.goals[0].nudges.is_empty(),
            "an offer is written once it is up"
        );
        assert!(posted_follow_up(&mut o, "Elden Ring · Finish", at));
        assert!(
            !posted_follow_up(&mut o, "Hades · Finish", at),
            "no such goal"
        );
        assert_eq!(o.goals[0].nudges.len(), 1);
        followed_up(&mut o, &id, false, at + 7 * DAY_MS);
        let last = at + 14 * DAY_MS;
        assert!(
            follow_up_candidates(&mut o, last).is_empty(),
            "the last line is a note"
        );
        assert_eq!(o.goals[0].nudges.len(), 3);
        assert!(o.goals[0].nudges[2].stop, "written as gone out");
        assert_eq!(
            shown_follow_up(&o, last).map(|f| f.line),
            Some("I'll stop asking about Elden Ring, just say the word.".to_string())
        );
    }
}
