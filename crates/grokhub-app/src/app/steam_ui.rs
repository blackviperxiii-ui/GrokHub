//! Pulse's Steam source: the user's local Steam library feeds each game's
//! public news into the Feed, most recently played first. The library is read
//! from disk at most once a day, on its own thread; the toggle and per-game
//! mutes live in `{config}/pulse-steam.json`, which the agent's `pulse_steam`
//! tool writes too.

use super::*;
use grokhub_core::pulse_sources::SourceReq;
use grokhub_core::steam::{self, SteamGame, SteamPrefs};
use std::path::PathBuf;
use std::sync::mpsc;

/// How often the library is read again.
const LIBRARY_TTL_MS: u64 = 24 * 60 * 60 * 1000;
/// How often prefs are read again, so an agent change shows without a restart.
const PREFS_TTL_MS: u64 = 2_000;

#[derive(Default)]
pub(super) struct SteamState {
    /// `None` until the first read lands; then whether Steam was found.
    found: Option<bool>,
    games: Vec<SteamGame>,
    read_ms: u64,
    /// A library read still running on its thread.
    pending: Option<mpsc::Receiver<Vec<SteamGame>>>,
    prefs: SteamPrefs,
    prefs_ms: u64,
}

impl SteamState {
    #[cfg(test)]
    pub(super) fn for_test(games: Vec<SteamGame>, read_ms: u64) -> Self {
        Self {
            found: Some(true),
            games,
            read_ms,
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(super) fn mute_for_test(&mut self, app_id: u32) {
        self.prefs.set_muted(app_id, true);
    }

    /// The library and prefs, read again when they are stale. Finding Steam
    /// is a few path checks; reading the library (every manifest plus the play
    /// stats in `localconfig.vdf`, megabytes after years of play) runs on its
    /// own thread and lands on a later frame, so no frame waits on the disk.
    fn refresh(&mut self, now: u64, root: impl FnOnce() -> Option<PathBuf>) {
        self.take_read(now);
        if self.pending.is_none()
            && (self.found.is_none() || now.saturating_sub(self.read_ms) >= LIBRARY_TTL_MS)
        {
            self.start_read(now, root());
        }
        if self.prefs_ms == 0 || now.saturating_sub(self.prefs_ms) >= PREFS_TTL_MS {
            self.prefs = steam::load_prefs(&crate::config::config_dir());
            self.prefs_ms = now.max(1);
        }
    }

    /// Start reading the library under `root` on its own thread. No root
    /// means no Steam, known right away.
    fn start_read(&mut self, now: u64, root: Option<PathBuf>) {
        let Some(root) = root else {
            self.games.clear();
            self.found = Some(false);
            self.read_ms = now;
            return;
        };
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("pulse-steam".into())
            .spawn(move || {
                let _ = tx.send(steam::read_library(&root));
            });
        // A spawn that fails (thread limit) leaves nothing pending, so the
        // next refresh tries again.
        if spawned.is_ok() {
            self.pending = Some(rx);
        }
    }

    /// Keep a finished read, if one landed.
    fn take_read(&mut self, now: u64) {
        let Some(rx) = &self.pending else {
            return;
        };
        match rx.try_recv() {
            Ok(games) => self.land(games, now),
            Err(mpsc::TryRecvError::Empty) => {}
            // The reader died: Steam is there, keep the list we had.
            Err(mpsc::TryRecvError::Disconnected) => {
                let games = std::mem::take(&mut self.games);
                self.land(games, now);
            }
        }
    }

    fn land(&mut self, games: Vec<SteamGame>, now: u64) {
        self.games = games;
        self.found = Some(true);
        self.read_ms = now;
        self.pending = None;
    }

    /// Block until the read in flight lands (tests only).
    #[cfg(test)]
    fn wait_read(&mut self, now: u64) {
        if let Some(rx) = self.pending.take() {
            let games = rx.recv().unwrap_or_default();
            self.land(games, now);
        }
    }

    /// Steam was found and its first library read is still on its thread, so
    /// the games aren't known yet. Callers wait a frame instead of showing
    /// "no library".
    pub(super) fn reading(&self) -> bool {
        self.pending.is_some() && self.found.is_none()
    }

    fn found(&self) -> bool {
        self.found == Some(true)
    }

    /// Library games when Steam is found and the Pulse Steam source is on.
    pub(super) fn library_for_recap(&self) -> Option<&[SteamGame]> {
        if self.found() && self.prefs.enabled() {
            Some(&self.games)
        } else {
            None
        }
    }

    /// Seed the library cache for tests (no disk read). `now` keeps the TTL fresh.
    #[cfg(test)]
    pub(super) fn seed_library_for_test(&mut self, games: Vec<SteamGame>, on: bool, now: u64) {
        self.found = Some(true);
        self.games = games;
        self.prefs.on = Some(on);
        self.read_ms = now.max(1);
        self.prefs_ms = now.max(1);
    }

    /// Most recently played library title, if any (Linux controller nudges).
    #[cfg(target_os = "linux")]
    pub(super) fn top_game_name(&self) -> Option<String> {
        self.games
            .iter()
            .max_by_key(|g| g.last_played)
            .map(|g| g.name.clone())
            .filter(|n| !n.is_empty())
    }

    /// This check's Steam news fetches; none when Steam is missing or off.
    fn requests(&self) -> Vec<SourceReq> {
        if !self.found() {
            return Vec::new();
        }
        steam::requests(&self.games, &self.prefs, steam::FEEDS_PER_CHECK)
    }

    /// A post from a game the user muted after it was fetched.
    fn muted(&self, source_name: &str) -> bool {
        steam::muted_post(source_name, &self.games, &self.prefs)
    }

    /// Change the prefs and save them. The file is read again first, so a
    /// change the agent's `pulse_steam` tool made since the last read (up to
    /// `PREFS_TTL_MS` ago) stays instead of being overwritten by this copy.
    fn update_prefs(
        &mut self,
        now: u64,
        change: impl FnOnce(&mut SteamPrefs),
    ) -> Result<(), String> {
        let dir = crate::config::config_dir();
        self.prefs = steam::load_prefs(&dir);
        change(&mut self.prefs);
        self.prefs_ms = now.max(1);
        steam::save_prefs(&dir, &self.prefs)
    }
}

impl Cabin {
    pub(super) fn steam_refresh_library(&mut self, now: u64) {
        self.sources.steam.refresh(now, steam::detect_root);
    }

    pub(super) fn steam_requests(&mut self, now: u64) -> Vec<SourceReq> {
        self.steam_refresh_library(now);
        self.sources.steam.requests()
    }

    /// The whole library for the Library page, and whether Steam was found.
    pub(super) fn steam_library(&mut self, now: u64) -> (bool, &[SteamGame]) {
        self.sources.steam.refresh(now, steam::detect_root);
        (self.sources.steam.found(), &self.sources.steam.games)
    }

    /// Each game's last-played time in ms, for the daily rhythm.
    pub(super) fn steam_last_played(&mut self, now: u64) -> Vec<(String, u64)> {
        self.sources.steam.refresh(now, steam::detect_root);
        self.sources
            .steam
            .games
            .iter()
            .filter(|g| g.last_played > 0)
            .map(|g| (g.app_id.to_string(), g.last_played * 1000))
            .collect()
    }

    /// A library game's name from what was last read. Never reads Steam.
    pub(super) fn steam_cached_name(&self, app_id: u32) -> Option<&str> {
        self.sources
            .steam
            .games
            .iter()
            .find(|g| g.app_id == app_id)
            .map(|g| g.name.as_str())
    }

    /// Game names from the library while the Steam source is on.
    pub(super) fn steam_game_names(&mut self) -> Vec<String> {
        self.sources.steam.refresh(now_ms(), steam::detect_root);
        if !self.sources.steam.found() || !self.sources.steam.prefs.enabled() {
            return Vec::new();
        }
        self.sources
            .steam
            .games
            .iter()
            .map(|g| g.name.clone())
            .collect()
    }

    pub(super) fn steam_muted(&self, card: &grokhub_core::update_feed::UpdateCard) -> bool {
        card.pulse
            .source_name
            .as_deref()
            .is_some_and(|name| self.sources.steam.muted(name))
    }

    /// Settings → Pulse → Sources: the toggle, shown only when Steam is
    /// found, and the games feeding Pulse with a Mute on each.
    pub(super) fn ui_steam_source(&mut self, ui: &mut egui::Ui) {
        let now = now_ms();
        self.sources.steam.refresh(now, steam::detect_root);
        let state = &mut self.sources.steam;
        if state.reading() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
            return;
        }
        if !state.found() {
            return;
        }
        let mut on = state.prefs.enabled();
        if ui
            .checkbox(
                &mut on,
                RichText::new("Use my Steam library in Pulse").size(crate::theme::FONT_BODY),
            )
            .changed()
        {
            if let Err(err) = state.update_prefs(now, |p| p.on = Some(on)) {
                self.status = err;
                return;
            }
        }
        if on {
            let mut flip: Option<(u32, bool)> = None;
            ui.indent("pulse-steam", |ui| {
                crate::cards::help_text(
                    ui,
                    "News for the games you played most recently, from Steam's public news feeds. Read from this computer once a day.",
                );
                for game in state.games.iter().take(steam::SETTINGS_LIST_MAX) {
                    let muted = state.prefs.is_muted(game.app_id);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&game.name)
                                .size(crate::theme::FONT_TIP)
                                .color(if muted {
                                    crate::theme::muted()
                                } else {
                                    crate::theme::fg()
                                }),
                        );
                        if crate::cards::ghost_pill(ui, if muted { "Unmute" } else { "Mute" }) {
                            flip = Some((game.app_id, !muted));
                        }
                    });
                }
                let more = state.games.len().saturating_sub(steam::SETTINGS_LIST_MAX);
                if more > 0 {
                    ui.label(
                        RichText::new(format!("and {more} more"))
                            .size(crate::theme::FONT_TIP)
                            .color(crate::theme::muted()),
                    );
                }
            });
            if let Some((id, mute)) = flip {
                if let Err(err) = state.update_prefs(now, |p| p.set_muted(id, mute)) {
                    self.status = err;
                }
            }
        }
        ui.add_space(6.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: u32, name: &str) -> SteamGame {
        SteamGame {
            app_id: id,
            name: name.into(),
            last_played: u64::from(id),
            playtime_min: 0,
        }
    }

    #[test]
    fn no_steam_means_no_requests_and_no_reads_until_a_day_passes() {
        let mut state = SteamState::default();
        let mut reads = 0;
        state.refresh(1_000, || {
            reads += 1;
            None
        });
        state.refresh(1_000 + LIBRARY_TTL_MS - 1, || {
            reads += 1;
            None
        });
        assert_eq!(
            (reads, state.found(), state.requests().len()),
            (1, false, 0)
        );
    }

    #[test]
    fn a_found_library_is_read_off_the_ui_thread_then_kept_for_a_day() {
        let root = std::env::temp_dir().join(format!("gh-steam-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let apps = root.join("steamapps");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(
            apps.join("appmanifest_10.acf"),
            "\"AppState\" { \"appid\" \"10\" \"name\" \"Current Main\" \"LastPlayed\" \"10\" }",
        )
        .unwrap();
        let mut state = SteamState::default();
        let probes = std::cell::Cell::new(0);
        let probe = || {
            probes.set(probes.get() + 1);
            Some(root.clone())
        };
        state.refresh(1_000, probe);
        // The frame that starts the read knows nothing yet and asks Steam nothing.
        assert!(state.reading());
        assert_eq!((state.found(), state.requests().len()), (false, 0));
        state.wait_read(1_500);
        let names: Vec<&str> = state.games.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(
            (state.reading(), state.found(), names),
            (false, true, vec!["Current Main"])
        );
        assert_eq!(state.requests().len(), 1);
        // Within a day nothing is read again.
        state.refresh(1_500 + LIBRARY_TTL_MS - 1, probe);
        assert_eq!((probes.get(), state.pending.is_some()), (1, false));
        // A day later the re-read starts, and the old list stays while it runs.
        state.refresh(1_500 + LIBRARY_TTL_MS, probe);
        assert_eq!(
            (
                probes.get(),
                state.pending.is_some(),
                state.reading(),
                state.games.len()
            ),
            (2, true, false, 1)
        );
        state.wait_read(2_000 + LIBRARY_TTL_MS);
        assert_eq!(state.games.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_settings_change_keeps_what_the_agent_saved_meanwhile() {
        let _g = crate::config::hold_test_config();
        let dir = crate::config::test_config_root("steam-prefs");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _pin = crate::config::TestConfigDir::set(dir.clone());
        let mut state = SteamState {
            found: Some(true),
            games: vec![game(20, "Current Main"), game(10, "Old Save")],
            read_ms: 1,
            prefs_ms: 1,
            ..Default::default()
        };
        // The agent's pulse_steam tool mutes Old Save while the cached prefs
        // (read under 2 s ago) still say nothing is muted.
        let mut agent = SteamPrefs::default();
        agent.set_muted(10, true);
        steam::save_prefs(&dir, &agent).unwrap();
        state.update_prefs(2, |p| p.set_muted(20, true)).unwrap();
        assert_eq!(steam::load_prefs(&dir).muted, vec![10, 20]);
        assert_eq!(
            (state.prefs.muted.clone(), state.prefs_ms),
            (vec![10, 20], 2)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn muting_a_game_drops_its_requests_and_its_posts() {
        let mut state = SteamState {
            found: Some(true),
            games: vec![game(20, "Current Main"), game(10, "Old Save")],
            read_ms: 1,
            prefs_ms: 1,
            ..Default::default()
        };
        let names: Vec<String> = state.requests().into_iter().map(|r| r.name).collect();
        assert_eq!(names, vec!["Current Main on Steam", "Old Save on Steam"]);
        assert!(!state.muted("Old Save on Steam"));
        state.prefs.set_muted(10, true);
        let names: Vec<String> = state.requests().into_iter().map(|r| r.name).collect();
        assert_eq!(names, vec!["Current Main on Steam"]);
        assert!(state.muted("Old Save on Steam"));
        assert!(!state.muted("Current Main on Steam"));
    }
}
