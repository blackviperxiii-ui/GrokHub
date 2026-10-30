//! Per-match timelines on disk for the in-game watcher (design:
//! `docs/design/game-watcher/`). At match end the timeline is written once,
//! atomically, to `{config}/game-watch/<game>/<match-start>.json`, and old
//! matches are pruned. [`match_summary`] is the short plain text a model sees
//! when the user asks about a match; it never carries the raw feed.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::game_watch::{pointers, recap, WatchEvent};

/// The folder under the config dir that holds one folder per game.
pub const DIR: &str = "game-watch";
/// Matches that started longer ago than this are pruned.
pub const KEEP_MS: u64 = 30 * 24 * 60 * 60 * 1000;
/// The newest matches kept per game.
pub const KEEP_PER_GAME: usize = 200;

/// One finished match as it is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchRecord {
    /// The game's folder name, such as `cs2`.
    pub game: String,
    /// When the match started, in ms since the epoch. Also the file name.
    pub started_ms: u64,
    pub ended_ms: u64,
    pub events: Vec<WatchEvent>,
}

/// A game folder name is lowercase letters, digits, `-` and `_`, so it can
/// never climb out of [`DIR`].
fn valid_game(game: &str) -> bool {
    !game.is_empty()
        && game
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// The folder holding one game's matches.
pub fn game_dir(config_dir: &Path, game: &str) -> Option<PathBuf> {
    valid_game(game).then(|| config_dir.join(DIR).join(game))
}

/// Where one match is stored. `None` for a game name that isn't a plain slug.
pub fn match_path(config_dir: &Path, game: &str, started_ms: u64) -> Option<PathBuf> {
    game_dir(config_dir, game).map(|d| d.join(format!("{started_ms}.json")))
}

/// Write a finished match through a temp file and a rename, then prune that
/// game's folder. Returns the file written.
pub fn save_match(config_dir: &Path, rec: &MatchRecord, now_ms: u64) -> Result<PathBuf, String> {
    let path = match_path(config_dir, &rec.game, rec.started_ms)
        .ok_or_else(|| format!("not a game folder name: {:?}", rec.game))?;
    let text = serde_json::to_string_pretty(rec).map_err(|e| e.to_string())?;
    crate::pulse_writer::write_atomic(&path, &text)?;
    prune(config_dir, &rec.game, now_ms);
    Ok(path)
}

/// One stored match. `None` when the file is missing or not a match.
pub fn load_match(path: &Path) -> Option<MatchRecord> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// One game's stored matches as (start ms, file), newest first. Files that
/// aren't `<digits>.json` (a temp file mid-write) are skipped.
pub fn list_matches(config_dir: &Path, game: &str) -> Vec<(u64, PathBuf)> {
    let Some(dir) = game_dir(config_dir, game) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<(u64, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_str()?;
            let start = name.strip_suffix(".json")?;
            if start.is_empty() || !start.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            Some((start.parse().ok()?, path))
        })
        .collect();
    out.sort_by_key(|m| std::cmp::Reverse(m.0));
    out
}

/// Drop one game's matches that started over [`KEEP_MS`] before `now_ms`, and
/// any beyond the newest [`KEEP_PER_GAME`]. Returns how many were removed.
pub fn prune(config_dir: &Path, game: &str, now_ms: u64) -> usize {
    let cutoff = now_ms.saturating_sub(KEEP_MS);
    list_matches(config_dir, game)
        .into_iter()
        .enumerate()
        .filter(|(i, (start, _))| *i >= KEEP_PER_GAME || *start < cutoff)
        .filter(|(_, (_, path))| std::fs::remove_file(path).is_ok())
        .count()
}

/// The short plain summary a model sees for "Ask about this match": the
/// score, K/D, longest win streak and the pointers that fired. Built from the
/// recap and rules only, never the raw feed.
pub fn match_summary(timeline: &[WatchEvent]) -> String {
    let r = recap(timeline);
    let outcome = match r.won.cmp(&r.lost) {
        std::cmp::Ordering::Greater => "a win",
        std::cmp::Ordering::Less => "a loss",
        std::cmp::Ordering::Equal => "a draw",
    };
    let kd = f64::from(r.kills) / f64::from(r.deaths.max(1));
    let rounds = |n: u32| if n == 1 { "round" } else { "rounds" };
    let mut out = format!(
        "Score: {}-{}, {outcome}.\nKills {}, deaths {}, K/D {kd:.2}.\nLongest win streak: {} {}.\n",
        r.won,
        r.lost,
        r.kills,
        r.deaths,
        r.longest_win_streak,
        rounds(r.longest_win_streak)
    );
    // Each pointer once, in the order it first fired, with how often it did.
    let mut fired: Vec<(String, u32)> = Vec::new();
    for p in pointers(timeline) {
        match fired.iter_mut().find(|(t, _)| *t == p.text) {
            Some((_, n)) => *n += 1,
            None => fired.push((p.text, 1)),
        }
    }
    if fired.is_empty() {
        out.push_str("Pointers that fired: none.");
    } else {
        out.push_str("Pointers that fired:");
        for (text, n) in fired {
            out.push_str("\n- ");
            out.push_str(&text);
            if n > 1 {
                out.push_str(&format!(" ({n} times)"));
            }
        }
    }
    out
}

/// What "Ask about this match" asks when the user types nothing.
pub const DEFAULT_QUESTION: &str = "How did this match go, and what should I work on next?";

const ASK_RULES: &str = "You help a player look back at a match they just played. All you know is the summary below, made from the game's own event feed. Answer their question in a few plain sentences. If the summary doesn't say, tell them so instead of guessing.";

/// The (role, text) messages for one question about a match, built from its
/// summary. An empty question asks [`DEFAULT_QUESTION`].
pub fn ask_messages(summary: &str, question: &str) -> Vec<(String, String)> {
    let question = match question.trim() {
        "" => DEFAULT_QUESTION,
        q => q,
    };
    vec![
        ("system".into(), ASK_RULES.into()),
        (
            "user".into(),
            format!("Match summary:\n{summary}\n\nQuestion: {question}"),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_watch::EventKind;

    const DAY_MS: u64 = 24 * 60 * 60 * 1000;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-game-watch-store-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn ev(at_ms: u64, kind: EventKind) -> WatchEvent {
        WatchEvent { at_ms, kind }
    }

    fn rec(game: &str, started_ms: u64) -> MatchRecord {
        MatchRecord {
            game: game.into(),
            started_ms,
            ended_ms: started_ms + 1_000,
            events: vec![ev(started_ms, EventKind::Kill)],
        }
    }

    fn starts(dir: &Path, game: &str) -> Vec<u64> {
        list_matches(dir, game)
            .into_iter()
            .map(|(s, _)| s)
            .collect()
    }

    #[test]
    fn a_match_round_trips_through_its_file() {
        let dir = temp("round-trip");
        let m = MatchRecord {
            game: "cs2".into(),
            started_ms: 1_700_000_000_000,
            ended_ms: 1_700_002_400_000,
            events: vec![
                ev(1_700_000_010_000, EventKind::RoundStart { round: 1 }),
                ev(1_700_000_020_000, EventKind::Kill),
                ev(
                    1_700_000_090_000,
                    EventKind::RoundEnd {
                        round: 1,
                        won: true,
                    },
                ),
                ev(1_700_002_400_000, EventKind::MatchEnd { won: 13, lost: 9 }),
            ],
        };
        let path = save_match(&dir, &m, 1_700_002_400_000).unwrap();
        assert_eq!(
            path,
            dir.join("game-watch")
                .join("cs2")
                .join("1700000000000.json")
        );
        assert_eq!(load_match(&path), Some(m));
        // Nothing is left behind from the temp file.
        assert_eq!(
            std::fs::read_dir(dir.join("game-watch").join("cs2"))
                .unwrap()
                .count(),
            1
        );
        assert_eq!(load_match(&dir.join("missing.json")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_game_name_that_is_not_a_slug_is_refused() {
        let dir = temp("slug");
        assert_eq!(match_path(&dir, "../cs2", 1), None);
        assert_eq!(match_path(&dir, "CS2", 1), None);
        assert_eq!(match_path(&dir, "", 1), None);
        assert_eq!(
            save_match(&dir, &rec("../x", 1), 1),
            Err("not a game folder name: \"../x\"".to_string())
        );
        assert_eq!(
            match_path(&dir, "dota2", 5),
            Some(dir.join("game-watch").join("dota2").join("5.json"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn matches_older_than_thirty_days_are_pruned() {
        let dir = temp("age");
        let now = 100 * DAY_MS;
        let game = dir.join("game-watch").join("cs2");
        std::fs::create_dir_all(&game).unwrap();
        for start in [now - 31 * DAY_MS, now - 30 * DAY_MS, now - DAY_MS] {
            std::fs::write(game.join(format!("{start}.json")), "{}").unwrap();
        }
        // A temp file mid-write and a stray file are left alone.
        std::fs::write(game.join("5.1234.tmp"), "").unwrap();
        std::fs::write(game.join("notes.json"), "").unwrap();
        // Another game's old match is not this game's to prune.
        save_match(&dir, &rec("dota2", 1), 1).unwrap();
        assert_eq!(prune(&dir, "cs2", now), 1);
        assert_eq!(starts(&dir, "cs2"), vec![now - DAY_MS, now - 30 * DAY_MS]);
        assert!(game.join("5.1234.tmp").exists() && game.join("notes.json").exists());
        assert_eq!(starts(&dir, "dota2"), vec![1]);
        // Saving a match prunes its own game.
        save_match(&dir, &rec("dota2", now), now).unwrap();
        assert_eq!(starts(&dir, "dota2"), vec![now]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_newest_two_hundred_matches_per_game_are_kept() {
        let dir = temp("count");
        let now = 10 * DAY_MS;
        let game = dir.join("game-watch").join("cs2");
        std::fs::create_dir_all(&game).unwrap();
        for i in 0..205u64 {
            std::fs::write(game.join(format!("{}.json", now - 1_000 * (i + 1))), "{}").unwrap();
        }
        assert_eq!(prune(&dir, "cs2", now), 5);
        let kept = starts(&dir, "cs2");
        assert_eq!(kept.len(), 200);
        assert_eq!(kept.first(), Some(&(now - 1_000)));
        assert_eq!(kept.last(), Some(&(now - 200_000)));
        // The 201st save pushes the oldest one out.
        save_match(&dir, &rec("cs2", now), now).unwrap();
        let kept = starts(&dir, "cs2");
        assert_eq!(kept.len(), 200);
        assert_eq!(
            (kept.first(), kept.last()),
            (Some(&now), Some(&(now - 199_000)))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn round(t: &mut Vec<WatchEvent>, start: u64, round: u32, death_after: Option<u64>, won: bool) {
        t.push(ev(start, EventKind::RoundStart { round }));
        t.push(ev(start + 5_000, EventKind::Kill));
        if let Some(after) = death_after {
            t.push(ev(start + after, EventKind::Death));
        }
        t.push(ev(start + 90_000, EventKind::RoundEnd { round, won }));
    }

    #[test]
    fn the_summary_gives_score_kd_streak_and_pointers_only() {
        let mut t = Vec::new();
        // Won 1-2, then lost 3-5 with early deaths, then won 6.
        round(&mut t, 0, 1, None, true);
        round(&mut t, 100_000, 2, Some(60_000), true);
        for (i, r) in (3..=5).enumerate() {
            round(&mut t, (i as u64 + 2) * 100_000, r, Some(10_000), false);
        }
        round(&mut t, 500_000, 6, None, true);
        t.push(ev(600_000, EventKind::MatchEnd { won: 13, lost: 9 }));
        assert_eq!(
            match_summary(&t),
            "Score: 13-9, a win.\n\
             Kills 6, deaths 4, K/D 1.50.\n\
             Longest win streak: 2 rounds.\n\
             Pointers that fired:\n\
             - You died in the first 20 seconds in 3 of the last 5 rounds. Try a slower opening.\n\
             - Three rounds lost in a row. A save round or a timeout might reset things."
        );
    }

    #[test]
    fn a_quiet_match_summary_says_no_pointers_fired() {
        let mut t = Vec::new();
        round(&mut t, 0, 1, Some(60_000), false);
        assert_eq!(
            match_summary(&t),
            "Score: 0-1, a loss.\nKills 1, deaths 1, K/D 1.00.\nLongest win streak: 0 rounds.\nPointers that fired: none."
        );
        assert_eq!(
            match_summary(&[]),
            "Score: 0-0, a draw.\nKills 0, deaths 0, K/D 0.00.\nLongest win streak: 0 rounds.\nPointers that fired: none."
        );
    }

    #[test]
    fn a_pointer_that_fires_twice_is_listed_once_with_its_count() {
        let mut t = Vec::new();
        let lost = |at: u64, round: u32| ev(at, EventKind::RoundEnd { round, won: false });
        t.extend((1..=3).map(|r| lost(u64::from(r), r)));
        t.push(ev(
            4,
            EventKind::RoundEnd {
                round: 4,
                won: true,
            },
        ));
        t.extend((5..=7).map(|r| lost(u64::from(r), r)));
        assert!(match_summary(&t).ends_with(
            "Pointers that fired:\n- Three rounds lost in a row. A save round or a timeout might reset things. (2 times)"
        ));
    }

    #[test]
    fn the_ask_carries_the_summary_and_the_question() {
        let m = ask_messages("Score: 13-9, a win.", "  Why did I lose round 4? ");
        assert_eq!(m[0].0, "system");
        assert_eq!(
            m[1],
            (
                "user".to_string(),
                "Match summary:\nScore: 13-9, a win.\n\nQuestion: Why did I lose round 4?"
                    .to_string()
            )
        );
        assert_eq!(
            ask_messages("S", " ")[1].1,
            "Match summary:\nS\n\nQuestion: How did this match go, and what should I work on next?"
        );
    }
}
