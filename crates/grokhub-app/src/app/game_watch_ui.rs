//! Game watcher setup (`grokhub_core::game_watch_gsi`), opt-in under
//! Settings → Behavior. On: the cabin writes one small Game State
//! Integration file into each supported game's own cfg folder (Valve's
//! documented way for an app to get the match state) and listens for the
//! game on `127.0.0.1` only. Off: the listener stops and the file goes.
//! Nothing reads the game's memory or touches the game. Turning the events
//! into toasts and the recap card comes next; for now Settings shows that
//! the game is heard.

use std::path::PathBuf;

use super::*;
use grokhub_core::game_watch_gsi::{self as gsi, GsiEvent, GsiGame, GsiServer, GsiToken};

/// Events held for the cabin; past this new ones are dropped, never queued
/// without end.
const EVENT_QUEUE: usize = 256;

#[derive(Default)]
pub(super) struct WatchState {
    server: Option<GsiServer>,
    events: Option<mpsc::Receiver<GsiEvent>>,
    /// Set up once per session, on the first tick with the watcher on.
    started: bool,
    /// What Settings shows under the toggle.
    note: String,
    heard: u32,
    last_game: Option<GsiGame>,
    /// Tests listen on a free port and use a temp Steam folder.
    port: Option<u16>,
    steam_root: Option<PathBuf>,
}

impl WatchState {
    fn steam_root(&self) -> Option<PathBuf> {
        self.steam_root
            .clone()
            .or_else(grokhub_core::steam::detect_root)
    }
}

fn names(games: &[GsiGame]) -> String {
    let names: Vec<&str> = games.iter().map(|g| g.name()).collect();
    names.join(" and ")
}

/// The line under the toggle once the listener is up: which games were set
/// up, or why none was.
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
    /// Each frame: set up once when the watcher is on, then count what the
    /// game sent. The listener itself sits blocked between posts.
    pub(super) fn tick_game_watch(&mut self) {
        if !self.cfg.game_watch {
            return;
        }
        if !self.focus.watch.started {
            self.start_game_watch();
        }
        let w = &mut self.focus.watch;
        if let Some(rx) = &w.events {
            for e in rx.try_iter() {
                w.heard += 1;
                w.last_game = Some(e.game);
            }
        }
    }

    /// Listen first, then write the cfg files that point the games here. A
    /// port another app holds writes nothing and says so.
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
        let (tx, rx) = mpsc::sync_channel(EVENT_QUEUE);
        match GsiServer::start(w.port.unwrap_or(gsi::GSI_PORT), token.clone(), tx) {
            Ok(server) => {
                let found = w
                    .steam_root()
                    .map(|root| gsi::install_cfgs(&root, &token, server.port()));
                w.note = setup_note(found.as_deref());
                w.server = Some(server);
                w.events = Some(rx);
            }
            Err(e) => w.note = e.to_string(),
        }
    }

    /// Stop listening and remove the cfg files Loadout wrote.
    fn stop_game_watch(&mut self) -> String {
        let w = &mut self.focus.watch;
        if let Some(mut server) = w.server.take() {
            server.stop();
        }
        w.events = None;
        w.started = false;
        w.note.clear();
        w.heard = 0;
        w.last_game = None;
        let removed = w.steam_root().map(|root| gsi::remove_cfgs(&root));
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
            "Follows your match as you play, through Game State Integration, the feed Valve built and supports for this. Turning it on writes one small file into the game's own config folder that tells the game to send its state to Loadout, here on this computer only. Turning it off removes that file. Nothing reads the game's memory or touches the game.",
            &mut on,
        ) {
            self.set_game_watch(on);
        }
        if !self.cfg.game_watch {
            return;
        }
        let w = &self.focus.watch;
        let heard = match w.last_game {
            Some(game) if w.heard > 0 => format!(
                "Heard {} game events this session, the last from {}.",
                w.heard,
                game.name()
            ),
            _ => String::new(),
        };
        let note = w.note.clone();
        ui.indent("game-watch", |ui| {
            if !note.is_empty() {
                crate::cards::help_text(ui, &note);
            }
            if !heard.is_empty() {
                crate::cards::help_text(ui, &heard);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn turning_the_watcher_on_listens_and_writes_the_file_and_off_removes_it() {
        let root = crate::config::test_config_root("game-watch");
        let _ = std::fs::remove_dir_all(&root);
        let config = root.join("config");
        std::fs::create_dir_all(&config).unwrap();
        let _pin = crate::config::TestConfigDir::set(config.clone());
        let steam = root.join("steam");
        let cfg_dir = steam.join("steamapps/common/Counter-Strike Global Offensive/game/csgo/cfg");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        std::fs::write(cfg_dir.join("autoexec.cfg"), "bind x noclip").unwrap();

        let mut cabin = Cabin::quiet_for_test();
        assert!(!cabin.cfg.game_watch, "off by default");
        cabin.focus.watch.port = Some(0);
        cabin.focus.watch.steam_root = Some(steam.clone());
        cabin.set_game_watch(true);
        assert_eq!(
            cabin.focus.watch.note,
            "Set up for Counter-Strike 2. Start the game, or restart it if it's open, so it reads the file."
        );
        let port = cabin.focus.watch.server.as_ref().expect("listening").port();
        let cfg = std::fs::read_to_string(cfg_dir.join(gsi::CFG_FILE)).unwrap();
        assert!(cfg.contains(&format!("\"http://127.0.0.1:{port}/\"")));
        let token = std::fs::read_to_string(config.join(gsi::TOKEN_FILE)).unwrap();
        assert!(cfg.contains(&format!("\"token\"\t\"{token}\"")));

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
}
