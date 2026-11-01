//! Minecraft: Java Edition (`grokhub_core::game_watch_minecraft`).
//!
//! While the game watcher is on, this follows the `logs/latest.log` the
//! game writes in the vanilla launcher's folder and in each launcher
//! instance, shows the session so far in Settings, and posts each session
//! to Pulse when the game logs that it is closing. Folders are found once
//! when the watcher starts. The only reads are those files.

use std::path::PathBuf;

use super::*;
use grokhub_core::game_watch_minecraft::{self as mc, McWatcher};

/// How often the logs are read.
const POLL_EVERY_MS: u64 = 2_000;

#[derive(Default)]
pub(super) struct McState {
    watcher: Option<McWatcher>,
    polled_ms: u64,
    /// Tests point this at a temp home instead of the real one.
    home: Option<PathBuf>,
}

impl Cabin {
    /// Each frame while the game watcher is on: read the logs now and then.
    pub(super) fn tick_minecraft(&mut self) {
        let now = now_ms();
        let m = &mut self.focus.minecraft;
        if !self.cfg.game_watch {
            m.watcher = None;
            return;
        }
        if now.saturating_sub(m.polled_ms) < POLL_EVERY_MS {
            return;
        }
        m.polled_ms = now;
        if m.watcher.is_none() {
            let Some(home) = m.home.clone().or_else(|| {
                std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(PathBuf::from)
            }) else {
                return;
            };
            m.watcher = Some(McWatcher::new(mc::log_dirs(&home)));
        }
        let ended = m.watcher.as_mut().map(McWatcher::poll).unwrap_or_default();
        let mut posted = false;
        for session in ended {
            posted |= grokhub_core::post_update(&mut self.updates, mc::session_card(&session, now));
        }
        if posted {
            self.persist_updates();
        }
    }

    /// The session so far, for the game watcher's row in Settings.
    pub(super) fn minecraft_line(&self) -> Option<String> {
        let session = self.focus.minecraft.watcher.as_ref()?.current()?;
        Some(format!("{}: {}", mc::NAME, session.summary()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn a_session_shows_while_playing_and_posts_when_the_game_stops() {
        let root = crate::config::test_config_root("mc-session");
        let _ = std::fs::remove_dir_all(&root);
        let config = root.join("config");
        std::fs::create_dir_all(&config).unwrap();
        let _pin = crate::config::TestConfigDir::set(config);
        let home = root.join("home");
        let logs = home.join(".minecraft/logs");
        std::fs::create_dir_all(&logs).unwrap();
        let log = logs.join("latest.log");
        std::fs::write(&log, "").unwrap();

        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.game_watch = true;
        cabin.focus.minecraft.home = Some(home);
        cabin.tick_minecraft();
        assert_eq!(cabin.minecraft_line(), None);

        let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
        writeln!(
            f,
            "[21:14:00] [Render thread/INFO]: Setting user: Testcrafter"
        )
        .unwrap();
        writeln!(f, "[21:15:00] [Server thread/INFO]: Testcrafter drowned").unwrap();
        cabin.focus.minecraft.polled_ms = 0;
        cabin.tick_minecraft();
        assert_eq!(
            cabin.minecraft_line().as_deref(),
            Some("Minecraft: Java Edition: Testcrafter. 1 death.")
        );

        let before = cabin.updates.len();
        writeln!(f, "[21:20:00] [Render thread/INFO]: Stopping!").unwrap();
        cabin.focus.minecraft.polled_ms = 0;
        cabin.tick_minecraft();
        assert_eq!(cabin.updates.len(), before + 1);
        assert_eq!(
            cabin.updates.last().unwrap().title,
            "Minecraft: Java Edition session"
        );
        assert_eq!(cabin.minecraft_line(), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
