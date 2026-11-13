//! Round 3 item 35: the mod planner checks every install plan against a
//! dated anti-cheat rule table before the install card shows.
//!
//! Three kinds of file are judged:
//! - **Anti-cheat tampering**: anything that replaces, removes or bypasses a
//!   game's anti-cheat (its folder, its launcher, a "no EAC" toggle). Refused
//!   for every game in every mode, protected or not.
//! - **Proxy-DLL loaders** (`d3d9.dll`, `dxgi.dll`, `dsound.dll`,
//!   `dinput8.dll`, `version.dll`, `winmm.dll` and kin) dropped into the game.
//! - **ReShade and SweetFX**.
//!
//! The last two only matter for a game whose profile shows an anti-cheat. If
//! the game is played online through its own sanctioned mod system (the
//! allowlist allows [`Mode::Online`]), they are refused. Otherwise the
//! install is offered as offline only, the default path: the card says to
//! keep the game offline while the mod is in, and the installed mod keeps
//! that note.
//!
//! The table is data with a date, so a stale table fails a test and gets
//! checked again.

use super::allowlist::{Entry, Mode};
use super::install::Plan;
use crate::game_profile::Profile;

/// When the rules below were last checked against their sources.
pub const RULES_CHECKED: &str = "2026-11-05";

/// Where each rule comes from.
pub const SOURCES: &[(&str, &str)] = &[
    (
        "proxy loaders",
        "https://www.battleye.com/support/faq/ (\"Ensure that there is no d3d9.dll, dxgi.dll or dsound.dll file in your game directory\")",
    ),
    (
        "ReShade and SweetFX",
        "https://www.battleye.com/support/faq/ (blocked in PUBG, Fortnite and Islands of Nyne; Loadout treats them as blocked online under any anti-cheat)",
    ),
    (
        "anti-cheat tampering",
        "https://www.riotgames.com/en/DevRel/vanguard-faq (\"There is absolutely no allow list for Vanguard.\")",
    ),
];

/// File names that a game loads by name from its own folder, so a mod can
/// slip code in by shipping one. Lowercase.
pub const PROXY_DLLS: &[&str] = &[
    "d3d8.dll",
    "d3d9.dll",
    "d3d10.dll",
    "d3d11.dll",
    "d3d12.dll",
    "dxgi.dll",
    "dsound.dll",
    "dinput8.dll",
    "version.dll",
    "winmm.dll",
    "winhttp.dll",
    "opengl32.dll",
    "xinput1_3.dll",
    "xinput1_4.dll",
];

/// Path parts that are an anti-cheat's own files. Lowercase, whole part.
const ANTI_CHEAT_PARTS: &[&str] = &[
    "easyanticheat",
    "easyanticheat_eos",
    "start_protected_game.exe",
    "battleye",
    "beservice.exe",
    "beservice_x64.exe",
    "beclient.dll",
    "beclient_x64.dll",
];

/// Words in a file path or mod name that mean "turn the anti-cheat off".
/// Lowercase. A name is split into letter-and-digit runs, and a word matches
/// one run or two or three runs written together ("No-EAC", "EAC_Bypass").
/// Any run with "anticheat" in it matches too.
const TAMPER_WORDS: &[&str] = &[
    "anticheat",
    "noeac",
    "eacbypass",
    "bypasseac",
    "disableeac",
    "eacdisable",
    "eacoff",
    "nobattleye",
    "battleyebypass",
    "bypassbattleye",
];

/// Whether `s` names anti-cheat tampering by [`TAMPER_WORDS`].
fn names_tampering(s: &str) -> bool {
    let runs: Vec<String> = s
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|r| !r.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    (1..=3).any(|n| {
        runs.windows(n).any(|w| {
            let joined = w.concat();
            joined.contains("anticheat") || TAMPER_WORDS.contains(&joined.as_str())
        })
    })
}

/// What the rule table says about one plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Clear,
    /// Offered as offline only; the note says why, in plain words.
    OfflineOnly(String),
    /// Not offered; the line says why.
    Refused(String),
}

fn parts(to: &str) -> impl Iterator<Item = String> + '_ {
    to.split(['/', '\\'])
        .filter(|p| !p.is_empty())
        .map(str::to_ascii_lowercase)
}

/// The planned file (or the mod's name) that touches anti-cheat files.
fn tampering(plan: &Plan) -> Option<String> {
    let hit = names_tampering;
    if hit(&plan.name) {
        return Some(plan.name.clone());
    }
    plan.files
        .iter()
        .find(|f| parts(&f.to).any(|p| ANTI_CHEAT_PARTS.contains(&p.as_str()) || hit(&p)))
        .map(|f| f.to.clone())
}

fn file_name(to: &str) -> String {
    parts(to).last().unwrap_or_default()
}

fn proxy_loader(plan: &Plan) -> Option<String> {
    plan.files
        .iter()
        .find(|f| PROXY_DLLS.contains(&file_name(&f.to).as_str()))
        .map(|f| f.to.clone())
}

fn overlay(plan: &Plan) -> Option<String> {
    plan.files
        .iter()
        .find(|f| parts(&f.to).any(|p| p.starts_with("reshade") || p.contains("sweetfx")))
        .map(|f| f.to.clone())
}

/// Check `plan` for game `entry` with profile `profile`.
pub fn screen(plan: &Plan, entry: &Entry, profile: &Profile) -> Screen {
    let ac = profile
        .anti_cheat
        .clone()
        .unwrap_or_else(|| "the game's anti-cheat".to_string());
    if let Some(file) = tampering(plan) {
        return Screen::Refused(format!(
            "Loadout won't install this: {file} touches {ac}. Turning anti-cheat off or around it can get the account banned, so Loadout never does it."
        ));
    }
    if !profile.protected() {
        return Screen::Clear;
    }
    let found = proxy_loader(plan)
        .map(|f| (f, "a loader that the game picks up by name"))
        .or_else(|| overlay(plan).map(|f| (f, "a ReShade or SweetFX file")));
    let Some((file, kind)) = found else {
        return Screen::Clear;
    };
    if entry.allowed.contains(&Mode::Online) {
        return Screen::Refused(format!(
            "Loadout won't install this: {file} is {kind}, and {} is played online under {ac}, which blocks it.",
            entry.name
        ));
    }
    Screen::OfflineOnly(format!(
        "Offline only: {file} is {kind}, which {ac} blocks online. Keep {} offline while this mod is in, and uninstall it before you play online.",
        entry.name
    ))
}

#[cfg(test)]
#[path = "anticheat_tests.rs"]
mod tests;
