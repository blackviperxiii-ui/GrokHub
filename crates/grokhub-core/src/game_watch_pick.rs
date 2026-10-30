//! Which game the in-game watcher supports first (design:
//! `docs/design/game-watcher/`): the supported game the user has played most
//! in their Steam library, read at runtime from `SteamGame::playtime_min`.
//!
//! League of Legends and World of Warcraft aren't on Steam. Their processes
//! can be seen while they run (`game_guard` knows League's), but nothing on
//! disk records how long they were played, so they have no playtime to rank
//! and stay out of the pick; Settings says so.

use crate::steam::SteamGame;

/// A game the watcher has (or will have) an adapter for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchGame {
    Cs2,
    Dota2,
    League,
    Wow,
}

impl WatchGame {
    pub fn name(self) -> &'static str {
        match self {
            Self::Cs2 => "Counter-Strike 2",
            Self::Dota2 => "Dota 2",
            Self::League => "League of Legends",
            Self::Wow => "World of Warcraft",
        }
    }

    /// The Steam app id, for the games that are on Steam.
    pub fn steam_app_id(self) -> Option<u32> {
        match self {
            Self::Cs2 => Some(730),
            Self::Dota2 => Some(570),
            Self::League | Self::Wow => None,
        }
    }
}

/// The design's order. It breaks playtime ties, and its first game is the
/// default while no supported game has playtime.
pub const ORDER: [WatchGame; 4] = [
    WatchGame::Cs2,
    WatchGame::Dota2,
    WatchGame::League,
    WatchGame::Wow,
];

/// The supported Steam game with the most playtime, and its minutes. `None`
/// when no supported game in the library has any playtime.
pub fn first_game(library: &[SteamGame]) -> Option<(WatchGame, u64)> {
    let mut best: Option<(WatchGame, u64)> = None;
    for game in ORDER {
        let Some(id) = game.steam_app_id() else {
            continue;
        };
        let minutes = library
            .iter()
            .filter(|g| g.app_id == id)
            .map(|g| g.playtime_min)
            .max()
            .unwrap_or(0);
        // Strictly more, so a tie keeps the game earlier in the design's order.
        if minutes > 0 && best.is_none_or(|(_, m)| minutes > m) {
            best = Some((game, minutes));
        }
    }
    best
}

const NOT_ON_STEAM: &str =
    "League of Legends and World of Warcraft aren't on Steam, so they aren't ranked.";

/// The one line Settings shows: which game the watcher supports first and why.
pub fn settings_line(library: &[SteamGame]) -> String {
    match first_game(library) {
        Some((game, _)) => format!(
            "Game watcher: {} first, most played in your Steam library. {NOT_ON_STEAM}",
            game.name()
        ),
        None => format!(
            "Game watcher: {} first by default, since no supported game has playtime in your Steam library. {NOT_ON_STEAM}",
            ORDER[0].name()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(app_id: u32, name: &str, playtime_min: u64) -> SteamGame {
        SteamGame {
            app_id,
            name: name.into(),
            last_played: 1_700_000_000,
            playtime_min,
        }
    }

    #[test]
    fn a_library_with_no_supported_games_falls_back_to_cs2() {
        let lib = vec![game(1_245_620, "Elden Ring", 9_000), game(440, "TF2", 50)];
        assert_eq!(first_game(&lib), None);
        assert_eq!(first_game(&[]), None);
        assert_eq!(
            settings_line(&lib),
            "Game watcher: Counter-Strike 2 first by default, since no supported game has playtime in your Steam library. League of Legends and World of Warcraft aren't on Steam, so they aren't ranked."
        );
    }

    #[test]
    fn one_supported_game_is_picked_over_more_played_unsupported_ones() {
        let lib = vec![
            game(1_245_620, "Elden Ring", 9_000),
            game(570, "Dota 2", 120),
        ];
        assert_eq!(first_game(&lib), Some((WatchGame::Dota2, 120)));
        assert_eq!(
            settings_line(&lib),
            "Game watcher: Dota 2 first, most played in your Steam library. League of Legends and World of Warcraft aren't on Steam, so they aren't ranked."
        );
    }

    #[test]
    fn the_most_played_supported_game_wins() {
        let lib = vec![game(730, "Counter-Strike 2", 300), game(570, "Dota 2", 301)];
        assert_eq!(first_game(&lib), Some((WatchGame::Dota2, 301)));
    }

    #[test]
    fn a_tie_goes_to_cs2_by_the_designs_order() {
        let lib = vec![game(570, "Dota 2", 500), game(730, "Counter-Strike 2", 500)];
        assert_eq!(first_game(&lib), Some((WatchGame::Cs2, 500)));
        assert_eq!(
            settings_line(&lib),
            "Game watcher: Counter-Strike 2 first, most played in your Steam library. League of Legends and World of Warcraft aren't on Steam, so they aren't ranked."
        );
    }

    #[test]
    fn zero_playtime_counts_as_not_played() {
        // Installed but never played: no pick from playtime.
        let lib = vec![game(570, "Dota 2", 0), game(730, "Counter-Strike 2", 0)];
        assert_eq!(first_game(&lib), None);
        // A played Dota 2 beats a never-played CS2.
        let lib = vec![game(730, "Counter-Strike 2", 0), game(570, "Dota 2", 1)];
        assert_eq!(first_game(&lib), Some((WatchGame::Dota2, 1)));
    }

    #[test]
    fn league_and_wow_have_no_steam_id_to_rank() {
        assert_eq!(WatchGame::League.steam_app_id(), None);
        assert_eq!(WatchGame::Wow.steam_app_id(), None);
        assert_eq!(
            (
                WatchGame::Cs2.steam_app_id(),
                WatchGame::Dota2.steam_app_id()
            ),
            (Some(730), Some(570))
        );
    }
}
