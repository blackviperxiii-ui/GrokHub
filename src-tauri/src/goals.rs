//! Objectives → My goals in the shell (desktop, web and phone): the same
//! `objectives.json` and page rows as the desktop app, read and changed
//! through `grokhub-core::objectives`.

use std::path::Path;

use grokhub_core::objectives::{self, Board, GoalAct, KnownGame, TurnGoals};
use serde::Serialize;

/// The page after a press, with the words to show when an Add was refused.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub board: Board,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Steam games by name and app id, for linking a typed goal to its game.
pub type Library = Vec<(String, u32)>;

pub fn board(config_dir: &Path, now_ms: u64, offset_min: i32) -> Board {
    objectives::board(&objectives::load(config_dir), now_ms, offset_min)
}

pub fn act(
    config_dir: &Path,
    acts: &[GoalAct],
    library: &Library,
    now_ms: u64,
    offset_min: i32,
) -> Result<Saved, String> {
    let known = known(library);
    let (o, refused) =
        objectives::change(config_dir, |o| objectives::apply(o, acts, &known, now_ms))
            .map_err(|e| format!("Couldn't save your goals: {e}"))?;
    Ok(Saved {
        board: objectives::board(&o, now_ms, offset_min),
        note: refused.map(str::to_string),
    })
}

/// Goals from one finished chat turn: an asked-for goal is tracked, a goal
/// the reply noticed waits in the tray, and progress the player said moves
/// a tracked goal. Nothing is written when the turn holds no goal.
pub fn read_chat_turn(config_dir: &Path, chat: &str, said: &str, reply: &str) -> TurnGoals {
    let now = grokhub_core::now_ms();
    if !objectives::worth_a_look(said, reply) {
        return TurnGoals::default();
    }
    read_turn_with(config_dir, chat, said, reply, &library(), now)
}

fn read_turn_with(
    config_dir: &Path,
    chat: &str,
    said: &str,
    reply: &str,
    library: &Library,
    now: u64,
) -> TurnGoals {
    let known = known(library);
    objectives::change(config_dir, |o| {
        objectives::read_turn(o, chat, said, reply, &known, now)
    })
    .map(|(_, t)| t)
    .unwrap_or_default()
}

fn known(library: &Library) -> Vec<KnownGame<'_>> {
    library
        .iter()
        .map(|(name, app_id)| KnownGame {
            name,
            steam_app_id: Some(*app_id),
            source: None,
        })
        .collect()
}

/// The installed Steam library, or nothing when Steam isn't found.
pub fn library() -> Library {
    grokhub_core::steam::detect_root()
        .map(|root| {
            grokhub_core::steam::read_library(&root)
                .into_iter()
                .map(|g| (g.name, g.app_id))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Monday 2 Nov 2026, 12:00 UTC.
    const NOW: u64 = 1_793_620_800_000;

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("shell-goals-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_typed_goal_links_its_game_moves_and_lands_on_the_shelf() {
        let d = dir("typed");
        assert!(board(&d, NOW, 0).mine.is_empty());
        let lib = vec![("Hades".to_string(), 1_145_360)];
        let add = GoalAct::Add {
            text: "50 achievements in hades".into(),
        };
        let saved = act(&d, &[add], &lib, NOW, 0).unwrap();
        assert_eq!(saved.note, None);
        assert_eq!(saved.board.mine.len(), 1);
        let row = &saved.board.mine[0];
        assert_eq!(row.title, "Hades · 50 achievements");
        assert_eq!(row.detail, "0 / 50 · 50 achievements to go");
        assert!(row.plus_one);
        let saved = act(&d, &[GoalAct::Bump { id: row.id.clone() }], &lib, NOW, 0).unwrap();
        assert_eq!(saved.board.mine[0].detail, "1 / 50 · 49 achievements to go");
        let saved = act(&d, &[GoalAct::Done { id: row.id.clone() }], &lib, NOW, 0).unwrap();
        assert!(saved.board.mine.is_empty());
        assert_eq!(saved.board.season.done, 1);
        assert_eq!(
            saved.board.season.trophies[0].done_on.as_deref(),
            Some("Nov 2")
        );
        // A second reader sees the same file.
        assert_eq!(board(&d, NOW, 0), saved.board);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_chat_turn_suggests_tracks_and_moves_goals() {
        let d = dir("chat");
        let lib = vec![("Elden Ring".to_string(), 1_245_620)];
        // Small talk writes nothing.
        let t = read_turn_with(&d, "c1", "hey, what's up", "Not much!", &lib, NOW);
        assert!(!t.changed());
        assert!(!objectives::objectives_path(&d).exists());
        // A noticed goal waits in the tray.
        let reply = "Love it.\nGAME_GOAL: Elden Ring | finish | finish | 1";
        let t = read_turn_with(&d, "c1", "i want to beat it", reply, &lib, NOW);
        assert!(t.ask.is_some());
        let b = board(&d, NOW, 0);
        assert_eq!(b.suggested[0].title, "Elden Ring · Finish");
        assert!(b.mine.is_empty());
        // Asked to track: on the list at once.
        let reply = "GAME_GOAL: osrs | 99 fishing | skill | 99";
        let t = read_turn_with(&d, "c2", "add a goal: 99 fishing in osrs", reply, &lib, NOW);
        assert_eq!(t.tracked.len(), 1);
        // Progress the player said moves it.
        let reply = "GAME_PROGRESS: osrs | 99 fishing | 80";
        let t = read_turn_with(&d, "c2", "hit 80 fishing today", reply, &lib, NOW + 1);
        assert_eq!(t.moved.len(), 1);
        assert_eq!(
            board(&d, NOW + 1, 0).mine[0].detail,
            "80 / 99 · 19 levels to 99"
        );
        assert!(objectives::worth_a_look("trying to finish hades", ""));
        assert!(!objectives::worth_a_look("what's the weather", ""));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_empty_goal_says_why_and_writes_nothing() {
        let d = dir("empty");
        let add = GoalAct::Add { text: " ".into() };
        let saved = act(&d, &[add], &Vec::new(), NOW, 0).unwrap();
        assert_eq!(
            saved.note.as_deref(),
            Some("Say what you want to do first.")
        );
        assert!(!objectives::objectives_path(&d).exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
