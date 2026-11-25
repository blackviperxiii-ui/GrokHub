//! The chats a paired browser started, kept on disk
//! ([`grokhub_agent::web_origin::CHATS_FILE`]) so the rule outlives a pairing
//! and a restart: a browser goes on only with a chat a browser started, never
//! with one this computer's window started, whether the window has it open
//! now or not (remote.rs). A browser reads only those chats, through the same
//! check the agent's chat search asks for a browser's run
//! ([`grokhub_agent::web_origin::Readable`]).

use std::path::Path;

use grokhub_agent::web_origin::{Readable, RunOrigin};

/// A paired browser started (or goes on with) chat `id`.
pub fn note(config_dir: &Path, id: &str) -> Result<(), String> {
    grokhub_agent::web_origin::note_chat(config_dir, id)
}

/// Whether this computer's window started chat `id`: it is saved here and no
/// browser started it. A chat that is new to this computer is no one's yet.
pub fn the_windows(config_dir: &Path, id: &str) -> bool {
    grokhub_agent::web_origin::started_here(config_dir, id)
}

/// The saved chats a paired browser may read: the ones a browser started.
pub fn readable(config_dir: &Path) -> Readable {
    Readable::for_run(config_dir, RunOrigin::Browser)
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
        assert!(readable(&dir).sees("shell-web-1"));
        assert!(!readable(&dir).sees("shell-desk-1"));
        // Noting twice keeps one row.
        note(&dir, "shell-web-1").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join(grokhub_agent::web_origin::CHATS_FILE)).unwrap(),
            r#"["shell-web-1"]"#
        );
    }
}
