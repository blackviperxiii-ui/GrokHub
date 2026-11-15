//! The game launch companion (`grokhub_core::game_companion`) on the shell,
//! so Home's Patch Notes tile turns on in place. Every ten seconds, while
//! switched on, it asks what Steam says is running. A launch of a library
//! game reads that game's news and posts one Pulse card: what changed since
//! the last play, a guides link and the note from last time. Nothing opens
//! or touches the game. One card per game: a relaunch replaces it. With the
//! ping on, a quiet desktop notification comes with the card. When the game
//! closes, the chip row offers a note for next time for half an hour.

use std::path::{Path, PathBuf};

use grokhub_core::game_companion::{self as gc, Companion, Launch};
use grokhub_core::pulse_sources::{self as ps, Fetched, SourceKind, SourceReq};
use grokhub_core::steam::SteamGame;
use grokhub_core::QuickChip;
use serde::{Deserialize, Serialize};
#[cfg(desktop)]
use std::time::Duration;

pub const FILE: &str = "shell-game-companion.json";
/// A launch card can wait this long; /proc is read once per sample.
#[cfg(desktop)]
const SAMPLE_EVERY: Duration = Duration::from_secs(10);

/// The page's event: the session changed, so the chip rows ask again.
pub const EVENT: &str = "game-session";
/// A closed game's note offer stays in the chip row this long.
pub const NOTE_OFFER_MS: u64 = 30 * 60 * 1000;

/// The companion's switches. Older files have no `ping` and read it as off.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Switch {
    pub on: bool,
    /// A quiet desktop notification comes with the launch card.
    #[serde(default)]
    pub ping: bool,
}

/// The switches as saved; a missing or broken file reads as both off.
pub fn load(dir: &Path) -> Switch {
    std::fs::read_to_string(dir.join(FILE))
        .ok()
        .and_then(|t| serde_json::from_str::<Switch>(&t).ok())
        .unwrap_or_default()
}

/// Whether the companion is on.
pub fn is_on(dir: &Path) -> bool {
    load(dir).on
}

fn save(dir: &Path, switch: Switch) -> Result<(), String> {
    let text = serde_json::to_string(&switch).map_err(|e| e.to_string())?;
    grokhub_core::pulse_writer::write_atomic(&dir.join(FILE), &text)
}

/// Home's Turn on or Turn off for Patch Notes, and Settings → Behavior.
pub fn set(dir: &Path, on: bool) -> Result<String, String> {
    save(dir, Switch { on, ..load(dir) })?;
    Ok(if on {
        "On. Start a game and what changed since you last played shows here.".into()
    } else {
        "Off. Nothing is read when a game starts.".into()
    })
}

/// Settings → Behavior: the quiet desktop notification with the card.
pub fn set_ping(dir: &Path, ping: bool) -> Result<String, String> {
    save(dir, Switch { ping, ..load(dir) })?;
    Ok(if ping {
        "On. A quiet notification comes with the card.".into()
    } else {
        "Off. The card shows in Pulse only.".into()
    })
}

/// One on/off setting from the old desktop app's settings file (`app.json`,
/// camelCase keys), read only. Missing, unreadable or oversized reads as off.
fn legacy_flag(dir: &Path, key: &str) -> bool {
    let path = dir.join("app.json");
    if !std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= 1024 * 1024) {
        return false;
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| v.get(key).and_then(serde_json::Value::as_bool))
        .unwrap_or(false)
}

/// Carries the old desktop app's companion switches over once: only when
/// this app has no file of its own yet, and only when one of them was on.
/// After that the old file is never read again, so Turn off sticks.
pub fn migrate_legacy(dir: &Path) -> Result<(), String> {
    if dir.join(FILE).exists() {
        return Ok(());
    }
    let old = Switch {
        on: legacy_flag(dir, "gameCompanion"),
        ping: legacy_flag(dir, "gameCompanionPing"),
    };
    if old == Switch::default() {
        return Ok(());
    }
    save(dir, old)
}

/// The game running now and the one that just closed, in this process only.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Session {
    pub playing: Option<SteamGame>,
    /// The library game that closed last, and when.
    pub ended: Option<(SteamGame, u64)>,
}

impl Session {
    /// Follows launch events. Only a game in the Steam library counts: one
    /// outside it changes nothing, and closing it keeps the earlier offer.
    /// True when anything changed.
    pub fn note(&mut self, events: &[Launch], library: &[SteamGame], now: u64) -> bool {
        let before = self.clone();
        for event in events {
            match *event {
                Launch::Started(id) => {
                    let Some(game) = library.iter().find(|g| g.app_id == id) else {
                        continue;
                    };
                    if self.ended.as_ref().is_some_and(|(g, _)| g.app_id == id) {
                        self.ended = None;
                    }
                    self.playing = Some(game.clone());
                }
                Launch::Ended(id) => {
                    self.playing = None;
                    if let Some(game) = library.iter().find(|g| g.app_id == id) {
                        self.ended = Some((game.clone(), now));
                    }
                }
            }
        }
        *self != before
    }

    /// The chips for the row: a note for the game running now, then "Leave a
    /// note for next time" for the one that closed in the last half hour.
    /// The guides chip stays out: the page has no way to open it.
    pub fn chips(&self, now: u64) -> Vec<QuickChip> {
        let playing = self.playing.iter().flat_map(|g| gc::chips(g, false));
        let ended = self
            .ended
            .iter()
            .filter(|(g, at)| {
                now.saturating_sub(*at) < NOTE_OFFER_MS
                    && self.playing.as_ref().is_none_or(|p| p.app_id != g.app_id)
            })
            .flat_map(|(g, _)| gc::chips(g, true));
        playing
            .chain(ended)
            .filter(|c| c.id.starts_with(gc::NOTE_PREFIX))
            .enumerate()
            .map(|(i, mut c)| {
                // Feature chips rank above the ranker's own, in this order.
                c.score = 100.0 - i as f32 * 0.001;
                c
            })
            .collect()
    }

    /// Drops a closed game's note offer once its half hour is up. True when
    /// that changed the row, so a row left open stops offering it.
    pub fn expire(&mut self, now: u64) -> bool {
        if self
            .ended
            .as_ref()
            .is_some_and(|(_, at)| now.saturating_sub(*at) >= NOTE_OFFER_MS)
        {
            self.ended = None;
            return true;
        }
        false
    }

    /// The game a note chip names: the one running, else the one that closed.
    pub fn game(&self, app_id: u32) -> Option<&SteamGame> {
        self.playing
            .iter()
            .chain(self.ended.iter().map(|(g, _)| g))
            .find(|g| g.app_id == app_id)
    }

    /// A note was saved for `app_id`: its closed-game offer goes. True when
    /// that changed the row.
    pub fn noted(&mut self, app_id: u32) -> bool {
        if self.ended.as_ref().is_some_and(|(g, _)| g.app_id == app_id) {
            self.ended = None;
            return true;
        }
        false
    }

    /// Forgets both games, when the companion is switched off. True when
    /// there was anything to forget.
    pub fn clear(&mut self) -> bool {
        std::mem::take(self) != Session::default()
    }
}

/// The game chips a row starts from: the note offers lead Home's row and the
/// open chat's. The palette gets none, and neither does a paired browser,
/// which can't open the note window.
pub fn row_extra(session: &Session, surface: &str, remote: bool, now: u64) -> Vec<QuickChip> {
    if remote || surface == crate::chips::PALETTE {
        return Vec::new();
    }
    session.chips(now)
}

/// Posts a launch card, replacing that game's earlier one, so relaunching
/// never stacks "Back to …" cards.
pub fn post_card(dir: &Path, card: grokhub_core::UpdateCard) -> Result<bool, String> {
    let source = card.source_id.clone();
    let mut card = Some(card);
    crate::pulse::edit_cards(dir, &mut |cards| {
        let Some(card) = card.take() else {
            return 0;
        };
        // A dismissed card stays, so the store still knows it was put away.
        cards
            .retain(|c| c.source_id != source || c.status == grokhub_core::UpdateStatus::Dismissed);
        // The body quotes the user's note, so their words are not judged.
        usize::from(grokhub_core::post_user_worded(cards, card))
    })
    .map(|added| added > 0)
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

/// One sample's launch events: the session first, under its lock and only
/// while the switch is on, then `changed` when that moved the chip rows, so
/// they ask again before the news is read. Then each launch card goes up.
/// The switch is read again before each card, so an Off that lands
/// mid-sample posts nothing more. Returns the cards to ping for: ones that
/// were really posted, while the ping is on.
pub fn on_launch(
    dir: &Path,
    session: &std::sync::Mutex<Session>,
    events: &[Launch],
    library: &[SteamGame],
    news: ReadNews<'_>,
    now: u64,
    changed: &mut dyn FnMut(),
) -> Vec<grokhub_core::UpdateCard> {
    // Turn off saves the switch before it clears the session under this
    // lock, so reading the switch here can't refill a cleared session.
    let moved = {
        let mut session = lock(session);
        if !is_on(dir) {
            // Off: nothing is read when a game starts.
            return Vec::new();
        }
        session.note(events, library, now)
    };
    if moved {
        changed();
    }
    let mut pings = Vec::new();
    for card in launch_cards(events, library, dir, news, now) {
        let switch = load(dir);
        if !switch.on {
            break;
        }
        if matches!(post_card(dir, card.clone()), Ok(true)) && switch.ping {
            pings.push(card);
        }
    }
    pings
}

/// Samples what Steam runs while the switch is on. Starts after the shell's
/// state is managed: it keeps the session there for the chip rows.
#[cfg(desktop)]
pub fn start(app: tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    std::thread::spawn(move || {
        let dir: PathBuf = app.state::<crate::ShellState>().config_dir.clone();
        if let Err(err) = migrate_legacy(&dir) {
            eprintln!("game companion: {err}");
        }
        let mut watch = gc::LaunchWatch::default();
        loop {
            std::thread::sleep(SAMPLE_EVERY);
            let state = app.state::<crate::ShellState>();
            if !is_on(&dir) || !crate::capabilities::current().pulse {
                // Primed again on the next sample, so a game already running
                // when it is switched on doesn't count as a launch.
                watch = gc::LaunchWatch::default();
                if lock(&state.game_session).clear() {
                    let _ = app.emit(EVENT, ());
                }
                continue;
            }
            if lock(&state.game_session).expire(grokhub_core::now_ms()) {
                let _ = app.emit(EVENT, ());
            }
            let events = watch.note(grokhub_desktop::game_guard::steam_app_now());
            if events.is_empty() {
                continue;
            }
            let library = grokhub_core::steam::detect_root()
                .map(|r| grokhub_core::steam::read_library(&r))
                .unwrap_or_default();
            let pings = on_launch(
                &dir,
                &state.game_session,
                &events,
                &library,
                &read_news,
                grokhub_core::now_ms(),
                &mut || {
                    let _ = app.emit(EVENT, ());
                },
            );
            for card in &pings {
                crate::os_extras::game_launch(&app, card);
            }
        }
    });
}

#[cfg(mobile)]
pub fn start(_app: tauri::AppHandle) {}

/// The session, past a poisoned lock.
pub fn lock(session: &std::sync::Mutex<Session>) -> std::sync::MutexGuard<'_, Session> {
    session.lock().unwrap_or_else(|e| e.into_inner())
}

/// What the note dialog shows for a game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteFor {
    pub game: String,
    pub note: String,
}

/// The game's name and saved note. The name comes from the session, then
/// the Steam library (`library`, read only when needed).
pub fn note_for(
    dir: &Path,
    session: &Session,
    app_id: u32,
    library: impl FnOnce() -> Vec<SteamGame>,
) -> NoteFor {
    let game = session
        .game(app_id)
        .map(|g| g.name.clone())
        .or_else(|| {
            library()
                .into_iter()
                .find(|g| g.app_id == app_id)
                .map(|g| g.name)
        })
        .unwrap_or_else(|| "this game".into());
    NoteFor {
        game,
        note: get_note(dir, app_id),
    }
}

/// One news headline for the companion frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsItem {
    pub title: String,
    pub url: String,
}

/// What the per-game companion frame shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    pub note: String,
    pub news: Vec<NewsItem>,
    pub guides_url: Option<String>,
}

/// Notes + news since last play + guides link. `steam` gates the Steam
/// guides URL; Battle.net tiles pass false and keep notes by cover seed.
pub fn frame(
    dir: &Path,
    note_id: u32,
    steam_app_id: Option<u32>,
    news: Option<Vec<(String, String)>>,
) -> Frame {
    let notes = gc::load_notes(dir);
    Frame {
        note: notes.get(note_id).unwrap_or("").to_string(),
        news: news
            .unwrap_or_default()
            .into_iter()
            .map(|(title, url)| NewsItem { title, url })
            .collect(),
        guides_url: steam_app_id.filter(|id| *id != 0).map(gc::guides_url),
    }
}

/// Save the user's note for a game (Steam app id, or a Battle.net cover seed).
pub fn set_note(dir: &Path, app_id: u32, text: &str) -> Result<String, String> {
    let mut notes = gc::load_notes(dir);
    notes.set(app_id, text);
    gc::save_notes(dir, &notes)?;
    Ok(notes.get(app_id).unwrap_or("").to_string())
}

/// The note for a game, if any.
pub fn get_note(dir: &Path, app_id: u32) -> String {
    gc::load_notes(dir).get(app_id).unwrap_or("").to_string()
}

/// News for a Steam library game since it was last played.
pub fn steam_news(app_id: u32, last_played: u64) -> Option<Vec<(String, String)>> {
    let root = grokhub_core::steam::detect_root()?;
    let game = grokhub_core::steam::read_library(&root)
        .into_iter()
        .find(|g| g.app_id == app_id)?;
    let mut game = game;
    game.last_played = last_played;
    read_news(&game)
}

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

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "shell-companion-{tag}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn game(app_id: u32, name: &str) -> SteamGame {
        SteamGame {
            app_id,
            name: name.into(),
            last_played: 0,
            playtime_min: 0,
        }
    }

    fn ids(chips: &[QuickChip]) -> Vec<&str> {
        chips.iter().map(|c| c.id.as_str()).collect()
    }

    #[test]
    fn the_switch_keeps_the_ping_and_an_old_switch_carries_over_once() {
        let dir = scratch("switch");
        std::fs::write(dir.join(FILE), r#"{"on":true}"#).unwrap();
        assert_eq!(
            load(&dir),
            Switch {
                on: true,
                ping: false
            }
        );
        assert_eq!(
            set_ping(&dir, true).unwrap(),
            "On. A quiet notification comes with the card."
        );
        set(&dir, false).unwrap();
        assert_eq!(
            load(&dir),
            Switch {
                on: false,
                ping: true
            }
        );
        assert_eq!(
            set_ping(&dir, false).unwrap(),
            "Off. The card shows in Pulse only."
        );
        let _ = std::fs::remove_dir_all(&dir);

        // No file of this app's: the old app's switches carry over.
        let dir = scratch("legacy");
        std::fs::write(
            dir.join("app.json"),
            r#"{"gameCompanion":true,"gameCompanionPing":true}"#,
        )
        .unwrap();
        migrate_legacy(&dir).unwrap();
        assert_eq!(
            load(&dir),
            Switch {
                on: true,
                ping: true
            }
        );
        // Turned off here, a second carry-over doesn't turn it back on.
        set(&dir, false).unwrap();
        migrate_legacy(&dir).unwrap();
        assert_eq!(
            load(&dir),
            Switch {
                on: false,
                ping: true
            }
        );
        let _ = std::fs::remove_dir_all(&dir);

        // Both old switches off: nothing is written.
        let dir = scratch("legacy-off");
        std::fs::write(dir.join("app.json"), r#"{"gameCompanion":false}"#).unwrap();
        migrate_legacy(&dir).unwrap();
        assert!(!dir.join(FILE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_closed_game_offers_a_note_for_half_an_hour_until_one_is_saved() {
        let library = [game(7, "Hades")];
        let mut watch = LaunchWatch::default();
        let mut session = Session::default();
        assert!(!session.note(&watch.note(None), &library, 1_000));
        assert!(session.note(&watch.note(Some(7)), &library, 1_500));
        let playing = session.chips(1_500);
        assert_eq!(ids(&playing), ["game:note:7"]);
        assert_eq!(playing[0].label, "Note for Hades");
        assert_eq!(playing[0].score, 100.0);

        assert!(session.note(&watch.note(None), &library, 2_000));
        let offer = session.chips(2_000);
        assert_eq!(ids(&offer), ["game:note:7"]);
        assert_eq!(offer[0].label, "Leave a note for Hades next time");
        assert_eq!(offer[0].value, "note 7");
        assert_eq!(session.chips(2_000 + NOTE_OFFER_MS - 1).len(), 1);
        assert!(session.chips(2_000 + NOTE_OFFER_MS).is_empty());

        assert_eq!(session.game(7).map(|g| g.name.as_str()), Some("Hades"));
        assert!(session.noted(7));
        assert!(session.chips(2_001).is_empty());
        assert!(!session.noted(7));
    }

    #[test]
    fn a_direct_switch_keeps_the_old_offer_and_a_relaunch_drops_it() {
        let library = [game(7, "Hades"), game(8, "Celeste")];
        let mut watch = LaunchWatch::default();
        let mut session = Session::default();
        watch.note(None);
        session.note(&watch.note(Some(7)), &library, 1_000);
        // Hades to a game outside the library: Hades closed, nothing plays.
        session.note(&watch.note(Some(99)), &library, 2_000);
        assert_eq!(session.playing, None);
        assert_eq!(session.ended, Some((game(7, "Hades"), 2_000)));
        // The outside game closing keeps the Hades offer.
        assert!(!session.note(&watch.note(None), &library, 3_000));
        assert_eq!(session.ended, Some((game(7, "Hades"), 2_000)));

        session.note(&watch.note(Some(7)), &library, 4_000);
        session.note(&watch.note(Some(8)), &library, 5_000);
        let row = session.chips(5_000);
        assert_eq!(ids(&row), ["game:note:8", "game:note:7"]);
        assert_eq!(row[1].score, 100.0 - 0.001);
        // A note for the game running now leaves the closed one's offer.
        assert!(!session.noted(8));
        assert_eq!(session.ended, Some((game(7, "Hades"), 5_000)));
        // Relaunching Hades answers its own offer; Celeste just closed, so
        // it gets one.
        session.note(&watch.note(Some(7)), &library, 6_000);
        let row = session.chips(6_000);
        assert_eq!(ids(&row), ["game:note:7", "game:note:8"]);
        assert_eq!(row[0].label, "Note for Hades");
        assert_eq!(row[1].label, "Leave a note for Celeste next time");
        assert_eq!(session.ended, Some((game(8, "Celeste"), 6_000)));
        assert!(session.clear());
        assert!(!session.clear());
    }

    #[test]
    fn a_game_outside_the_library_changes_nothing() {
        let mut watch = LaunchWatch::default();
        let mut session = Session::default();
        watch.note(None);
        assert!(!session.note(&watch.note(Some(99)), &[game(7, "Hades")], 1_000));
        assert!(!session.note(&watch.note(None), &[game(7, "Hades")], 2_000));
        assert_eq!(session, Session::default());
    }

    #[test]
    fn a_relaunch_replaces_the_games_card_and_keeps_a_dismissed_one() {
        let dir = scratch("relaunch");
        let dota = game(570, "Dota 2");
        let post = |at: u64, news: ReadNews<'_>| {
            let mut card = launch_cards(
                &[Launch::Started(570)],
                std::slice::from_ref(&dota),
                &dir,
                news,
                at,
            )
            .remove(0);
            card.created_at = at;
            post_card(&dir, card).unwrap()
        };
        let quiet = |_: &SteamGame| Some(Vec::new());
        let patched =
            |_: &SteamGame| Some(vec![("Patch 7.38".into(), "https://example.com/p".into())]);
        assert!(post(1_000, &quiet));
        assert!(post(2_000, &patched));
        let shown = |dir: &Path| -> Vec<(String, grokhub_core::UpdateStatus, u64)> {
            let mut cards: Vec<_> = crate::pulse::cards(dir)
                .unwrap()
                .into_iter()
                .filter(|c| c.title == "Back to Dota 2")
                .map(|c| (c.body.unwrap_or_default(), c.status, c.created_at))
                .collect();
            cards.sort_by_key(|c| c.2);
            cards
        };
        let open = grokhub_core::UpdateStatus::Unread;
        assert_eq!(
            shown(&dir),
            [(
                "What changed since you last played: Patch 7.38.".to_string(),
                open,
                2_000
            )]
        );
        // A card the user put away stays put away; the next launch adds one.
        crate::pulse::edit_cards(&dir, &mut |cards| {
            for c in cards.iter_mut() {
                c.status = grokhub_core::UpdateStatus::Dismissed;
            }
            1
        })
        .unwrap();
        assert!(post(3_000, &quiet));
        assert_eq!(
            shown(&dir),
            [
                (
                    "What changed since you last played: Patch 7.38.".to_string(),
                    grokhub_core::UpdateStatus::Dismissed,
                    2_000
                ),
                ("No news since you last played.".to_string(), open, 3_000),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_launch_moves_the_session_before_the_news_read_and_pings_for_the_posted_card() {
        use std::cell::{Cell, RefCell};
        let dir = scratch("on-launch");
        let library = [game(7, "Hades")];
        let session = std::sync::Mutex::new(Session::default());
        let heard = Cell::new(0);
        // How many session changes the rows had heard at each news read.
        let reads = RefCell::new(Vec::new());
        let news = |_: &SteamGame| {
            reads.borrow_mut().push(heard.get());
            Some(Vec::new())
        };
        let started = [Launch::Started(7)];
        let launch = |at: u64, news: ReadNews<'_>| {
            on_launch(&dir, &session, &started, &library, news, at, &mut || {
                heard.set(heard.get() + 1)
            })
        };
        let bodies = || -> Vec<String> {
            let mut cards = crate::pulse::cards(&dir).unwrap();
            cards.sort_by_key(|c| c.created_at);
            cards
                .into_iter()
                .map(|c| c.body.unwrap_or_default())
                .collect()
        };

        // Off: nothing moves, nothing is read and nothing posts.
        assert!(launch(1_000, &news).is_empty());
        assert_eq!(heard.get(), 0);
        assert!(reads.borrow().is_empty());
        assert_eq!(*lock(&session), Session::default());
        assert!(bodies().is_empty());

        // On, ping off: the rows hear first, then the news is read, and the
        // card goes up with no ping.
        set(&dir, true).unwrap();
        assert!(launch(2_000, &news).is_empty());
        assert_eq!(*reads.borrow(), [1]);
        assert_eq!(lock(&session).playing, Some(game(7, "Hades")));
        assert_eq!(bodies(), ["No news since you last played."]);

        // Ping on, and a note in the user's own words: the card still goes
        // up, and it is the one to ping for.
        set_ping(&dir, true).unwrap();
        set_note(&dir, 7, "finish this task").unwrap();
        lock(&session).clear();
        let pings = launch(3_000, &news);
        assert_eq!(
            pings.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
            ["Back to Hades"]
        );
        assert_eq!(
            bodies(),
            ["No news since you last played. Your note from last time: finish this task"]
        );

        // Off lands while the news is read: no card and no ping.
        lock(&session).clear();
        let off_midway = |_: &SteamGame| {
            set(&dir, false).unwrap();
            Some(Vec::new())
        };
        assert!(launch(4_000, &off_midway).is_empty());
        assert_eq!(
            bodies(),
            ["No news since you last played. Your note from last time: finish this task"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_closed_games_offer_expires_once_and_stays_off_the_palette_and_a_browser() {
        let mut session = Session {
            playing: Some(game(8, "Celeste")),
            ended: Some((game(7, "Hades"), 1_000)),
        };
        assert_eq!(
            ids(&row_extra(&session, crate::chips::HOME, false, 2_000)),
            ["game:note:8", "game:note:7"]
        );
        assert_eq!(
            ids(&row_extra(&session, crate::chips::COMPOSER, false, 2_000)),
            ["game:note:8", "game:note:7"]
        );
        assert!(row_extra(&session, crate::chips::PALETTE, false, 2_000).is_empty());
        assert!(row_extra(&session, crate::chips::HOME, true, 2_000).is_empty());

        assert!(!session.expire(1_000 + NOTE_OFFER_MS - 1));
        assert!(session.expire(1_000 + NOTE_OFFER_MS));
        assert_eq!(session.ended, None);
        assert!(!session.expire(1_000 + NOTE_OFFER_MS));
        assert_eq!(session.playing, Some(game(8, "Celeste")));
    }

    #[test]
    fn the_note_dialog_names_the_game_from_the_session_then_the_library() {
        let dir = scratch("note-for");
        set_note(&dir, 7, "beat Megaera next").unwrap();
        let session = Session {
            playing: None,
            ended: Some((game(7, "Hades"), 1)),
        };
        let unread = || -> Vec<SteamGame> { panic!("the session knew the name") };
        assert_eq!(
            note_for(&dir, &session, 7, unread),
            NoteFor {
                game: "Hades".into(),
                note: "beat Megaera next".into()
            }
        );
        let from_library = note_for(&dir, &Session::default(), 7, || vec![game(7, "Hades II")]);
        assert_eq!(from_library.game, "Hades II");
        let unknown = note_for(&dir, &Session::default(), 9, Vec::new);
        assert_eq!(
            unknown,
            NoteFor {
                game: "this game".into(),
                note: String::new()
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn companion_notes_round_trip_and_frame_shows_guides() {
        let dir =
            std::env::temp_dir().join(format!("shell-companion-frame-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(
            set_note(&dir, 620, "finish the co-op").unwrap(),
            "finish the co-op"
        );
        assert_eq!(get_note(&dir, 620), "finish the co-op");
        let f = frame(
            &dir,
            620,
            Some(620),
            Some(vec![("Update".into(), "https://example.com/u".into())]),
        );
        assert_eq!(f.note, "finish the co-op");
        assert_eq!(f.news.len(), 1);
        assert_eq!(f.news[0].title, "Update");
        assert_eq!(
            f.guides_url.as_deref(),
            Some("https://steamcommunity.com/app/620/guides/?browsefilter=toprated")
        );
        assert_eq!(set_note(&dir, 620, "").unwrap(), "");
        assert_eq!(get_note(&dir, 620), "");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
