//! The Loadout mod for Factorio (`addons/factorio/loadout/`), built into the
//! app.
//!
//! The mod only listens to Factorio's official mod API events and writes
//! `script-output/loadout/state.json` on the player's own computer (see its
//! README); `factorio_state` reads that file. The app installs the mod into
//! Factorio's `mods` folder when the user clicks Install, keeps that copy in
//! step with the version built into the app, and removes only that folder on
//! Remove. A `loadout` folder this app didn't write is never touched, and
//! `mod-list.json` is never edited: the game asks the user to enable it.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// Factorio on Steam.
pub const APP_ID: u32 = 427520;
pub const NAME: &str = "Factorio";

/// The repo copy, built in.
pub const INFO: &str = include_str!("../../../addons/factorio/loadout/info.json");
pub const CONTROL: &str = include_str!("../../../addons/factorio/loadout/control.lua");

/// The mod's folder and file names under `mods`.
pub const FOLDER: &str = "loadout";
const INFO_FILE: &str = "info.json";
const CONTROL_FILE: &str = "control.lua";
/// The `info.json` key that marks a copy this app wrote. Factorio ignores
/// keys it doesn't know.
const MARK: &str = "loadout_app";

/// Factorio's user data folder: `%AppData%\Factorio` on Windows,
/// `~/.factorio` on Linux, `~/Library/Application Support/factorio` on
/// macOS.
pub fn user_dir(home: &Path) -> PathBuf {
    if cfg!(windows) {
        home.join("AppData/Roaming/Factorio")
    } else if cfg!(target_os = "macos") {
        home.join("Library/Application Support/factorio")
    } else {
        home.join(".factorio")
    }
}

/// This computer's Factorio user data folder.
pub fn this_user_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| user_dir(&h))
}

pub fn mods_dir(user_dir: &Path) -> PathBuf {
    user_dir.join("mods")
}

/// The mod folder in a `mods` folder.
pub fn mod_dir(mods: &Path) -> PathBuf {
    mods.join(FOLDER)
}

fn version(info: &str) -> Option<String> {
    let v: Value = serde_json::from_str(info).ok()?;
    v.get("version")?.as_str().map(str::to_owned)
}

fn marked(info: &str) -> bool {
    serde_json::from_str::<Value>(info)
        .ok()
        .is_some_and(|v| v.get(MARK).is_some())
}

/// The version of the mod built into this app.
pub fn bundled_version() -> String {
    version(INFO).unwrap_or_else(|| "0".into())
}

fn version_key(v: &str) -> Vec<u64> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// Where the mod stands in a `mods` folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModState {
    NotInstalled,
    /// This app's copy, at the version built in (or newer).
    Current {
        version: String,
    },
    /// This app's copy, older than the one built in.
    Outdated {
        version: String,
    },
    /// A `loadout` folder this app didn't write; left alone.
    Foreign,
}

impl ModState {
    pub fn installed(&self) -> bool {
        matches!(self, Self::Current { .. } | Self::Outdated { .. })
    }
}

pub fn state(mods: &Path) -> ModState {
    let dir = mod_dir(mods);
    if !dir.exists() {
        return ModState::NotInstalled;
    }
    let Ok(info) = std::fs::read_to_string(dir.join(INFO_FILE)) else {
        return ModState::Foreign;
    };
    if !marked(&info) {
        return ModState::Foreign;
    }
    let version = version(&info).unwrap_or_else(|| "0".into());
    if version_key(&version) < version_key(&bundled_version()) {
        ModState::Outdated { version }
    } else {
        ModState::Current { version }
    }
}

fn write_file(path: &Path, text: &str) -> Result<(), String> {
    let tmp = path.with_extension("loadout-tmp");
    std::fs::write(&tmp, text)
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| format!("Couldn't write {}: {e}", path.display()))
}

/// Install (or update) the built-in mod into a `mods` folder. Factorio's
/// user folder must exist (the game has run once).
pub fn install(mods: &Path) -> Result<(), String> {
    if state(mods) == ModState::Foreign {
        return Err(format!(
            "{} already holds a mod named {FOLDER} that Loadout didn't install, so it was left alone.",
            mod_dir(mods).display()
        ));
    }
    let user = mods.parent().unwrap_or(mods);
    if !user.is_dir() {
        return Err(format!(
            "{} isn't there. Start Factorio once, then install.",
            user.display()
        ));
    }
    let dir = mod_dir(mods);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    write_file(&dir.join(CONTROL_FILE), CONTROL)?;
    // info.json goes last: a folder with the mark is a complete copy.
    write_file(&dir.join(INFO_FILE), INFO)
}

/// Remove this app's copy. What the mod wrote to `script-output` stays.
pub fn remove(mods: &Path) -> Result<(), String> {
    match state(mods) {
        ModState::NotInstalled => Ok(()),
        ModState::Foreign => Err(format!(
            "{} is a mod Loadout didn't install, so it was left alone.",
            mod_dir(mods).display()
        )),
        ModState::Current { .. } | ModState::Outdated { .. } => {
            let dir = mod_dir(mods);
            for f in [INFO_FILE, CONTROL_FILE] {
                let p = dir.join(f);
                if p.exists() {
                    std::fs::remove_file(&p)
                        .map_err(|e| format!("Couldn't remove {}: {e}", p.display()))?;
                }
            }
            // Only an empty folder goes; anything else in it stays.
            let _ = std::fs::remove_dir(&dir);
            Ok(())
        }
    }
}

/// Bring an installed, older copy up to the built-in version. `Ok(true)`
/// when it was updated; a folder without the mod is left as it is.
pub fn sync(mods: &Path) -> Result<bool, String> {
    match state(mods) {
        ModState::Outdated { .. } => install(mods).map(|()| true),
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "loadout-factorio-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_built_in_mod_is_a_marked_runtime_only_2_0_mod() {
        let info: Value = serde_json::from_str(INFO).unwrap();
        assert_eq!(info["name"], FOLDER);
        assert_eq!(info["factorio_version"], "2.0");
        assert_eq!(bundled_version(), "1.0.0");
        assert!(marked(INFO));
        // Writes only through helpers.write_file for one player, and keeps
        // no gameplay hooks: no prototypes, no commands, no game changes.
        assert!(CONTROL.contains("helpers.write_file(FILE"));
        assert!(CONTROL.contains("player.index)"));
        for banned in [
            "data:extend",
            "commands.add_command",
            "game.print",
            ".insert(",
            ".create_entity",
            ".teleport",
            "research_queue",
            ".cancel_current_research",
            "game.write_file",
            "remote.call",
            "rcon",
        ] {
            assert!(!CONTROL.contains(banned), "control.lua uses {banned}");
        }
    }

    #[test]
    fn install_remove_and_sync_touch_only_loadouts_copy() {
        let user = tmp("cycle");
        let mods = mods_dir(&user);
        assert_eq!(state(&mods), ModState::NotInstalled);
        install(&mods).unwrap();
        assert_eq!(
            state(&mods),
            ModState::Current {
                version: "1.0.0".into()
            }
        );
        assert_eq!(
            std::fs::read_to_string(mod_dir(&mods).join("control.lua")).unwrap(),
            CONTROL
        );
        assert!(!mods.join("mod-list.json").exists());
        assert_eq!(sync(&mods), Ok(false));

        let old = INFO.replace("\"1.0.0\"", "\"0.9.0\"");
        std::fs::write(mod_dir(&mods).join("info.json"), old).unwrap();
        assert!(matches!(state(&mods), ModState::Outdated { .. }));
        assert_eq!(sync(&mods), Ok(true));
        assert!(matches!(state(&mods), ModState::Current { .. }));

        remove(&mods).unwrap();
        assert_eq!(state(&mods), ModState::NotInstalled);
        let _ = std::fs::remove_dir_all(&user);
    }

    #[test]
    fn a_foreign_loadout_folder_is_never_touched() {
        let user = tmp("foreign");
        let mods = mods_dir(&user);
        let dir = mod_dir(&mods);
        std::fs::create_dir_all(&dir).unwrap();
        let theirs = r#"{"name":"loadout","version":"3.0.0","factorio_version":"2.0"}"#;
        std::fs::write(dir.join("info.json"), theirs).unwrap();
        assert_eq!(state(&mods), ModState::Foreign);
        assert!(install(&mods).is_err());
        assert!(remove(&mods).is_err());
        assert_eq!(sync(&mods), Ok(false));
        assert_eq!(
            std::fs::read_to_string(dir.join("info.json")).unwrap(),
            theirs
        );
        let _ = std::fs::remove_dir_all(&user);
    }

    #[test]
    fn remove_keeps_files_it_did_not_write() {
        let user = tmp("keep");
        let mods = mods_dir(&user);
        install(&mods).unwrap();
        std::fs::write(mod_dir(&mods).join("notes.txt"), "mine").unwrap();
        remove(&mods).unwrap();
        assert!(mod_dir(&mods).join("notes.txt").exists());
        let _ = std::fs::remove_dir_all(&user);
    }

    #[test]
    fn install_needs_factorio_to_have_run_once() {
        let user = tmp("missing").join("never-ran");
        assert!(install(&mods_dir(&user)).is_err());
    }
}
