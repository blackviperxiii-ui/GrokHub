//! Elite Dangerous (`grokhub_core::game_watch_elite`).
//!
//! While the game watcher is on and the game runs from Steam, this follows
//! the Player Journal the game writes under Saved Games (inside the Proton
//! prefix on Linux), keeps a session summary for Settings, and posts it to
//! Pulse when the game closes. The only read is that file; nothing opens a
//! handle to the game.

use super::*;
use grokhub_core::game_watch_elite::{self as elite, EliteWatcher};

/// How often the journal is read while the game runs.
const POLL_EVERY_MS: u64 = 1_000;

#[derive(Default)]
pub(super) struct EliteState {
    running: bool,
    watcher: Option<EliteWatcher>,
    polled_ms: u64,
}

impl Cabin {
    /// Each frame: follow the game while it runs, read its journal, post the
    /// session when it closes.
    pub(super) fn tick_elite(&mut self) {
        let now = now_ms();
        let running = self.cfg.game_watch && self.game_running() == Some(elite::APP_ID);
        self.follow_elite(running, now);
        self.poll_elite_journal(now);
    }

    fn follow_elite(&mut self, running: bool, now: u64) {
        let e = &self.focus.elite;
        // The journal folder can appear after launch, on a first run.
        if running == e.running && (!running || e.watcher.is_some()) {
            return;
        }
        if !running {
            self.end_elite_session(now);
            self.focus.elite.running = false;
            return;
        }
        let dir = elite::find_journal(self.focus.watch.steam_root().as_deref());
        let e = &mut self.focus.elite;
        e.running = true;
        if e.watcher.is_none() {
            e.watcher = dir.map(EliteWatcher::new);
            e.polled_ms = 0;
        }
    }

    fn poll_elite_journal(&mut self, now: u64) {
        let e = &mut self.focus.elite;
        if !e.running || now.saturating_sub(e.polled_ms) < POLL_EVERY_MS {
            return;
        }
        e.polled_ms = now;
        let Some(watcher) = e.watcher.as_mut() else {
            return;
        };
        // A journal that can't be read now is tried again next poll.
        for event in watcher.poll(now).unwrap_or_default() {
            self.note_watch_event(elite::NAME, event, now);
        }
    }

    /// The game closed: read what it wrote last and post the session.
    fn end_elite_session(&mut self, now: u64) {
        let Some(mut watcher) = self.focus.elite.watcher.take() else {
            return;
        };
        let _ = watcher.poll(now);
        let session = watcher.session();
        if session.is_empty() {
            return;
        }
        let card = elite::session_card(session, now);
        if grokhub_core::post_update(&mut self.updates, card) {
            self.persist_updates();
        }
    }

    /// The session so far, for the game watcher's row in Settings.
    pub(super) fn elite_line(&self) -> Option<String> {
        let session = self.focus.elite.watcher.as_ref()?.session();
        (!session.is_empty()).then(|| format!("{}: {}", elite::NAME, session.state_line()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn a_session_is_followed_while_the_game_runs_and_posted_when_it_closes() {
        let root = crate::config::test_config_root("elite-session");
        let _ = std::fs::remove_dir_all(&root);
        let config = root.join("config");
        std::fs::create_dir_all(&config).unwrap();
        let _pin = crate::config::TestConfigDir::set(config);
        let steam = root.join("steam");
        let journal = steam.join(
            "steamapps/compatdata/359320/pfx/drive_c/users/steamuser/Saved Games/Frontier Developments/Elite Dangerous",
        );
        std::fs::create_dir_all(&journal).unwrap();
        let log = journal.join("Journal.2026-10-31T211403.01.log");
        std::fs::write(&log, "{\"event\":\"Died\"}\n").unwrap();

        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.game_watch = true;
        cabin.focus.watch.steam_root = Some(steam);
        cabin.note_running(None, 1);
        cabin.note_running(Some(359320), 2);
        cabin.tick_elite();
        assert!(cabin.focus.elite.running);
        assert_eq!(cabin.elite_line(), None);

        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        writeln!(
            f,
            r#"{{"event":"LoadGame","Commander":"Testpilot","Ship_Localised":"Cobra Mk III"}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"event":"FSDJump","StarSystem":"Lave","JumpDist":8.0}}"#
        )
        .unwrap();
        cabin.focus.elite.polled_ms = 0;
        cabin.tick_elite();
        assert_eq!(
            cabin.elite_line().as_deref(),
            Some("Elite Dangerous: CMDR Testpilot: Cobra Mk III. 1 jump, 8.0 ly, last in Lave. No deaths.")
        );

        let before = cabin.updates.len();
        cabin.note_running(None, 3);
        cabin.tick_elite();
        assert!(!cabin.focus.elite.running);
        assert_eq!(cabin.updates.len(), before + 1);
        let card = cabin.updates.last().unwrap();
        assert_eq!(card.title, "Elite Dangerous session: 1 jump");
        assert!(!card.body.as_deref().unwrap_or("").contains("Testpilot"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn nothing_is_followed_with_the_game_watcher_off() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.game_watch = false;
        cabin.note_running(None, 1);
        cabin.note_running(Some(359320), 2);
        cabin.tick_elite();
        assert!(!cabin.focus.elite.running);
        assert!(cabin.focus.elite.watcher.is_none());
    }
}
