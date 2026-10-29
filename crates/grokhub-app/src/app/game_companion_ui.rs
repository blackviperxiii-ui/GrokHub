//! Game launch companion (`grokhub_core::game_companion`), opt-in under
//! Settings → Behavior. Every few seconds, off the UI thread, the cabin asks
//! what Steam says is running. A launch of a library game fetches that
//! game's news and posts one Feed card with what changed since the user last
//! played, a guides link and their note from last time; a quiet desktop
//! notification follows when the user turned that on. When the game closes,
//! a one-tap offers to leave a note for next time. Nothing touches the game.

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
        if !self.cfg.game_companion {
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

    /// One sample. A launch of a library game fetches its news off the UI thread.
    fn note_running(&mut self, running: Option<u32>, now: u64) {
        let events = self.focus.game.watch.note(running);
        if events.is_empty() {
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

    pub(super) fn ui_game_companion_row(&mut self, ui: &mut egui::Ui) {
        let mut on = self.cfg.game_companion;
        if crate::cards::settings_toggle(
            ui,
            "Game launch companion",
            "When you start a game from your Steam library, the Feed shows what changed since you last played, a link to top guides, and your note from last time. It only reads what Steam reports as running: no screen capture, nothing drawn over the game, nothing a game's anti-cheat would see.",
            &mut on,
        ) {
            self.cfg.game_companion = on;
            self.focus.game = GameUi::default();
            self.chip_paint_key.clear();
            self.persist_cfg();
            self.status = "Saved".into();
        }
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
}
