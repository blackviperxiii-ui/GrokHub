//! Game launch companion (`grokhub_core::game_companion`), opt-in under
//! Settings → Behavior. Every few seconds, off the UI thread, the cabin asks
//! what Steam says is running. A launch of a library game fetches that
//! game's news and posts one Feed card with what changed since the user last
//! played, a guides link and their note from last time; a quiet desktop
//! notification follows when the user turned that on. When the game closes,
//! a one-tap offers to leave a note for next time. Nothing touches the game.
//! The same sample tells the game watcher (`game_watch_ui`) when a game it
//! listens for runs, so the sampling also runs with only the watcher on.

use std::path::PathBuf;

use super::*;
use grokhub_core::game_companion::{self as gc, Companion, GameNotes, Launch, LaunchWatch};
use grokhub_core::pulse_sources::{Fetched, SourceKind, SourceReq};
use grokhub_core::steam::SteamGame;
use grokhub_core::QuickChip;

const SAMPLE_EVERY_MS: u64 = 5_000;
/// How long "Leave a note for next time" stays after the game closes.
const NOTE_OFFER_MS: u64 = 30 * 60 * 1000;

#[derive(Default)]
pub(super) struct GameUi {
    watch: LaunchWatch,
    sampled_ms: u64,
    sample_rx: Option<mpsc::Receiver<Option<u32>>>,
    news_rx: Option<mpsc::Receiver<Companion>>,
    playing: Option<SteamGame>,
    ended: Option<(SteamGame, u64)>,
    /// The note being written: game and text.
    editing: Option<(SteamGame, String)>,
}

/// What Steam says is running: its launch wrapper's command line on Linux
/// (or `registry.vdf`), its `RunningAppID` value on Windows.
fn running_now() -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        if let Some(id) =
            super::local_signals::processes().and_then(|p| gc::running_from_cmdlines(&p))
        {
            return Some(id);
        }
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        let text = std::fs::read_to_string(home.join(".steam").join("registry.vdf")).ok()?;
        gc::running_from_registry_vdf(&text)
    }
    #[cfg(windows)]
    {
        crate::win_native::hkcu_dword(r"Software\Valve\Steam", "RunningAppID").filter(|id| *id != 0)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        None
    }
}

/// The game's news since it was last played, through the source fetcher.
fn read_news(game: &SteamGame) -> Option<Vec<(String, String)>> {
    let req = SourceReq {
        kind: SourceKind::Rss,
        url: game.news_feed(),
        name: format!("{} on Steam", game.name),
        topic: None,
    };
    let body = match super::pulse_sources_ui::fetch_source(&req, None) {
        Fetched::Body { body, .. } => body,
        Fetched::NotModified | Fetched::Failed(_) => return None,
    };
    let items = grokhub_core::pulse_sources::parse(&req, &body);
    Some(
        gc::news_since(items, game.last_played)
            .into_iter()
            .map(|i| (i.title, i.url))
            .collect(),
    )
}

fn notes_dir() -> PathBuf {
    crate::config::config_dir()
}

impl Cabin {
    /// Each frame: sample what is running every few seconds, and take the
    /// news for a launch when it is back.
    pub(super) fn tick_game_companion(&mut self) {
        if !self.cfg.game_companion && !self.cfg.game_watch {
            return;
        }
        if let Some(rx) = self.focus.game.news_rx.take() {
            match rx.try_recv() {
                Ok(companion) => self.post_companion(companion),
                Err(mpsc::TryRecvError::Empty) => self.focus.game.news_rx = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
        if let Some(rx) = self.focus.game.sample_rx.take() {
            match rx.try_recv() {
                Ok(running) => self.note_running(running, now_ms()),
                Err(mpsc::TryRecvError::Empty) => self.focus.game.sample_rx = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
            return;
        }
        let now = now_ms();
        if now.saturating_sub(self.focus.game.sampled_ms) < SAMPLE_EVERY_MS {
            return;
        }
        self.focus.game.sampled_ms = now;
        let (tx, rx) = mpsc::channel();
        self.focus.game.sample_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(running_now());
        });
    }

    /// The library game that closed in the last half hour, while no other runs.
    pub(super) fn game_just_ended(&self) -> Option<&str> {
        let (game, at) = self.focus.game.ended.as_ref()?;
        (self.game_running().is_none() && now_ms().saturating_sub(*at) < NOTE_OFFER_MS)
            .then_some(game.name.as_str())
    }

    /// The Steam app id the last sample saw running.
    pub(super) fn game_running(&self) -> Option<u32> {
        self.focus.game.watch.current
    }

    /// One sample. A launch of a library game fetches its news off the UI thread.
    pub(super) fn note_running(&mut self, running: Option<u32>, now: u64) {
        let events = self.focus.game.watch.note(running);
        if events.is_empty() || !self.cfg.game_companion {
            return;
        }
        let library = self.steam_library(now).1.to_vec();
        let find = |id: u32| library.iter().find(|g| g.app_id == id).cloned();
        for event in events {
            match event {
                Launch::Started(id) => {
                    let Some(game) = find(id) else {
                        continue;
                    };
                    self.focus.game.playing = Some(game.clone());
                    // The game the user switched away from keeps its note offer.
                    if self
                        .focus
                        .game
                        .ended
                        .as_ref()
                        .is_some_and(|(g, _)| g.app_id == id)
                    {
                        self.focus.game.ended = None;
                    }
                    self.chip_paint_key.clear();
                    let note = gc::load_notes(&notes_dir()).get(id).map(str::to_string);
                    let (tx, rx) = mpsc::channel();
                    self.focus.game.news_rx = Some(rx);
                    std::thread::spawn(move || {
                        let news = read_news(&game);
                        let _ = tx.send(Companion { game, news, note });
                    });
                }
                Launch::Ended(id) => {
                    self.focus.game.playing = None;
                    // An untracked app closing keeps the last library game's offer.
                    if let Some(game) = find(id) {
                        self.focus.game.ended = Some((game, now));
                    }
                    self.chip_paint_key.clear();
                }
            }
        }
    }

    fn post_companion(&mut self, companion: Companion) {
        let card = companion.companion_card(now_ms());
        if self.cfg.game_companion_ping && !self.window_focused && !self.quiet_now() {
            crate::notify::ping(&card.title, card.body.as_deref().unwrap_or_default());
        }
        // One card per game: the latest launch replaces the last one.
        self.updates.retain(|c| c.source_id != card.source_id);
        if grokhub_core::post_update(&mut self.updates, card) {
            self.persist_updates();
        }
    }

    /// Guides and a note while a game runs; a note offer once it closes, also
    /// while the next game runs after a direct switch.
    pub(super) fn game_chips(&self) -> Vec<QuickChip> {
        if !self.cfg.game_companion {
            return Vec::new();
        }
        let state = &self.focus.game;
        let offer = match &state.ended {
            Some((game, at))
                if now_ms().saturating_sub(*at) < NOTE_OFFER_MS
                    && state
                        .playing
                        .as_ref()
                        .is_none_or(|p| p.app_id != game.app_id) =>
            {
                gc::chips(game, true)
            }
            _ => Vec::new(),
        };
        let mut chips = state
            .playing
            .as_ref()
            .map(|game| gc::chips(game, false))
            .unwrap_or_default();
        chips.extend(offer);
        chips
            .into_iter()
            .filter(|c| !self.chip_dismissed.iter().any(|d| d == &c.id))
            .collect()
    }

    /// The guides link opens; the note one-tap opens the note box. True when
    /// `chip` was one of them.
    pub(super) fn take_game_chip(&mut self, chip: &QuickChip) -> bool {
        if chip.id.starts_with(gc::GUIDES_PREFIX) {
            self.follow_update_action(Some(grokhub_core::UpdateAction::DeepLink {
                href: chip.value.clone(),
            }));
            return true;
        }
        let Some(id) = chip
            .id
            .strip_prefix(gc::NOTE_PREFIX)
            .and_then(|id| id.parse::<u32>().ok())
        else {
            return false;
        };
        let state = &self.focus.game;
        let game = state
            .playing
            .iter()
            .chain(state.ended.as_ref().map(|(g, _)| g))
            .find(|g| g.app_id == id)
            .cloned();
        if let Some(game) = game {
            let text = gc::load_notes(&notes_dir())
                .get(id)
                .unwrap_or_default()
                .to_string();
            self.focus.game.editing = Some((game, text));
        }
        true
    }

    fn save_game_note(&mut self, app_id: u32, text: &str) {
        let dir = notes_dir();
        let mut notes: GameNotes = gc::load_notes(&dir);
        notes.set(app_id, text);
        self.status = match gc::save_notes(&dir, &notes) {
            Ok(()) => "Saved. It shows on the card next time you launch it.".into(),
            Err(err) => err,
        };
        if self
            .focus
            .game
            .ended
            .as_ref()
            .is_some_and(|(g, _)| g.app_id == app_id)
        {
            self.focus.game.ended = None;
        }
        self.chip_paint_key.clear();
    }

    /// The note box, a small window over the cabin.
    pub(super) fn paint_game_note(&mut self, ctx: &egui::Context) {
        let Some((game, mut text)) = self.focus.game.editing.take() else {
            return;
        };
        let mut done: Option<bool> = None;
        egui::Window::new(format!("Note for {}", game.name))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                crate::cards::help_text(ui, "Shown on the card the next time you launch it.");
                ui.add(
                    egui::TextEdit::multiline(&mut text)
                        .hint_text("remember I was farming X")
                        .desired_rows(3)
                        .desired_width(320.0)
                        .char_limit(gc::NOTE_CHARS),
                );
                ui.horizontal(|ui| {
                    if crate::cards::white_pill(ui, "Save") {
                        done = Some(true);
                    }
                    if crate::cards::ghost_pill(ui, "Cancel") {
                        done = Some(false);
                    }
                });
            });
        match done {
            Some(true) => self.save_game_note(game.app_id, &text),
            Some(false) => {}
            None => self.focus.game.editing = Some((game, text)),
        }
    }

    /// Which game the in-game watcher supports first, from the Steam
    /// library's playtime read now (`grokhub_core::game_watch_pick`).
    fn game_watch_first_line(&mut self, now: u64) -> String {
        let (_, games) = self.steam_library(now);
        grokhub_core::game_watch_pick::settings_line(games)
    }

    pub(super) fn ui_game_companion_row(&mut self, ui: &mut egui::Ui) {
        let mut on = self.cfg.game_companion;
        if crate::cards::settings_toggle(
            ui,
            "Game launch companion",
            "When you start a game from your Steam library, the Feed shows what changed since you last played, a link to top guides, and your note from last time. It only reads what Steam reports as running: no screen capture, nothing drawn over the game, nothing a game's anti-cheat would see.",
            &mut on,
        ) {
            self.cfg.game_companion = on;
            // The game watcher keeps what the last sample saw, so its
            // listener doesn't drop mid-match.
            let seen = std::mem::take(&mut self.focus.game.watch);
            self.focus.game = GameUi::default();
            if self.cfg.game_watch {
                self.focus.game.watch = seen;
            }
            self.chip_paint_key.clear();
            self.persist_cfg();
            self.status = "Saved".into();
        }
        let line = self.game_watch_first_line(now_ms());
        crate::cards::settings_note(ui, &line);
        if !self.cfg.game_companion {
            return;
        }
        let mut ping = self.cfg.game_companion_ping;
        ui.indent("game-companion", |ui| {
            if ui
                .checkbox(&mut ping, "Also send a quiet desktop notification")
                .changed()
            {
                self.cfg.game_companion_ping = ping;
                self.persist_cfg();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: u32, name: &str) -> SteamGame {
        SteamGame {
            app_id: id,
            name: name.into(),
            last_played: 1_700_000_000,
            playtime_min: 60,
        }
    }

    #[test]
    fn a_launch_posts_the_card_and_a_close_offers_a_note() {
        let root = crate::config::test_config_root("game-companion");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let _pin = crate::config::TestConfigDir::set(root);
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.game_companion = true;
        cabin.focus.game.watch.primed = true;
        cabin.focus.game.playing = Some(game(7, "Hades"));
        let labels: Vec<String> = cabin.game_chips().into_iter().map(|c| c.label).collect();
        assert_eq!(labels, vec!["Top guides for Hades", "Note for Hades"]);
        cabin.post_companion(Companion {
            game: game(7, "Hades"),
            news: Some(vec![(
                "Patch 1.1".into(),
                "https://store.steampowered.com/news/1".into(),
            )]),
            note: Some("try the spear".into()),
        });
        let card = cabin
            .updates
            .iter()
            .find(|c| c.title == "Back to Hades")
            .expect("card");
        assert_eq!(
            card.body.as_deref(),
            Some("What changed since you last played: Patch 1.1. Your note from last time: try the spear")
        );
        cabin.post_companion(Companion {
            game: game(7, "Hades"),
            news: Some(Vec::new()),
            note: None,
        });
        let bodies: Vec<&str> = cabin
            .updates
            .iter()
            .filter(|c| c.title == "Back to Hades")
            .filter_map(|c| c.body.as_deref())
            .collect();
        assert_eq!(
            bodies,
            vec!["No news since you last played."],
            "the latest launch replaces the card"
        );
        cabin.focus.game.playing = None;
        cabin.focus.game.ended = Some((game(7, "Hades"), now_ms()));
        let note = cabin.game_chips().remove(0);
        assert_eq!(note.label, "Leave a note for next time");
        assert!(cabin.take_game_chip(&note));
        assert_eq!(
            cabin
                .focus
                .game
                .editing
                .as_ref()
                .map(|(g, t)| (g.app_id, t.as_str())),
            Some((7, ""))
        );
        cabin.save_game_note(7, "beat Megaera next");
        assert_eq!(
            gc::load_notes(&notes_dir()).get(7),
            Some("beat Megaera next")
        );
        assert!(
            cabin.game_chips().is_empty(),
            "the offer goes once the note is saved"
        );
        cabin.cfg.game_companion = false;
        cabin.focus.game.playing = Some(game(7, "Hades"));
        assert!(cabin.game_chips().is_empty());
    }

    #[test]
    fn a_direct_switch_closes_the_old_game_and_keeps_its_note_offer() {
        let root = crate::config::test_config_root("game-companion-switch");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let _pin = crate::config::TestConfigDir::set(root);
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.game_companion = true;
        cabin.sources.steam = super::super::steam_ui::SteamState::for_test(
            vec![game(7, "Hades"), game(8, "Celeste")],
            1_000,
        );
        cabin.focus.game.watch = LaunchWatch {
            current: Some(7),
            primed: true,
        };
        cabin.focus.game.playing = Some(game(7, "Hades"));
        // Straight to a game outside the library: Hades still closes.
        cabin.note_running(Some(99), 2_000);
        assert!(cabin.focus.game.playing.is_none());
        assert_eq!(
            cabin
                .focus
                .game
                .ended
                .as_ref()
                .map(|(g, at)| (g.app_id, *at)),
            Some((7, 2_000))
        );
        // The untracked app closing keeps Hades's offer.
        cabin.note_running(None, 3_000);
        assert_eq!(
            cabin
                .focus
                .game
                .ended
                .as_ref()
                .map(|(g, at)| (g.app_id, *at)),
            Some((7, 2_000))
        );
        cabin.focus.game.ended = Some((game(7, "Hades"), now_ms()));
        cabin.focus.game.playing = Some(game(8, "Celeste"));
        let chips: Vec<(String, String)> = cabin
            .game_chips()
            .into_iter()
            .map(|c| (c.id, c.label))
            .collect();
        assert_eq!(
            chips,
            vec![
                (
                    "game:guides:8".to_string(),
                    "Top guides for Celeste".to_string()
                ),
                ("game:note:8".to_string(), "Note for Celeste".to_string()),
                (
                    "game:note:7".to_string(),
                    "Leave a note for next time".to_string()
                ),
            ]
        );
        // A note for the running game leaves the other game's offer.
        cabin.save_game_note(8, "dash earlier");
        assert_eq!(
            cabin.focus.game.ended.as_ref().map(|(g, _)| g.app_id),
            Some(7)
        );
        // Relaunching the same game drops its own offer.
        cabin.focus.game.playing = Some(game(7, "Hades"));
        let labels: Vec<String> = cabin.game_chips().into_iter().map(|c| c.label).collect();
        assert_eq!(labels, vec!["Top guides for Hades", "Note for Hades"]);
    }

    #[test]
    fn the_sampler_runs_for_the_game_watcher_alone_and_not_with_both_off() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.tick_game_companion();
        assert!(cabin.focus.game.sample_rx.is_none(), "both off: no sample");
        cabin.cfg.game_watch = true;
        cabin.tick_game_companion();
        assert!(cabin.focus.game.sample_rx.is_some());
        // A sample with only the watcher on is kept for it, and starts no
        // companion card.
        cabin.note_running(Some(730), 1_000);
        cabin.note_running(None, 2_000);
        cabin.note_running(Some(730), 3_000);
        assert_eq!(cabin.game_running(), Some(730));
        assert!(cabin.focus.game.playing.is_none());
        assert!(cabin.focus.game.news_rx.is_none());
    }

    #[test]
    fn a_game_outside_the_library_starts_nothing() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.game_companion = true;
        cabin.focus.game.watch.primed = true;
        cabin.sources.steam =
            super::super::steam_ui::SteamState::for_test(vec![game(7, "Hades")], 1_000);
        cabin.note_running(Some(99), 1_000);
        assert!(cabin.focus.game.playing.is_none() && cabin.focus.game.news_rx.is_none());
        cabin.note_running(None, 2_000);
        assert!(cabin.focus.game.ended.is_none());
    }

    #[test]
    fn settings_names_the_watchers_first_game_from_steam_playtime() {
        let mut cabin = Cabin::quiet_for_test();
        let now = now_ms();
        let played = |id, name: &str, playtime_min| SteamGame {
            playtime_min,
            ..game(id, name)
        };
        cabin.sources.steam.seed_library_for_test(
            vec![
                played(7, "Hades", 9_000),
                played(730, "Counter-Strike 2", 40),
                played(570, "Dota 2", 400),
            ],
            true,
            now,
        );
        assert_eq!(
            cabin.game_watch_first_line(now),
            "Game watcher: Dota 2 first, most played in your Steam library. League of Legends and World of Warcraft aren't on Steam, so they aren't ranked."
        );
        cabin
            .sources
            .steam
            .seed_library_for_test(vec![game(7, "Hades")], true, now);
        assert_eq!(
            cabin.game_watch_first_line(now),
            "Game watcher: Counter-Strike 2 first by default, since no supported game has playtime in your Steam library. League of Legends and World of Warcraft aren't on Steam, so they aren't ranked."
        );
    }

    fn chip_labels(cabin: &mut Cabin) -> Vec<String> {
        cabin.chip_paint_key.clear();
        cabin.refresh_chips();
        cabin
            .visible_chips
            .iter()
            .map(|c| c.label.clone())
            .collect()
    }

    #[test]
    fn quick_chips_follow_the_game_that_runs_and_the_one_that_closed() {
        let mut cabin = Cabin::quiet_for_test();
        // A returning user: a first-run cabin shows the guide row instead.
        cabin.usage.messages = 20;
        cabin.sources.steam =
            super::super::steam_ui::SteamState::for_test(vec![game(7, "Hades")], 1_000);
        cabin.focus.game.watch.primed = true;
        cabin.focus.game.watch.current = Some(7);
        let playing = chip_labels(&mut cabin);
        assert!(
            playing.iter().any(|l| l == "Fix stutter in Hades"),
            "{playing:?}"
        );
        cabin.focus.game.watch.current = None;
        cabin.focus.game.ended = Some((game(7, "Hades"), now_ms()));
        let closed = chip_labels(&mut cabin);
        assert!(closed.iter().any(|l| l == "How'd Hades go?"), "{closed:?}");
        assert!(closed.iter().all(|l| !l.starts_with("Fix stutter")));
        cabin.focus.game.ended = Some((game(7, "Hades"), now_ms() - NOTE_OFFER_MS - 1));
        let later = chip_labels(&mut cabin);
        assert!(later.iter().all(|l| !l.contains("Hades")), "{later:?}");
    }

    /// The chip signals read state the cabin already holds: no Steam read,
    /// process sample, drive probe, file or network call of their own.
    #[test]
    fn chip_signals_read_only_cached_state() {
        let body = |src: &str, start: &str, end: &str| -> String {
            let from = src.find(start).unwrap_or_else(|| panic!("missing {start}"));
            let rest = &src[from..];
            rest[..rest[start.len()..].find(end).expect(end) + start.len()].to_string()
        };
        let chips = include_str!("chips.rs").replace("\r\n", "\n");
        let fill = body(&chips, "let game = self", "let key = format!(");
        let steam = include_str!("steam_ui.rs").replace("\r\n", "\n");
        let name = body(&steam, "fn steam_cached_name(", "\n    }\n");
        let own = include_str!("game_companion_ui.rs").replace("\r\n", "\n");
        let ended = body(&own, "fn game_just_ended(", "\n    }\n");
        let live = include_str!("home/live.rs").replace("\r\n", "\n");
        let drives = body(&live, "fn cached_drives(", "\n}\n");
        for (what, src) in [
            ("fill", &fill),
            ("name", &name),
            ("ended", &ended),
            ("drives", &drives),
        ] {
            for io in [
                "refresh(",
                "steam_library(",
                "running_now(",
                "spawn",
                "std::fs",
                "Command",
                "live_facts(",
                "local_drives(",
                "peek(",
                "http",
            ] {
                assert!(!src.contains(io), "{what} must not call {io}: {src}");
            }
        }
    }
}
