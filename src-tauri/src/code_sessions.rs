//! The folder each Code tab session works in. A session takes the open
//! project's folder when the agent window first shows it, and keeps that
//! folder for every later message, whatever project the app opens after.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Session id to its folder, for this launch.
#[derive(Default)]
pub struct Folders(Mutex<HashMap<String, PathBuf>>);

impl Folders {
    /// Gives `conversation_id` the folder `open` unless it has one already.
    /// Returns the folder the session works in.
    pub fn bind(&self, conversation_id: &str, open: &Path) -> PathBuf {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(conversation_id.to_string())
            .or_insert_with(|| open.to_path_buf())
            .clone()
    }

    /// The session's folder, when it has one that is still on disk. A folder
    /// deleted since gives `None`, and the message runs in the app's folder.
    pub fn of(&self, conversation_id: &str) -> Option<PathBuf> {
        let map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.get(conversation_id).filter(|dir| dir.is_dir()).cloned()
    }

    /// True for a session the agent window has shown and not closed since.
    pub fn holds(&self, conversation_id: &str) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(conversation_id)
    }

    /// The session was closed or deleted. Opened again in the agent window,
    /// it takes the project open then.
    pub fn forget(&self, conversation_id: &str) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(conversation_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-code-sessions-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn a_session_keeps_its_first_folder_when_the_open_project_changes() {
        let (first, second) = (temp_dir("keep-a"), temp_dir("keep-b"));
        let folders = Folders::default();
        assert_eq!(folders.bind("s1", &first), first);
        assert_eq!(folders.bind("s1", &second), first);
        assert_eq!(folders.of("s1"), Some(first.clone()));
        assert_eq!(folders.bind("s2", &second), second);
        assert_eq!(folders.of("s2"), Some(second.clone()));
    }

    #[test]
    fn a_session_with_no_folder_or_a_deleted_one_has_none() {
        let folders = Folders::default();
        assert_eq!(folders.of("never-bound"), None);
        assert!(!folders.holds("never-bound"));
        let gone = temp_dir("gone");
        folders.bind("s1", &gone);
        std::fs::remove_dir_all(&gone).expect("remove");
        assert_eq!(folders.of("s1"), None);
        // Still the agent window's session, though its folder is gone.
        assert!(folders.holds("s1"));
    }

    #[test]
    fn forget_drops_the_folder_so_the_next_bind_takes_the_open_one() {
        let (first, second) = (temp_dir("forget-a"), temp_dir("forget-b"));
        let folders = Folders::default();
        folders.bind("s1", &first);
        folders.forget("s1");
        assert_eq!(folders.of("s1"), None);
        assert!(!folders.holds("s1"));
        assert_eq!(folders.bind("s1", &second), second);
    }
}
