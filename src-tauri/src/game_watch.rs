//! Game watcher switch (`app.json` `gameWatch`), same key and default as the
//! cabin. On: write one Game State Integration cfg into each supported game's
//! folder (`grokhub_core::game_watch_gsi`). Off: remove those files. Opt-in,
//! off by default. The shell does not run the loopback listener here; the
//! companion screens and a later listener follow the same flag.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use grokhub_core::game_watch_gsi::{self as gsi, GsiGame, GsiToken};
use serde::Serialize;
use serde_json::json;

const KEY: &str = "gameWatch";

static FILES: Mutex<()> = Mutex::new(());

fn files() -> MutexGuard<'static, ()> {
    FILES.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameWatch {
    /// Whether the watcher is on. Off by default.
    pub on: bool,
    /// What Settings shows under the toggle (setup or removed note).
    pub note: String,
}

/// The cabin's toggle label and help, kept so both apps say the same thing.
pub const TITLE: &str = "Game watcher (Counter-Strike 2, Dota 2)";
pub const HELP: &str = "Follows your match as you play, through Game State Integration, the feed Valve built and supports for this. Turning it on writes one small file into the game's own config folder that tells the game to send its state to Loadout, here on this computer only, while the game runs. Turning it off removes that file. Nothing reads the game's memory or touches the game.";

fn names(games: &[GsiGame]) -> String {
    let names: Vec<&str> = games.iter().map(|g| g.name()).collect();
    names.join(" and ")
}

fn setup_note(found: Option<&[(GsiGame, Result<PathBuf, String>)]>) -> String {
    let Some(found) = found else {
        return "Steam wasn't found on this computer, so no game was set up.".into();
    };
    if found.is_empty() {
        return "No supported game (Counter-Strike 2 or Dota 2) is installed in your Steam library yet.".into();
    }
    let ok: Vec<GsiGame> = found
        .iter()
        .filter(|(_, r)| r.is_ok())
        .map(|(g, _)| *g)
        .collect();
    let mut note = String::new();
    if !ok.is_empty() {
        note = format!(
            "Set up for {}. Start the game, or restart it if it's open, so it reads the file.",
            names(&ok)
        );
    }
    for (_, r) in found {
        if let Err(e) = r {
            if !note.is_empty() {
                note.push(' ');
            }
            note.push_str(e);
        }
    }
    note
}

fn removed_note(removed: Option<&[(GsiGame, Result<bool, String>)]>) -> String {
    let removed = removed.unwrap_or_default();
    let gone: Vec<GsiGame> = removed
        .iter()
        .filter(|(_, r)| r == &Ok(true))
        .map(|(g, _)| *g)
        .collect();
    let mut note = if gone.is_empty() {
        "Game watcher off.".to_string()
    } else {
        format!("Game watcher off. Removed its file from {}.", names(&gone))
    };
    for (_, r) in removed {
        if let Err(e) = r {
            note.push(' ');
            note.push_str(e);
        }
    }
    note
}

/// Whether `gameWatch` is on in `app.json` (default false).
pub fn is_on(config_dir: &Path) -> bool {
    grokhub_core::app_json::bool_or(config_dir, KEY, false)
}

fn view(config_dir: &Path, note: String) -> GameWatch {
    GameWatch {
        on: is_on(config_dir),
        note,
    }
}

pub fn read(config_dir: &Path) -> GameWatch {
    let _files = files();
    view(config_dir, String::new())
}

/// Turn the watcher on or off. On writes the GSI cfg files; off removes them.
pub fn set(
    config_dir: &Path,
    on: bool,
    steam_root: Option<&Path>,
    port: u16,
) -> Result<GameWatch, String> {
    let _files = files();
    grokhub_core::app_json::patch(config_dir, &[(KEY, json!(on))])?;
    let note = if on {
        let token = GsiToken::load_or_create(config_dir)?;
        let found = steam_root
            .map(|p| p.to_path_buf())
            .or_else(grokhub_core::steam::detect_root)
            .map(|root| gsi::install_cfgs(&root, &token, port));
        setup_note(found.as_deref())
    } else {
        let removed = steam_root
            .map(|p| p.to_path_buf())
            .or_else(grokhub_core::steam::detect_root)
            .map(|root| gsi::remove_cfgs(&root));
        removed_note(removed.as_deref())
    };
    Ok(view(config_dir, note))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("game-watch-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn steam_with_cs2(root: &Path) -> PathBuf {
        let cfg = root
            .join("steamapps/common/Counter-Strike Global Offensive/game/csgo/cfg");
        fs::create_dir_all(&cfg).unwrap();
        fs::write(cfg.join("autoexec.cfg"), "bind x noclip").unwrap();
        root.to_path_buf()
    }

    #[test]
    fn defaults_off_and_writes_nothing() {
        let d = dir("off");
        let steam = steam_with_cs2(&d.join("steam"));
        let view = read(&d);
        assert!(!view.on);
        assert!(view.note.is_empty());
        let cfg = steam
            .join("steamapps/common/Counter-Strike Global Offensive/game/csgo/cfg")
            .join(gsi::CFG_FILE);
        assert!(!cfg.exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn on_writes_the_gsi_file_and_off_removes_it() {
        let d = dir("on-off");
        let steam = steam_with_cs2(&d.join("steam"));
        let cfg = steam
            .join("steamapps/common/Counter-Strike Global Offensive/game/csgo/cfg")
            .join(gsi::CFG_FILE);

        let on = set(&d, true, Some(&steam), 29174).unwrap();
        assert!(on.on);
        assert!(cfg.exists(), "GSI cfg should exist after on");
        let text = fs::read_to_string(&cfg).unwrap();
        assert!(text.starts_with(gsi::CFG_MARKER));
        assert!(
            on.note.contains("Counter-Strike 2"),
            "setup note names the game: {}",
            on.note
        );

        let off = set(&d, false, Some(&steam), 29174).unwrap();
        assert!(!off.on);
        assert!(!cfg.exists(), "GSI cfg should be gone after off");
        assert!(
            off.note.contains("Removed its file from Counter-Strike 2"),
            "{}",
            off.note
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn on_without_steam_explains_and_stays_on() {
        let d = dir("no-steam");
        // Pass an empty steam root with no games.
        let steam = d.join("empty-steam");
        fs::create_dir_all(&steam).unwrap();
        let view = set(&d, true, Some(&steam), 29174).unwrap();
        assert!(view.on);
        assert!(
            view.note.contains("No supported game")
                || view.note.contains("Steam wasn't found")
                || view.note.contains("isn't there"),
            "{}",
            view.note
        );
        let _ = fs::remove_dir_all(&d);
    }
}
