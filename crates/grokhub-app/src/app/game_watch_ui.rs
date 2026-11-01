//! Game watcher (`grokhub_core::game_watch_gsi`), opt-in under Settings →
//! Behavior. On: the cabin writes one small Game State Integration file into
//! each supported game's own cfg folder (Valve's documented way for an app to
//! get the match state). The loopback listener opens only while the game
//! launch companion's sample (`game_companion_ui`, what Steam says is
//! running) shows a supported game, and closes when the game exits. Pointers
//! show as a quiet toast here in the cabin, at most one a minute, and the
//! match's end posts one recap card to Pulse. Off: the listener stops and the
//! file goes. Nothing reads the game's memory, touches the game or draws over
//! it. The event side (`note_watch_event`) takes any game's `WatchEvent`s, so
//! adapters that aren't GSI can feed it too.

use std::path::PathBuf;
use std::time::Duration;

use super::*;
use grokhub_core::game_watch::{self as gw, MatchNews, MatchTracker, ToastGate, WatchEvent};
use grokhub_core::game_watch_gsi::{self as gsi, GsiEvent, GsiGame, GsiServer, GsiToken};

/// Events held for the cabin; past this new ones are dropped, never queued
/// without end.
const EVENT_QUEUE: usize = 256;
/// How long a pointer toast stays.
const POINTER_TOAST_SECS: u64 = 8;

#[derive(Default)]
pub(super) struct WatchState {
    server: Option<GsiServer>,
    events: Option<mpsc::Receiver<GsiEvent>>,
    token: Option<GsiToken>,
    /// Set up once per session, on the first tick with the watcher on.
    started: bool,
    /// What Settings shows under the toggle.
    note: String,
    /// The supported game the last sample saw, once the listener was tried
    /// for it. Cleared when it exits, so the next launch tries again.
    listening_for: Option<GsiGame>,
    /// Why the listener couldn't open for the game running now.
    listen_note: String,
    heard: u32,
    last_game: Option<GsiGame>,
    /// The match being followed, and the game it is in.
    tracker: MatchTracker,
    match_game: Option<&'static str>,
    toast_gate: ToastGate,
    /// Tests listen on a free port and use a temp Steam folder.
    port: Option<u16>,
    pub(super) steam_root: Option<PathBuf>,
}

impl WatchState {
    pub(super) fn steam_root(&self) -> Option<PathBuf> {
        self.steam_root
            .clone()
            .or_else(grokhub_core::steam::detect_root)
    }

    /// Close the port and forget the match in progress.
    fn stop_listening(&mut self) {
        if let Some(mut server) = self.server.take() {
            server.stop();
        }
        self.events = None;
        self.tracker = MatchTracker::default();
        self.match_game = None;
    }
}

fn names(games: &[GsiGame]) -> String {
    let names: Vec<&str> = games.iter().map(|g| g.name()).collect();
    names.join(" and ")
}

/// The line under the toggle once the cfg files are written: which games
/// were set up, or why none was.
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

/// The status after turning the watcher off.
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

impl Cabin {
    /// Each frame: set up once when the watcher is on, open or close the
    /// listener as the game starts or exits, then follow what the game sent.
    /// The listener itself sits blocked between posts.
    pub(super) fn tick_game_watch(&mut self) {
        if !self.cfg.game_watch {
            return;
        }
        if !self.focus.watch.started {
            self.start_game_watch();
        }
        let running = self.game_running().and_then(GsiGame::from_app_id);
        self.follow_game(running);
        self.take_watch_events(now_ms());
    }

    /// Write the cfg files that point the games here. They stay while the
    /// watcher is on; the game reads them once at launch.
    fn start_game_watch(&mut self) {
        let w = &mut self.focus.watch;
        w.started = true;
        let token = match GsiToken::load_or_create(&crate::config::config_dir()) {
            Ok(token) => token,
            Err(e) => {
                w.note = e;
                return;
            }
        };
        let found = w
            .steam_root()
            .map(|root| gsi::install_cfgs(&root, &token, w.port.unwrap_or(gsi::GSI_PORT)));
        w.note = setup_note(found.as_deref());
        w.token = Some(token);
    }

    /// Listen while a supported game runs, and close the port when it
    /// exits. A port another app holds is tried once per launch.
    fn follow_game(&mut self, running: Option<GsiGame>) {
        let w = &mut self.focus.watch;
        let Some(game) = running else {
            if w.listening_for.take().is_some() {
                w.stop_listening();
            }
            w.listen_note.clear();
            return;
        };
        if w.listening_for.replace(game) == Some(game) || w.server.is_some() {
            return;
        }
        let Some(token) = w.token.clone() else {
            return;
        };
        let (tx, rx) = mpsc::sync_channel(EVENT_QUEUE);
        match GsiServer::start(w.port.unwrap_or(gsi::GSI_PORT), token, tx) {
            Ok(server) => {
                w.server = Some(server);
                w.events = Some(rx);
                w.listen_note.clear();
            }
            Err(e) => w.listen_note = e.to_string(),
        }
    }

    /// Everything the listener queued since the last frame.
    fn take_watch_events(&mut self, now: u64) {
        let Some(rx) = &self.focus.watch.events else {
            return;
        };
        let events: Vec<GsiEvent> = rx.try_iter().collect();
        for e in events {
            self.focus.watch.heard += 1;
            self.focus.watch.last_game = Some(e.game);
            self.note_watch_event(e.game.name(), e.event, now);
        }
    }

    /// One match event from any game's adapter: a new pointer becomes a
    /// quiet toast, the match's end the recap card.
    pub(super) fn note_watch_event(&mut self, game: &'static str, event: WatchEvent, now: u64) {
        let w = &mut self.focus.watch;
        if w.match_game != Some(game) {
            w.tracker = MatchTracker::default();
            w.match_game = Some(game);
        }
        for news in w.tracker.take(event) {
            match news {
                MatchNews::Pointer(text) => self.toast_pointer(text, now),
                MatchNews::End { recap, pointers } => {
                    let card = gw::recap_card(game, &recap, &pointers, now);
                    if grokhub_core::post_update(&mut self.updates, card) {
                        self.persist_updates();
                    }
                }
            }
        }
    }

    /// An in-cabin toast: no sound, no focus change, nothing over the game.
    /// One that comes within a minute of the last is left to the recap.
    fn toast_pointer(&mut self, text: String, now: u64) {
        if self.focus.watch.toast_gate.allow(now) {
            self.toasts
                .info(text)
                .duration(Some(Duration::from_secs(POINTER_TOAST_SECS)))
                .closable(true);
        }
    }

    /// Stop listening and remove the cfg files Loadout wrote.
    fn stop_game_watch(&mut self) -> String {
        let w = &mut self.focus.watch;
        w.stop_listening();
        let removed = w.steam_root().map(|root| gsi::remove_cfgs(&root));
        let (port, steam_root) = (w.port, w.steam_root.take());
        *w = WatchState {
            port,
            steam_root,
            ..WatchState::default()
        };
        removed_note(removed.as_deref())
    }

    fn set_game_watch(&mut self, on: bool) {
        self.cfg.game_watch = on;
        self.persist_cfg();
        if on {
            self.focus.watch.started = false;
            self.tick_game_watch();
            self.status = "Saved".into();
        } else {
            self.status = self.stop_game_watch();
        }
    }

    pub(super) fn ui_game_watch_row(&mut self, ui: &mut egui::Ui) {
        let mut on = self.cfg.game_watch;
        if crate::cards::settings_toggle(
            ui,
            "Game watcher (Counter-Strike 2, Dota 2)",
            "Follows your match as you play, through Game State Integration, the feed Valve built and supports for this. Turning it on writes one small file into the game's own config folder that tells the game to send its state to Loadout, here on this computer only, while the game runs. Tips show as a quiet note here in Loadout, never in the game, and a recap posts to Pulse when the match ends. Turning it off removes that file. Nothing reads the game's memory or touches the game.",
            &mut on,
        ) {
            self.set_game_watch(on);
        }
        ui_supported_games(ui);
        if !self.cfg.game_watch {
            return;
        }
        let elite = self.elite_line();
        let stardew = self.stardew_line();
        let w = &self.focus.watch;
        let listening = match (&w.server, w.listening_for) {
            (Some(_), Some(game)) => format!("Listening while {} runs.", game.name()),
            _ if !w.listen_note.is_empty() => w.listen_note.clone(),
            _ => "Listens only while Counter-Strike 2 or Dota 2 runs, and stops when it closes."
                .into(),
        };
        let heard = match w.last_game {
            Some(game) if w.heard > 0 => format!(
                "Heard {} game events this session, the last from {}.",
                w.heard,
                game.name()
            ),
            _ => String::new(),
        };
        let note = w.note.clone();
        let games: Vec<String> = [self.poe_line(), self.minecraft_line()]
            .into_iter()
            .flatten()
            .collect();
        let rl = self.rl_line();
        ui.indent("game-watch", |ui| {
            if let Some(elite) = &elite {
                crate::cards::help_text(ui, elite);
            }
            if let Some(stardew) = &stardew {
                crate::cards::help_text(ui, stardew);
            }
            if !note.is_empty() {
                crate::cards::help_text(ui, &note);
            }
            crate::cards::help_text(ui, &listening);
            if !heard.is_empty() {
                crate::cards::help_text(ui, &heard);
            }
            for line in &games {
                crate::cards::help_text(ui, line);
            }
            if let Some((line, how_to)) = &rl {
                crate::cards::help_text(ui, line);
                if *how_to && crate::cards::ghost_pill(ui, "Copy the setting") {
                    ui.ctx()
                        .copy_text(grokhub_core::game_watch_rl::INI_LINE.to_string());
                }
            }
        });
        self.ui_eso(ui);
        self.ui_gw2(ui);
        self.ui_destiny(ui);
    }
}

/// The games Loadout can read and how, folded away under the game watcher.
fn ui_supported_games(ui: &mut egui::Ui) {
    egui::CollapsingHeader::new("Games Loadout can read, and how")
        .id_salt("game-integrations")
        .default_open(false)
        .show(ui, |ui| {
            for game in grokhub_core::game_integrations::ALL {
                crate::cards::help_text(ui, &supported_line(game));
            }
            crate::cards::help_text(
                ui,
                "All read-only: nothing reads game memory, hooks the game or sends it input.",
            );
        });
}

fn supported_line(game: &grokhub_core::game_integrations::Integration) -> String {
    format!("{}: {}.", game.name, game.how())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_supported_games_list_names_each_game_and_its_way_in() {
        let wow = grokhub_core::game_integrations::by_id("wow").unwrap();
        let line = supported_line(wow);
        assert!(line.starts_with("World of Warcraft: Loadout's WoW addon"));
        assert!(line.ends_with("Read-only alongside Warden."));
    }
    use std::io::{Read, Write};

    #[test]
    fn the_setup_and_off_notes_say_what_happened() {
        assert_eq!(
            setup_note(None),
            "Steam wasn't found on this computer, so no game was set up."
        );
        assert_eq!(
            setup_note(Some(&[])),
            "No supported game (Counter-Strike 2 or Dota 2) is installed in your Steam library yet."
        );
        assert_eq!(
            setup_note(Some(&[
                (GsiGame::Cs2, Ok(PathBuf::from("a"))),
                (GsiGame::Dota2, Err("Couldn't write the Dota 2 game watcher file: denied".into())),
            ])),
            "Set up for Counter-Strike 2. Start the game, or restart it if it's open, so it reads the file. Couldn't write the Dota 2 game watcher file: denied"
        );
        assert_eq!(
            removed_note(Some(&[
                (GsiGame::Cs2, Ok(true)),
                (GsiGame::Dota2, Ok(true))
            ])),
            "Game watcher off. Removed its file from Counter-Strike 2 and Dota 2."
        );
        assert_eq!(
            removed_note(Some(&[(GsiGame::Cs2, Ok(false))])),
            "Game watcher off."
        );
        assert_eq!(removed_note(None), "Game watcher off.");
    }

    /// A loopback port nothing holds right now, so the cfg can name it
    /// before the listener binds it.
    fn free_port() -> u16 {
        std::net::TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    /// A cabin with its config and Steam folder in a temp dir, CS2 installed.
    fn watch_cabin(label: &str) -> (Cabin, PathBuf, PathBuf, crate::config::TestConfigDir) {
        let root = crate::config::test_config_root(label);
        let _ = std::fs::remove_dir_all(&root);
        let config = root.join("config");
        std::fs::create_dir_all(&config).unwrap();
        let pin = crate::config::TestConfigDir::set(config);
        let steam = root.join("steam");
        let cfg_dir = steam.join("steamapps/common/Counter-Strike Global Offensive/game/csgo/cfg");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        std::fs::write(cfg_dir.join("autoexec.cfg"), "bind x noclip").unwrap();
        let mut cabin = Cabin::quiet_for_test();
        cabin.focus.watch.steam_root = Some(steam);
        (cabin, root, cfg_dir, pin)
    }

    /// A fake listener: events sent here reach the cabin as if the game
    /// posted them.
    fn fake_feed(cabin: &mut Cabin) -> mpsc::SyncSender<GsiEvent> {
        let (tx, rx) = mpsc::sync_channel(EVENT_QUEUE);
        cabin.focus.watch.events = Some(rx);
        tx
    }

    fn cs2(at_ms: u64, kind: gw::EventKind) -> GsiEvent {
        GsiEvent {
            game: GsiGame::Cs2,
            event: WatchEvent { at_ms, kind },
        }
    }

    fn round(at_ms: u64, round: u32, won: bool) -> GsiEvent {
        cs2(at_ms, gw::EventKind::RoundEnd { round, won })
    }

    /// The recap cards on Pulse, oldest first.
    fn recaps(cabin: &Cabin) -> Vec<(&str, Option<&str>)> {
        let mut cards: Vec<_> = cabin
            .updates
            .iter()
            .filter(|c| c.source_id == "source:game-watch:counter-strike-2")
            .collect();
        cards.sort_by_key(|c| c.created_at);
        cards
            .into_iter()
            .map(|c| (c.title.as_str(), c.body.as_deref()))
            .collect()
    }

    #[test]
    fn the_listener_opens_only_while_a_supported_game_runs() {
        let (mut cabin, root, cfg_dir, _pin) = watch_cabin("game-watch-life");
        assert!(!cabin.cfg.game_watch, "off by default");
        let port = free_port();
        cabin.focus.watch.port = Some(port);
        cabin.set_game_watch(true);
        assert_eq!(
            cabin.focus.watch.note,
            "Set up for Counter-Strike 2. Start the game, or restart it if it's open, so it reads the file."
        );
        // The file is written right away; the port stays closed.
        let cfg = std::fs::read_to_string(cfg_dir.join(gsi::CFG_FILE)).unwrap();
        assert!(cfg.contains(&format!("\"http://127.0.0.1:{port}/\"")));
        let token =
            std::fs::read_to_string(crate::config::config_dir().join(gsi::TOKEN_FILE)).unwrap();
        assert!(cfg.contains(&format!("\"token\"\t\"{token}\"")));
        assert!(cabin.focus.watch.server.is_none());
        assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());

        // A game the watcher doesn't support: still closed.
        cabin.note_running(Some(1145360), 1_000);
        cabin.tick_game_watch();
        assert!(cabin.focus.watch.server.is_none());

        // Counter-Strike 2 (app 730) starts: the port opens.
        cabin.note_running(Some(730), 2_000);
        cabin.tick_game_watch();
        assert_eq!(
            cabin.focus.watch.server.as_ref().map(GsiServer::port),
            Some(port)
        );
        assert_eq!(cabin.focus.watch.listening_for, Some(GsiGame::Cs2));

        // The game posts: a baseline, then a kill.
        for kills in [0, 1] {
            let body = format!(
                r#"{{"provider":{{"appid":730}},"map":{{"phase":"live"}},"player":{{"team":"CT","match_stats":{{"kills":{kills},"deaths":0}}}},"auth":{{"token":"{token}"}}}}"#
            );
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            write!(
                s,
                "POST / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            let mut reply = String::new();
            s.read_to_string(&mut reply).unwrap();
            assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
        }
        cabin.tick_game_watch();
        assert_eq!(
            (cabin.focus.watch.heard, cabin.focus.watch.last_game),
            (1, Some(GsiGame::Cs2))
        );

        // The game exits: the port closes, the file stays.
        cabin.note_running(None, 3_000);
        cabin.tick_game_watch();
        assert!(cabin.focus.watch.server.is_none());
        assert!(cabin.focus.watch.events.is_none());
        assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
        assert!(cfg_dir.join(gsi::CFG_FILE).exists());

        // Dota 2 (app 570) starts next: open again.
        cabin.note_running(Some(570), 4_000);
        cabin.tick_game_watch();
        assert_eq!(cabin.focus.watch.listening_for, Some(GsiGame::Dota2));
        assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_ok());

        // Off: listener and file both go, the user's own cfg stays.
        cabin.set_game_watch(false);
        assert_eq!(
            cabin.status,
            "Game watcher off. Removed its file from Counter-Strike 2."
        );
        assert!(!cfg_dir.join(gsi::CFG_FILE).exists());
        assert_eq!(
            std::fs::read_to_string(cfg_dir.join("autoexec.cfg")).unwrap(),
            "bind x noclip"
        );
        assert!(cabin.focus.watch.server.is_none());
        assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_taken_port_is_tried_once_per_launch() {
        let (mut cabin, root, _cfg_dir, _pin) = watch_cabin("game-watch-taken");
        let holder = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = holder.local_addr().unwrap().port();
        cabin.focus.watch.port = Some(port);
        cabin.set_game_watch(true);
        cabin.note_running(Some(730), 1_000);
        cabin.tick_game_watch();
        assert!(cabin.focus.watch.server.is_none());
        assert_eq!(
            cabin.focus.watch.listen_note,
            format!("Another app is already using port {port} on this computer, so the game watcher can't hear the game. Close that app, then turn the game watcher off and on again.")
        );
        // Freed mid-game: no retry until the next launch.
        drop(holder);
        cabin.tick_game_watch();
        assert!(cabin.focus.watch.server.is_none());
        cabin.note_running(None, 2_000);
        cabin.tick_game_watch();
        assert_eq!(cabin.focus.watch.listen_note, "");
        cabin.note_running(Some(730), 3_000);
        cabin.tick_game_watch();
        assert_eq!(
            cabin.focus.watch.server.as_ref().map(GsiServer::port),
            Some(port)
        );
        cabin.set_game_watch(false);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn with_the_watcher_off_nothing_listens_toasts_or_posts() {
        let (mut cabin, root, cfg_dir, _pin) = watch_cabin("game-watch-off");
        let port = free_port();
        cabin.focus.watch.port = Some(port);
        cabin.note_running(Some(730), 1_000);
        let tx = fake_feed(&mut cabin);
        for i in 1..=3 {
            tx.send(round(i, i as u32, false)).unwrap();
        }
        tx.send(cs2(4, gw::EventKind::MatchEnd { won: 0, lost: 3 }))
            .unwrap();
        cabin.tick_game_watch();
        assert!(cabin.focus.watch.server.is_none());
        assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
        assert!(!cfg_dir.join(gsi::CFG_FILE).exists());
        assert_eq!(cabin.focus.watch.heard, 0);
        assert_eq!(cabin.toasts.len(), 0);
        assert_eq!(recaps(&cabin), vec![]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pointer_toasts_are_rate_limited() {
        let (mut cabin, root, _cfg_dir, _pin) = watch_cabin("game-watch-toast");
        cabin.cfg.game_watch = true;
        let tx = fake_feed(&mut cabin);
        // Three lost rounds fire the loss-streak pointer: one toast.
        for i in 1..=3 {
            tx.send(round(i, i as u32, false)).unwrap();
        }
        cabin.take_watch_events(100_000);
        assert_eq!(cabin.toasts.len(), 1);
        // A win, three more losses: the pointer fires again 30 s later,
        // inside the minute, so no second toast.
        tx.send(round(4, 4, true)).unwrap();
        for i in 5..=7 {
            tx.send(round(i, i as u32, false)).unwrap();
        }
        cabin.take_watch_events(130_000);
        assert_eq!(cabin.toasts.len(), 1);
        // Past the minute it shows again.
        tx.send(round(8, 8, true)).unwrap();
        for i in 9..=11 {
            tx.send(round(i, i as u32, false)).unwrap();
        }
        cabin.take_watch_events(161_000);
        assert_eq!(cabin.toasts.len(), 2);
        // A frame with nothing new adds nothing.
        cabin.take_watch_events(400_000);
        assert_eq!(cabin.toasts.len(), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn each_match_posts_exactly_one_recap_card() {
        let (mut cabin, root, _cfg_dir, _pin) = watch_cabin("game-watch-recap");
        cabin.cfg.game_watch = true;
        let tx = fake_feed(&mut cabin);
        tx.send(cs2(1, gw::EventKind::Kill)).unwrap();
        tx.send(round(2, 1, true)).unwrap();
        tx.send(round(3, 2, true)).unwrap();
        tx.send(cs2(4, gw::EventKind::Death)).unwrap();
        tx.send(cs2(5, gw::EventKind::MatchEnd { won: 13, lost: 9 }))
            .unwrap();
        cabin.take_watch_events(10_000);
        assert_eq!(
            recaps(&cabin),
            vec![(
                "Counter-Strike 2 match: won 13-9",
                Some("1 kills, 1 deaths. Longest win streak: 2 rounds.")
            )]
        );
        // The game repeats its game-over state: still one card.
        tx.send(cs2(6, gw::EventKind::MatchEnd { won: 13, lost: 9 }))
            .unwrap();
        cabin.take_watch_events(11_000);
        assert_eq!(recaps(&cabin).len(), 1);
        // The next match gets its own card.
        tx.send(cs2(7, gw::EventKind::Kill)).unwrap();
        tx.send(cs2(8, gw::EventKind::MatchEnd { won: 7, lost: 13 }))
            .unwrap();
        cabin.take_watch_events(20_000);
        assert_eq!(
            recaps(&cabin),
            vec![
                (
                    "Counter-Strike 2 match: won 13-9",
                    Some("1 kills, 1 deaths. Longest win streak: 2 rounds.")
                ),
                (
                    "Counter-Strike 2 match: lost 7-13",
                    Some("1 kills, 0 deaths.")
                ),
            ]
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_game_closing_mid_match_drops_that_match() {
        let (mut cabin, root, _cfg_dir, _pin) = watch_cabin("game-watch-close");
        cabin.focus.watch.port = Some(free_port());
        cabin.set_game_watch(true);
        cabin.note_running(Some(730), 1_000);
        cabin.tick_game_watch();
        let tx = fake_feed(&mut cabin);
        tx.send(cs2(1, gw::EventKind::Kill)).unwrap();
        cabin.take_watch_events(2_000);
        cabin.note_running(None, 3_000);
        cabin.tick_game_watch();
        // Relaunched: the old match's kill isn't in the new one's recap.
        cabin.note_running(Some(730), 4_000);
        cabin.tick_game_watch();
        let tx2 = fake_feed(&mut cabin);
        drop(tx);
        tx2.send(cs2(5, gw::EventKind::Death)).unwrap();
        tx2.send(cs2(6, gw::EventKind::MatchEnd { won: 1, lost: 13 }))
            .unwrap();
        cabin.take_watch_events(5_000);
        assert_eq!(
            recaps(&cabin),
            vec![(
                "Counter-Strike 2 match: lost 1-13",
                Some("0 kills, 1 deaths.")
            )]
        );
        cabin.set_game_watch(false);
        let _ = std::fs::remove_dir_all(&root);
    }
}
