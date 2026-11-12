//! Chat tab strip + Hangout TALK/TYPE for the Tauri shell (parity card 732).
//!
//! Soft archive / soft-delete live in `{config}/shell-chat-tabs.json` so the
//! session files stay on disk until trash is purged (30 days, cabin window).
//! Pin and rename reuse `history` / `shell-pins.json`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::history;

/// Soft-deleted chats stay on disk this long before permanent purge (cabin).
pub const TRASH_RETAIN_MS: u64 = 30 * 24 * 60 * 60 * 1000;

const STORE_FILE: &str = "shell-chat-tabs.json";

pub const BAD_MODE: &str = "Hangout mode is talk or type.";
pub const NOT_ARCHIVED: &str = "That chat is not archived.";
pub const NOT_DELETED: &str = "That chat is not in Recently deleted.";
pub const ALREADY_ARCHIVED: &str = "That chat is already archived.";
pub const ALREADY_DELETED: &str = "That chat is already deleted.";
pub const NO_CHAT: &str = "That chat isn't one of this app's chats.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum HangoutMode {
    #[default]
    Type,
    Talk,
}

impl HangoutMode {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "type" => Ok(Self::Type),
            "talk" => Ok(Self::Talk),
            _ => Err(BAD_MODE.into()),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Type => "type",
            Self::Talk => "talk",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct TabMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    archived_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    deleted_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Store {
    #[serde(default)]
    hangout_mode: HangoutMode,
    #[serde(default)]
    tabs: HashMap<String, TabMeta>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            hangout_mode: HangoutMode::Type,
            tabs: HashMap::new(),
        }
    }
}

/// One row on the tab strip or a restore menu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTab {
    pub id: String,
    pub title: String,
    pub pinned: bool,
    pub updated: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<u64>,
}

fn store_path(config_dir: &Path) -> PathBuf {
    config_dir.join(STORE_FILE)
}

fn load_store(config_dir: &Path) -> Store {
    std::fs::read_to_string(store_path(config_dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_store(config_dir: &Path, store: &Store) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{STORE_FILE}.tmp"));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, store_path(config_dir)).map_err(|e| e.to_string())
}

fn shell_id(id: &str) -> Result<&str, String> {
    let id = id.trim();
    if id.starts_with(history::PREFIX) {
        Ok(id)
    } else {
        Err(NO_CHAT.into())
    }
}

fn meta_of<'a>(store: &'a Store, id: &str) -> Option<&'a TabMeta> {
    store.tabs.get(id)
}

fn is_archived(store: &Store, id: &str) -> bool {
    meta_of(store, id).and_then(|m| m.archived_at).is_some()
}

fn is_deleted(store: &Store, id: &str) -> bool {
    meta_of(store, id).and_then(|m| m.deleted_at).is_some()
}

fn row_from(history: &history::ChatRow, store: &Store) -> ChatTab {
    let meta = meta_of(store, &history.id);
    ChatTab {
        id: history.id.clone(),
        title: history.title.clone(),
        pinned: history.pinned,
        updated: history.updated,
        archived_at: meta.and_then(|m| m.archived_at),
        deleted_at: meta.and_then(|m| m.deleted_at),
    }
}

fn ensure_known(config_dir: &Path, id: &str) -> Result<(), String> {
    let id = shell_id(id)?;
    history::saved(config_dir, id).map(|_| ())
}

/// Live tabs: not archived, not soft-deleted. Pinned first, then newest.
pub fn list_tabs(config_dir: &Path) -> Vec<ChatTab> {
    let mut store = load_store(config_dir);
    let _ = purge_expired(config_dir, &mut store, now_ms());
    history::list(config_dir)
        .into_iter()
        .filter(|row| !is_archived(&store, &row.id) && !is_deleted(&store, &row.id))
        .map(|row| row_from(&row, &store))
        .collect()
}

pub fn list_archived(config_dir: &Path) -> Vec<ChatTab> {
    let store = load_store(config_dir);
    history::list(config_dir)
        .into_iter()
        .filter(|row| is_archived(&store, &row.id) && !is_deleted(&store, &row.id))
        .map(|row| row_from(&row, &store))
        .collect()
}

pub fn list_deleted(config_dir: &Path) -> Vec<ChatTab> {
    let mut store = load_store(config_dir);
    let _ = purge_expired(config_dir, &mut store, now_ms());
    history::list(config_dir)
        .into_iter()
        .filter(|row| is_deleted(&store, &row.id))
        .map(|row| row_from(&row, &store))
        .collect()
}

/// Switch focus: proves the chat exists and is live (not trash). UI loads it.
pub fn switch_tab(config_dir: &Path, id: &str) -> Result<ChatTab, String> {
    let id = shell_id(id)?;
    let store = load_store(config_dir);
    if is_deleted(&store, id) {
        return Err(ALREADY_DELETED.into());
    }
    if is_archived(&store, id) {
        return Err(ALREADY_ARCHIVED.into());
    }
    let row = history::list(config_dir)
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| NO_CHAT.to_string())?;
    Ok(row_from(&row, &store))
}

pub fn pin_tab(config_dir: &Path, id: &str) -> Result<bool, String> {
    ensure_known(config_dir, id)?;
    history::toggle_pin(config_dir, id)
}

pub fn rename_tab(config_dir: &Path, id: &str, title: &str) -> Result<(), String> {
    ensure_known(config_dir, id)?;
    history::rename(config_dir, id, title)
}

pub fn archive_tab(config_dir: &Path, id: &str, now_ms: u64) -> Result<ChatTab, String> {
    let id = shell_id(id)?;
    ensure_known(config_dir, id)?;
    let mut store = load_store(config_dir);
    if is_deleted(&store, id) {
        return Err(ALREADY_DELETED.into());
    }
    if is_archived(&store, id) {
        return Err(ALREADY_ARCHIVED.into());
    }
    store.tabs.entry(id.to_string()).or_default().archived_at = Some(now_ms);
    save_store(config_dir, &store)?;
    let row = history::list(config_dir)
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| NO_CHAT.to_string())?;
    Ok(row_from(&row, &store))
}

pub fn unarchive_tab(config_dir: &Path, id: &str) -> Result<ChatTab, String> {
    let id = shell_id(id)?;
    ensure_known(config_dir, id)?;
    let mut store = load_store(config_dir);
    {
        let meta = store.tabs.entry(id.to_string()).or_default();
        if meta.archived_at.take().is_none() {
            return Err(NOT_ARCHIVED.into());
        }
    }
    tidy_meta(&mut store, id);
    save_store(config_dir, &store)?;
    let row = history::list(config_dir)
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| NO_CHAT.to_string())?;
    Ok(row_from(&row, &load_store(config_dir)))
}

pub fn soft_delete_tab(config_dir: &Path, id: &str, now_ms: u64) -> Result<ChatTab, String> {
    let id = shell_id(id)?;
    ensure_known(config_dir, id)?;
    let mut store = load_store(config_dir);
    if is_deleted(&store, id) {
        return Err(ALREADY_DELETED.into());
    }
    let meta = store.tabs.entry(id.to_string()).or_default();
    meta.archived_at = None;
    meta.deleted_at = Some(now_ms);
    save_store(config_dir, &store)?;
    let row = history::list(config_dir)
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| NO_CHAT.to_string())?;
    Ok(row_from(&row, &store))
}

pub fn restore_deleted_tab(config_dir: &Path, id: &str) -> Result<ChatTab, String> {
    let id = shell_id(id)?;
    ensure_known(config_dir, id)?;
    let mut store = load_store(config_dir);
    {
        let meta = store.tabs.entry(id.to_string()).or_default();
        if meta.deleted_at.take().is_none() {
            return Err(NOT_DELETED.into());
        }
    }
    tidy_meta(&mut store, id);
    save_store(config_dir, &store)?;
    let row = history::list(config_dir)
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| NO_CHAT.to_string())?;
    Ok(row_from(&row, &load_store(config_dir)))
}

/// Drop soft-deleted rows older than the retain window and hard-delete their
/// session files. Returns the purged ids.
fn purge_expired(config_dir: &Path, store: &mut Store, now_ms: u64) -> Result<Vec<String>, String> {
    let mut purged = Vec::new();
    let stale: Vec<String> = store
        .tabs
        .iter()
        .filter_map(|(id, meta)| {
            meta.deleted_at.and_then(|at| {
                if now_ms.saturating_sub(at) >= TRASH_RETAIN_MS {
                    Some(id.clone())
                } else {
                    None
                }
            })
        })
        .collect();
    for id in stale {
        match history::delete(config_dir, &id) {
            Ok(()) => {
                store.tabs.remove(&id);
                purged.push(id);
            }
            Err(_) => {
                // File already gone: still drop the flag.
                store.tabs.remove(&id);
                purged.push(id);
            }
        }
    }
    if !purged.is_empty() {
        save_store(config_dir, store)?;
    }
    Ok(purged)
}

pub fn get_hangout_mode(config_dir: &Path) -> HangoutMode {
    load_store(config_dir).hangout_mode
}

pub fn set_hangout_mode(config_dir: &Path, mode: HangoutMode) -> Result<HangoutMode, String> {
    let mut store = load_store(config_dir);
    store.hangout_mode = mode;
    save_store(config_dir, &store)?;
    Ok(mode)
}

fn tidy_meta(store: &mut Store, id: &str) {
    let empty = store
        .tabs
        .get(id)
        .is_some_and(|m| m.archived_at.is_none() && m.deleted_at.is_none());
    if empty {
        store.tabs.remove(id);
    }
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::perm::ConfigGuard;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-chat-tabs-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn write_session(config: &Path, id: &str, title: &str, created: u64) {
        let _guard = ConfigGuard::set(config);
        let sessions = config.join("sessions");
        std::fs::create_dir_all(&sessions).expect("sessions");
        let line = serde_json::json!({
            "id": id,
            "title": title,
            "created": created,
            "cwd": "/home/user",
            "model": "test",
            "engine": "native",
            "renamed": true,
        });
        std::fs::write(sessions.join(format!("{id}.jsonl")), format!("{line}\n")).expect("session");
    }

    #[test]
    fn archive_then_restore_brings_a_tab_back() {
        let config = temp_dir("archive");
        write_session(&config, "shell-archive-a", "Alpha chat", 1_000);
        write_session(&config, "shell-archive-b", "Beta chat", 2_000);

        let live = list_tabs(&config);
        assert_eq!(live.len(), 2, "{live:?}");

        let archived = archive_tab(&config, "shell-archive-a", 9_000).expect("archive");
        assert_eq!(archived.archived_at, Some(9_000));
        assert_eq!(list_tabs(&config).len(), 1);
        assert_eq!(list_archived(&config).len(), 1);

        let restored = unarchive_tab(&config, "shell-archive-a").expect("restore");
        assert!(restored.archived_at.is_none());
        let live = list_tabs(&config);
        assert_eq!(live.len(), 2, "{live:?}");
        assert!(live.iter().any(|t| t.id == "shell-archive-a"));
    }

    #[test]
    fn soft_delete_then_restore_within_window() {
        let config = temp_dir("trash");
        write_session(&config, "shell-trash-a", "Gone soon", 1_000);
        let now = grokhub_core::now_ms();

        let gone = soft_delete_tab(&config, "shell-trash-a", now).expect("soft delete");
        assert_eq!(gone.deleted_at, Some(now));
        assert!(list_tabs(&config).is_empty());
        assert_eq!(list_deleted(&config).len(), 1);

        let back = restore_deleted_tab(&config, "shell-trash-a").expect("restore");
        assert!(back.deleted_at.is_none());
        assert_eq!(list_tabs(&config).len(), 1);
        assert!(list_deleted(&config).is_empty());
    }

    #[test]
    fn expired_trash_is_purged() {
        let config = temp_dir("purge");
        write_session(&config, "shell-purge-a", "Old trash", 1_000);
        let deleted_at = 1_000u64;
        soft_delete_tab(&config, "shell-purge-a", deleted_at).expect("soft delete");

        // Drive purge with an explicit clock so list_deleted's real-now sweep
        // is not required: call purge_expired through list_deleted by aging
        // the flag past the retain window via a direct store rewrite.
        let mut store = load_store(&config);
        store
            .tabs
            .get_mut("shell-purge-a")
            .expect("meta")
            .deleted_at = Some(deleted_at);
        let purged =
            purge_expired(&config, &mut store, deleted_at + TRASH_RETAIN_MS).expect("purge");
        assert_eq!(purged, vec!["shell-purge-a".to_string()]);
        assert!(list_tabs(&config).is_empty());
        assert!(
            !config
                .join("sessions")
                .join("shell-purge-a.jsonl")
                .is_file(),
            "session file should be gone"
        );
    }

    #[test]
    fn pins_persist_across_list() {
        let config = temp_dir("pins");
        write_session(&config, "shell-pin-a", "Pinned", 1_000);
        write_session(&config, "shell-pin-b", "Loose", 2_000);

        assert!(pin_tab(&config, "shell-pin-a").expect("pin"));
        let tabs = list_tabs(&config);
        assert_eq!(tabs[0].id, "shell-pin-a");
        assert!(tabs[0].pinned);
        assert!(!tabs[1].pinned);

        // Sidecar reload still sees the pin file.
        let again = list_tabs(&config);
        assert!(again.iter().find(|t| t.id == "shell-pin-a").unwrap().pinned);
    }

    #[test]
    fn hangout_mode_round_trips() {
        let config = temp_dir("hangout");
        assert_eq!(get_hangout_mode(&config), HangoutMode::Type);
        assert_eq!(
            set_hangout_mode(&config, HangoutMode::Talk).expect("set"),
            HangoutMode::Talk
        );
        assert_eq!(get_hangout_mode(&config), HangoutMode::Talk);
        assert!(HangoutMode::parse("nope").is_err());
    }
}
