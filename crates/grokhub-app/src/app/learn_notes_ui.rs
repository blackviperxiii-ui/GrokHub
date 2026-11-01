//! The learning loop for the file memory (legacy backend): after each turn,
//! and on reflect, what the user taught the cabin lands as clean lines in
//! USER.md, SOUL.md and MEMORY.md (`grokhub_core::learn_notes`). Every write
//! is logged to `memory/learn-log.jsonl` (time, file, change, key; never the
//! chat text).

use std::io::Write;
use std::sync::mpsc;

use grokhub_core::learn_notes::{apply_notes, note_from_message, Note, NoteFile};
use grokhub_core::MemoryEdit;

use super::*;
use crate::config;

/// What a reflect or a turn wrote: the memory-repo diff when AMR is on, else
/// each file whose body changed.
#[derive(Debug, Default)]
pub(super) struct LearnWrites {
    pub amr_diff: Option<String>,
    pub files: Vec<(&'static str, MemoryEdit)>,
}

const FILES: [NoteFile; 3] = [NoteFile::User, NoteFile::Soul, NoteFile::Memory];

/// Notes from user messages and the model's `USER_FACT:` lines.
pub(super) fn notes_from(messages: &[String], user_facts: &[String]) -> Vec<Note> {
    let mut notes: Vec<Note> = messages
        .iter()
        .filter_map(|m| note_from_message(m))
        .collect();
    notes.extend(user_facts.iter().filter_map(|f| fact_note(f)));
    notes
}

/// A model `USER_FACT:` line is already a summary; it goes to USER.md as is.
fn fact_note(fact: &str) -> Option<Note> {
    let text = fact.trim().trim_end_matches('.').trim();
    if text.len() < 8 || !grokhub_core::learn_notes::is_safe_note_text(text) {
        return None;
    }
    let line = format!("- {text}.");
    let key = grokhub_core::learn_notes::line_key(NoteFile::User, &line)?;
    Some(Note {
        file: NoteFile::User,
        key,
        text: format!("{text}."),
    })
}

/// Apply `notes` to each file on disk (or `open`, the editor's unsaved body
/// for that file), write the ones that changed, and log each write.
pub(super) fn write_notes(
    notes: &[Note],
    day: &str,
    open: Option<(String, String)>,
) -> LearnWrites {
    let mut out = LearnWrites::default();
    for file in FILES {
        if !notes.iter().any(|n| n.file == file) {
            continue;
        }
        let name = file.file_name();
        let current = match &open {
            Some((open_name, body)) if open_name == name => body.clone(),
            _ => config::read_memory(name),
        };
        let edit = apply_notes(&current, file, notes, day);
        if edit.diff.is_empty() {
            continue;
        }
        match config::write_memory(name, &edit.next) {
            Ok(()) => {
                log_write(name, &edit.diff, notes, file);
                out.files.push((name, edit));
            }
            Err(err) => log_failure(name, &err),
        }
    }
    out
}

fn log_path() -> std::path::PathBuf {
    config::memory_dir().join("learn-log.jsonl")
}

fn append_log(row: serde_json::Value) {
    let path = log_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{row}");
    }
}

fn log_write(name: &str, diff: &str, notes: &[Note], file: NoteFile) {
    let keys: Vec<&str> = notes
        .iter()
        .filter(|n| n.file == file)
        .map(|n| n.key.as_str())
        .collect();
    let added = diff.lines().filter(|l| l.starts_with("+ ")).count();
    let replaced = diff.lines().filter(|l| l.starts_with("~ ")).count();
    append_log(serde_json::json!({
        "at": grokhub_core::now_ms(),
        "file": name,
        "added": added,
        "replaced": replaced,
        "keys": keys,
    }));
}

fn log_failure(name: &str, err: &str) {
    append_log(serde_json::json!({
        "at": grokhub_core::now_ms(),
        "file": name,
        "error": err,
    }));
}

impl Cabin {
    /// After a turn: what the user's last message and the model's
    /// `USER_FACT:` lines teach goes into the files now, not at the next
    /// idle reflect. AMR keeps its own store; scratch chats write nothing.
    /// Notes queue while another write or a reflect is in flight.
    pub(super) fn learn_from_turn(&mut self, user: &str, user_facts: &[String]) {
        if self.amr_on() || self.scratch() {
            return;
        }
        self.learn_pending
            .extend(notes_from(&[user.to_string()], user_facts));
        self.start_learn_write();
    }

    /// Write the queued notes, one writer at a time: each write starts from
    /// the bodies the last one adopted, so no line is overwritten.
    pub(super) fn start_learn_write(&mut self) {
        if self.learn_pending.is_empty() || self.learn_rx.is_some() || self.reflect_rx.is_some() {
            return;
        }
        let notes = std::mem::take(&mut self.learn_pending);
        let day = Self::local_day();
        let open = Some((self.mem_name.clone(), self.mem_body.clone()));
        let (tx, rx) = mpsc::channel();
        self.learn_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(write_notes(&notes, &day, open));
        });
    }

    pub(super) fn poll_learn(&mut self) {
        let Some(rx) = self.learn_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(writes) => {
                for (name, edit) in writes.files {
                    self.adopt_memory_write(name, edit.next);
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.learn_rx = Some(rx);
                return;
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
        if std::mem::take(&mut self.reflect_after_learn) {
            self.run_reflect();
        }
        self.start_learn_write();
    }

    /// A memory file changed on disk: refresh its cached body, and the editor
    /// when it shows that file.
    pub(super) fn adopt_memory_write(&mut self, name: &str, next: String) {
        if let Some(i) = Self::mem_file_idx(name) {
            self.mem_cache_at[i] = config::memory_updated_at(name);
            self.mem_cache_body[i] = next.clone();
        }
        if self.mem_name == name {
            self.mem_body = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> std::path::PathBuf {
        let root = config::test_config_root(tag);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::env::set_var("GROKHUB_CONFIG", &root);
        config::ensure_memory_seeds();
        root
    }

    fn wait_learn(cabin: &mut Cabin) {
        let start = std::time::Instant::now();
        while cabin.learn_rx.is_some() && start.elapsed() < std::time::Duration::from_secs(5) {
            cabin.poll_learn();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(cabin.learn_rx.is_none(), "learn write did not finish");
    }

    /// A short session: a few facts, one correction, some ordinary chat.
    /// The three files go from templates to clean lines, a correction flips
    /// the old habit, and no chat sentence lands verbatim.
    #[test]
    fn a_session_teaches_user_soul_and_memory_and_never_stores_raw_chat() {
        let _g = config::hold_test_config();
        let root = temp_root("learn-loop-session");
        assert_eq!(
            config::read_memory("USER.md"),
            "# User\n\nWho you are. Edit this.\n"
        );
        assert_eq!(
            config::read_memory("SOUL.md"),
            "# Soul\n\nWho your assistant is. Edit this.\n"
        );
        let mut cabin = Cabin::quiet_for_test();
        cabin.new_thread(false);
        let session = [
            ("My name is Sam", vec![]),
            ("can you check the weather for tomorrow", vec![]),
            ("I work as a line cook", vec![]),
            ("always use emoji in replies", vec![]),
            ("fix the bash script in my notes folder and run it", vec![]),
            (
                "I play chess most nights",
                vec!["Unwinds with online chess after late shifts".to_string()],
            ),
            ("remember that the spare key is under the planter", vec![]),
            ("don't use emoji in replies", vec![]),
            ("call me Sammy", vec![]),
        ];
        for (msg, facts) in &session {
            cabin.learn_from_turn(msg, facts);
            wait_learn(&mut cabin);
        }
        let day = Cabin::local_day();
        assert_eq!(
            config::read_memory("USER.md"),
            "# User\n\n- Name: Sammy.\n- Works as a line cook.\n- Plays chess most nights.\n- Unwinds with online chess after late shifts.\n"
        );
        assert_eq!(
            config::read_memory("SOUL.md"),
            "# Soul\n\n- Don't use emoji in replies.\n"
        );
        assert_eq!(
            config::read_memory("MEMORY.md"),
            format!("# Memory\n\nLong-term notes.\n- {day}: the spare key is under the planter\n")
        );
        for (msg, _) in &session {
            for name in ["USER.md", "SOUL.md", "MEMORY.md"] {
                assert!(
                    !config::read_memory(name).contains(msg),
                    "{name} holds the raw message {msg:?}"
                );
            }
        }
        // Every write is logged by file and key, never with the chat text.
        let log = std::fs::read_to_string(root.join("memory/learn-log.jsonl")).unwrap();
        let rows: Vec<serde_json::Value> = log
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(rows.len(), 7, "{log}");
        assert_eq!(rows[0]["file"], "USER.md");
        assert_eq!(rows[0]["keys"], serde_json::json!(["user:name"]));
        assert_eq!(rows[0]["added"], 1);
        assert_eq!(rows[6]["file"], "USER.md");
        assert_eq!(rows[6]["replaced"], 1);
        assert_eq!(rows[5]["file"], "SOUL.md");
        assert_eq!(rows[5]["replaced"], 1);
        assert!(
            !log.contains("Sammy") && !log.contains("the spare key is under the planter"),
            "{log}"
        );
        std::env::remove_var("GROKHUB_CONFIG");
    }

    #[test]
    fn scratch_and_amr_chats_write_no_file() {
        let _g = config::hold_test_config();
        let _root = temp_root("learn-loop-scratch");
        let mut cabin = Cabin::quiet_for_test();
        cabin.new_thread(true);
        cabin.learn_from_turn("My name is Sam", &[]);
        assert!(cabin.learn_rx.is_none());
        cabin.new_thread(false);
        cabin.cfg.memory_backend = grokhub_core::amr::MemoryBackend::Amr;
        cabin.learn_from_turn("My name is Sam", &[]);
        assert!(cabin.learn_rx.is_none());
        assert_eq!(
            config::read_memory("USER.md"),
            "# User\n\nWho you are. Edit this.\n"
        );
        std::env::remove_var("GROKHUB_CONFIG");
    }

    /// A model `USER_FACT:` gets the same privacy and safety checks as a
    /// user message, so an email or a secret never lands in USER.md.
    #[test]
    fn model_facts_with_contact_details_or_secrets_are_dropped() {
        let facts = [
            "Email is sam@example.com".to_string(),
            "Api key is sk-abcdefghijklmnopqrstuv".to_string(),
            "Works night shifts at a diner".to_string(),
        ];
        let notes = notes_from(&[], &facts);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert_eq!(notes[0].text, "Works night shifts at a diner.");
    }

    /// A turn that ends while the last write is still running queues its
    /// notes instead of dropping them.
    #[test]
    fn back_to_back_turns_queue_and_both_land() {
        let _g = config::hold_test_config();
        let _root = temp_root("learn-loop-queue");
        let mut cabin = Cabin::quiet_for_test();
        cabin.new_thread(false);
        cabin.learn_from_turn("My name is Sam", &[]);
        cabin.learn_from_turn("I play chess most nights", &[]);
        assert!(cabin.learn_rx.is_some());
        assert_eq!(cabin.learn_pending.len(), 1);
        wait_learn(&mut cabin);
        assert!(cabin.learn_pending.is_empty());
        assert_eq!(
            config::read_memory("USER.md"),
            "# User\n\n- Name: Sam.\n- Plays chess most nights.\n"
        );
        std::env::remove_var("GROKHUB_CONFIG");
    }

    /// A reflect asked for mid-write runs after it, on top of its lines.
    #[test]
    fn reflect_waits_for_the_turn_write_and_keeps_its_line() {
        let _g = config::hold_test_config();
        let _root = temp_root("learn-loop-reflect");
        let mut cabin = Cabin::quiet_for_test();
        cabin.new_thread(false);
        cabin.mem_name = "USER.md".into();
        cabin.mem_body = config::read_memory("USER.md");
        cabin.learn_from_turn("I use a standing desk", &[]);
        cabin
            .live_mut()
            .push(("user".into(), "I play chess most nights".into()));
        cabin.run_reflect();
        assert!(cabin.reflect_rx.is_none() && cabin.reflect_after_learn);
        let start = std::time::Instant::now();
        while (cabin.learn_rx.is_some() || cabin.reflect_rx.is_some())
            && start.elapsed() < std::time::Duration::from_secs(5)
        {
            cabin.poll_learn();
            cabin.poll_reflect();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(cabin.status, "Reflected USER.md");
        assert_eq!(
            config::read_memory("USER.md"),
            "# User\n\n- Uses a standing desk.\n- Plays chess most nights.\n"
        );
        assert_eq!(cabin.mem_body, config::read_memory("USER.md"));
        std::env::remove_var("GROKHUB_CONFIG");
    }

    #[test]
    fn an_open_editor_body_is_the_base_and_gets_the_new_line() {
        let _g = config::hold_test_config();
        let _root = temp_root("learn-loop-open");
        let mut cabin = Cabin::quiet_for_test();
        cabin.new_thread(false);
        cabin.mem_name = "USER.md".into();
        cabin.mem_body = "# User\n\n- Plays chess.\n".into();
        cabin.learn_from_turn("I use a standing desk", &[]);
        wait_learn(&mut cabin);
        assert_eq!(
            cabin.mem_body,
            "# User\n\n- Plays chess.\n- Uses a standing desk.\n"
        );
        assert_eq!(config::read_memory("USER.md"), cabin.mem_body);
        std::env::remove_var("GROKHUB_CONFIG");
    }
}
