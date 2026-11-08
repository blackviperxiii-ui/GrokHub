//! Progress from game data the player already allowed: the per-game counts
//! the opt-in Steam achievements check keeps, and the saved Old School
//! RuneScape hiscores reads. Both are files on disk that other features
//! wrote; nothing here talks to a game, its process or the network, so it
//! can't touch anti-cheat. A goal moved this way shows "Checked".

use std::collections::BTreeMap;
use std::path::Path;

use super::typed::words;
use super::{change, Goal, GoalKind, GoalState, Objectives};
use crate::osrs_hiscores::{self as osrs, OsrsStats};
use crate::steam_achievements::{self as sa, Count};

/// What the saved game data says, for [`apply_game_data`].
#[derive(Debug, Clone, Copy, Default)]
pub struct GameData<'a> {
    /// The newest OSRS hiscores read.
    pub osrs: Option<&'a OsrsStats>,
    /// Achievements unlocked and in all, by Steam app id.
    pub achievements: Option<&'a BTreeMap<u32, Count>>,
}

/// Moves tracked goals to what the game data says. Returns the goals moved.
/// A goal's first reading also becomes its start, so a 93 / 99 bar shows the
/// work already done without paying milestones for it.
pub fn apply_game_data(o: &mut Objectives, data: &GameData<'_>, now: u64) -> Vec<String> {
    let mut moved = Vec::new();
    let readings: Vec<(String, u32, Option<u32>)> = o
        .goals
        .iter()
        .filter(|g| g.state == GoalState::Tracking)
        .filter_map(|g| reading(g, data).map(|(v, t)| (g.id.clone(), v, t)))
        .collect();
    for (id, value, target) in readings {
        if let Some(g) = o.find(&id) {
            if let Some(total) = target {
                if g.target != total {
                    g.kind = GoalKind::Achievements;
                    g.target = total.max(1);
                }
            }
            if !g.checked && g.current == g.start {
                g.start = value.min(g.target);
            }
        }
        if o.set_progress(&id, value, true, now) {
            moved.push(id);
        }
    }
    moved
}

/// The value game data gives one goal, and a new target for a "100%" goal.
fn reading(g: &Goal, data: &GameData<'_>) -> Option<(u32, Option<u32>)> {
    if is_osrs(g) {
        return data
            .osrs
            .and_then(|stats| osrs_reading(g, stats))
            .map(|v| (v, None));
    }
    let count = data.achievements?.get(&g.game.steam_app_id?)?;
    let done = u32::try_from(count.done).ok()?;
    let total = u32::try_from(count.total).ok()?;
    match g.kind {
        GoalKind::Achievements => Some((done, None)),
        GoalKind::Custom if wants_all(&g.text) && total > 0 => Some((done, Some(total))),
        _ => None,
    }
}

fn is_osrs(g: &Goal) -> bool {
    g.game.source.as_deref() == Some("osrs")
        || g.game.steam_app_id == Some(osrs::APP_ID)
        || g.game.name.eq_ignore_ascii_case(osrs::NAME)
}

/// "100% achievements", "all achievements".
fn wants_all(text: &str) -> bool {
    let w = words(text);
    w.iter().any(|x| x.starts_with("achievement"))
        && w.iter().any(|x| x == "100%" || x == "100" || x == "all")
}

/// A skill level for a skill goal ("99 fishing", "2000 total level"), or a
/// boss or clue count for a count goal ("100 zulrah kills").
fn osrs_reading(g: &Goal, stats: &OsrsStats) -> Option<u32> {
    let w = words(&g.text);
    let value = match g.kind {
        GoalKind::Skill => {
            let skill = stats.skills.iter().find(|s| {
                let name = s.name.to_lowercase();
                w.contains(&name) || (name == "overall" && w.iter().any(|x| x == "total"))
            })?;
            skill.level?
        }
        GoalKind::Count => {
            let text = format!(" {} ", w.join(" "));
            let activity = stats.activities.iter().find(|a| {
                let name = words(&a.name).join(" ");
                name.len() >= 4 && text.contains(&format!(" {name} "))
            })?;
            activity.score?
        }
        _ => return None,
    };
    u32::try_from(value).ok()
}

/// A watched match ended (the opt-in Game State Integration feed the game's
/// maker supports). Count goals for that game move: "win 10 matches" on a
/// win, "play 5 matches" on any match, "500 kills" by the match's kills.
/// Returns the goals moved.
pub fn match_finished(
    o: &mut Objectives,
    game: &str,
    won: bool,
    kills: u32,
    now: u64,
) -> Vec<String> {
    let steps: Vec<(String, u32)> = o
        .goals
        .iter()
        .filter(|g| g.state == GoalState::Tracking && g.kind == GoalKind::Count)
        .filter(|g| g.game.name.eq_ignore_ascii_case(game))
        .filter_map(|g| {
            let w = words(&g.text);
            let has = |cues: &[&str]| w.iter().any(|x| cues.contains(&x.as_str()));
            let step = if has(&["win", "wins", "won"]) {
                u32::from(won)
            } else if has(&["kill", "kills"]) {
                kills
            } else if has(&["match", "matches", "game", "games"]) {
                1
            } else {
                0
            };
            (step > 0).then(|| (g.id.clone(), g.current.saturating_add(step)))
        })
        .collect();
    steps
        .into_iter()
        .filter(|(id, value)| o.set_progress(id, *value, true, now))
        .map(|(id, _)| id)
        .collect()
}

/// The most goal games put first in one achievements check.
pub const GOAL_GAMES_PER_CHECK: usize = 4;

/// Steam games with a tracked achievements goal, newest goal first, for the
/// achievements check to read before the most played ones.
pub fn goal_steam_games(o: &Objectives) -> Vec<(u32, String)> {
    let mut out: Vec<(u32, String)> = Vec::new();
    let mut goals: Vec<&Goal> = o
        .goals
        .iter()
        .filter(|g| g.state == GoalState::Tracking)
        .filter(|g| match g.kind {
            GoalKind::Achievements => true,
            GoalKind::Custom => wants_all(&g.text),
            _ => false,
        })
        .collect();
    // Newest first; goals added in the same moment keep the later one first.
    goals.reverse();
    goals.sort_by_key(|g| std::cmp::Reverse(g.created_ms));
    for g in goals {
        if let Some(id) = g.game.steam_app_id {
            if !out.iter().any(|(x, _)| *x == id) {
                out.push((id, g.game.name.clone()));
            }
        }
    }
    out.truncate(GOAL_GAMES_PER_CHECK);
    out
}

/// Reads the saved game data and moves goals by it. Writes nothing when
/// there is no game data or nothing moved. Returns the goals moved.
pub fn refresh_from_games(config_dir: &Path, now: u64) -> Vec<String> {
    let saved = osrs::load(config_dir);
    let tracker = sa::load(config_dir);
    let data = GameData {
        osrs: saved.as_ref().and_then(|s| s.latest()),
        achievements: (!tracker.counts.is_empty()).then_some(&tracker.counts),
    };
    if data.osrs.is_none() && data.achievements.is_none() {
        return Vec::new();
    }
    change(config_dir, |o| apply_game_data(o, &data, now))
        .map(|(_, moved)| moved)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::super::{apply, GoalAct, KnownGame};
    use super::*;
    use crate::osrs_hiscores::{Activity, Skill};

    const NOW: u64 = 1_793_620_800_000;

    fn stats(fishing: u64, zulrah: u64) -> OsrsStats {
        OsrsStats {
            skills: vec![
                Skill {
                    name: "Overall".into(),
                    rank: Some(1),
                    level: Some(1500),
                    xp: Some(1),
                },
                Skill {
                    name: "Fishing".into(),
                    rank: Some(1),
                    level: Some(fishing),
                    xp: Some(1),
                },
            ],
            activities: vec![Activity {
                name: "Zulrah".into(),
                rank: Some(1),
                score: Some(zulrah),
            }],
            fetched_ms: NOW,
        }
    }

    fn add(o: &mut Objectives, text: &str, games: &[KnownGame<'_>]) -> String {
        apply(o, &[GoalAct::Add { text: text.into() }], games, NOW);
        o.goals.last().unwrap().id.clone()
    }

    #[test]
    fn osrs_levels_and_kills_move_goals_from_where_the_player_is() {
        let mut o = Objectives::default();
        let fish = add(&mut o, "99 fishing in osrs", &[]);
        let total = add(&mut o, "2000 total level in osrs", &[]);
        let kills = add(&mut o, "kill zulrah 100 times in osrs", &[]);
        let data = GameData {
            osrs: Some(&stats(93, 40)),
            achievements: None,
        };
        let moved = apply_game_data(&mut o, &data, NOW + 1);
        assert_eq!(moved.len(), 3, "{:?}", o.goals);
        let g = o.goals.iter().find(|g| g.id == fish).unwrap();
        assert_eq!((g.start, g.current, g.checked), (93, 93, true));
        // Starting at 93 pays no milestone for levels earned before.
        assert!(g.paid.is_empty());
        let t = o.goals.iter().find(|g| g.id == total).unwrap();
        assert_eq!(t.current, 1500);
        let k = o.goals.iter().find(|g| g.id == kills).unwrap();
        assert_eq!(k.current, 40);
        // The same reading again changes nothing.
        assert!(apply_game_data(&mut o, &data, NOW + 2).is_empty());
        // Later levels move it on from there.
        let data = GameData {
            osrs: Some(&stats(99, 40)),
            achievements: None,
        };
        apply_game_data(&mut o, &data, NOW + 3);
        let g = o.goals.iter().find(|g| g.id == fish).unwrap();
        assert_eq!(g.state, GoalState::Done);
    }

    #[test]
    fn steam_achievement_counts_move_counted_and_all_goals() {
        let library = [KnownGame {
            name: "Hades",
            steam_app_id: Some(1_145_360),
            source: None,
        }];
        let mut o = Objectives::default();
        let some = add(&mut o, "get 30 achievements in hades", &library);
        let all = add(&mut o, "100% achievements in hades", &library);
        let other = add(&mut o, "finish hades", &library);
        let counts = BTreeMap::from([(
            1_145_360,
            Count {
                done: 12,
                total: 49,
            },
        )]);
        let data = GameData {
            osrs: None,
            achievements: Some(&counts),
        };
        apply_game_data(&mut o, &data, NOW + 1);
        let find = |id: &str| o.goals.iter().find(|g| g.id == id).unwrap().clone();
        assert_eq!((find(&some).current, find(&some).target), (12, 30));
        let a = find(&all);
        assert_eq!(
            (a.kind, a.current, a.target),
            (GoalKind::Achievements, 12, 49)
        );
        assert_eq!(
            super::super::goal_detail(&a),
            "12 / 49 · 37 achievements to go · Checked"
        );
        // A finish goal has nothing to read.
        assert_eq!(find(&other).current, 0);
        assert!(!find(&other).checked);
    }

    #[test]
    fn achievement_goals_put_their_games_first_in_the_check() {
        let library = [
            KnownGame {
                name: "Hades",
                steam_app_id: Some(1_145_360),
                source: None,
            },
            KnownGame {
                name: "Celeste",
                steam_app_id: Some(504_230),
                source: None,
            },
        ];
        let mut o = Objectives::default();
        add(&mut o, "get 30 achievements in hades", &library);
        add(&mut o, "finish celeste", &library);
        add(&mut o, "100% achievements in celeste", &library);
        assert_eq!(
            goal_steam_games(&o),
            vec![(504_230, "Celeste".into()), (1_145_360, "Hades".into())]
        );
    }

    #[test]
    fn a_watched_match_moves_win_play_and_kill_goals_for_its_game() {
        let mut o = Objectives::default();
        let wins = add(&mut o, "win 10 matches in cs2", &[]);
        let played = add(&mut o, "play 5 matches in cs2", &[]);
        let kills = add(&mut o, "500 kills in cs2", &[]);
        let dota = add(&mut o, "win 10 games in dota 2", &[]);
        let current = |o: &Objectives, id: &str| {
            let g = o.goals.iter().find(|g| g.id == id).unwrap();
            (g.current, g.checked)
        };
        assert_eq!(o.goals[0].game.name, "Counter-Strike 2");
        let moved = match_finished(&mut o, "Counter-Strike 2", false, 18, NOW + 1);
        assert_eq!(moved.len(), 2);
        assert_eq!(current(&o, &wins), (0, false));
        assert_eq!(current(&o, &played), (1, true));
        assert_eq!(current(&o, &kills), (18, true));
        match_finished(&mut o, "Counter-Strike 2", true, 20, NOW + 2);
        assert_eq!(current(&o, &wins), (1, true));
        assert_eq!(current(&o, &played), (2, true));
        assert_eq!(current(&o, &kills), (38, true));
        assert_eq!(current(&o, &dota), (0, false));
    }

    #[test]
    fn no_game_data_reads_and_writes_nothing() {
        let dir = std::env::temp_dir().join(format!("goals-games-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(refresh_from_games(&dir, NOW).is_empty());
        assert!(!super::super::objectives_path(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
