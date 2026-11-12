//! Settings → Pulse → Know me, as the cabin's `knowme_ui.rs` has it: the
//! opt-in scan of harmless local signals, a switch per signal, and "What I
//! know about you" with Delete on each tag and Wipe all. Tags that are
//! interests join Pulse's topics as "From your PC"; deleting a tag takes its
//! topic out again unless you added it yourself. The state is the cabin's own
//! `know-me.json` (`grokhub_core::know_me`), so both apps show the same tags.
//! The scan runs here on Scan now, or when Know me is turned on.

use std::path::Path;

use grokhub_core::interests::{InterestProfile, InterestSource};
use grokhub_core::know_me::{self, KnowMe, Scan, ScanInput, Signal};
use serde::Serialize;

pub const INTRO: &str = "Learn your games, hobbies and setup from names and counts on this computer, never file contents, browser data, mail, messages or keys. Only the tags below are kept.";
pub const MEMORY_NOTE: &str = "What chats teach goes to USER.md (you), SOUL.md (how your assistant behaves) and MEMORY.md (what you asked it to remember).";
pub const SWITCH: &str = "Know me from this computer";
pub const KNOWS_HEAD: &str = "What I know about you";
pub const NOTHING_YET: &str = "Nothing yet. Scan now fills this in.";
pub const WIPED: &str = "Know me is off and everything it learned is gone.";

fn path(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join(know_me::FILE_NAME)
}

fn load(config_dir: &Path) -> KnowMe {
    know_me::load(&path(config_dir))
}

fn save(config_dir: &Path, state: &KnowMe) -> Result<(), String> {
    know_me::save(&path(config_dir), state)
}

fn signal_id(s: Signal) -> String {
    serde_json::to_value(s)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn signal_of(id: &str) -> Result<Signal, String> {
    Signal::ALL
        .into_iter()
        .find(|s| signal_id(*s) == id)
        .ok_or_else(|| format!("No signal called {id}."))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalRow {
    pub id: String,
    pub label: String,
    pub reads: String,
    pub on: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagRow {
    pub tag: String,
    pub from: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowMeView {
    pub on: bool,
    pub signals: Vec<SignalRow>,
    pub last_scan: Option<String>,
    pub tags: Vec<TagRow>,
}

/// One change to the Pulse topics; says whether it changed any.
pub type Edit<'a> = &'a mut dyn FnMut(&mut InterestProfile) -> bool;

pub fn view(config_dir: &Path, now_ms: u64) -> KnowMeView {
    let state = load(config_dir);
    let last_scan = state.log.last().map(|log| {
        let parts: Vec<String> = log
            .counts
            .iter()
            .map(|(s, n)| format!("{} {n}", s.label().to_lowercase()))
            .collect();
        format!(
            "Last scan {}: looked at {}.",
            grokhub_core::pulse::ago_label(log.at_ms, now_ms),
            parts.join(", ")
        )
    });
    KnowMeView {
        on: state.on,
        signals: Signal::ALL
            .into_iter()
            .map(|s| SignalRow {
                id: signal_id(s),
                label: s.label().to_string(),
                reads: s.reads().to_string(),
                on: state.enabled(s),
            })
            .collect(),
        last_scan,
        tags: state
            .tags
            .iter()
            .map(|t| TagRow {
                tag: t.tag.clone(),
                from: format!("from {}", t.signal.label().to_lowercase()),
            })
            .collect(),
    }
}

/// Turn Know me on or off. Turning it on scans right away, as the cabin does.
pub fn set_on(config_dir: &Path, on: bool) -> Result<bool, String> {
    let mut state = load(config_dir);
    state.on = on;
    save(config_dir, &state)?;
    Ok(on)
}

pub fn set_signal(config_dir: &Path, id: &str, on: bool) -> Result<(), String> {
    let signal = signal_of(id)?;
    let mut state = load(config_dir);
    state.set_signal(signal, on);
    save(config_dir, &state)
}

/// Save a finished scan and add its interest tags to Pulse's topics.
pub fn apply(
    config_dir: &Path,
    scan: Scan,
    now_ms: u64,
    interests: impl FnOnce(Edit<'_>) -> Result<(), String>,
) -> Result<(), String> {
    let mut state = load(config_dir);
    if !state.on {
        return Ok(());
    }
    state.apply(scan, now_ms);
    save(config_dir, &state)?;
    let topics: Vec<String> = state.tags.iter().filter_map(|t| t.topic.clone()).collect();
    interests(&mut |profile| {
        let mut added = false;
        for topic in &topics {
            added |= profile.add(topic, InterestSource::KnowMe, now_ms);
        }
        added
    })
}

/// A deleted tag's topic leaves Pulse, unless you added it yourself.
fn drop_topic(profile: &mut InterestProfile, topic: Option<&str>) -> bool {
    let Some(topic) = topic else {
        return false;
    };
    let from_pc = profile
        .find(topic)
        .is_some_and(|t| t.source == InterestSource::KnowMe);
    from_pc && profile.remove(topic)
}

pub fn delete(
    config_dir: &Path,
    tag: &str,
    interests: impl FnOnce(Edit<'_>) -> Result<(), String>,
) -> Result<String, String> {
    let mut state = load(config_dir);
    let Some(gone) = state.delete(tag) else {
        return Ok(format!("{tag} is already gone."));
    };
    save(config_dir, &state)?;
    interests(&mut |profile| drop_topic(profile, gone.topic.as_deref()))?;
    Ok(String::new())
}

pub fn wipe(
    config_dir: &Path,
    interests: impl FnOnce(Edit<'_>) -> Result<(), String>,
) -> Result<String, String> {
    let mut state = load(config_dir);
    let gone = state.wipe();
    save(config_dir, &state)?;
    interests(&mut |profile| {
        let mut dropped = false;
        for tag in &gone {
            dropped |= drop_topic(profile, tag.topic.as_deref());
        }
        dropped
    })?;
    Ok(WIPED.into())
}

/// The scan's inputs from this computer, as the cabin reads them.
fn scan_input(config_dir: &Path) -> ScanInput {
    let home = grokhub_core::paths::user_home().unwrap_or_else(|| std::path::PathBuf::from("."));
    let path_dirs = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    let mut app_dirs = vec![
        home.join(".local/share/applications"),
        std::path::PathBuf::from("/usr/share/applications"),
        std::path::PathBuf::from("/var/lib/flatpak/exports/share/applications"),
    ];
    for var in ["APPDATA", "ProgramData"] {
        if let Some(dir) = std::env::var_os(var) {
            app_dirs
                .push(std::path::PathBuf::from(dir).join("Microsoft/Windows/Start Menu/Programs"));
        }
    }
    let games = grokhub_core::steam::detect_root()
        .map(|root| grokhub_core::steam::read_library(&root))
        .unwrap_or_default();
    let machine = crate::local_ai::read_machine(config_dir);
    let gpu = machine
        .gpus
        .iter()
        .max_by_key(|g| g.vram_mb)
        .map(|g| g.name.trim().to_string())
        .unwrap_or_default();
    let locale = ["LC_ALL", "LANG"]
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()))
        .unwrap_or_default();
    ScanInput {
        home,
        path_dirs,
        app_dirs,
        games,
        hardware: machine.ram_mb.filter(|mb| *mb > 0).map(|mb| (gpu, mb)),
        locale,
    }
}

fn scan_now(config_dir: &Path) -> Result<(), String> {
    let state = load(config_dir);
    if !state.on {
        return Ok(());
    }
    let scan = know_me::scan(&state, &scan_input(config_dir));
    apply(config_dir, scan, grokhub_core::now_ms(), |f| {
        crate::interests::update(config_dir, |p| f(p))
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    pub view: KnowMeView,
    pub status: String,
}

fn answer(config_dir: &Path, status: Result<String, String>) -> Answer {
    Answer {
        view: view(config_dir, grokhub_core::now_ms()),
        status: status.unwrap_or_else(|e| e),
    }
}

#[tauri::command]
pub(crate) async fn get_know_me(
    state: tauri::State<'_, crate::ShellState>,
) -> Result<KnowMeView, String> {
    let dir = state.config_dir.clone();
    crate::off_main(move || view(&dir, grokhub_core::now_ms())).await
}

#[tauri::command]
pub(crate) async fn set_know_me_on(
    state: tauri::State<'_, crate::ShellState>,
    on: bool,
) -> Result<Answer, String> {
    let dir = state.config_dir.clone();
    crate::off_main(move || {
        let done = set_on(&dir, on).and_then(|on| {
            if on {
                scan_now(&dir).map(|()| String::new())
            } else {
                Ok(String::new())
            }
        });
        answer(&dir, done)
    })
    .await
}

#[tauri::command]
pub(crate) async fn set_know_me_signal(
    state: tauri::State<'_, crate::ShellState>,
    id: String,
    on: bool,
) -> Result<Answer, String> {
    let dir = state.config_dir.clone();
    crate::off_main(move || {
        let done = set_signal(&dir, &id, on).map(|()| String::new());
        answer(&dir, done)
    })
    .await
}

#[tauri::command]
pub(crate) async fn know_me_scan(
    state: tauri::State<'_, crate::ShellState>,
) -> Result<Answer, String> {
    let dir = state.config_dir.clone();
    crate::off_main(move || {
        let done = scan_now(&dir).map(|()| String::new());
        answer(&dir, done)
    })
    .await
}

#[tauri::command]
pub(crate) async fn know_me_delete(
    state: tauri::State<'_, crate::ShellState>,
    tag: String,
) -> Result<Answer, String> {
    let dir = state.config_dir.clone();
    crate::off_main(move || {
        let done = delete(&dir, &tag, |f| crate::interests::update(&dir, |p| f(p)));
        answer(&dir, done)
    })
    .await
}

#[tauri::command]
pub(crate) async fn know_me_wipe(
    state: tauri::State<'_, crate::ShellState>,
) -> Result<Answer, String> {
    let dir = state.config_dir.clone();
    crate::off_main(move || {
        let done = wipe(&dir, |f| crate::interests::update(&dir, |p| f(p)));
        answer(&dir, done)
    })
    .await
}

#[cfg(test)]
#[path = "know_me_tests.rs"]
mod tests;
