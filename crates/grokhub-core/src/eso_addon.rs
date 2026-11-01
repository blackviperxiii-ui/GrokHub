//! The Loadout addon for The Elder Scrolls Online (`addons/eso/Loadout/`),
//! built into the app, and what it saved.
//!
//! The addon only listens to ZeniMax's public addon events and writes the
//! `LoadoutDB` saved variable (see its README). The app installs it into a
//! server folder's `AddOns/Loadout` (`live` for NA, `liveeu` for EU) when the
//! user clicks Install, keeps that copy in step with the version built in,
//! and removes only that folder on Remove. A `Loadout` folder this app
//! didn't write is never touched. `SavedVariables/Loadout.lua` is read as
//! data with the WoW saved-variables parser and never run.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::update_feed::{digest_card, UpdateCard};

/// The Elder Scrolls Online on Steam.
pub const APP_ID: u32 = 306130;
pub const NAME: &str = "The Elder Scrolls Online";

/// The repo copy, built in.
pub const MANIFEST: &str = include_str!("../../../addons/eso/Loadout/Loadout.txt");
pub const LUA: &str = include_str!("../../../addons/eso/Loadout/Loadout.lua");

pub const FOLDER: &str = "Loadout";
const MANIFEST_FILE: &str = "Loadout.txt";
const LUA_FILE: &str = "Loadout.lua";
/// The manifest line that marks a copy this app wrote.
const MARK: &str = "## X-Loadout-App:";
/// Biggest saved-variables file read.
const MAX_SAVED: u64 = 4 * 1024 * 1024;
/// The server folders under `Elder Scrolls Online`.
const SERVERS: [&str; 2] = ["live", "liveeu"];

fn field<'a>(manifest: &'a str, key: &str) -> Option<&'a str> {
    manifest
        .lines()
        .find_map(|l| l.trim().strip_prefix(key))
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

/// The version of the addon built into this app.
pub fn bundled_version() -> &'static str {
    field(MANIFEST, "## Version:").unwrap_or("0")
}

fn version_key(v: &str) -> Vec<u64> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// The server folders that exist: under the user's Documents, then the
/// Proton prefix in each Steam library under `steam_root`.
pub fn server_dirs(documents: Option<&Path>, steam_root: Option<&Path>) -> Vec<PathBuf> {
    let mut bases = Vec::new();
    if let Some(docs) = documents {
        bases.push(docs.join("Elder Scrolls Online"));
    }
    if let Some(root) = steam_root {
        for lib in crate::steam::library_folders(root) {
            bases.push(
                lib.join("steamapps/compatdata")
                    .join(APP_ID.to_string())
                    .join("pfx/drive_c/users/steamuser/Documents/Elder Scrolls Online"),
            );
        }
    }
    bases
        .iter()
        .flat_map(|b| SERVERS.iter().map(move |s| b.join(s)))
        .filter(|d| d.is_dir())
        .collect()
}

/// This computer's server folders.
pub fn find_server_dirs(steam_root: Option<&Path>) -> Vec<PathBuf> {
    server_dirs(dirs::document_dir().as_deref(), steam_root)
}

/// `live` → `NA`, `liveeu` → `EU`.
pub fn server_label(server_dir: &Path) -> &'static str {
    match server_dir.file_name().and_then(|n| n.to_str()) {
        Some("liveeu") => "EU",
        _ => "NA",
    }
}

pub fn addon_dir(server_dir: &Path) -> PathBuf {
    server_dir.join("AddOns").join(FOLDER)
}

/// Where the addon stands in one server folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddonState {
    NotInstalled,
    Current {
        version: String,
    },
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

pub fn state(server_dir: &Path) -> AddonState {
    let dir = addon_dir(server_dir);
    if !dir.exists() {
        return AddonState::NotInstalled;
    }
    let Ok(manifest) = std::fs::read_to_string(dir.join(MANIFEST_FILE)) else {
        return AddonState::Foreign;
    };
    if field(&manifest, MARK).is_none() {
        return AddonState::Foreign;
    }
    let version = field(&manifest, "## Version:").unwrap_or("0").to_owned();
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

/// Install (or update) the built-in addon into one server folder.
pub fn install(server_dir: &Path) -> Result<(), String> {
    if state(server_dir) == AddonState::Foreign {
        return Err(format!(
            "{} already holds an addon named {FOLDER} that Loadout didn't install, so it was left alone.",
            addon_dir(server_dir).display()
        ));
    }
    if !server_dir.is_dir() {
        return Err(format!("{} isn't there.", server_dir.display()));
    }
    let dir = addon_dir(server_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    write_file(&dir.join(LUA_FILE), LUA)?;
    // The manifest goes last: a folder with the mark is a complete copy.
    write_file(&dir.join(MANIFEST_FILE), MANIFEST)
}

/// Remove this app's copy. Saved data in `SavedVariables` stays.
pub fn remove(server_dir: &Path) -> Result<(), String> {
    match state(server_dir) {
        AddonState::NotInstalled => Ok(()),
        AddonState::Foreign => Err(format!(
            "{} is an addon Loadout didn't install, so it was left alone.",
            addon_dir(server_dir).display()
        )),
        AddonState::Current { .. } | AddonState::Outdated { .. } => {
            let dir = addon_dir(server_dir);
            for f in [MANIFEST_FILE, LUA_FILE] {
                let p = dir.join(f);
                if p.exists() {
                    std::fs::remove_file(&p)
                        .map_err(|e| format!("Couldn't remove {}: {e}", p.display()))?;
                }
            }
            let _ = std::fs::remove_dir(&dir);
            Ok(())
        }
    }
}

/// Bring an installed, older copy up to the built-in version.
pub fn sync(server_dir: &Path) -> Result<bool, String> {
    match state(server_dir) {
        AddonState::Outdated { .. } => install(server_dir).map(|()| true),
        _ => Ok(false),
    }
}

/// One character the addon recorded.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EsoCharacter {
    pub name: String,
    pub class: Option<String>,
    pub race: Option<String>,
    pub level: Option<u64>,
    pub champion: Option<u64>,
    pub zone: Option<String>,
    pub gold: Option<u64>,
    pub played_s: Option<u64>,
    pub deaths: u64,
    pub seen: u64,
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k)?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn u(v: &Value, k: &str) -> Option<u64> {
    let n = v.get(k)?;
    n.as_u64()
        .or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64))
}

/// The characters in a `Loadout.lua` saved-variables file, newest first.
pub fn parse_characters(text: &str) -> Vec<EsoCharacter> {
    let globals = crate::wow_saved::parse_saved(text);
    let Some(chars) = globals
        .get("LoadoutDB")
        .and_then(|db| db.get("characters"))
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };
    let mut out: Vec<EsoCharacter> = chars
        .values()
        .filter_map(|c| {
            Some(EsoCharacter {
                name: s(c, "name")?,
                class: s(c, "class"),
                race: s(c, "race"),
                level: u(c, "level"),
                champion: u(c, "champion"),
                zone: s(c, "zone"),
                gold: u(c, "gold"),
                played_s: u(c, "played"),
                deaths: u(c, "deaths").unwrap_or(0),
                seen: u(c, "seen").unwrap_or(0),
            })
        })
        .collect();
    out.sort_by(|a, b| b.seen.cmp(&a.seen).then_with(|| a.name.cmp(&b.name)));
    out
}

/// What the addon saved in one server folder, size-capped.
pub fn read_characters(server_dir: &Path) -> Vec<EsoCharacter> {
    let path = server_dir.join("SavedVariables").join("Loadout.lua");
    match std::fs::metadata(&path) {
        Ok(m) if m.is_file() && m.len() <= MAX_SAVED => {}
        _ => return Vec::new(),
    }
    std::fs::read_to_string(&path)
        .map(|t| parse_characters(&t))
        .unwrap_or_default()
}

/// `12,345`.
fn thousands(n: u64) -> String {
    let d = n.to_string();
    let mut out = String::new();
    for (i, c) in d.chars().enumerate() {
        if i > 0 && (d.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `Testknight: level 50 Dragonknight, CP 812, Vivec City, 12,345 gold, 123h played, 4 deaths.`
pub fn character_line(c: &EsoCharacter) -> String {
    let mut parts = Vec::new();
    let mut who = String::new();
    if let Some(l) = c.level {
        who.push_str(&format!("level {l}"));
    }
    if let Some(class) = &c.class {
        if !who.is_empty() {
            who.push(' ');
        }
        who.push_str(class);
    }
    if !who.is_empty() {
        parts.push(who);
    }
    if let Some(cp) = c.champion.filter(|cp| *cp > 0) {
        parts.push(format!("CP {cp}"));
    }
    if let Some(z) = &c.zone {
        parts.push(z.clone());
    }
    if let Some(g) = c.gold {
        parts.push(format!("{} gold", thousands(g)));
    }
    if let Some(p) = c.played_s {
        parts.push(format!("{}h played", p / 3600));
    }
    if c.deaths > 0 {
        let d = if c.deaths == 1 { "death" } else { "deaths" };
        parts.push(format!("{} {d}", c.deaths));
    }
    if parts.is_empty() {
        format!("{}.", c.name)
    } else {
        format!("{}: {}.", c.name, parts.join(", "))
    }
}

/// What one play session changed for the character played last: the
/// characters as saved when the game started and as saved when it closed.
#[derive(Debug, Clone, PartialEq)]
pub struct EsoSession {
    pub before: Option<EsoCharacter>,
    pub after: EsoCharacter,
}

impl EsoSession {
    /// The session, or `None` when nothing new was saved.
    pub fn between(before: &[EsoCharacter], after: &[EsoCharacter]) -> Option<Self> {
        let now = after.first()?;
        let was = before.iter().find(|c| c.name == now.name).cloned();
        if was.as_ref() == Some(now) {
            return None;
        }
        Some(Self {
            before: was,
            after: now.clone(),
        })
    }

    fn gain(&self, f: impl Fn(&EsoCharacter) -> Option<u64>) -> u64 {
        let after = f(&self.after).unwrap_or(0);
        let before = self.before.as_ref().and_then(&f).unwrap_or(after);
        after.saturating_sub(before)
    }

    fn levels(&self) -> u64 {
        self.gain(|c| c.level)
    }

    /// `2 levels, 15 champion points, 3,200 gold earned, 1 death.`
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        match self.levels() {
            0 => {}
            1 => parts.push("1 level".to_string()),
            n => parts.push(format!("{n} levels")),
        }
        match self.gain(|c| c.champion) {
            0 => {}
            1 => parts.push("1 champion point".to_string()),
            n => parts.push(format!("{n} champion points")),
        }
        let gold = self.gain(|c| c.gold);
        if gold > 0 {
            parts.push(format!("{} gold earned", thousands(gold)));
        }
        let deaths = self
            .after
            .deaths
            .saturating_sub(self.before.as_ref().map_or(self.after.deaths, |b| b.deaths));
        parts.push(match deaths {
            0 => "no deaths".into(),
            1 => "1 death".into(),
            n => format!("{n} deaths"),
        });
        let mut line = parts.join(", ");
        if let Some(first) = line.get(..1) {
            line = first.to_uppercase() + &line[1..];
        }
        if let Some(z) = &self.after.zone {
            line.push_str(&format!(". Last in {z}"));
        }
        line + "."
    }
}

/// The session as a Pulse card, posted when the game closes. The
/// character's name stays out of it.
pub fn session_card(s: &EsoSession, at_ms: u64) -> UpdateCard {
    let title = match (s.levels(), s.after.level) {
        (0, _) | (_, None) => format!("{NAME} session"),
        (_, Some(l)) => format!("{NAME} session: reached level {l}"),
    };
    digest_card("source:game-watch:eso", &title, &s.summary(), at_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "loadout-eso-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_built_in_addon_declares_its_saved_variable_and_stays_read_only() {
        assert_eq!(field(MANIFEST, "## SavedVariables:"), Some("LoadoutDB"));
        assert_eq!(bundled_version(), "1.0.0");
        assert!(field(MANIFEST, MARK).is_some());
        assert!(MANIFEST.lines().any(|l| l.trim() == LUA_FILE));
        assert!(LUA.contains("LoadoutDB"));
        for banned in [
            "SendChatMessage",
            "StartChatInput",
            "CHAT_SYSTEM",
            "RequestMoveItem",
            "CallSecureProtected",
            "UseItem",
            "QueueForCampaign",
            "TransferCurrency",
            "WINDOW_MANAGER",
            "zo_callLater",
            "SLASH_COMMANDS",
        ] {
            assert!(!LUA.contains(banned), "Loadout.lua uses {banned}");
        }
    }

    #[test]
    fn install_remove_and_sync_touch_only_loadouts_copy() {
        let live = tmp("cycle").join("live");
        std::fs::create_dir_all(&live).unwrap();
        assert_eq!(state(&live), AddonState::NotInstalled);
        install(&live).unwrap();
        assert!(matches!(state(&live), AddonState::Current { .. }));
        let old = MANIFEST.replace("## Version: 1.0.0", "## Version: 0.9.0");
        std::fs::write(addon_dir(&live).join(MANIFEST_FILE), old).unwrap();
        assert!(matches!(state(&live), AddonState::Outdated { .. }));
        assert_eq!(sync(&live), Ok(true));
        assert_eq!(sync(&live), Ok(false));
        std::fs::write(addon_dir(&live).join("notes.txt"), "mine").unwrap();
        remove(&live).unwrap();
        assert!(addon_dir(&live).join("notes.txt").exists());
        assert!(!addon_dir(&live).join(LUA_FILE).exists());
    }

    #[test]
    fn a_foreign_loadout_folder_is_never_touched() {
        let live = tmp("foreign").join("live");
        let dir = addon_dir(&live);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(MANIFEST_FILE), "## Title: Someone else's\n").unwrap();
        assert_eq!(state(&live), AddonState::Foreign);
        assert!(install(&live).is_err());
        assert!(remove(&live).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join(MANIFEST_FILE)).unwrap(),
            "## Title: Someone else's\n"
        );
    }

    #[test]
    fn server_folders_are_found_in_documents_and_the_proton_prefix() {
        let root = tmp("dirs");
        let docs = root.join("Documents");
        std::fs::create_dir_all(docs.join("Elder Scrolls Online/liveeu")).unwrap();
        let steam = root.join("steam");
        let proton = steam.join(
            "steamapps/compatdata/306130/pfx/drive_c/users/steamuser/Documents/Elder Scrolls Online/live",
        );
        std::fs::create_dir_all(&proton).unwrap();
        let found = server_dirs(Some(&docs), Some(&steam));
        assert_eq!(found.len(), 2);
        assert_eq!(server_label(&found[0]), "EU");
        assert_eq!(found[1], proton);
        assert_eq!(server_label(&proton), "NA");
    }

    // A saved-variables file shaped like ESO writes it, made-up character.
    const SAVED: &str = r#"LoadoutDB =
{
    ["schema"] = 1,
    ["addon"] = "1.0.0",
    ["characters"] =
    {
        ["8796093022208"] =
        {
            ["name"] = "Testknight",
            ["class"] = "Dragonknight",
            ["race"] = "Nord",
            ["level"] = 50,
            ["champion"] = 812,
            ["zone"] = "Vivec City",
            ["gold"] = 12345,
            ["played"] = 443000,
            ["deaths"] = 4,
            ["seen"] = 1793500000,
        },
        ["8796093022209"] =
        {
            ["name"] = "Testalt",
            ["level"] = 12,
            ["seen"] = 1793400000,
        },
    },
    ["events"] =
    {
    },
}
"#;

    #[test]
    fn saved_characters_parse_newest_first_and_read_plainly() {
        let chars = parse_characters(SAVED);
        assert_eq!(chars.len(), 2);
        assert_eq!(chars[0].name, "Testknight");
        assert_eq!(
            character_line(&chars[0]),
            "Testknight: level 50 Dragonknight, CP 812, Vivec City, 12,345 gold, 123h played, 4 deaths."
        );
        assert_eq!(character_line(&chars[1]), "Testalt: level 12.");
        assert!(parse_characters("not lua").is_empty());
        assert!(parse_characters(&SAVED[..SAVED.len() / 3]).is_empty());
    }

    #[test]
    fn an_oversize_saved_file_is_skipped() {
        let live = tmp("big").join("live");
        let saved = live.join("SavedVariables");
        std::fs::create_dir_all(&saved).unwrap();
        let mut text = SAVED.to_string();
        text.push_str(&" ".repeat(MAX_SAVED as usize));
        std::fs::write(saved.join("Loadout.lua"), text).unwrap();
        assert!(read_characters(&live).is_empty());
        std::fs::write(saved.join("Loadout.lua"), SAVED).unwrap();
        assert_eq!(read_characters(&live).len(), 2);
    }

    #[test]
    fn a_session_counts_what_the_last_character_gained() {
        let before = parse_characters(SAVED);
        assert_eq!(EsoSession::between(&before, &before), None);
        assert_eq!(EsoSession::between(&before, &[]), None);
        let mut after = before.clone();
        after[0].level = Some(50);
        after[0].champion = Some(827);
        after[0].gold = Some(15545);
        after[0].deaths = 5;
        after[0].zone = Some("Craglorn".into());
        let s = EsoSession::between(&before, &after).unwrap();
        assert_eq!(
            s.summary(),
            "15 champion points, 3,200 gold earned, 1 death. Last in Craglorn."
        );
        let card = session_card(&s, 1);
        assert_eq!(card.title, "The Elder Scrolls Online session");
        assert!(!card.body.as_deref().unwrap_or("").contains("Testknight"));

        let mut up = before.clone();
        up[1].level = Some(14);
        up[1].seen = 1793600000;
        up.sort_by_key(|c| std::cmp::Reverse(c.seen));
        let s = EsoSession::between(&before, &up).unwrap();
        assert_eq!(
            session_card(&s, 1).title,
            "The Elder Scrolls Online session: reached level 14"
        );
        assert_eq!(s.summary(), "2 levels, no deaths.");
    }
}
