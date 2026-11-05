//! What Loadout knows about one game on this PC, read without touching it.
//!
//! Round 3 item 35: a game's profile records the anti-cheat it uses. The
//! probe only lists names: the game folder's top level (an `EasyAntiCheat`
//! or `BattlEye` folder, `start_protected_game.exe`, a `BEService` exe) and
//! the running service names the Service Control Manager reports. It opens no
//! file in the folder and no handle to a game process.
//!
//! The allowlist's own `protection` counts too, so a game it already knows
//! is protected stays protected when the folder says nothing.
//!
//! Round 3 item 37: [`MATCH_RULES`] is the per-publisher table of games
//! whose rules bar live coaching, and [`live_match_now`] says which of them is
//! in a match on this PC. [`crate::live_coaching`] holds back gameplay
//! advice while one is.

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::game_guard::ANTI_CHEATS;
use crate::game_integrations::Protection;

/// One game's profile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    /// The anti-cheat the game runs under, by its usual name.
    pub anti_cheat: Option<String>,
    /// What showed it, in a few plain words ("an EasyAntiCheat folder").
    pub sign: Option<String>,
}

impl Profile {
    pub fn protected(&self) -> bool {
        self.anti_cheat.is_some()
    }
}

/// A name in a game folder's top level that means an anti-cheat ships with
/// the game. Matched case-insensitively against the whole name.
struct FolderSign {
    name: &'static str,
    dir: bool,
    anti_cheat: &'static str,
}

const FOLDER_SIGNS: &[FolderSign] = &[
    FolderSign {
        name: "easyanticheat",
        dir: true,
        anti_cheat: "Easy Anti-Cheat",
    },
    FolderSign {
        name: "easyanticheat_eos",
        dir: true,
        anti_cheat: "Easy Anti-Cheat",
    },
    FolderSign {
        name: "start_protected_game.exe",
        dir: false,
        anti_cheat: "Easy Anti-Cheat",
    },
    FolderSign {
        name: "battleye",
        dir: true,
        anti_cheat: "BattlEye",
    },
    FolderSign {
        name: "beservice.exe",
        dir: false,
        anti_cheat: "BattlEye",
    },
    FolderSign {
        name: "beservice_x64.exe",
        dir: false,
        anti_cheat: "BattlEye",
    },
];

/// The anti-cheat a game folder's top level shows, and the sign. Reads the
/// folder listing only.
pub fn probe_folder(game_dir: &Path) -> Option<(&'static str, String)> {
    let entries = std::fs::read_dir(game_dir).ok()?;
    let mut names: Vec<(String, bool)> = entries
        .flatten()
        .map(|e| {
            let dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            (e.file_name().to_string_lossy().to_ascii_lowercase(), dir)
        })
        .collect();
    names.sort();
    FOLDER_SIGNS.iter().find_map(|sign| {
        names
            .iter()
            .find(|(n, dir)| *n == sign.name && *dir == sign.dir)
            .map(|(n, dir)| {
                let what = if *dir { "folder" } else { "file" };
                (sign.anti_cheat, format!("{what} {n} in the game folder"))
            })
    })
}

/// The always-on anti-cheat (one that runs from install on, like Riot
/// Vanguard) that protects a game in this folder: its service runs and one
/// of the exes it protects sits in the folder's top level.
fn always_on_for<S: AsRef<str>>(game_dir: &Path, services: &[S]) -> Option<(&'static str, String)> {
    let services: Vec<String> = services
        .iter()
        .map(|s| s.as_ref().trim().to_ascii_lowercase())
        .collect();
    let files: Vec<String> = std::fs::read_dir(game_dir)
        .ok()?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_ascii_lowercase())
        .collect();
    ANTI_CHEATS.iter().find_map(|ac| {
        let on = ac.always_on?;
        let service = ac
            .services
            .iter()
            .find(|s| services.iter().any(|r| r == *s))?;
        let exe = on.games.iter().find(|g| files.iter().any(|f| f == *g))?;
        Some((ac.name, format!("service {service} and {exe}")))
    })
}

/// The profile of a game whose allowlist protection is `protection`, with its
/// folder (when found) and the running service names (empty off Windows).
pub fn profile<S: AsRef<str>>(
    protection: Protection,
    game_dir: Option<&Path>,
    services: &[S],
) -> Profile {
    let found = game_dir.and_then(|dir| probe_folder(dir).or_else(|| always_on_for(dir, services)));
    if let Some((ac, sign)) = found {
        return Profile {
            anti_cheat: Some(ac.to_string()),
            sign: Some(sign),
        };
    }
    match protection {
        Protection::None => Profile::default(),
        Protection::UserMode(name) | Protection::Kernel(name) => Profile {
            anti_cheat: Some(name.to_string()),
            sign: Some("Loadout's game list".to_string()),
        },
    }
}

/// A publisher whose rules bar coaching during a live match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchRule {
    pub publisher: &'static str,
    /// Game processes (lowercase exe names) that mean a match is up, each
    /// with the game's name. The publisher's launcher and lobby clients are
    /// not listed: they run outside a match.
    pub match_exes: &'static [(&'static str, &'static str)],
    /// Where the rule comes from.
    pub source: &'static str,
    /// When the rule was last checked against its source.
    pub checked: &'static str,
}

/// Publishers whose rules bar live coaching. Riot's third-party policy bans
/// apps "drawing conclusions for you during gameplay"; the penalty runs up to
/// a permanent suspension.
///
/// League of Legends' game client (`League of Legends.exe`) runs only in a
/// match; the lobby is `LeagueClient.exe`. VALORANT and 2XKO run one game
/// process from the main menu on and Riot publishes no match state for them,
/// so the game being open counts as a match.
pub const MATCH_RULES: &[MatchRule] = &[MatchRule {
    publisher: "Riot Games",
    match_exes: &[
        ("league of legends.exe", "League of Legends"),
        ("valorant.exe", "VALORANT"),
        ("valorant-win64-shipping.exe", "VALORANT"),
        ("2xko.exe", "2XKO"),
        ("lion-win64-shipping.exe", "2XKO"),
    ],
    source: "https://support.riotgames.com/en-us/riot/events/third-party-applications",
    checked: "2026-11-05",
}];

/// A game from [`MATCH_RULES`] in a match on this PC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveMatch {
    pub game: &'static str,
    pub rule: &'static MatchRule,
}

/// The first game from [`MATCH_RULES`] whose match process is in `exes`
/// (process file names, any case, with or without a folder).
pub fn live_match<E: AsRef<str>>(exes: &[E]) -> Option<LiveMatch> {
    let names: Vec<String> = exes
        .iter()
        .map(|e| crate::game_guard::file_name_lower(e.as_ref()))
        .collect();
    MATCH_RULES.iter().find_map(|rule| {
        rule.match_exes
            .iter()
            .find(|(exe, _)| names.iter().any(|n| n == exe))
            .map(|(_, game)| LiveMatch { game, rule })
    })
}

/// Lists the running process names. The app sets it at startup; the agent
/// crate can't list processes itself.
type ExeProbe = fn() -> Vec<String>;

/// The probe and its last reading.
static PROBE: Mutex<Option<ExeProbe>> = Mutex::new(None);
static LAST: Mutex<Option<(Instant, Option<LiveMatch>)>> = Mutex::new(None);
/// A reading this new is used again instead of listing processes once more.
const FRESH: Duration = Duration::from_secs(2);

/// Set the process lister [`live_match_now`] reads.
pub fn set_exe_probe(probe: ExeProbe) {
    *PROBE.lock().unwrap_or_else(|e| e.into_inner()) = Some(probe);
    *LAST.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// The game from [`MATCH_RULES`] in a match right now. `None` without a
/// probe (off Windows, in tests that set none).
pub fn live_match_now() -> Option<LiveMatch> {
    let probe = (*PROBE.lock().unwrap_or_else(|e| e.into_inner()))?;
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, m)) = *last {
        if at.elapsed() < FRESH {
            return m;
        }
    }
    let m = live_match(&probe());
    *last = Some((Instant::now(), m));
    m
}

#[cfg(test)]
#[path = "game_profile_tests.rs"]
mod tests;
