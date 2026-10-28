//! Game launch companion. Opt-in. When a game from the user's Steam library
//! starts, the cabin posts one card: the game's news since they last played,
//! a link to its top-rated community guides, and the note they left
//! themselves last time. Launches are seen from what Steam itself publishes
//! (its launch command line on Linux, its `RunningAppID` value on Windows):
//! no screen capture, no reading of game memory, nothing a game's anti-cheat
//! would see. Nothing is drawn over the game.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::chips::{ChipKind, QuickChip};
use crate::pulse_sources::SourceItem;
use crate::steam::{SteamGame, Vdf};
use crate::update_feed::{digest_card, UpdateAction, UpdateCard};

pub const NOTES_FILE: &str = "game-notes.json";
/// News items named on the card.
pub const NEWS_MAX: usize = 3;
/// A note longer than this is cut when saved.
pub const NOTE_CHARS: usize = 500;
pub const GUIDES_PREFIX: &str = "game:guides:";
pub const NOTE_PREFIX: &str = "game:note:";

/// The app id in Steam's launch wrapper (`reaper SteamLaunch AppId=570 -- …`).
pub fn running_from_cmdlines(procs: &[(String, String)]) -> Option<u32> {
    procs.iter().find_map(|(_, cmdline)| {
        if !cmdline.contains("SteamLaunch") {
            return None;
        }
        cmdline
            .split_whitespace()
            .find_map(|w| w.strip_prefix("AppId="))
            .and_then(|id| id.parse().ok())
            .filter(|id| *id != 0)
    })
}

/// `RunningAppID` from Steam's `registry.vdf`; 0 means nothing is running.
pub fn running_from_registry_vdf(text: &str) -> Option<u32> {
    crate::steam::parse_vdf(text)
        .at(&[
            "Registry",
            "HKCU",
            "Software",
            "Valve",
            "Steam",
            "RunningAppID",
        ])
        .and_then(Vdf::str)
        .and_then(|s| s.trim().parse().ok())
        .filter(|id| *id != 0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Launch {
    Started(u32),
    Ended(u32),
}

/// Turns "what is running now" samples into start and end events.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaunchWatch {
    pub current: Option<u32>,
    /// False until the first sample, so a game already running when the
    /// cabin starts does not count as a launch.
    pub primed: bool,
}

impl LaunchWatch {
    pub fn note(&mut self, running: Option<u32>) -> Option<Launch> {
        let was = std::mem::replace(&mut self.current, running);
        if !std::mem::replace(&mut self.primed, true) || was == running {
            return None;
        }
        match (was, running) {
            (_, Some(id)) => Some(Launch::Started(id)),
            (Some(id), None) => Some(Launch::Ended(id)),
            (None, None) => None,
        }
    }
}

/// The game's news posted after it was last played, newest first.
pub fn news_since(mut items: Vec<SourceItem>, last_played_s: u64) -> Vec<SourceItem> {
    let since_ms = last_played_s.saturating_mul(1000);
    items.retain(|i| i.published_ms > since_ms);
    items.sort_by_key(|i| std::cmp::Reverse(i.published_ms));
    items.truncate(NEWS_MAX);
    items
}

/// The game's top-rated community guides.
pub fn guides_url(app_id: u32) -> String {
    format!("https://steamcommunity.com/app/{app_id}/guides/?browsefilter=toprated")
}

/// The user's own notes, one per game.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameNotes {
    #[serde(default)]
    pub notes: BTreeMap<u32, String>,
}

impl GameNotes {
    pub fn get(&self, app_id: u32) -> Option<&str> {
        self.notes.get(&app_id).map(String::as_str)
    }

    /// Save `text` for the game; an empty note removes it.
    pub fn set(&mut self, app_id: u32, text: &str) {
        let text: String = text.trim().chars().take(NOTE_CHARS).collect();
        if text.is_empty() {
            self.notes.remove(&app_id);
        } else {
            self.notes.insert(app_id, text);
        }
    }
}

pub fn load_notes(config_dir: &Path) -> GameNotes {
    std::fs::read_to_string(config_dir.join(NOTES_FILE))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_notes(config_dir: &Path, notes: &GameNotes) -> Result<(), String> {
    let text = serde_json::to_string_pretty(notes).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    crate::pulse_writer::write_atomic(&config_dir.join(NOTES_FILE), &text)
}

/// What the card shows for one launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Companion {
    pub game: SteamGame,
    /// Title and link, newest first. `None` when the news could not be read.
    pub news: Option<Vec<(String, String)>>,
    pub note: Option<String>,
}

impl Companion {
    pub fn body(&self) -> String {
        let mut out = Vec::new();
        out.push(match &self.news {
            None => "Couldn't read the news for it just now.".to_string(),
            Some(n) if n.is_empty() => "No news since you last played.".to_string(),
            Some(n) => format!(
                "What changed since you last played: {}.",
                n.iter()
                    .map(|(t, _)| t.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        });
        if let Some(note) = &self.note {
            out.push(format!("Your note from last time: {note}"));
        }
        out.join(" ")
    }

    pub fn companion_card(&self, now_ms: u64) -> UpdateCard {
        let mut card = digest_card(
            &format!("source:game-companion:{}", self.game.app_id),
            &format!("Back to {}", self.game.name),
            &self.body(),
            now_ms,
        );
        let href = self
            .news
            .as_ref()
            .and_then(|n| n.first())
            .map(|(_, url)| url.clone())
            .unwrap_or_else(|| guides_url(self.game.app_id));
        card.action = Some(UpdateAction::DeepLink { href });
        card
    }
}

/// One-taps for a game: its guides, and a note for next time.
pub fn chips(game: &SteamGame, ended: bool) -> Vec<QuickChip> {
    let chip = |id: String, label: String, value: String, hint: &str| QuickChip {
        id,
        label,
        value,
        kind: ChipKind::Chat,
        score: 1.0,
        hint: hint.into(),
        primary: false,
    };
    let note = chip(
        format!("{NOTE_PREFIX}{}", game.app_id),
        if ended {
            "Leave a note for next time".into()
        } else {
            format!("Note for {}", game.name)
        },
        format!("note {}", game.app_id),
        "Shown on the card the next time you launch it",
    );
    if ended {
        return vec![note];
    }
    vec![
        chip(
            format!("{GUIDES_PREFIX}{}", game.app_id),
            format!("Top guides for {}", game.name),
            guides_url(game.app_id),
            "Opens the game's top-rated community guides",
        ),
        note,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pulse_sources::SourceKind;

    fn game() -> SteamGame {
        SteamGame {
            app_id: 548430,
            name: "Deep Rock Galactic".into(),
            last_played: 1_700_000_000,
            playtime_min: 600,
        }
    }

    fn item(title: &str, published_ms: u64) -> SourceItem {
        SourceItem {
            title: title.into(),
            url: format!("https://store.steampowered.com/news/{published_ms}"),
            kind: SourceKind::Rss,
            source_name: "Deep Rock Galactic on Steam".into(),
            published_ms,
            summary: String::new(),
            image: None,
            topic: None,
            engagement: 0,
        }
    }

    #[test]
    fn launches_come_from_steams_own_launch_line_and_registry() {
        let procs = vec![
            ("bash".to_string(), "bash".to_string()),
            (
                "reaper".to_string(),
                "/home/user/.steam/steam/ubuntu12_32/reaper SteamLaunch AppId=548430 -- /home/user/.steam/steam/steamapps/common/Deep Rock Galactic/FSD.exe".to_string(),
            ),
        ];
        assert_eq!(running_from_cmdlines(&procs), Some(548430));
        assert_eq!(
            running_from_cmdlines(&[("steam".into(), "steam -silent".into())]),
            None
        );
        let reg = r#""Registry"
{
	"HKCU"
	{
		"Software"
		{
			"Valve"
			{
				"Steam"
				{
					"RunningAppID"		"548430"
				}
			}
		}
	}
}"#;
        assert_eq!(running_from_registry_vdf(reg), Some(548430));
        assert_eq!(running_from_registry_vdf(&reg.replace("548430", "0")), None);
    }

    #[test]
    fn the_watch_sees_starts_and_ends_but_not_a_game_already_running() {
        let mut w = LaunchWatch::default();
        assert_eq!(w.note(Some(1)), None, "running before the cabin started");
        assert_eq!(w.note(Some(1)), None);
        assert_eq!(w.note(None), Some(Launch::Ended(1)));
        assert_eq!(w.note(None), None);
        assert_eq!(w.note(Some(2)), Some(Launch::Started(2)));
        assert_eq!(w.note(Some(3)), Some(Launch::Started(3)));
    }

    #[test]
    fn news_is_filtered_by_the_last_played_date() {
        let since = 1_700_000_000;
        let items = vec![
            item("Old patch", since * 1000 - 1),
            item("Season 6", since * 1000 + 50),
            item("Hotfix 2", since * 1000 + 90),
        ];
        let titles: Vec<String> = news_since(items, since)
            .into_iter()
            .map(|i| i.title)
            .collect();
        assert_eq!(titles, vec!["Hotfix 2", "Season 6"]);
    }

    #[test]
    fn notes_persist_per_game_and_show_on_the_card() {
        let dir = std::env::temp_dir().join(format!("grokhub-game-notes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut notes = GameNotes::default();
        notes.set(548430, "  remember I was farming morkite  ");
        notes.set(1, "other game");
        save_notes(&dir, &notes).unwrap();
        let mut loaded = load_notes(&dir);
        assert_eq!(loaded.get(548430), Some("remember I was farming morkite"));
        assert_eq!(loaded.get(1), Some("other game"));
        loaded.set(1, " ");
        assert_eq!(loaded.get(1), None, "an empty note removes it");
        let c = Companion {
            game: game(),
            news: Some(vec![(
                "Season 6".into(),
                "https://store.steampowered.com/news/6".into(),
            )]),
            note: loaded.get(548430).map(str::to_string),
        };
        let card = c.companion_card(5);
        assert_eq!(card.title, "Back to Deep Rock Galactic");
        assert_eq!(
            card.body.as_deref(),
            Some("What changed since you last played: Season 6. Your note from last time: remember I was farming morkite")
        );
        assert_eq!(
            card.action,
            Some(UpdateAction::DeepLink {
                href: "https://store.steampowered.com/news/6".into()
            })
        );
        let none = Companion {
            news: Some(Vec::new()),
            note: None,
            ..c
        };
        assert_eq!(none.body(), "No news since you last played.");
        assert_eq!(
            none.companion_card(5).action,
            Some(UpdateAction::DeepLink {
                href: guides_url(548430)
            })
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn chips_offer_guides_while_playing_and_a_note_after() {
        let labels =
            |ended| -> Vec<String> { chips(&game(), ended).into_iter().map(|c| c.label).collect() };
        assert_eq!(
            labels(false),
            vec![
                "Top guides for Deep Rock Galactic",
                "Note for Deep Rock Galactic"
            ]
        );
        assert_eq!(labels(true), vec!["Leave a note for next time"]);
        assert_eq!(
            guides_url(548430),
            "https://steamcommunity.com/app/548430/guides/?browsefilter=toprated"
        );
    }
}
