//! The game launch companion (`grokhub_core::game_companion`) on the shell,
//! so Home's Patch Notes tile turns on in place. Every ten seconds, while
//! switched on, it asks what Steam says is running. A launch of a library
//! game reads that game's news and posts one Pulse card: what changed since
//! the last play, a guides link and the note from last time. Nothing opens
//! or touches the game. The shell keeps its own switch; the desktop app's
//! companion setting still counts as on.

use std::path::{Path, PathBuf};

use grokhub_core::game_companion::{self as gc, Companion, Launch};
use grokhub_core::pulse_sources::{self as ps, Fetched, SourceKind, SourceReq};
use grokhub_core::steam::SteamGame;
use serde::{Deserialize, Serialize};
#[cfg(desktop)]
use std::time::Duration;

pub const FILE: &str = "shell-game-companion.json";
/// A launch card can wait this long; /proc is read once per sample.
#[cfg(desktop)]
const SAMPLE_EVERY: Duration = Duration::from_secs(10);

#[derive(Debug, Default, Serialize, Deserialize)]
struct Switch {
    on: bool,
}

/// Whether the shell's own companion switch is on.
pub fn is_on(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join(FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<Switch>(&t).ok())
        .is_some_and(|s| s.on)
}

/// Home's Turn on or Turn off for Patch Notes.
pub fn set(dir: &Path, on: bool) -> Result<String, String> {
    let text = serde_json::to_string(&Switch { on }).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(FILE), text).map_err(|e| e.to_string())?;
    Ok(if on {
        "On. Start a game and what changed since you last played shows here.".into()
    } else {
        "Off. Nothing is read when a game starts.".into()
    })
}

/// The game's news since it was last played, through the Pulse fetcher.
fn read_news(game: &SteamGame) -> Option<Vec<(String, String)>> {
    let req = SourceReq {
        kind: SourceKind::Rss,
        url: game.news_feed(),
        name: format!("{} on Steam", game.name),
        topic: None,
    };
    let body = match crate::pulse_feed::net().source(&req, None) {
        Fetched::Body { body, .. } => body,
        Fetched::NotModified | Fetched::Failed(_) => return None,
    };
    Some(
        gc::news_since(ps::parse(&req, &body), game.last_played)
            .into_iter()
            .map(|i| (i.title, i.url))
            .collect(),
    )
}

/// Reads a game's news: title and link, newest first; `None` when it failed.
pub type ReadNews<'a> = &'a dyn Fn(&SteamGame) -> Option<Vec<(String, String)>>;

/// The library games a launch event names, as cards to post. Only a game in
/// the Steam library counts.
pub fn launch_cards(
    events: &[Launch],
    library: &[SteamGame],
    dir: &Path,
    news: ReadNews<'_>,
    now: u64,
) -> Vec<grokhub_core::UpdateCard> {
    let notes = gc::load_notes(dir);
    events
        .iter()
        .filter_map(|e| match e {
            Launch::Started(id) => library.iter().find(|g| g.app_id == *id),
            Launch::Ended(_) => None,
        })
        .map(|game| {
            Companion {
                game: game.clone(),
                news: news(game),
                note: notes.get(game.app_id).map(str::to_string),
            }
            .companion_card(now)
        })
        .collect()
}

/// Samples what Steam runs while the switch is on.
#[cfg(desktop)]
pub fn start(dir: PathBuf) {
    std::thread::spawn(move || {
        let mut watch = gc::LaunchWatch::default();
        loop {
            std::thread::sleep(SAMPLE_EVERY);
            if !is_on(&dir) || !crate::capabilities::current().pulse {
                // Primed again on the next sample, so a game already running
                // when it is switched on doesn't count as a launch.
                watch = gc::LaunchWatch::default();
                continue;
            }
            let events = watch.note(grokhub_desktop::game_guard::steam_app_now());
            if events.is_empty() {
                continue;
            }
            let library = grokhub_core::steam::detect_root()
                .map(|r| grokhub_core::steam::read_library(&r))
                .unwrap_or_default();
            for card in launch_cards(&events, &library, &dir, &read_news, grokhub_core::now_ms()) {
                let _ = crate::pulse::add_post(&dir, card);
            }
        }
    });
}

#[cfg(mobile)]
pub fn start(_dir: PathBuf) {}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::game_companion::LaunchWatch;

    #[test]
    fn a_library_launch_posts_one_card_with_its_news_and_note() {
        let dir = std::env::temp_dir().join(format!("shell-companion-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut notes = gc::GameNotes::default();
        notes.set(570, "try the new hero");
        gc::save_notes(&dir, &notes).unwrap();
        let dota = SteamGame {
            app_id: 570,
            name: "Dota 2".into(),
            last_played: 0,
            playtime_min: 0,
        };
        let news =
            |_: &SteamGame| Some(vec![("Patch 7.37".into(), "https://example.com/p".into())]);
        let mut watch = LaunchWatch::default();
        assert!(watch.note(None).is_empty());
        let started = watch.note(Some(570));
        let cards = launch_cards(&started, std::slice::from_ref(&dota), &dir, &news, 9);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].title, "Back to Dota 2");
        let body = cards[0].body.clone().unwrap_or_default();
        assert!(body.contains("Patch 7.37"), "{body}");
        assert!(body.contains("try the new hero"), "{body}");
        // A game outside the library, and a game closing, post nothing.
        assert!(launch_cards(&watch.note(Some(1)), &[dota], &dir, &news, 10).is_empty());
        assert!(!is_on(&dir));
        set(&dir, true).unwrap();
        assert!(is_on(&dir));
        set(&dir, false).unwrap();
        assert!(!is_on(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
