//! Pulse's Steam source: the user's local Steam library feeds each game's
//! public news into the Feed, most recently played first. The library is read
//! from disk at most once a day; the toggle and per-game mutes live in
//! `{config}/pulse-steam.json`, which the agent's `pulse_steam` tool writes too.

use super::*;
use grokhub_core::pulse_sources::SourceReq;
use grokhub_core::steam::{self, SteamGame, SteamPrefs};

/// How often the library is read again.
const LIBRARY_TTL_MS: u64 = 24 * 60 * 60 * 1000;
/// How often prefs are read again, so an agent change shows without a restart.
const PREFS_TTL_MS: u64 = 2_000;

#[derive(Default)]
pub(super) struct SteamState {
    /// `None` until the first read; then whether Steam was found.
    found: Option<bool>,
    games: Vec<SteamGame>,
    read_ms: u64,
    prefs: SteamPrefs,
    prefs_ms: u64,
}

impl SteamState {
    /// The library and prefs, read again when they are stale.
    fn refresh(&mut self, now: u64, root: impl FnOnce() -> Option<std::path::PathBuf>) {
        if self.found.is_none() || now.saturating_sub(self.read_ms) >= LIBRARY_TTL_MS {
            let root = root();
            self.games = root.as_deref().map(steam::read_library).unwrap_or_default();
            self.found = Some(root.is_some());
            self.read_ms = now;
        }
        if self.prefs_ms == 0 || now.saturating_sub(self.prefs_ms) >= PREFS_TTL_MS {
            self.prefs = steam::load_prefs(&crate::config::config_dir());
            self.prefs_ms = now.max(1);
        }
    }

    fn found(&self) -> bool {
        self.found == Some(true)
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

    fn save(&mut self, now: u64) -> Result<(), String> {
        self.prefs_ms = now.max(1);
        steam::save_prefs(&crate::config::config_dir(), &self.prefs)
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
            state.prefs.on = Some(on);
            if let Err(err) = state.save(now) {
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
                state.prefs.set_muted(id, mute);
                if let Err(err) = state.save(now) {
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
