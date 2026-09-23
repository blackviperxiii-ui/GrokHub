use grokhub_core::{empty_chat_draft, uid, ThreadGoal};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::config;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatThread {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub scratch: bool,
    #[serde(default)]
    pub messages: Arc<Vec<(String, String)>>,
    #[serde(default)]
    pub goal: ThreadGoal,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub title_locked: bool,
    #[serde(default)]
    pub accessed_ms: u64,
    #[serde(default)]
    pub grok_session: Option<String>,
    /// Worktree this Grok session was created in. Resume must load here, not the currently bound tree.
    #[serde(default)]
    pub grok_cwd: Option<String>,
    /// Resume from ~/.grok (TUI session) instead of cabin GROK_HOME.
    #[serde(default)]
    pub grok_user_home: bool,
    /// Sidebar project folder. `None` is a global chat.
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub grok_fork: bool,
    #[serde(default)]
    pub grok_worktree: bool,
    /// Last plan from Plan mode or a `plan` stream event. Empty until one exists.
    #[serde(default)]
    pub plan_body: String,
}

impl ChatThread {
    pub fn new(title: &str, scratch: bool) -> Self {
        Self {
            id: uid("thr"),
            title: title.to_string(),
            scratch,
            messages: Arc::new(Vec::new()),
            goal: ThreadGoal::default(),
            pinned: false,
            title_locked: false,
            accessed_ms: 0,
            grok_session: None,
            grok_cwd: None,
            grok_user_home: false,
            grok_fork: false,
            grok_worktree: false,
            project_id: None,
            plan_body: String::new(),
        }
    }

    /// Copy-on-write. persist() clones every ChatThread; other tabs keep this Arc.
    pub fn messages_mut(&mut self) -> &mut Vec<(String, String)> {
        Arc::make_mut(&mut self.messages)
    }
}

/// Highest `accessed_ms`. Skip scratch when another thread exists.
pub fn most_recently_accessed_index(threads: &[ChatThread]) -> Option<usize> {
    let has_real = threads.iter().any(|t| !t.scratch);
    threads
        .iter()
        .enumerate()
        .filter(|(_, t)| !has_real || !t.scratch)
        .max_by_key(|(i, t)| (t.accessed_ms, *i))
        .map(|(i, _)| i)
}

/// Quiet MidThought line for a last-accessed titled thread. Empty for scratch or default names.
pub fn continue_thread_hint(threads: &[ChatThread]) -> String {
    let Some(idx) = most_recently_accessed_index(threads) else {
        return String::new();
    };
    let Some(t) = threads.get(idx) else {
        return String::new();
    };
    let title = t.title.trim();
    if t.scratch || title.is_empty() {
        return String::new();
    }
    if title.eq_ignore_ascii_case("chat") || title.eq_ignore_ascii_case("scratch") {
        return String::new();
    }
    format!("Continue {title}").chars().take(80).collect()
}

pub fn threads_path() -> std::path::PathBuf {
    config::config_dir().join("threads.json")
}

pub fn load() -> Vec<ChatThread> {
    config::load_json(&threads_path(), config::JSON_STORE_CAP)
}

pub fn save(threads: &[ChatThread]) -> Result<(), String> {
    let s = serde_json::to_string_pretty(threads).map_err(|e| e.to_string())?;
    config::atomic_write(&threads_path(), s.as_bytes())
}

/// Sidebar History filter. No selection shows global chats. A project shows only that folder.
pub fn session_in_chat_folder(thread_project: Option<&str>, selected: Option<&str>) -> bool {
    match selected {
        Some(id) => thread_project == Some(id),
        None => thread_project.is_none(),
    }
}

/// Extra History row for a project folder. Grok session rows are painted separately.
/// An unused Chat draft (no dialogue, no session) stays off the rail.
pub fn project_folder_history_row(already_listed: bool, empty: bool, has_session: bool) -> bool {
    !already_listed && !empty_chat_draft(empty, has_session)
}

/// Delete Project puts chats back in History. Transcripts stay on the thread.
pub fn release_project_chats(threads: &mut [ChatThread], project_id: &str) -> usize {
    let mut n = 0;
    for t in threads.iter_mut() {
        if t.project_id.as_deref() == Some(project_id) {
            t.project_id = None;
            n += 1;
        }
    }
    n
}

pub fn export_markdown(t: &ChatThread) -> String {
    let mut out = format!("# {}\n\n", t.title);
    for (role, content) in t.messages.iter() {
        out.push_str(&format!("## {role}\n\n{content}\n\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn thread_roundtrip() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("thr");
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        let mut t = ChatThread::new("night", true);
        t.messages_mut().push(("user".into(), "hi".into()));
        save(&[t.clone()]).expect("save");
        let loaded = load();
        assert_eq!(loaded[0].title, "night");
        assert!(loaded[0].scratch);
        assert!(loaded[0].goal.label.is_empty());
        assert!(!loaded[0].pinned);
        assert!(!loaded[0].title_locked);
        assert_eq!(loaded[0].accessed_ms, 0);
        assert!(export_markdown(&loaded[0]).contains("hi"));
        let mut grok = ChatThread::new("Grok session", false);
        grok.grok_session = Some("01a01b0f-7e06-74b1-8f22-5236c9d57d45".into());
        save(&[grok]).expect("save grok");
        let loaded = load();
        assert_eq!(
            loaded[0].grok_session.as_deref(),
            Some("01a01b0f-7e06-74b1-8f22-5236c9d57d45")
        );
        let old: ChatThread = serde_json::from_str(r#"{"id":"t1","title":"legacy"}"#).unwrap();
        assert_eq!(old.accessed_ms, 0);
        assert!(old.grok_cwd.is_none());
        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }

    #[test]
    fn most_recent_skips_scratch_and_prefers_access() {
        let mut scratch = ChatThread::new("Scratch", true);
        scratch.accessed_ms = 9_000;
        let mut older = ChatThread::new("Older", false);
        older.accessed_ms = 1_000;
        let mut newer = ChatThread::new("Night cabin", false);
        newer.accessed_ms = 5_000;
        let threads = vec![scratch, older, newer];
        assert_eq!(most_recently_accessed_index(&threads), Some(2));
        assert_eq!(continue_thread_hint(&threads), "Continue Night cabin");
        let only_scratch = vec![ChatThread::new("Scratch", true)];
        assert_eq!(most_recently_accessed_index(&only_scratch), Some(0));
        assert!(continue_thread_hint(&only_scratch).is_empty());
        let untitled = vec![ChatThread::new("Chat", false)];
        assert!(continue_thread_hint(&untitled).is_empty());
    }

    #[test]
    fn a_real_history_survives_a_reload() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("bighist");
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);

        // Ordinary use: a few dozen conversations of a hundred turns each. This is well
        // past the 1 MiB memory-file cap the loader used to read through, at which point
        // the JSON was severed mid-token, parsed as nothing, and the next persist tick
        // wrote the empty list back over every thread.
        let mut want = Vec::new();
        for t in 0..24 {
            let mut thread = ChatThread::new(&format!("Session {t}"), false);
            for m in 0..100 {
                thread.messages_mut().push((
                    if m % 2 == 0 { "user".into() } else { "assistant".into() },
                    format!("turn {m} of session {t}: {}", "x".repeat(400)),
                ));
            }
            want.push(thread);
        }
        save(&want).expect("save");

        let bytes = fs::metadata(threads_path()).expect("meta").len();
        assert!(
            bytes > 1024 * 1024,
            "fixture must exceed the old 1MiB cap, got {bytes} bytes"
        );

        let got = load();
        assert_eq!(
            got.len(),
            want.len(),
            "reload lost threads: {} of {} survived a {bytes} byte history",
            got.len(),
            want.len()
        );
        for (a, b) in want.iter().zip(got.iter()) {
            assert_eq!(a.title, b.title);
            assert_eq!(a.messages.len(), b.messages.len(), "{} lost turns", a.title);
            assert_eq!(a.messages.as_ref(), b.messages.as_ref());
        }

        // The store is intact, so a persist right after boot must not destroy it.
        save(&got).expect("resave");
        assert_eq!(load().len(), want.len(), "a persist after reload wiped history");

        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }

    #[test]
    fn load_does_not_slurp_a_huge_file() {
        let src = include_str!("threads.rs");
        let load = src
            .split("pub fn load(")
            .nth(1)
            .and_then(|s| s.split("pub fn save(").next())
            .expect("threads load");
        assert!(
            load.contains("load_json") && !load.contains("read_to_string"),
            "boot must not slurp an unbounded threads.json: {load}"
        );
        assert!(
            !load.contains("MEMORY_FILE_CAP"),
            "threads.json is chat history, not a memory file: a 1MiB cap truncates it \
             into unparseable JSON and the next persist saves the empty default back \
             over every thread: {load}"
        );
    }

    #[test]
    fn clone_shares_message_bodies() {
        let mut t = ChatThread::new("a", false);
        t.messages_mut()
            .push(("user".into(), "x".repeat(64)));
        let mut u = t.clone();
        assert!(
            Arc::ptr_eq(&t.messages, &u.messages),
            "persist must not clone an 8MB HOST_RESULT when cloning ChatThread"
        );
        u.messages_mut().push(("assistant".into(), "y".into()));
        assert!(!Arc::ptr_eq(&t.messages, &u.messages));
        assert_eq!(t.messages.len(), 1);
        assert_eq!(u.messages.len(), 2);
    }

    #[test]
    fn project_folder_filters_sidebar_and_delete_returns_chats() {
        let mut global = ChatThread::new("Global", false);
        global.messages_mut().push(("user".into(), "keep-global".into()));
        let mut filed = ChatThread::new("Lab notes", false);
        filed.project_id = Some("proj-lab".into());
        filed.messages_mut().push(("user".into(), "keep-lab".into()));
        let mut other = ChatThread::new("Other", false);
        other.project_id = Some("proj-other".into());
        other.messages_mut().push(("assistant".into(), "stay".into()));
        let threads = vec![global, filed, other];

        let visible = |selected: Option<&str>| {
            threads
                .iter()
                .filter(|t| session_in_chat_folder(t.project_id.as_deref(), selected))
                .map(|t| t.title.as_str())
                .collect::<Vec<_>>()
        };
        assert_eq!(visible(None), ["Global"]);
        assert_eq!(visible(Some("proj-lab")), ["Lab notes"]);
        assert!(visible(Some("proj-lab")).iter().all(|t| *t != "Global"));

        let mut owned = threads.clone();
        let n = release_project_chats(&mut owned, "proj-lab");
        assert_eq!(n, 1);
        assert!(owned[1].project_id.is_none());
        assert_eq!(owned[1].messages[0].1, "keep-lab");
        assert_eq!(owned[0].messages[0].1, "keep-global");
        assert_eq!(owned[2].project_id.as_deref(), Some("proj-other"));
        assert_eq!(owned.len(), 3);
        assert!(session_in_chat_folder(owned[1].project_id.as_deref(), None));

        let legacy: ChatThread = serde_json::from_str(r#"{"id":"t1","title":"legacy"}"#).unwrap();
        assert!(legacy.project_id.is_none());
    }

    #[test]
    fn project_history_skips_the_unused_empty_draft() {
        assert!(
            !project_folder_history_row(false, true, false),
            "clicking Chat must not paint the empty draft in the project folder"
        );
        assert!(project_folder_history_row(false, false, false));
        assert!(!project_folder_history_row(true, false, true));
        assert!(empty_chat_draft(true, false));
        assert!(!empty_chat_draft(false, false));
    }
}
