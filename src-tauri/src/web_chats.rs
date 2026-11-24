//! The chats a paired browser started, kept on disk ([`FILE`]) so the rule
//! outlives a pairing and a restart: a browser goes on only with a chat a
//! browser started, never with one this computer's window started, whether
//! the window has it open now or not (remote.rs).

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

pub const FILE: &str = "web-chats.json";
/// The newest ids kept. An older web chat counts as the window's, which only
/// means a browser can't pick it up again.
const KEEP: usize = 4096;

/// One change to [`FILE`] at a time.
static STORE: Mutex<()> = Mutex::new(());

/// The ids on file, oldest first. A file that can't be read lists none, so
/// every saved chat counts as the window's.
fn load(config_dir: &Path) -> Vec<String> {
    std::fs::read_to_string(config_dir.join(FILE))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Whether a paired browser started chat `id`.
pub fn started_on_web(config_dir: &Path, id: &str) -> bool {
    load(config_dir).iter().any(|kept| kept == id)
}

/// Every chat a paired browser started.
pub fn all(config_dir: &Path) -> HashSet<String> {
    load(config_dir).into_iter().collect()
}

/// A paired browser started (or goes on with) chat `id`.
pub fn note(config_dir: &Path, id: &str) -> Result<(), String> {
    let _store = STORE.lock().unwrap_or_else(|err| err.into_inner());
    let mut ids = load(config_dir);
    if ids.iter().any(|kept| kept == id) {
        return Ok(());
    }
    ids.push(id.to_string());
    let extra = ids.len().saturating_sub(KEEP);
    ids.drain(..extra);
    let raw = serde_json::to_string(&ids).map_err(|e| e.to_string())?;
    grokhub_core::pulse_writer::write_atomic(&config_dir.join(FILE), &raw)
}

/// Whether chat `id` has saved turns on this computer.
fn saved_here(config_dir: &Path, id: &str) -> bool {
    let _config = grokhub_agent::perm::ConfigGuard::set(config_dir);
    grokhub_agent::session_file(id).is_ok_and(|path| path.exists())
}

/// Whether this computer's window started chat `id`: it is saved here and no
/// browser started it. A chat that is new to this computer is no one's yet.
pub fn the_windows(config_dir: &Path, id: &str) -> bool {
    saved_here(config_dir, id) && !started_on_web(config_dir, id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-web-chats-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn a_saved_chat_is_the_windows_unless_a_browser_started_it() {
        let dir = temp_dir("saved");
        let _config = grokhub_agent::perm::ConfigGuard::set(&dir);
        // New to this computer: no one's yet.
        assert!(!the_windows(&dir, "shell-new-1"));
        crate::agent::save_bench_exchange("shell-desk-1", "/home/user", "hi", "Hello.").unwrap();
        crate::agent::save_bench_exchange("shell-web-1", "/home/user", "hi", "Hello.").unwrap();
        note(&dir, "shell-web-1").unwrap();
        assert!(the_windows(&dir, "shell-desk-1"));
        assert!(!the_windows(&dir, "shell-web-1"));
        assert!(started_on_web(&dir, "shell-web-1"));
        assert!(!started_on_web(&dir, "shell-desk-1"));
        // Noting twice keeps one row.
        note(&dir, "shell-web-1").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join(FILE)).unwrap(),
            r#"["shell-web-1"]"#
        );
    }

    #[test]
    fn only_the_newest_web_chats_are_kept() {
        let dir = temp_dir("keep");
        let ids: Vec<String> = (0..KEEP).map(|n| format!("shell-w-{n}")).collect();
        std::fs::write(dir.join(FILE), serde_json::to_string(&ids).unwrap()).unwrap();
        note(&dir, "shell-w-new").unwrap();
        let kept = load(&dir);
        assert_eq!(kept.len(), KEEP);
        assert_eq!(kept.first().map(String::as_str), Some("shell-w-1"));
        assert_eq!(kept.last().map(String::as_str), Some("shell-w-new"));
        assert!(!started_on_web(&dir, "shell-w-0"));
    }
}
