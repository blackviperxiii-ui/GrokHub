//! Artifacts gallery: images, videos, files and links from every shell chat.
//!
//! Library → Creations is the saved wall (`imagine-wall`). This module lists
//! what each chat actually produced or held: `sessions/<id>/media/`,
//! `sessions/<id>/attach/`, and https links found in that chat's transcript.
//! Each row carries its chat id so the UI can jump back.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;

use crate::history::{self, ChatRow};
use crate::imagine::{picture_mime, video_mime};
use crate::opener;

/// One gallery item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    /// Absolute file path, or the https URL for a link.
    pub path: String,
    pub chat_id: String,
    pub chat_title: String,
    /// Unix ms the file was written, or the chat's last update for a link.
    pub created: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Image,
    Video,
    File,
    Link,
}

const LINK_CAP_PER_CHAT: usize = 40;
const NOT_OURS: &str = "Not a chat artifact.";
const GONE: &str = "That artifact is gone.";

/// Every artifact across this app's shell chats, newest first.
pub fn list(config_dir: &Path) -> Vec<Artifact> {
    list_with(config_dir, &|_| true)
}

/// [`list`] with a chat-ownership filter for remote browser callers.
pub fn list_with(config_dir: &Path, may_read: &dyn Fn(&str) -> bool) -> Vec<Artifact> {
    let mut out = Vec::new();
    for chat in history::list(config_dir) {
        if !may_read(&chat.id) {
            continue;
        }
        out.extend(from_chat(config_dir, &chat));
    }
    out.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| a.id.cmp(&b.id)));
    out
}

fn from_chat(config_dir: &Path, chat: &ChatRow) -> Vec<Artifact> {
    let mut out = Vec::new();
    let media = config_dir
        .join("sessions")
        .join(&chat.id)
        .join("media");
    out.extend(scan_dir(&media, chat, true));
    let attach = config_dir
        .join("sessions")
        .join(&chat.id)
        .join("attach");
    out.extend(scan_dir(&attach, chat, false));
    out.extend(links_in_transcript(config_dir, chat));
    out
}

fn scan_dir(dir: &Path, chat: &ChatRow, media_only: bool) -> Vec<Artifact> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        // Never follow a link out of the chat folder.
        if std::fs::symlink_metadata(&path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(true)
        {
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name.is_empty() || name.starts_with('.') {
            continue;
        }
        let kind = if picture_mime(&name).is_some() {
            Kind::Image
        } else if video_mime(&name).is_some() {
            Kind::Video
        } else if media_only {
            continue;
        } else {
            Kind::File
        };
        let created = file_ms(&path).unwrap_or(chat.updated);
        let path_s = path.display().to_string();
        out.push(Artifact {
            id: format!("{}:{}", chat.id, name),
            kind,
            name,
            path: path_s,
            chat_id: chat.id.clone(),
            chat_title: chat.title.clone(),
            created,
        });
    }
    out
}

fn file_ms(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as u64)
}

fn links_in_transcript(config_dir: &Path, chat: &ChatRow) -> Vec<Artifact> {
    let path = config_dir
        .join("sessions")
        .join(format!("{}.jsonl", chat.id));
    // Refuse a symlink session file the same way History does.
    if std::fs::symlink_metadata(&path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(true)
    {
        return Vec::new();
    }
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for url in extract_urls(&text) {
        if !opener::url_ok(&url) || !seen.insert(url.clone()) {
            continue;
        }
        let name = link_name(&url);
        out.push(Artifact {
            id: format!("{}:link:{}", chat.id, out.len()),
            kind: Kind::Link,
            name,
            path: url,
            chat_id: chat.id.clone(),
            chat_title: chat.title.clone(),
            created: chat.updated,
        });
        if out.len() >= LINK_CAP_PER_CHAT {
            break;
        }
    }
    out
}

/// Visible label for a link: host + short path, capped.
fn link_name(url: &str) -> String {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let trimmed = rest.trim_end_matches('/');
    if trimmed.len() <= 64 {
        trimmed.to_string()
    } else {
        format!("{}…", &trimmed[..63])
    }
}

fn extract_urls(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 8 < lower.len() {
        let at = if lower[i..].starts_with("https://") {
            Some(8)
        } else if lower[i..].starts_with("http://") {
            Some(7)
        } else {
            None
        };
        let Some(scheme_len) = at else {
            i += 1;
            continue;
        };
        // Only start at a boundary so `xhttps://` is skipped.
        if i > 0 {
            let prev = bytes[i - 1] as char;
            if prev.is_ascii_alphanumeric() || prev == '_' || prev == '-' || prev == '.' {
                i += 1;
                continue;
            }
        }
        let start = i;
        i += scheme_len;
        while i < bytes.len() {
            let c = bytes[i] as char;
            if c.is_whitespace()
                || c.is_control()
                || matches!(c, '"' | '\'' | '<' | '>' | ')' | ']' | '}' | ',' | ';')
            {
                break;
            }
            i += 1;
        }
        let mut end = i;
        // Trim trailing punctuation common in prose.
        while end > start + scheme_len {
            let c = bytes[end - 1] as char;
            if matches!(c, '.' | '!' | '?' | ':' | '/') {
                end -= 1;
            } else {
                break;
            }
        }
        if let Ok(url) = std::str::from_utf8(&bytes[start..end]) {
            out.push(url.to_string());
        }
    }
    out
}

/// Whether `name` matches the search box (case-insensitive, name or chat title).
pub fn matches_query(item: &Artifact, query: &str) -> bool {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return true;
    }
    item.name.to_ascii_lowercase().contains(&q)
        || item.chat_title.to_ascii_lowercase().contains(&q)
}

/// Open a listed artifact: a file in its app, or a link in the browser.
pub fn open(config_dir: &Path, path: &str) -> Result<(), String> {
    open_with(config_dir, path, &|_| true)
}

/// [`open`] with a chat-ownership check for remote browser callers.
pub fn open_with(
    config_dir: &Path,
    path: &str,
    may_read: &dyn Fn(Option<&str>) -> bool,
) -> Result<(), String> {
    if opener::url_ok(path) {
        return opener::open(path);
    }
    let (file, chat) = resolve_file(config_dir, path)?;
    if !may_read(chat.as_deref()) {
        return Err(NOT_OURS.into());
    }
    crate::capabilities::needs(cfg!(desktop), "Opening a chat artifact")?;
    tauri_plugin_opener::open_path(&file, None::<&str>)
        .map_err(|e| format!("Couldn't open it: {e}"))
}

/// Reveal a listed file in the file manager. Links are refused.
pub fn reveal(config_dir: &Path, path: &str) -> Result<(), String> {
    reveal_with(config_dir, path, &|_| true)
}

pub fn reveal_with(
    config_dir: &Path,
    path: &str,
    may_read: &dyn Fn(Option<&str>) -> bool,
) -> Result<(), String> {
    if opener::url_ok(path) {
        return Err("A link has no folder to show.".into());
    }
    let (file, chat) = resolve_file(config_dir, path)?;
    if !may_read(chat.as_deref()) {
        return Err(NOT_OURS.into());
    }
    crate::capabilities::needs(cfg!(desktop), "Showing a chat artifact")?;
    tauri_plugin_opener::reveal_item_in_dir(&file).map_err(|e| format!("Couldn't show it: {e}"))
}

/// Canonical file under `sessions/<id>/{media,attach}/`, plus the chat id.
fn resolve_file(config_dir: &Path, path: &str) -> Result<(PathBuf, Option<String>), String> {
    let file = std::fs::canonicalize(path).map_err(|_| GONE.to_string())?;
    let root = std::fs::canonicalize(config_dir).map_err(|_| NOT_OURS.to_string())?;
    let rel = file.strip_prefix(&root).map_err(|_| NOT_OURS.to_string())?;
    let parts: Vec<_> = rel.components().map(|c| c.as_os_str()).collect();
    let chat = match parts.as_slice() {
        [sessions, id, folder, _]
            if *sessions == "sessions" && (*folder == "media" || *folder == "attach") =>
        {
            Some(id.to_str().ok_or_else(|| NOT_OURS.to_string())?.to_string())
        }
        _ => return Err(NOT_OURS.into()),
    };
    if !file.is_file() {
        return Err(GONE.into());
    }
    // Media files must still look like media; attach can be anything listed.
    if parts.get(2).is_some_and(|f| *f == "media") {
        let name = file.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if picture_mime(name).is_none() && video_mime(name).is_none() {
            return Err(NOT_OURS.into());
        }
    }
    Ok((file, chat))
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::perm::ConfigGuard;

    struct Dirs {
        config: PathBuf,
        _guard: ConfigGuard,
    }

    impl Drop for Dirs {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.config);
        }
    }

    fn dirs(name: &str) -> Dirs {
        let config = std::env::temp_dir().join(format!(
            "grokhub-shell-artifacts-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&config);
        std::fs::create_dir_all(&config).expect("cfg");
        let guard = ConfigGuard::set(&config);
        Dirs {
            config,
            _guard: guard,
        }
    }

    fn write_session(config: &Path, id: &str, title: &str, body: &str) {
        let sessions = config.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        // Minimal native header + one assistant line so history::list can see it.
        let header = format!(
            r#"{{"id":"{id}","title":"{title}","cwd":"/tmp","created":1700000000000,"model":"","engine":"native","renamed":false}}"#
        );
        let line = format!(
            r#"{{"type":"message","role":"assistant","content":[{{"type":"output_text","text":{}}}]}}"#,
            serde_json::to_string(body).unwrap()
        );
        std::fs::write(sessions.join(format!("{id}.jsonl")), format!("{header}\n{line}\n"))
            .unwrap();
    }

    fn png_bytes() -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.resize(64, 7);
        bytes
    }

    #[test]
    fn lists_chat_media_attach_and_links_with_chat_ids() {
        let d = dirs("list");
        let id = "shell-art-1";
        write_session(
            &d.config,
            id,
            "make a cabin",
            "Here is https://example.com/docs and also https://example.com/docs again.",
        );
        let media = d.config.join("sessions").join(id).join("media");
        std::fs::create_dir_all(&media).unwrap();
        std::fs::write(media.join("cabin.png"), png_bytes()).unwrap();
        std::fs::write(media.join("clip.mp4"), b"mp4").unwrap();
        let attach = d.config.join("sessions").join(id).join("attach");
        std::fs::create_dir_all(&attach).unwrap();
        std::fs::write(attach.join("notes.txt"), b"hello").unwrap();

        let items = list(&d.config);
        let kinds: Vec<_> = items.iter().map(|a| a.kind).collect();
        assert!(kinds.contains(&Kind::Image), "{items:?}");
        assert!(kinds.contains(&Kind::Video), "{items:?}");
        assert!(kinds.contains(&Kind::File), "{items:?}");
        assert!(kinds.contains(&Kind::Link), "{items:?}");
        let img = items.iter().find(|a| a.kind == Kind::Image).unwrap();
        assert_eq!(img.name, "cabin.png");
        assert_eq!(img.chat_id, id);
        assert_eq!(img.chat_title, "make a cabin");
        let links: Vec<_> = items
            .iter()
            .filter(|a| a.kind == Kind::Link)
            .map(|a| a.path.as_str())
            .collect();
        assert_eq!(links, vec!["https://example.com/docs"]);
    }

    #[test]
    fn search_matches_name_and_chat_title() {
        let item = Artifact {
            id: "a".into(),
            kind: Kind::Image,
            name: "snowy-cabin.png".into(),
            path: "/tmp/x".into(),
            chat_id: "shell-1".into(),
            chat_title: "weekend trip".into(),
            created: 1,
        };
        assert!(matches_query(&item, "cabin"));
        assert!(matches_query(&item, "WEEKEND"));
        assert!(!matches_query(&item, "ocean"));
        assert!(matches_query(&item, "  "));
    }

    #[test]
    fn resolve_file_stays_inside_session_folders() {
        let d = dirs("resolve");
        let id = "shell-art-2";
        write_session(&d.config, id, "x", "hi");
        let media = d.config.join("sessions").join(id).join("media");
        std::fs::create_dir_all(&media).unwrap();
        let pic = media.join("a.png");
        std::fs::write(&pic, png_bytes()).unwrap();
        let (got, chat) = resolve_file(&d.config, &pic.display().to_string()).unwrap();
        assert_eq!(chat.as_deref(), Some(id));
        assert_eq!(got, std::fs::canonicalize(&pic).unwrap());

        let outside = d.config.join("secret.txt");
        std::fs::write(&outside, b"no").unwrap();
        assert_eq!(
            resolve_file(&d.config, &outside.display().to_string()).unwrap_err(),
            NOT_OURS
        );
    }

    #[test]
    fn ownership_filter_hides_other_chats() {
        let d = dirs("own");
        write_session(&d.config, "shell-mine", "mine", "https://mine.example/");
        write_session(&d.config, "shell-theirs", "theirs", "https://theirs.example/");
        let media = d
            .config
            .join("sessions")
            .join("shell-mine")
            .join("media");
        std::fs::create_dir_all(&media).unwrap();
        std::fs::write(media.join("a.png"), png_bytes()).unwrap();

        let mine = list_with(&d.config, &|id| id == "shell-mine");
        assert!(mine.iter().all(|a| a.chat_id == "shell-mine"), "{mine:?}");
        assert!(mine.iter().any(|a| a.kind == Kind::Image));
        assert!(!mine.iter().any(|a| a.path.contains("theirs")));
    }
}
