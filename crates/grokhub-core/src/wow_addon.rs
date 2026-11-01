//! The Loadout addon for World of Warcraft (`addons/wow/Loadout/`), built
//! into the app.
//!
//! The addon only listens to Blizzard's public addon events and writes the
//! `LoadoutDB` saved variable (see its README). The app installs it into a
//! flavor's `Interface/AddOns/Loadout` when the user clicks Install, keeps
//! that copy in step with the version built into the app, and removes only
//! that folder on Remove. A folder named `Loadout` that this app didn't
//! write is never touched.
//!
//! WoW marks an addon whose `## Interface:` is older than the client as out
//! of date and won't load it, so the toc written for a flavor carries the
//! interface numbers of the builds Blizzard's `.build.info` lists, on top of
//! the ones in the repo copy.

use std::path::{Path, PathBuf};

use crate::battlenet::WowFlavor;

/// The repo copy, built in.
pub const TOC: &str = include_str!("../../../addons/wow/Loadout/Loadout.toc");
pub const LUA: &str = include_str!("../../../addons/wow/Loadout/Loadout.lua");

/// The addon's folder and file names under `Interface/AddOns`.
pub const FOLDER: &str = "Loadout";
const TOC_FILE: &str = "Loadout.toc";
const LUA_FILE: &str = "Loadout.lua";
/// The toc line that marks a copy this app wrote.
const MARK: &str = "## X-Loadout-App:";

fn toc_field<'a>(toc: &'a str, key: &str) -> Option<&'a str> {
    toc.lines()
        .find_map(|l| l.trim().strip_prefix(key))
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

/// The version of the addon built into this app.
pub fn bundled_version() -> &'static str {
    toc_field(TOC, "## Version:").unwrap_or("0")
}

fn version_key(v: &str) -> Vec<u64> {
    v.split(['.', '-'])
        .map(|p| p.parse().unwrap_or(0))
        .collect()
}

/// `11.2.7.64521` → `110207`: WoW's interface number for a client version.
pub fn interface_for(version: &str) -> Option<u32> {
    let mut parts = version.trim().split('.').map(|p| p.parse::<u32>().ok());
    let (major, minor, patch) = (parts.next()??, parts.next()??, parts.next()??);
    (minor < 100 && patch < 100).then_some(major * 10_000 + minor * 100 + patch)
}

/// The client versions `.build.info` lists. It is a `|`-separated table
/// whose header names each column as `Name!TYPE:size`.
pub fn build_versions(build_info: &str) -> Vec<String> {
    let mut lines = build_info.lines().filter(|l| !l.trim().is_empty());
    let Some(header) = lines.next() else {
        return Vec::new();
    };
    let Some(col) = header.split('|').position(|h| {
        h.split('!')
            .next()
            .is_some_and(|n| n.trim().eq_ignore_ascii_case("Version"))
    }) else {
        return Vec::new();
    };
    lines
        .filter_map(|l| l.split('|').nth(col))
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .collect()
}

/// The toc to write for an install: the repo copy with its interface list
/// widened by the install's own builds, newest first.
pub fn toc_for(install: &Path) -> String {
    let info = std::fs::read_to_string(install.join(".build.info")).unwrap_or_default();
    let mut ifaces: Vec<u32> = build_versions(&info)
        .iter()
        .filter_map(|v| interface_for(v))
        .chain(
            toc_field(TOC, "## Interface:")
                .unwrap_or("")
                .split(',')
                .filter_map(|n| n.trim().parse().ok()),
        )
        .collect();
    ifaces.sort_unstable_by(|a, b| b.cmp(a));
    ifaces.dedup();
    let line = format!(
        "## Interface: {}",
        ifaces
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    );
    TOC.lines()
        .map(|l| {
            if l.trim_start().starts_with("## Interface:") {
                line.as_str()
            } else {
                l
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// `dir/name`, or the child whose name differs from `name` only in case.
/// WoW under Wine sees folder names without case, so an existing
/// `interface/addons` is the folder the game loads from, and writing a
/// second `Interface/AddOns` next to it would hide the addon from the game.
pub(crate) fn child_any_case(dir: &Path, name: &str) -> PathBuf {
    let exact = dir.join(name);
    if exact.exists() {
        return exact;
    }
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .find(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        })
        .map_or(exact, |e| e.path())
}

/// The addon folder in one flavor's game folder.
pub fn addon_dir(flavor_dir: &Path) -> PathBuf {
    let interface = child_any_case(flavor_dir, "Interface");
    let addons = child_any_case(&interface, "AddOns");
    child_any_case(&addons, FOLDER)
}

/// Where the addon stands in one flavor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddonState {
    NotInstalled,
    /// This app's copy, at the version built in (or newer).
    Current {
        version: String,
    },
    /// This app's copy, older than the one built in.
    Outdated {
        version: String,
    },
    /// A `Loadout` folder this app didn't write; left alone.
    Foreign,
}

impl AddonState {
    pub fn installed(&self) -> bool {
        matches!(self, Self::Current { .. } | Self::Outdated { .. })
    }
}

pub fn state(flavor_dir: &Path) -> AddonState {
    let dir = addon_dir(flavor_dir);
    if !dir.exists() {
        return AddonState::NotInstalled;
    }
    let Ok(toc) = std::fs::read_to_string(dir.join(TOC_FILE)) else {
        return AddonState::Foreign;
    };
    if toc_field(&toc, MARK).is_none() {
        return AddonState::Foreign;
    }
    let version = toc_field(&toc, "## Version:").unwrap_or("0").to_owned();
    if version_key(&version) < version_key(bundled_version()) {
        AddonState::Outdated { version }
    } else {
        AddonState::Current { version }
    }
}

fn write_file(path: &Path, text: &str) -> Result<(), String> {
    let tmp = path.with_extension("loadout-tmp");
    std::fs::write(&tmp, text)
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| format!("Couldn't write {}: {e}", path.display()))
}

/// Install (or update) the built-in addon into one flavor of an install.
pub fn install(install: &Path, flavor_dir: &Path) -> Result<(), String> {
    if state(flavor_dir) == AddonState::Foreign {
        return Err(format!(
            "{} already holds an addon named {FOLDER} that Loadout didn't install, so it was left alone.",
            addon_dir(flavor_dir).display()
        ));
    }
    if !flavor_dir.is_dir() {
        return Err(format!("{} isn't there.", flavor_dir.display()));
    }
    let dir = addon_dir(flavor_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    write_file(&dir.join(LUA_FILE), LUA)?;
    // The toc goes last: a folder with the mark is a complete copy.
    write_file(&dir.join(TOC_FILE), &toc_for(install))
}

/// Remove this app's copy from one flavor. Saved data in `WTF` stays.
pub fn remove(flavor_dir: &Path) -> Result<(), String> {
    match state(flavor_dir) {
        AddonState::NotInstalled => Ok(()),
        AddonState::Foreign => Err(format!(
            "{} is an addon Loadout didn't install, so it was left alone.",
            addon_dir(flavor_dir).display()
        )),
        AddonState::Current { .. } | AddonState::Outdated { .. } => {
            let dir = addon_dir(flavor_dir);
            for f in [TOC_FILE, LUA_FILE] {
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
/// when it was updated; a flavor without the addon is left as it is.
pub fn sync(install_root: &Path, flavor_dir: &Path) -> Result<bool, String> {
    match state(flavor_dir) {
        AddonState::Outdated { .. } => install(install_root, flavor_dir).map(|()| true),
        _ => Ok(false),
    }
}

/// Install or update the addon in every flavor of an install that lacks
/// it or has an older copy, on the user's one click. Each flavor's result
/// comes back; a flavor already current, or holding someone else's
/// `Loadout` folder, is left alone and not listed.
pub fn install_all(install_root: &Path) -> Vec<(WowFlavor, Result<(), String>)> {
    states(install_root)
        .into_iter()
        .filter(|(_, s)| matches!(s, AddonState::NotInstalled | AddonState::Outdated { .. }))
        .map(|(f, _)| (f, install(install_root, &f.dir(install_root))))
        .collect()
}

/// The flavors of an install and each one's addon state.
pub fn states(install: &Path) -> Vec<(WowFlavor, AddonState)> {
    crate::battlenet::wow_flavors(install)
        .into_iter()
        .map(|f| (f, state(&f.dir(install))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "loadout-addon-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const BUILD_INFO: &str =
        "Branch!STRING:0|Active!DEC:1|Build Key!HEX:16|Version!STRING:0|Product!STRING:0\n\
us|1|abc|11.2.7.64521|wow\n\
us|1|def|1.15.8.63829|wow_classic_era\n";

    #[test]
    fn the_built_in_addon_declares_its_saved_variable_and_version() {
        assert_eq!(toc_field(TOC, "## SavedVariables:"), Some("LoadoutDB"));
        assert_eq!(bundled_version(), "1.2.0");
        assert!(toc_field(TOC, MARK).is_some());
        assert!(LUA.contains("LoadoutDB"));
        // Read-only: no protected or automation calls.
        for banned in [
            "CastSpell",
            "RunMacro",
            "SendChatMessage",
            "C_ChatInfo.SendAddonMessage",
            "UseAction",
            "SecureActionButton",
            "RunScript",
        ] {
            assert!(!LUA.contains(banned), "addon calls {banned}");
        }
    }

    #[test]
    fn build_info_versions_become_interface_numbers() {
        assert_eq!(
            build_versions(BUILD_INFO),
            vec!["11.2.7.64521", "1.15.8.63829"]
        );
        assert_eq!(interface_for("11.2.7.64521"), Some(110207));
        assert_eq!(interface_for("1.15.8.63829"), Some(11508));
        assert_eq!(interface_for("5.5.1"), Some(50501));
        assert_eq!(interface_for("garbage"), None);
        assert!(build_versions("").is_empty());
        assert!(build_versions("Branch!STRING:0|Product!STRING:0\nus|wow\n").is_empty());
    }

    #[test]
    fn the_written_toc_carries_the_install_builds_newest_first() {
        let root = tmp("toc");
        std::fs::write(
            root.join(".build.info"),
            BUILD_INFO.replace("11.2.7", "12.0.5"),
        )
        .unwrap();
        let toc = toc_for(&root);
        assert!(
            toc.contains("## Interface: 120107, 120105, 120005, 120001, 50504, 38002, 20506, 16001, 11509, 11508\n"),
            "{toc}"
        );
        assert!(toc.contains("## SavedVariables: LoadoutDB\n"));
        assert!(toc.ends_with("Loadout.lua\n"));
    }

    #[test]
    fn install_update_and_remove_touch_only_loadouts_own_copy() {
        let root = tmp("install");
        std::fs::write(root.join(".build.info"), BUILD_INFO).unwrap();
        let retail = root.join("_retail_");
        std::fs::create_dir_all(retail.join("Interface/AddOns/Details")).unwrap();
        assert_eq!(state(&retail), AddonState::NotInstalled);
        install(&root, &retail).unwrap();
        assert_eq!(
            state(&retail),
            AddonState::Current {
                version: bundled_version().to_owned()
            }
        );
        assert_eq!(
            std::fs::read_to_string(addon_dir(&retail).join("Loadout.lua")).unwrap(),
            LUA
        );
        // An older copy is brought up to date; a current one is left.
        let toc = addon_dir(&retail).join("Loadout.toc");
        std::fs::write(
            &toc,
            TOC.replace(
                &format!("## Version: {}", bundled_version()),
                "## Version: 0.9.0",
            ),
        )
        .unwrap();
        assert_eq!(
            state(&retail),
            AddonState::Outdated {
                version: "0.9.0".into()
            }
        );
        assert_eq!(sync(&root, &retail), Ok(true));
        assert_eq!(sync(&root, &retail), Ok(false));
        // A user file in the folder survives Remove, and so does the folder.
        std::fs::write(addon_dir(&retail).join("notes.txt"), "mine").unwrap();
        remove(&retail).unwrap();
        assert!(!toc.exists());
        assert!(addon_dir(&retail).join("notes.txt").exists());
        assert!(retail.join("Interface/AddOns/Details").is_dir());
    }

    #[test]
    fn a_loadout_folder_this_app_didnt_write_is_left_alone() {
        let root = tmp("foreign");
        let classic = root.join("_classic_");
        let dir = addon_dir(&classic);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("Loadout.toc"),
            "## Interface: 11508\n## Title: Someone else's Loadout\n",
        )
        .unwrap();
        assert_eq!(state(&classic), AddonState::Foreign);
        assert!(install(&root, &classic).is_err());
        assert!(remove(&classic).is_err());
        assert_eq!(sync(&root, &classic), Ok(false));
        assert_eq!(
            std::fs::read_to_string(dir.join("Loadout.toc")).unwrap(),
            "## Interface: 11508\n## Title: Someone else's Loadout\n"
        );
    }

    #[test]
    fn states_lists_each_installed_flavor() {
        let root = tmp("states");
        std::fs::create_dir_all(root.join("_retail_/WTF")).unwrap();
        std::fs::create_dir_all(root.join("_classic_era_/WTF")).unwrap();
        install(&root, &root.join("_classic_era_")).unwrap();
        assert_eq!(
            states(&root),
            vec![
                (WowFlavor::Retail, AddonState::NotInstalled),
                (
                    WowFlavor::ClassicEra,
                    AddonState::Current {
                        version: bundled_version().to_owned()
                    }
                ),
            ]
        );
    }

    #[test]
    fn one_click_installs_into_every_flavor_and_the_toc_loads_on_each() {
        let root = tmp("all");
        for f in ["_retail_", "_classic_", "_anniversary_", "_classic_era_"] {
            std::fs::create_dir_all(root.join(f).join("WTF")).unwrap();
        }
        // Someone else's Loadout folder in Classic Era stays as it is.
        let foreign = root.join("_classic_era_/Interface/AddOns/Loadout");
        std::fs::create_dir_all(&foreign).unwrap();
        std::fs::write(foreign.join("Loadout.toc"), "## Interface: 11509\n").unwrap();
        let done = install_all(&root);
        assert_eq!(
            done.iter()
                .map(|(f, r)| (*f, r.is_ok()))
                .collect::<Vec<_>>(),
            vec![
                (WowFlavor::Retail, true),
                (WowFlavor::Classic, true),
                (WowFlavor::Anniversary, true),
            ]
        );
        assert_eq!(state(&root.join("_classic_era_")), AddonState::Foreign);
        // A second click has nothing left to do.
        assert!(install_all(&root).is_empty());
        // The live clients each find their own number in the one toc.
        let ifaces: Vec<u32> = toc_field(TOC, "## Interface:")
            .unwrap()
            .split(',')
            .map(|n| n.trim().parse().unwrap())
            .collect();
        for live in [120105, 50504, 20506, 11509] {
            assert!(ifaces.contains(&live), "{live} missing from {ifaces:?}");
        }
    }

    #[test]
    fn an_addons_folder_in_another_case_is_the_one_used() {
        let root = tmp("case");
        let retail = root.join("_retail_");
        std::fs::create_dir_all(retail.join("interface/Addons/Details")).unwrap();
        install(&root, &retail).unwrap();
        // The names on disk, not `exists()`, which a case-blind file system
        // (Windows, macOS) answers yes for either spelling.
        let names = |dir: &Path| -> Vec<String> {
            let mut n: Vec<String> = std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            n.sort();
            n
        };
        assert_eq!(names(&retail), vec!["interface"], "no second Interface");
        assert_eq!(names(&retail.join("interface")), vec!["Addons"]);
        assert_eq!(
            names(&retail.join("interface/Addons")),
            vec!["Details", "Loadout"]
        );
        assert!(addon_dir(&retail).join("Loadout.toc").is_file());
        assert!(state(&retail).installed());
        // With no folder yet, the game's own spelling is used.
        let fresh = root.join("_ptr_");
        std::fs::create_dir_all(&fresh).unwrap();
        assert_eq!(addon_dir(&fresh), fresh.join("Interface/AddOns/Loadout"));
    }

    #[test]
    fn the_toc_shows_loadout_in_the_addon_menu() {
        assert_eq!(
            toc_field(TOC, "## AddonCompartmentFunc:"),
            Some("Loadout_OnAddonCompartmentClick")
        );
        assert!(LUA.contains("function Loadout_OnAddonCompartmentClick()"));
        assert!(TOC.starts_with("## Interface:"));
        // Gold and item level read as 0 at logout, so the addon never
        // reads then.
        assert!(!LUA.contains("PLAYER_LOGOUT\")"));
    }
}
