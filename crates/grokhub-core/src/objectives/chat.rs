//! Goals from chat. When a reply is about a game, the chat model may add
//! hidden lines (kept off the chat view like `USER_FACT:`):
//!
//! - `GAME_GOAL: game | goal | kind | target` for a goal the player said
//!   they want ("I want 99 fishing"), and
//! - `GAME_PROGRESS: game | goal | number` for where they are now on one.
//!
//! A goal the player asked to add or track is tracked at once. Any other goal
//! becomes one suggestion for the chat (Track / Not now), and a chat asks at
//! most once. With no marker, a local check of the player's own words (a
//! cue like "I want to" or "trying to" plus a game they own or a well-known
//! short name) puts a goal in the quiet tray only. Progress from chat moves
//! a tracked goal only when the player's own message says that number, so a
//! model's guess never moves a bar.

use super::typed::{find_game, number, words};
use super::{parse_goal, GameRef, GoalKind, GoalState, KnownGame, NewGoal, Objectives, Origin};

/// The one rule line both apps add to the chat model's rules.
pub const GOAL_MARKER_RULE: &str = "When they say they want to reach something in a game they play, add one line: GAME_GOAL: game | goal in a few words | count, finish, achievements, skill, rating or custom | target number. When they say where they are now on such a goal, add one line: GAME_PROGRESS: game | goal | number. Never mention these lines.";

pub const GOAL_PREFIX: &str = "GAME_GOAL:";
pub const PROGRESS_PREFIX: &str = "GAME_PROGRESS:";

/// More than this many markers in one reply are ignored.
const MARKERS_MAX: usize = 3;

/// Words that say the player wants a goal tracked now.
const TRACK_NOW: [&str; 14] = [
    "add this goal",
    "add that goal",
    "add it as a goal",
    "add a goal",
    "add to my goals",
    "add it to my goals",
    "track this goal",
    "track that goal",
    "track this",
    "track that",
    "set a goal",
    "make it a goal",
    "make that a goal",
    "new goal",
];

/// Words that mark a goal in the player's own message.
const CUES: [&str; 16] = [
    "i want to",
    "i wanna",
    "want to get",
    "trying to",
    "try to",
    "i need to",
    "my goal",
    "goal is",
    "going for",
    "aiming for",
    "i'm going to",
    "im going to",
    "gonna",
    "working on",
    "working towards",
    "working toward",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Marker {
    Goal {
        game: String,
        goal: String,
        kind: Option<GoalKind>,
        target: Option<u32>,
    },
    Progress {
        game: String,
        goal: String,
        value: u32,
    },
}

fn kind_word(w: &str) -> Option<GoalKind> {
    Some(match w.trim().to_lowercase().as_str() {
        "count" => GoalKind::Count,
        "finish" => GoalKind::Finish,
        "achievements" | "achievement" => GoalKind::Achievements,
        "skill" | "level" => GoalKind::Skill,
        "rating" | "rank" => GoalKind::Rating,
        "custom" => GoalKind::Custom,
        _ => return None,
    })
}

fn field_number(s: &str) -> Option<u32> {
    words(s).iter().find_map(|w| number(w))
}

/// The marker lines in a reply, at most three.
pub fn markers(reply: &str) -> Vec<Marker> {
    reply
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix(GOAL_PREFIX) {
                let f: Vec<&str> = rest.split('|').map(str::trim).collect();
                let goal = f.get(1).copied().unwrap_or_default();
                if goal.is_empty() {
                    return None;
                }
                return Some(Marker::Goal {
                    game: f[0].to_string(),
                    goal: goal.to_string(),
                    kind: f.get(2).and_then(|k| kind_word(k)),
                    target: f.get(3).and_then(|t| field_number(t)),
                });
            }
            let rest = line.strip_prefix(PROGRESS_PREFIX)?;
            let f: Vec<&str> = rest.split('|').map(str::trim).collect();
            Some(Marker::Progress {
                game: f.first().copied().unwrap_or_default().to_string(),
                goal: f.get(1).copied().unwrap_or_default().to_string(),
                value: f.get(2).and_then(|v| field_number(v))?,
            })
        })
        .take(MARKERS_MAX)
        .collect()
}

/// True for a marker line, which never shows in the chat.
pub fn is_marker_line(line: &str) -> bool {
    let t = line.trim();
    t.starts_with(GOAL_PREFIX) || t.starts_with(PROGRESS_PREFIX)
}

fn lower_words(s: &str) -> String {
    format!(" {} ", words(s).join(" "))
}

fn says(text: &str, phrase: &str) -> bool {
    lower_words(text).contains(&lower_words(phrase))
}

/// True when the player asked for a goal to be added or tracked now.
pub fn wants_tracking(said: &str) -> bool {
    TRACK_NOW.iter().any(|p| says(said, p))
}

/// A goal from a `GAME_GOAL` line. The game is matched to the library;
/// a game the player doesn't own keeps the name the model wrote.
fn from_marker(
    game: &str,
    goal: &str,
    kind: Option<GoalKind>,
    target: Option<u32>,
    games: &[KnownGame<'_>],
) -> NewGoal {
    let mut new = parse_goal(goal, games);
    if let Some((found, _)) = find_game(game, games) {
        new.game = found;
    } else if new.game.name.is_empty() && !game.trim().is_empty() {
        new.game = GameRef {
            name: game.trim().to_string(),
            ..GameRef::default()
        };
    }
    if let Some(k) = kind {
        new.kind = k;
    }
    if new.kind == GoalKind::Finish {
        new.target = 1;
    } else if let Some(t) = target {
        new.target = t;
    }
    new.origin = Origin::Chat;
    new
}

/// The part of a message after "add a goal:" or the like, else the sentence.
fn after_track_words(sentence: &str) -> &str {
    match sentence.split_once(':') {
        Some((head, tail)) if wants_tracking(head) && !tail.trim().is_empty() => tail.trim(),
        _ => sentence,
    }
}

/// A goal in the player's own words: a sentence with a cue (or an ask to
/// track) and a game from their library or a well-known short name.
pub fn detect(said: &str, games: &[KnownGame<'_>]) -> Option<NewGoal> {
    said.split(['.', '!', '?', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter(|s| wants_tracking(s) || CUES.iter().any(|c| says(s, c)))
        .find_map(|s| {
            let s = after_track_words(s);
            find_game(s, games)?;
            let mut new = parse_goal(s, games);
            new.origin = Origin::Detected;
            (!new.text.is_empty()).then_some(new)
        })
}

/// What a turn did to the goals.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TurnGoals {
    /// Goals tracked because the player asked.
    pub tracked: Vec<String>,
    /// A goal the model noticed, to ask about under the reply.
    pub ask: Option<String>,
    /// A goal from the local check, for the quiet tray only.
    pub quiet: Option<String>,
    /// Tracked goals moved by progress the player said.
    pub moved: Vec<String>,
}

impl TurnGoals {
    pub fn changed(&self) -> bool {
        !self.tracked.is_empty()
            || self.ask.is_some()
            || self.quiet.is_some()
            || !self.moved.is_empty()
    }
}

/// The tracked goal a progress line is about: same game, and the goal's
/// words when more than one goal tracks that game.
fn progress_target(
    o: &Objectives,
    game: &str,
    goal: &str,
    games: &[KnownGame<'_>],
) -> Option<String> {
    let name = find_game(game, games).map_or_else(|| game.trim().to_string(), |(g, _)| g.name);
    let on_game: Vec<_> = o
        .goals
        .iter()
        .filter(|g| g.state == GoalState::Tracking)
        .filter(|g| !name.is_empty() && g.game.name.eq_ignore_ascii_case(&name))
        .collect();
    match on_game.as_slice() {
        [one] => Some(one.id.clone()),
        many => {
            let want = lower_words(goal);
            many.iter()
                .find(|g| {
                    let have = lower_words(&g.text);
                    want.contains(&have) || have.contains(&want)
                })
                .map(|g| g.id.clone())
        }
    }
}

/// A cheap first look at a finished turn, so most turns skip reading the
/// game library and the goal file: a goal line in the reply, or words in
/// what the player said that could hold a goal.
pub fn worth_a_look(said: &str, reply: &str) -> bool {
    if !markers(reply).is_empty() || wants_tracking(said) || detect(said, &[]).is_some() {
        return true;
    }
    let lower = said.to_lowercase();
    [
        "want",
        "trying",
        "goal",
        "going for",
        "aiming",
        "gonna",
        "working on",
        "need to",
    ]
    .iter()
    .any(|cue| lower.contains(cue))
}

/// Reads one finished turn: the player's message and the reply.
pub fn read_turn(
    o: &mut Objectives,
    chat: &str,
    said: &str,
    reply: &str,
    games: &[KnownGame<'_>],
    now: u64,
) -> TurnGoals {
    let mut out = TurnGoals::default();
    let asked = wants_tracking(said);
    let found = markers(reply);
    let mut any_goal = false;
    for m in &found {
        match m {
            Marker::Goal {
                game,
                goal,
                kind,
                target,
            } => {
                any_goal = true;
                let mut new = from_marker(game, goal, *kind, *target, games);
                new.chat = Some(chat.to_string());
                if asked {
                    if let Ok(id) = o.add(new, now) {
                        out.tracked.push(id);
                    }
                } else if out.ask.is_none() {
                    out.ask = o.suggest(new, now);
                }
            }
            Marker::Progress { game, goal, value } => {
                // Only a number the player said moves a bar.
                if !words(said).iter().any(|w| number(w) == Some(*value)) {
                    continue;
                }
                if let Some(id) = progress_target(o, game, goal, games) {
                    if o.set_progress(&id, *value, false, now) {
                        out.moved.push(id);
                    }
                }
            }
        }
    }
    if !any_goal {
        if let Some(mut new) = detect(said, games) {
            new.chat = Some(chat.to_string());
            if asked {
                new.origin = Origin::Chat;
                if let Ok(id) = o.add(new, now) {
                    out.tracked.push(id);
                }
            } else {
                out.quiet = o.suggest(new, now);
            }
        }
    }
    out
}

/// Keeps marker lines off a streamed reply. A line is held only while it
/// could still turn into a marker; anything else passes as it comes.
#[derive(Debug, Default)]
pub struct MarkerFilter {
    line: String,
    state: Line,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Line {
    #[default]
    Undecided,
    Show,
    Drop,
}

impl MarkerFilter {
    /// The part of `chunk` that is shown.
    pub fn feed(&mut self, chunk: &str) -> String {
        let mut out = String::new();
        for c in chunk.chars() {
            match self.state {
                Line::Show => {
                    out.push(c);
                    if c == '\n' {
                        self.state = Line::Undecided;
                    }
                }
                Line::Drop => {
                    if c == '\n' {
                        self.state = Line::Undecided;
                    }
                }
                Line::Undecided => {
                    self.line.push(c);
                    let t = self.line.trim_start();
                    if c == '\n' {
                        if !is_marker_line(&self.line) {
                            out.push_str(&self.line);
                        }
                        self.line.clear();
                    } else if t.is_empty() {
                    } else if is_marker_line(t) {
                        self.line.clear();
                        self.state = Line::Drop;
                    } else if !(GOAL_PREFIX.starts_with(t) || PROGRESS_PREFIX.starts_with(t)) {
                        out.push_str(&self.line);
                        self.line.clear();
                        self.state = Line::Show;
                    }
                }
            }
        }
        out
    }

    /// End of the reply: a held tail that turned out to be text.
    pub fn finish(&mut self) -> String {
        let tail = std::mem::take(&mut self.line);
        self.state = Line::Undecided;
        if is_marker_line(&tail) {
            String::new()
        } else {
            tail
        }
    }
}

/// A saved reply without its marker lines.
pub fn without_markers(text: &str) -> String {
    let mut f = MarkerFilter::default();
    let mut out = f.feed(text);
    out.push_str(&f.finish());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Monday 2 Nov 2026, 12:00 UTC.
    const NOW: u64 = 1_793_620_800_000;

    const LIB: [KnownGame<'static>; 2] = [
        KnownGame {
            name: "Elden Ring",
            steam_app_id: Some(1_245_620),
            source: None,
        },
        KnownGame {
            name: "Hades",
            steam_app_id: Some(1_145_360),
            source: None,
        },
    ];

    #[test]
    fn marker_lines_read_and_hide() {
        let reply = "Nice, fishing trawler is fast.\nGAME_GOAL: OSRS | 99 Fishing | skill | 99\nGAME_PROGRESS: Old School RuneScape | 99 Fishing | level 82\nGAME_GOAL: | | |";
        assert_eq!(
            markers(reply),
            vec![
                Marker::Goal {
                    game: "OSRS".into(),
                    goal: "99 Fishing".into(),
                    kind: Some(GoalKind::Skill),
                    target: Some(99),
                },
                Marker::Progress {
                    game: "Old School RuneScape".into(),
                    goal: "99 Fishing".into(),
                    value: 82,
                },
            ]
        );
        assert!(is_marker_line("  GAME_GOAL: x | y"));
        assert!(!is_marker_line("My game goal: x"));
        assert!(
            GOAL_MARKER_RULE.contains(GOAL_PREFIX) && GOAL_MARKER_RULE.contains(PROGRESS_PREFIX)
        );
    }

    #[test]
    fn a_marker_asks_once_per_chat_and_an_ask_to_track_tracks() {
        let mut o = Objectives::default();
        let reply = "Good plan.\nGAME_GOAL: osrs | 99 fishing | skill | 99";
        let t = read_turn(&mut o, "c1", "i want 99 fishing", reply, &LIB, NOW);
        let ask = t.ask.clone().expect("asks");
        assert!(t.tracked.is_empty() && t.quiet.is_none());
        let g = &o.goals[0];
        assert_eq!((g.state, g.origin), (GoalState::Suggested, Origin::Chat));
        assert_eq!(g.game.name, "Old School RuneScape");
        assert_eq!((g.kind, g.target), (GoalKind::Skill, 99));
        assert_eq!(g.chat.as_deref(), Some("c1"));
        // The same chat never asks twice.
        let again = "GAME_GOAL: Hades | finish | finish | 1";
        assert_eq!(
            read_turn(&mut o, "c1", "and hades", again, &LIB, NOW).ask,
            None
        );
        // Asked to track: tracked at once, in another chat.
        let t = read_turn(&mut o, "c2", "add this goal please", again, &LIB, NOW);
        assert_eq!(t.tracked.len(), 1);
        let hades = o.goals.iter().find(|g| g.game.name == "Hades").unwrap();
        assert_eq!(
            (hades.state, hades.kind, hades.target),
            (GoalState::Tracking, GoalKind::Finish, 1)
        );
        assert_ne!(t.tracked[0], ask);
    }

    #[test]
    fn progress_moves_a_goal_only_on_the_players_own_number() {
        let mut o = Objectives::default();
        read_turn(
            &mut o,
            "c1",
            "add a goal: 99 fishing in osrs",
            "",
            &LIB,
            NOW,
        );
        assert_eq!(o.goals[0].state, GoalState::Tracking);
        let reply = "GAME_PROGRESS: osrs | fishing | 82";
        let t = read_turn(&mut o, "c2", "almost there!", reply, &LIB, NOW);
        assert!(t.moved.is_empty(), "the model's number alone moves nothing");
        assert_eq!(o.goals[0].current, 0);
        let t = read_turn(&mut o, "c2", "just hit 82 fishing", reply, &LIB, NOW + 1);
        assert_eq!(t.moved, vec![o.goals[0].id.clone()]);
        assert_eq!(o.goals[0].current, 82);
        assert!(!o.goals[0].checked);
    }

    #[test]
    fn the_local_check_needs_a_cue_and_a_known_game_and_stays_quiet() {
        let mut o = Objectives::default();
        let t = read_turn(
            &mut o,
            "c1",
            "Elden Ring is so good. I want to finish elden ring this month!",
            "Go for it.",
            &LIB,
            NOW,
        );
        let id = t.quiet.expect("quiet suggestion");
        assert_eq!(t.ask, None);
        let g = o.goals.iter().find(|g| g.id == id).unwrap();
        assert_eq!(
            (g.state, g.origin, g.kind),
            (GoalState::Suggested, Origin::Detected, GoalKind::Finish)
        );
        assert_eq!(g.game.name, "Elden Ring");
        // No cue, or no game it knows: nothing.
        let mut o = Objectives::default();
        assert!(!read_turn(&mut o, "c1", "elden ring is hard", "", &LIB, NOW).changed());
        assert!(!read_turn(&mut o, "c2", "i want to finish my taxes", "", &LIB, NOW).changed());
        assert!(o.goals.is_empty());
        assert_eq!(
            detect("trying to win 10 matches in cs2", &[]).map(|g| (g.kind, g.target)),
            Some((GoalKind::Count, 10))
        );
        // The first look lets most turns through without reading anything.
        assert!(worth_a_look("i want 99 fishing", ""));
        assert!(worth_a_look(
            "ok",
            "Sure.\nGAME_GOAL: osrs | 99 fishing | skill | 99"
        ));
        assert!(!worth_a_look("what's the weather", "Sunny."));
    }

    #[test]
    fn the_stream_filter_drops_marker_lines_split_across_chunks() {
        let reply = "Nice.\nGAME_GOAL: osrs | 99 fishing | skill | 99\n  GAME_GA is a word\nGAME_PROGRESS: osrs | fishing | 82";
        for size in [1, 2, 3, 5, 7, 64] {
            let mut f = MarkerFilter::default();
            let chars: Vec<char> = reply.chars().collect();
            let mut out = String::new();
            for chunk in chars.chunks(size) {
                out.push_str(&f.feed(&chunk.iter().collect::<String>()));
            }
            out.push_str(&f.finish());
            assert_eq!(out, "Nice.\n  GAME_GA is a word\n", "chunks of {size}");
        }
        assert_eq!(
            without_markers("Plain reply, no lines."),
            "Plain reply, no lines."
        );
        assert_eq!(without_markers("GAME"), "GAME");
    }
}
