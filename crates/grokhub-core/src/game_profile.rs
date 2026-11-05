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

use std::path::Path;

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

#[cfg(test)]
#[path = "game_profile_tests.rs"]
mod tests;
