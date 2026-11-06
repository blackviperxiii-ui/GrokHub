//! Objectives → My goals in the shell (desktop, web and phone): the same
//! `objectives.json` and page rows as the desktop app, read and changed
//! through `grokhub-core::objectives`.

use std::path::Path;

use grokhub_core::objectives::{self, Board, GoalAct, KnownGame};
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
    let known: Vec<KnownGame<'_>> = library
        .iter()
        .map(|(name, app_id)| KnownGame {
            name,
            steam_app_id: Some(*app_id),
            source: None,
        })
        .collect();
    let (o, refused) =
        objectives::change(config_dir, |o| objectives::apply(o, acts, &known, now_ms))
            .map_err(|e| format!("Couldn't save your goals: {e}"))?;
    Ok(Saved {
        board: objectives::board(&o, now_ms, offset_min),
        note: refused.map(str::to_string),
    })
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
