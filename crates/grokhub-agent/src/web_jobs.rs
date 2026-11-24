//! The jobs whose words a paired browser wrote, kept in [`FILE`] next to
//! `automations.json`. The shell runs such a job without the desktop tools,
//! however its run starts. The shell marks a job when a browser saves its
//! words on the Automations page. The agent's job writer (`scheduler_create`,
//! `automation_create`, `automation_modify`) marks it when the chat that
//! called the tool is one a browser drives ([`set_web_chat`]), so asking a
//! web chat to write a job is no way around the mark.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Mutex;

pub const FILE: &str = "automations-web.json";

/// One change to [`FILE`] at a time.
static STORE: Mutex<()> = Mutex::new(());

/// The chats a paired browser drove since the app started.
static CHATS: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// The jobs a paired browser wrote. No file is none; one that can't be read
/// is an error, so a run treats its job as the browser's.
fn ids(config_dir: &Path) -> Result<Vec<String>, String> {
    match std::fs::read_to_string(config_dir.join(FILE)) {
        Ok(raw) => serde_json::from_str(&raw).map_err(|e| format!("{FILE}: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.to_string()),
    }
}

/// Marks job `id` as a browser's (`web`) or as this computer's.
pub fn note(config_dir: &Path, id: &str, web: bool) -> Result<(), String> {
    let _store = STORE.lock().unwrap_or_else(|err| err.into_inner());
    let mut ids = ids(config_dir)?;
    if ids.iter().any(|kept| kept == id) == web {
        return Ok(());
    }
    ids.retain(|kept| kept != id);
    if web {
        ids.push(id.to_string());
    }
    let raw = serde_json::to_string(&ids).map_err(|e| e.to_string())?;
    grokhub_core::pulse_writer::write_atomic(&config_dir.join(FILE), &raw)
}

/// Whether a paired browser wrote job `id`'s words.
pub fn web_wrote(config_dir: &Path, id: &str) -> bool {
    ids(config_dir).map_or(true, |ids| ids.iter().any(|kept| kept == id))
}

/// A paired browser drives chat `session`. From now until the app quits, a
/// job that chat's agent writes, or one of its helpers writes, is the
/// browser's, even on a later turn typed in the window.
pub fn set_web_chat(session: &str) {
    if session.is_empty() {
        return;
    }
    CHATS
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .insert(session.to_string());
}

/// Whether a paired browser drives the chat that `session` runs in. A
/// helper's session is `<chat>/<helper>`.
pub fn web_chat(session: &str) -> bool {
    let chat = session.split('/').next().unwrap_or(session);
    CHATS
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .contains(chat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mark_is_kept_once_and_a_broken_file_marks_every_job() {
        let dir = std::env::temp_dir().join(format!(
            "gh-web-jobs-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!web_wrote(&dir, "auto-1"));
        note(&dir, "auto-1", true).unwrap();
        note(&dir, "auto-1", true).unwrap();
        note(&dir, "auto-2", true).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join(FILE)).unwrap(),
            r#"["auto-1","auto-2"]"#
        );
        note(&dir, "auto-1", false).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join(FILE)).unwrap(),
            r#"["auto-2"]"#
        );
        assert!(!web_wrote(&dir, "auto-1"));
        std::fs::write(dir.join(FILE), "[not json").unwrap();
        assert!(web_wrote(&dir, "auto-1"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_helper_runs_in_its_chat() {
        set_web_chat("shell-web-chat-9");
        assert!(web_chat("shell-web-chat-9"));
        assert!(web_chat("shell-web-chat-9/helper-2"));
        assert!(!web_chat("shell-web-chat-90"));
        assert!(!web_chat("shell-desk-chat-9"));
    }
}
