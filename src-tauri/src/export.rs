//! Chat export in the window. `/export` and History's Export write the chat
//! as Markdown into the Chats folder of your files (`Documents/Loadout/Chats`
//! by default, the same folder the cabin uses, set in `folders.json`). The
//! chat then shows a card with the file's name, Open, Show in folder and Copy
//! path.
//!
//! The page never hands back a path to open: each saved file gets a token,
//! and Open and Show in folder take that token, so the page can only open
//! files this window wrote. Both are desktop only and kept off the paired
//! browser's command list.

use crate::history::{self, Role, Transcript};
use grokhub_core::user_files::{self, UserFolder};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const NOTHING_YET: &str = "There's nothing in this chat to save yet.";
pub const UNKNOWN_FILE: &str = "That file wasn't saved here, so it can't be opened from here.";
pub const GONE: &str = "That file was moved or deleted.";

/// One saved file, as the chat's card shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    /// What Open and Show in folder take.
    pub token: String,
    /// `Trip plan-20261106-1415.md`.
    pub name: String,
    /// The folder's own name: `Chats`.
    pub folder: String,
    /// The whole path, for Copy path and the hover.
    pub path: String,
    /// "Saved Trip plan-20261106-1415.md to Chats".
    pub line: String,
    /// "Show in File Explorer", "Show in Finder" or "Show in folder".
    pub reveal_label: &'static str,
}

/// The files this window saved, by token.
#[derive(Default)]
pub struct Saves(Mutex<HashMap<String, PathBuf>>);

impl Saves {
    /// Remember `path` and return its token.
    pub fn keep(&self, path: PathBuf) -> String {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let token = format!("saved-{}", map.len() + 1);
        map.insert(token.clone(), path);
        token
    }

    /// The file behind `token`, when this window saved it.
    pub fn path(&self, token: &str) -> Result<PathBuf, String> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(token)
            .cloned()
            .ok_or_else(|| UNKNOWN_FILE.to_string())
    }
}

/// The button's words, as each file manager is called.
pub fn reveal_label() -> &'static str {
    if cfg!(windows) {
        "Show in File Explorer"
    } else if cfg!(target_os = "macos") {
        "Show in Finder"
    } else {
        "Show in folder"
    }
}

/// The chat's name stamped with the local day and time, as the cabin names
/// its exports: `Trip plan?` at 14:15 on Nov 6 → `Trip plan-20261106-1415.md`.
pub fn file_name(title: &str, now_ms: u64, offset_min: i32) -> String {
    file_name_as(
        title,
        now_ms,
        offset_min,
        grokhub_core::ChatExport::Markdown,
    )
}

/// [`file_name`] with the extension of `format`.
pub fn file_name_as(
    title: &str,
    now_ms: u64,
    offset_min: i32,
    format: grokhub_core::ChatExport,
) -> String {
    let ext = format.file_name().rsplit('.').next().unwrap_or("md");
    let stem = grokhub_core::export_file_stem(title);
    let local = (now_ms as i64 + i64::from(offset_min) * 60_000).max(0) as u64;
    // "2026-11-06T14:15:00Z"
    let at = grokhub_core::oauth::unix_ms_to_rfc3339(local);
    let digits = |s: &str| s.chars().filter(char::is_ascii_digit).collect::<String>();
    let day = digits(at.get(..10).unwrap_or(""));
    let time = digits(at.get(11..16).unwrap_or(""));
    format!("{stem}-{day}-{time}.{ext}")
}

/// The chat as Markdown: its name, then each message under its role, and
/// each run of tool steps as one line naming them.
pub fn markdown(t: &Transcript) -> String {
    let tools: Vec<String> = t
        .lines
        .iter()
        .map(|l| match l.role {
            Role::Tools => format!("Used {}", l.text),
            _ => l.text.clone(),
        })
        .collect();
    let pairs = t.lines.iter().zip(&tools).map(|(l, text)| {
        let role = match l.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tools => "tools",
        };
        (role, text.as_str())
    });
    grokhub_core::chat_transcript_markdown(&t.title, pairs)
}

/// The chat in `format`: Markdown as [`markdown`], a standalone page for
/// HTML, or the whole transcript as JSON.
pub fn render(t: &Transcript, format: grokhub_core::ChatExport, now_ms: u64) -> String {
    let rows: Vec<(&str, String)> = t
        .lines
        .iter()
        .map(|l| match l.role {
            Role::User => ("user", l.text.clone()),
            Role::Assistant => ("assistant", l.text.clone()),
            Role::Tools => ("tools", format!("Used {}", l.text)),
        })
        .collect();
    let pairs = rows.iter().map(|(role, text)| (*role, text.as_str()));
    match format {
        grokhub_core::ChatExport::Markdown => markdown(t),
        grokhub_core::ChatExport::Html => {
            let footer = format!(
                "Exported from {} {}",
                grokhub_core::brand::product_name(),
                env!("CARGO_PKG_VERSION")
            );
            grokhub_core::chat_export_html(&t.title, pairs, &footer)
        }
        grokhub_core::ChatExport::Json => {
            grokhub_core::chat_export_json(&t.title, &t.id, pairs, now_ms)
        }
    }
}

/// Writes chat `id` into `dir` as a new file (never over an older one).
pub fn export_into(
    config_dir: &Path,
    id: &str,
    dir: &Path,
    now_ms: u64,
    offset_min: i32,
) -> Result<PathBuf, String> {
    export_into_as(
        config_dir,
        id,
        dir,
        now_ms,
        offset_min,
        grokhub_core::ChatExport::Markdown,
    )
}

/// [`export_into`] in `format`.
pub fn export_into_as(
    config_dir: &Path,
    id: &str,
    dir: &Path,
    now_ms: u64,
    offset_min: i32,
    format: grokhub_core::ChatExport,
) -> Result<PathBuf, String> {
    let t = transcript(config_dir, id)?;
    let name = file_name_as(&t.title, now_ms, offset_min, format);
    user_files::write_new_file(dir, &name, render(&t, format, now_ms).as_bytes()).map_err(|e| {
        format!(
            "Couldn't save the chat: {}. Nothing was saved.",
            user_files::plain_io_error(&e)
        )
    })
}

fn transcript(config_dir: &Path, id: &str) -> Result<Transcript, String> {
    if !id.trim().starts_with(history::PREFIX) {
        return Err(history::NOT_SHELL.into());
    }
    let saved = {
        let _config = grokhub_agent::perm::ConfigGuard::set(config_dir);
        grokhub_agent::session_file(id.trim()).is_ok_and(|p| p.is_file())
    };
    if !saved {
        return Err(NOTHING_YET.into());
    }
    let t = history::load(config_dir, id, usize::MAX)?;
    if t.lines.is_empty() {
        return Err(NOTHING_YET.into());
    }
    Ok(t)
}

/// `/export` and History's Export: chat `id` into the Chats folder. Runs
/// off the main thread; [`saved`] then makes the card.
pub fn export_to_chats(
    config_dir: &Path,
    id: &str,
    now_ms: u64,
    offset_min: i32,
) -> Result<PathBuf, String> {
    export_to_chats_as(
        config_dir,
        id,
        now_ms,
        offset_min,
        grokhub_core::ChatExport::Markdown,
    )
}

/// [`export_to_chats`] in `format`.
pub fn export_to_chats_as(
    config_dir: &Path,
    id: &str,
    now_ms: u64,
    offset_min: i32,
    format: grokhub_core::ChatExport,
) -> Result<PathBuf, String> {
    // Read the chat first, so an empty one doesn't make the folder.
    transcript(config_dir, id)?;
    let dir = user_files::folder(config_dir, UserFolder::Chats).map_err(|e| {
        format!(
            "Couldn't make the Chats folder: {}. Nothing was saved.",
            user_files::plain_io_error(&e)
        )
    })?;
    export_into_as(config_dir, id, &dir, now_ms, offset_min, format)
}

/// The card for a file this window just wrote.
pub fn saved(saves: &Saves, path: PathBuf) -> Saved {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let folder = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "its folder".into());
    Saved {
        line: format!("Saved {name} to {folder}"),
        path: path.display().to_string(),
        token: saves.keep(path),
        name,
        folder,
        reveal_label: reveal_label(),
    }
}

fn existing(saves: &Saves, token: &str) -> Result<PathBuf, String> {
    let path = saves.path(token)?;
    if path.is_file() {
        Ok(path)
    } else {
        Err(GONE.into())
    }
}

/// Open: the saved file in its own app.
pub fn open(saves: &Saves, token: &str) -> Result<(), String> {
    crate::capabilities::needs(cfg!(desktop), "Opening a saved file")?;
    let path = existing(saves, token)?;
    tauri_plugin_opener::open_path(&path, None::<&str>)
        .map_err(|e| format!("Couldn't open it: {e}"))
}

/// Show in folder: the file manager with the file selected.
pub fn reveal(saves: &Saves, token: &str) -> Result<(), String> {
    crate::capabilities::needs(cfg!(desktop), "Showing a saved file")?;
    let path = existing(saves, token)?;
    tauri_plugin_opener::reveal_item_in_dir(&path).map_err(|e| format!("Couldn't show it: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::Line;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "grokhub-shell-export-{tag}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_name_is_the_chats_name_stamped_in_local_time() {
        // 2026-11-06 20:15 UTC; six hours behind is 14:15 local.
        let at = 1_793_996_100_000;
        assert_eq!(
            file_name("Trip plan: Lisbon?", at, -360),
            "Trip plan Lisbon-20261106-1415.md"
        );
        assert_eq!(file_name("", at, 0), "chat-20261106-2015.md");
    }

    #[test]
    fn markdown_keeps_every_message_and_names_tool_steps() {
        let t = Transcript {
            id: "shell-x".into(),
            title: "Trip".into(),
            lines: vec![
                Line {
                    role: Role::User,
                    text: "plan it".into(),
                },
                Line {
                    role: Role::Tools,
                    text: "web_search, read_file".into(),
                },
                Line {
                    role: Role::Assistant,
                    text: "Here's the plan.".into(),
                },
            ],
            earlier: 0,
        };
        let md = markdown(&t);
        assert!(md.starts_with("# Trip\n"), "{md}");
        for part in ["plan it", "Used web_search, read_file", "Here's the plan."] {
            assert!(md.contains(part), "{part} missing from {md}");
        }
    }

    #[test]
    fn an_unsaved_or_foreign_chat_saves_nothing() {
        let config = temp("empty-cfg");
        let dir = temp("empty-out");
        assert_eq!(
            export_into(&config, "shell-never-sent", &dir, 0, 0),
            Err(NOTHING_YET.into())
        );
        assert_eq!(
            export_into(&config, "cabin-123", &dir, 0, 0),
            Err(history::NOT_SHELL.into())
        );
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    }

    #[test]
    fn only_files_saved_here_open_and_a_moved_one_says_so() {
        let saves = Saves::default();
        assert_eq!(saves.path("saved-1"), Err(UNKNOWN_FILE.into()));
        let dir = temp("tokens").join("Chats");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("chat.md");
        std::fs::write(&file, "x").unwrap();
        let card = saved(&saves, file.clone());
        assert_eq!(card.line, "Saved chat.md to Chats");
        assert_eq!(card.folder, "Chats");
        assert_eq!(saves.path(&card.token), Ok(file.clone()));
        assert_eq!(existing(&saves, &card.token), Ok(file.clone()));
        std::fs::remove_file(&file).unwrap();
        assert_eq!(existing(&saves, &card.token), Err(GONE.into()));
        // A path is never a token.
        assert_eq!(
            existing(&saves, &file.display().to_string()),
            Err(UNKNOWN_FILE.into())
        );
    }
}
