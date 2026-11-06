//! A goal typed in plain words ("99 fishing in osrs", "win 10 ranked
//! matches", "finish Elden Ring") turned into a [`NewGoal`]. The game is
//! matched against the player's own library, longest name first; the kind
//! and target come from a few plain cues. Anything else is a custom goal in
//! the player's own words.

use super::{GameRef, GoalKind, NewGoal, Origin};

/// A game the player owns or plays: its name and Steam app id if known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownGame<'a> {
    pub name: &'a str,
    pub steam_app_id: Option<u32>,
    pub source: Option<&'a str>,
}

/// Short names people use for games whose full name they rarely type.
const ALIASES: [(&str, &str); 8] = [
    ("osrs", "Old School RuneScape"),
    ("rs3", "RuneScape"),
    ("wow", "World of Warcraft"),
    ("poe", "Path of Exile"),
    ("poe2", "Path of Exile 2"),
    ("cs2", "Counter-Strike 2"),
    ("lol", "League of Legends"),
    ("ffxiv", "Final Fantasy XIV"),
];

const FINISH: [&str; 5] = [
    "finish",
    "beat",
    "complete",
    "roll credits",
    "clear the game",
];
const RATING: [&str; 8] = [
    "rating",
    "rank",
    "elo",
    "mmr",
    "ilvl",
    "item level",
    "gear score",
    "trophies",
];
const COUNT: [&str; 15] = [
    "win", "wins", "match", "matches", "game", "games", "run", "runs", "session", "sessions",
    "kill", "kills", "kc", "times", "raids",
];

pub(super) fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric() && c != '%')
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

pub(super) fn number(w: &str) -> Option<u32> {
    w.trim_end_matches('%').parse().ok().filter(|n| *n > 0)
}

/// The game named in `text`: an alias, or the longest library name found in
/// it as whole words.
pub(super) fn find_game<'a>(text: &str, games: &[KnownGame<'a>]) -> Option<(GameRef, String)> {
    let lower = format!(" {} ", words(text).join(" "));
    for (alias, full) in ALIASES {
        if lower.contains(&format!(" {alias} ")) {
            let known = games.iter().find(|g| g.name.eq_ignore_ascii_case(full));
            return Some((
                GameRef {
                    name: known.map_or(full, |g| g.name).to_string(),
                    steam_app_id: known.and_then(|g| g.steam_app_id),
                    source: known.and_then(|g| g.source.map(str::to_string)),
                },
                alias.to_string(),
            ));
        }
    }
    games
        .iter()
        .filter_map(|g| {
            let key = words(g.name).join(" ");
            (!key.is_empty() && lower.contains(&format!(" {key} "))).then_some((g, key))
        })
        .max_by_key(|(_, key)| key.len())
        .map(|(g, key)| {
            (
                GameRef {
                    name: g.name.to_string(),
                    steam_app_id: g.steam_app_id,
                    source: g.source.map(str::to_string),
                },
                key,
            )
        })
}

/// The goal's own words with the game and its glue words taken out:
/// "99 fishing in osrs" → "99 fishing".
fn goal_words(text: &str, game_words: Option<&str>) -> String {
    let mut out = format!(
        " {} ",
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    );
    if let Some(game) = game_words {
        let lower = out.to_lowercase();
        // Byte offsets only line up when lowercasing kept every length.
        if lower.len() == out.len() {
            if let Some(at) = lower.find(&format!(" {game} ")) {
                out.replace_range(at..at + game.len() + 1, "");
            }
        }
    }
    let mut s = out.trim().to_string();
    for tail in [" in", " on", " for", " at", " of"] {
        if s.to_lowercase().ends_with(tail) {
            s.truncate(s.len() - tail.len());
        }
    }
    for head in [
        "i want to ",
        "i'm going to ",
        "im going to ",
        "want to ",
        "trying to ",
        "try to ",
    ] {
        if s.to_lowercase().starts_with(head) {
            s = s[head.len()..].to_string();
        }
    }
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => s,
    }
}

/// Reads a typed goal. Unmatched games leave the goal unlinked; the player
/// sees it in their own words either way.
pub fn parse_goal(text: &str, games: &[KnownGame<'_>]) -> NewGoal {
    let found = find_game(text, games);
    let w = words(text);
    let lower = format!(" {} ", w.join(" "));
    let has = |cue: &str| lower.contains(&format!(" {} ", cue.trim()));
    let first_number = w.iter().find_map(|x| number(x));
    let level_number = w
        .windows(2)
        .find(|p| matches!(p[0].as_str(), "level" | "lvl" | "lv"))
        .and_then(|p| number(&p[1]));

    let (kind, target) = if FINISH.iter().any(|c| has(c)) {
        (GoalKind::Finish, 1)
    } else if w.iter().any(|x| x.starts_with("achievement")) {
        match first_number {
            Some(n) if !w.iter().any(|x| x.ends_with('%')) => (GoalKind::Achievements, n),
            _ => (GoalKind::Custom, 1),
        }
    } else if let (true, Some(n)) = (has("total level"), first_number) {
        (GoalKind::Skill, n)
    } else if let Some(n) = level_number {
        (GoalKind::Skill, n)
    } else if w.iter().any(|x| x == "99") {
        (GoalKind::Skill, 99)
    } else if let (true, Some(n)) = (RATING.iter().any(|c| has(c)), first_number) {
        (GoalKind::Rating, n)
    } else if let (true, Some(n)) = (COUNT.iter().any(|c| has(c)), first_number) {
        (GoalKind::Count, n)
    } else {
        (GoalKind::Custom, 1)
    };
    let (game, game_words) = match found {
        Some((g, key)) => (g, Some(key)),
        None => (GameRef::default(), None),
    };
    NewGoal {
        text: goal_words(text, game_words.as_deref()),
        game,
        kind,
        current: 0,
        target,
        origin: Origin::Typed,
        chat: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIB: [KnownGame<'static>; 4] = [
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
        KnownGame {
            name: "Hades II",
            steam_app_id: Some(1_145_350),
            source: None,
        },
        KnownGame {
            name: "Old School RuneScape",
            steam_app_id: None,
            source: Some("osrs"),
        },
    ];

    fn read(text: &str) -> (String, String, GoalKind, u32) {
        let g = parse_goal(text, &LIB);
        (g.game.name, g.text, g.kind, g.target)
    }

    #[test]
    fn plain_goals_find_their_game_kind_and_target() {
        assert_eq!(
            read("99 fishing in osrs"),
            (
                "Old School RuneScape".into(),
                "99 fishing".into(),
                GoalKind::Skill,
                99
            )
        );
        let osrs = parse_goal("99 fishing in osrs", &LIB);
        assert_eq!(osrs.game.source.as_deref(), Some("osrs"));
        assert_eq!(
            read("Finish Elden Ring"),
            ("Elden Ring".into(), "Finish".into(), GoalKind::Finish, 1)
        );
        assert_eq!(
            parse_goal("finish elden ring", &LIB).game.steam_app_id,
            Some(1_245_620)
        );
        // The longest name wins: Hades II, not Hades.
        assert_eq!(
            read("I want to clear 10 runs in Hades II"),
            (
                "Hades II".into(),
                "Clear 10 runs".into(),
                GoalKind::Count,
                10
            )
        );
        assert_eq!(
            read("get 30 achievements in hades"),
            (
                "Hades".into(),
                "Get 30 achievements".into(),
                GoalKind::Achievements,
                30
            )
        );
        assert_eq!(
            read("100% achievements in Hades"),
            (
                "Hades".into(),
                "100% achievements".into(),
                GoalKind::Custom,
                1
            )
        );
    }

    #[test]
    fn levels_ratings_and_unknown_games() {
        assert_eq!(
            read("hit level 90 in poe"),
            (
                "Path of Exile".into(),
                "Hit level 90".into(),
                GoalKind::Skill,
                90
            )
        );
        assert_eq!(
            read("Mythic+ rating 2500 on wow"),
            (
                "World of Warcraft".into(),
                "Mythic+ rating 2500".into(),
                GoalKind::Rating,
                2500
            )
        );
        assert_eq!(
            read("win 5 matches"),
            (String::new(), "Win 5 matches".into(), GoalKind::Count, 5)
        );
        assert_eq!(
            read("learn to parry"),
            (String::new(), "Learn to parry".into(), GoalKind::Custom, 1)
        );
        let g = parse_goal("beat the final boss", &[]);
        assert_eq!(
            (g.kind, g.origin, g.current),
            (GoalKind::Finish, Origin::Typed, 0)
        );
    }
}
