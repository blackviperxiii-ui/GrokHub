//! The folder each Code tab session works in. A session takes the open
//! project's folder when the agent window first shows it, and keeps that
//! folder for every later message, whatever project the app opens after.
//! The folder is written under the session's own directory so a restart
//! keeps it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The saved folder, one path, next to the session's plan.
pub(crate) const WORKSPACE_FILE: &str = "workspace";
/// Where the session left off. `code_keep` writes it; forget removes it.
pub(crate) const CHECKPOINT_FILE: &str = "checkpoint.json";

/// Session id to its folder. The map is this launch; the files outlive it.
pub struct Folders {
    map: Mutex<HashMap<String, PathBuf>>,
    config: Option<PathBuf>,
}

impl Default for Folders {
    fn default() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
            config: None,
        }
    }
}

impl Folders {
    /// Remembers folders under `config_dir`, including ones a previous
    /// launch wrote.
    pub fn open(config_dir: impl Into<PathBuf>) -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
            config: Some(config_dir.into()),
        }
    }

    /// Gives `conversation_id` the folder `open` unless it has one already
    /// (in memory or on disk). Returns the folder the session works in.
    pub fn bind(&self, conversation_id: &str, open: &Path) -> PathBuf {
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(kept) = map.get(conversation_id) {
            return kept.clone();
        }
        if let Some(saved) = self.saved(conversation_id) {
            map.insert(conversation_id.to_string(), saved.clone());
            return saved;
        }
        let path = open.to_path_buf();
        if let Some(config) = &self.config {
            let _ = write_workspace(config, conversation_id, &path);
        }
        map.insert(conversation_id.to_string(), path.clone());
        path
    }

    /// The session's folder, when it has one that is still on disk. A folder
    /// deleted since gives `None`, and the message runs in the app's folder.
    pub fn of(&self, conversation_id: &str) -> Option<PathBuf> {
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(kept) = map.get(conversation_id) {
            return kept.is_dir().then(|| kept.clone());
        }
        let saved = self.saved(conversation_id)?;
        map.insert(conversation_id.to_string(), saved.clone());
        saved.is_dir().then_some(saved)
    }

    /// True for a session the agent window has shown and not closed since,
    /// including one whose folder was saved by an earlier launch.
    pub fn holds(&self, conversation_id: &str) -> bool {
        if self
            .map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(conversation_id)
        {
            return true;
        }
        self.config.as_ref().is_some_and(|config| {
            workspace_file(config, conversation_id).is_some_and(|p| p.is_file())
        })
    }

    /// The session was closed or deleted. Opened again in the agent window,
    /// it takes the project open then.
    pub fn forget(&self, conversation_id: &str) {
        self.map
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(conversation_id);
        if let Some(config) = &self.config {
            if let Some(dir) = session_dir(config, conversation_id) {
                let _ = std::fs::remove_file(dir.join(WORKSPACE_FILE));
                let _ = std::fs::remove_file(dir.join(CHECKPOINT_FILE));
            }
        }
    }

    fn saved(&self, conversation_id: &str) -> Option<PathBuf> {
        read_workspace(self.config.as_ref()?, conversation_id)
    }
}

/// The id as a file name, or `None` for one that could leave the folder.
fn safe(conversation_id: &str) -> Option<&str> {
    let id = conversation_id.trim();
    let bad = id.is_empty()
        || id.len() > 200
        || id.contains('/')
        || id.contains('\\')
        || id.contains("..")
        || id.contains('\0');
    (!bad).then_some(id)
}

/// `<config>/sessions/<id>`, or `None` for an id that could leave that folder.
pub(crate) fn session_dir(config_dir: &Path, conversation_id: &str) -> Option<PathBuf> {
    Some(config_dir.join("sessions").join(safe(conversation_id)?))
}

fn workspace_file(config_dir: &Path, conversation_id: &str) -> Option<PathBuf> {
    Some(session_dir(config_dir, conversation_id)?.join(WORKSPACE_FILE))
}

fn read_workspace(config_dir: &Path, conversation_id: &str) -> Option<PathBuf> {
    let text = std::fs::read_to_string(workspace_file(config_dir, conversation_id)?).ok()?;
    let path = text.trim();
    if path.is_empty() || path.contains('\n') || path.contains('\0') {
        return None;
    }
    Some(PathBuf::from(path))
}

fn write_workspace(config_dir: &Path, conversation_id: &str, path: &Path) -> std::io::Result<()> {
    let text = path.to_string_lossy();
    if text.is_empty() || text.contains('\n') || text.contains('\0') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "bad workspace path",
        ));
    }
    let dir = session_dir(config_dir, conversation_id)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "bad session id"))?;
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(WORKSPACE_FILE), text.as_bytes())
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

    #[test]
    fn a_restart_keeps_the_first_folder_and_forget_lets_the_next_bind_take_the_new_one() {
        let cfg = temp_dir("restart-cfg");
        let (first, second) = (temp_dir("restart-a"), temp_dir("restart-b"));
        let folders = Folders::open(&cfg);
        assert_eq!(folders.bind("s1", &first), first);
        assert_eq!(
            std::fs::read_to_string(cfg.join("sessions").join("s1").join(WORKSPACE_FILE))
                .expect("saved")
                .trim(),
            first.to_string_lossy()
        );

        let again = Folders::open(&cfg);
        assert!(again.holds("s1"));
        assert_eq!(again.bind("s1", &second), first);
        assert_eq!(again.of("s1"), Some(first));

        again.forget("s1");
        assert!(!cfg
            .join("sessions")
            .join("s1")
            .join(WORKSPACE_FILE)
            .exists());
        assert!(!again.holds("s1"));
        let third = Folders::open(&cfg);
        assert_eq!(third.bind("s1", &second), second);
        assert_eq!(third.of("s1"), Some(second));
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn an_id_that_could_leave_the_folder_is_not_written() {
        let cfg = temp_dir("ids");
        let project = temp_dir("ids-project");
        let folders = Folders::open(&cfg);
        for id in ["a/b", "a\\b", "..", "", "nul\0"] {
            assert_eq!(session_dir(&cfg, id), None, "{id:?}");
            folders.bind(id, &project);
        }
        assert!(!cfg.join("sessions").exists());
        let _ = std::fs::remove_dir_all(&cfg);
    }
}
