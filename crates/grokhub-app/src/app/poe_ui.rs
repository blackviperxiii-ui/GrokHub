//! Path of Exile and Path of Exile 2 (`grokhub_core::game_watch_poe`).
//!
//! While the game watcher is on and either game runs from Steam, this
//! follows the `logs/Client.txt` the game writes in its own folder, keeps a
//! session summary for Settings, and posts it to Pulse when the game
//! closes. The only read is that file; nothing opens a handle to the game.

use std::path::PathBuf;

use super::*;
use grokhub_core::game_watch_poe::{self as poe, PoeGame, PoeWatcher};

/// How often the log is read while the game runs.
const POLL_EVERY_MS: u64 = 1_000;

#[derive(Default)]
pub(super) struct PoeState {
    running: Option<PoeGame>,
    watcher: Option<PoeWatcher>,
    polled_ms: u64,
}

impl Cabin {
    /// Each frame: follow the game that runs, read its log, post the
    /// session when it closes.
    pub(super) fn tick_poe(&mut self) {
        let now = now_ms();
        let running = if self.cfg.game_watch {
            self.game_running().and_then(PoeGame::from_app_id)
        } else {
            None
        };
        self.follow_poe(running, now);
        self.poll_poe_log(now);
    }

    fn follow_poe(&mut self, running: Option<PoeGame>, now: u64) {
        if running == self.focus.poe.running {
            return;
        }
        self.end_poe_session(now);
        let logs = running.and_then(|game| self.poe_logs_dir(game));
        let p = &mut self.focus.poe;
        p.running = running;
        p.watcher = logs.map(PoeWatcher::new);
        p.polled_ms = 0;
    }

    fn poe_logs_dir(&self, game: PoeGame) -> Option<PathBuf> {
        let root = self.focus.watch.steam_root()?;
        poe::find_install(&root, game).map(|dir| poe::logs_dir(&dir))
    }

    fn poll_poe_log(&mut self, now: u64) {
        let p = &mut self.focus.poe;
        if now.saturating_sub(p.polled_ms) < POLL_EVERY_MS {
            return;
        }
        p.polled_ms = now;
        let (Some(game), Some(watcher)) = (p.running, p.watcher.as_mut()) else {
            return;
        };
        // A log that can't be read now is tried again next poll.
        for event in watcher.poll(now).unwrap_or_default() {
            self.note_watch_event(game.name(), event, now);
        }
    }

    /// The game closed: read what it wrote last and post the session.
    fn end_poe_session(&mut self, now: u64) {
        let p = &mut self.focus.poe;
        let (Some(game), Some(mut watcher)) = (p.running, p.watcher.take()) else {
            return;
        };
        let _ = watcher.poll(now);
        let session = watcher.session();
        if session.is_empty() {
            return;
        }
        let card = poe::session_card(game.name(), session, now);
        if grokhub_core::post_update(&mut self.updates, card) {
            self.persist_updates();
        }
    }

    /// The session so far, for the game watcher's row in Settings.
    pub(super) fn poe_line(&self) -> Option<String> {
        let p = &self.focus.poe;
        let session = p.watcher.as_ref()?.session();
        let game = p.running?;
        (!session.is_empty()).then(|| format!("{}: {}", game.name(), session.summary()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn a_session_is_followed_while_the_game_runs_and_posted_when_it_closes() {
        let root = crate::config::test_config_root("poe-session");
        let _ = std::fs::remove_dir_all(&root);
        let config = root.join("config");
        std::fs::create_dir_all(&config).unwrap();
        let _pin = crate::config::TestConfigDir::set(config);
        let steam = root.join("steam");
        let logs = steam.join("steamapps/common/Path of Exile/logs");
        std::fs::create_dir_all(&logs).unwrap();
        let log = logs.join("Client.txt");
        let line = |msg: &str| format!("2026/10/31 21:14:03 1 cff94598 [INFO Client 1] : {msg}\n");
        std::fs::write(&log, line("Oldchar has been slain.")).unwrap();

        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.game_watch = true;
        cabin.focus.watch.steam_root = Some(steam);
        cabin.note_running(None, 1);
        cabin.note_running(Some(238960), 2);
        cabin.tick_poe();
        assert_eq!(cabin.focus.poe.running, Some(PoeGame::Poe1));
        assert_eq!(cabin.poe_line(), None);

        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        f.write_all(line("You have entered The Coast.").as_bytes())
            .unwrap();
        f.write_all(line("Testwitch (Witch) is now level 13").as_bytes())
            .unwrap();
        cabin.focus.poe.polled_ms = 0;
        cabin.tick_poe();
        assert_eq!(
            cabin.poe_line().as_deref(),
            Some("Path of Exile: Testwitch (Witch), level 12 to 13. No deaths. 1 area, last in The Coast.")
        );

        let before = cabin.updates.len();
        cabin.note_running(None, 3);
        cabin.tick_poe();
        assert_eq!(cabin.focus.poe.running, None);
        assert_eq!(cabin.updates.len(), before + 1);
        assert_eq!(
            cabin.updates.last().unwrap().title,
            "Path of Exile session: level 12 to 13"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
