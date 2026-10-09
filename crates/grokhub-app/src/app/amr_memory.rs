//! AMR M1–M2 in the cabin. Only runs when `app.json` sets
//! `"memory_backend": "amr"`; legacy never calls into this file and never
//! creates `amr/`.
//!
//! - New remembers (reflect, chat insights, `/memory note`, `/remember`, the
//!   MEMORY.md Save, native `/remember`) write one node each through
//!   `AmrStore::remember` instead of a MEMORY.md / USER.md / learning.json line.
//! - `/recall` and History read the store and the legacy files together.
//! - `/forget <topic>` tombstones the matching nodes.
//! - The first AMR read or write imports LearningState insights and the
//!   durable chip fields once (deterministic ids, so a rerun adds nothing).
//! - M3: once a night, after the review, the dream tidies the store
//!   (`AmrStore::dream_once`). `/memory dream` shows the latest report.
//! - M4 (Spike-3a): a turn with harness spans leaves one `trail` node
//!   (`harness_turn_end` → `write_turn_trail`); a fact learned from that turn
//!   links to it with `learned_from_span`.

use super::*;

use grokhub_core::amr::{AmrError, AmrStore, LineWrite, Remembered};

/// `{config}/amr` with the learned-tier key for private nodes.
pub(super) fn amr_store_at(dir: &std::path::Path) -> AmrStore {
    let vault = grokhub_agent::harness::LearnedVault::new(dir);
    AmrStore::at(dir.join("amr")).with_sealer(std::sync::Arc::new(vault))
}

/// One line for the status bar after a remember.
pub(super) fn remembered_status(result: &Result<Remembered, AmrError>) -> String {
    match result {
        Ok(Remembered::New(_)) => "Remembered in the memory repo".into(),
        Ok(Remembered::Known(_)) => "Already in the memory repo".into(),
        Err(AmrError::Scratch) => "Scratch — no memory writes".into(),
        Err(AmrError::Paused(why)) => format!("Learning paused: {why}. Nothing was written."),
        Err(err) => format!("Memory repo: {err}"),
    }
}

/// Split an edited MEMORY.md into what stays in the file and the lines that
/// are new against `disk` (trimmed, case-insensitive, like reflect's dedupe).
pub(super) fn split_new_lines(disk: &str, edited: &str) -> (String, Vec<String>) {
    let known: std::collections::HashSet<String> = disk
        .lines()
        .map(|l| l.trim().to_lowercase())
        .filter(|l| !l.is_empty())
        .collect();
    let mut kept = String::new();
    let mut added: Vec<String> = Vec::new();
    for line in edited.lines() {
        let key = line.trim().to_lowercase();
        if key.is_empty() || known.contains(&key) {
            kept.push_str(line);
            kept.push('\n');
        } else if !added.iter().any(|a| a.to_lowercase() == key) {
            added.push(line.trim().to_string());
        }
    }
    if !edited.ends_with('\n') && kept.ends_with('\n') && !edited.is_empty() {
        kept.pop();
    }
    (kept, added)
}

/// History rows from the store for `q`, as `(amr:<id>, amr:<id>: line)`.
/// Tombstoned and locked nodes are not among them.
pub(super) fn amr_history_hits(store: &AmrStore, q: &str) -> Vec<(String, String)> {
    store
        .recall(q)
        .into_iter()
        .map(|hit| (format!("amr:{}", hit.id), hit.display()))
        .collect()
}

/// `/recall` text with the AMR lines first, then legacy lines whose text is
/// not already there. `amr:<id>: x` and `MEMORY.md:3: x` count as the same.
pub(super) fn dual_read_hits(amr: Vec<String>, legacy: Vec<String>) -> Vec<String> {
    fn text_of(line: &str) -> String {
        let mut parts = line.splitn(3, ':');
        match (parts.next(), parts.next(), parts.next()) {
            (Some(_), Some(_), Some(text)) => text.trim().to_lowercase(),
            _ => line.trim().to_lowercase(),
        }
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for line in amr.into_iter().chain(legacy) {
        if seen.insert(text_of(&line)) {
            out.push(line);
        }
    }
    out
}

/// `/memory dream`: the newest `amr/dreams/<date>.md`, or "No dream yet".
/// Reads only, so legacy never creates `amr/`.
pub(super) fn memory_dream_text(config_dir: &std::path::Path) -> String {
    match grokhub_core::amr::latest_dream(&config_dir.join("amr")) {
        Some((_, text)) => text,
        None => "No dream yet. GrokHub dreams once a night after the review, in memory repo mode.".into(),
    }
}

impl Cabin {
    pub(super) fn amr_on(&self) -> bool {
        self.cfg.memory_backend == grokhub_core::amr::MemoryBackend::Amr
    }

    /// Is the chat this job writes to a scratch chat?
    pub(super) fn job_scratch(&self) -> bool {
        match self.chat_job_thread.as_deref() {
            Some(id) => self
                .threads
                .iter()
                .find(|t| t.id == id)
                .map(|t| t.scratch)
                .unwrap_or_else(|| self.scratch()),
            None => self.scratch(),
        }
    }

    /// The store for an AMR read or write. The first call outside scratch runs
    /// the one-time import and, when it brought anything in, posts one chat line.
    pub(super) fn amr_store(&mut self, scratch: bool) -> AmrStore {
        let mut store = amr_store_at(&config::config_dir());
        if !scratch && !self.amr_imported {
            // Only a store that opened counts as imported; a failed init tries again.
            if store.init().is_ok() {
                self.amr_imported = true;
                let report = grokhub_core::amr::import_legacy(
                    &store,
                    &self.learning.insights,
                    &self.chip_memory,
                    now_ms(),
                );
                if report.imported() > 0 {
                    let _ = grokhub_core::amr::write_import_report(&store, &report);
                    self.live_mut()
                        .push(("assistant".into(), mark_slash_result(&report.status_line())));
                    self.persist();
                }
            }
        }
        store.set_scratch(scratch);
        store
    }

    /// Remember one line the person asked for, now. Lifts a tombstone on the
    /// same line. Held secrets are masked first; the store redacts the rest.
    pub(super) fn amr_remember_now(&mut self, text: &str, tag: &str) -> String {
        let scratch = self.scratch();
        let text = redact_held_secrets(text.trim(), &self.secret_hold);
        let source = format!("chat:{}", self.visible_thread_id());
        let store = self.amr_store(scratch);
        let result = grokhub_core::amr::remember_line(
            &store,
            &LineWrite {
                text: &text,
                source: &source,
                tags: vec![tag.to_string()],
                confidence: 1.0,
                revive: true,
            },
            now_ms(),
        );
        remembered_status(&result)
    }

    /// Learned facts from a turn or a reflect, off the UI thread. Returns the
    /// receiver for the `+ line` of each new node. Scratch writes nothing.
    pub(super) fn amr_remember_facts(
        &mut self,
        facts: &[String],
        tag: &'static str,
    ) -> Option<mpsc::Receiver<Vec<String>>> {
        let scratch = self.job_scratch();
        if scratch {
            return None;
        }
        let thread = self
            .chat_job_thread
            .clone()
            .unwrap_or_else(|| self.visible_thread_id());
        let facts: Vec<String> = facts
            .iter()
            .map(|f| redact_held_secrets(f.trim(), &self.secret_hold))
            .filter(|f| !f.is_empty() && is_plain_text(f))
            .collect();
        let store = self.amr_store(scratch);
        // The turn of the chat the reply ran in, not of the chat on screen.
        let turn = if thread == self.visible_thread_id() {
            self.turn_no()
        } else {
            self.threads
                .iter()
                .find(|t| t.id == thread)
                .map_or(0, |t| t.messages.iter().filter(|(r, _)| r == "user").count() as u32)
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let source = format!("chat:{thread}");
            let mut added = Vec::new();
            for fact in &facts {
                let result = grokhub_core::amr::remember_line(
                    &store,
                    &LineWrite {
                        text: fact,
                        source: &source,
                        tags: vec![tag.to_string()],
                        confidence: 0.7,
                        revive: false,
                    },
                    now_ms(),
                );
                if let Ok(Remembered::New(id)) = &result {
                    // M4: a fact learned after this turn's trail points back at it.
                    let _ = grokhub_agent::harness::link_learned(&store, id, &thread, turn);
                    added.push(format!("+ {fact}"));
                }
            }
            let _ = tx.send(added);
        });
        Some(rx)
    }

    /// AMR reflect: one node per new fact, no MEMORY.md or USER.md line. The
    /// edit sent back has an empty `next`, which `poll_reflect` reads as AMR.
    pub(super) fn run_reflect_amr(&mut self, facts: &[String]) {
        let Some(added) = self.amr_remember_facts(facts, "reflect") else {
            self.status = "Scratch — no reflect".into();
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.reflect_rx = Some(rx);
        self.status = "Reflecting…".into();
        std::thread::spawn(move || {
            let diff = added.recv().unwrap_or_default().join("\n");
            let _ = tx.send((
                MemoryEdit {
                    next: String::new(),
                    diff,
                },
                None,
            ));
        });
    }

    /// The MEMORY.md Save button in AMR mode: lines new against the file become
    /// nodes and leave the editor; edits to lines already there are saved to
    /// the file as before. A private line that can't be sealed is not saved
    /// anywhere (fail closed) and the status says so.
    pub(super) fn save_memory_amr(&mut self) {
        let disk = config::read_memory("MEMORY.md");
        let (mut kept, added) = split_new_lines(&disk, &self.mem_body);
        let mut saved = 0usize;
        let mut paused = 0usize;
        let mut last = String::new();
        for line in &added {
            let status = self.amr_remember_now(line, "memory-save");
            if status.starts_with("Learning paused") {
                paused += 1;
            } else if status.starts_with("Memory repo:") {
                // The store refused it: the line stays in MEMORY.md, not lost.
                if !kept.is_empty() && !kept.ends_with('\n') {
                    kept.push('\n');
                }
                kept.push_str(line);
                kept.push('\n');
                last = status;
            } else {
                saved += 1;
            }
        }
        if kept != disk {
            let body = kept.clone();
            std::thread::spawn(move || {
                let _ = config::write_memory("MEMORY.md", &body);
            });
        }
        if let Some(i) = Self::mem_file_idx("MEMORY.md") {
            self.mem_cache_body[i] = kept.clone();
        }
        self.mem_body = kept;
        self.status = if !last.is_empty() {
            last
        } else if paused > 0 {
            format!("Saved {saved} to the memory repo. {paused} private paused (key locked), not saved.")
        } else if saved > 0 {
            format!("Saved {saved} to the memory repo")
        } else {
            "Wrote MEMORY.md".into()
        };
    }

    /// `/forget <topic>` in AMR mode: tombstone the matching nodes. MEMORY.md
    /// still drops the topic too, so the dual read does not bring it back.
    pub(super) fn forget_amr(&mut self, topic: &str) -> String {
        let store = self.amr_store(false);
        match store.forget_matching(topic) {
            Ok(report) => {
                let n = report.forgotten.len();
                let notes = if n == 1 {
                    "1 note".to_string()
                } else {
                    format!("{n} notes")
                };
                let mut line = format!("Forgot {topic}: {notes} in the memory repo");
                if report.locked > 0 {
                    line.push_str(&format!(", {} private not checked", report.locked));
                }
                line
            }
            Err(err) => format!("Memory repo: {err}"),
        }
    }

    /// Start tonight's dream off the UI thread when it is due: memory repo
    /// mode, past the review hour, the review not in flight, and no turn or
    /// card waiting on the user. Halt skips the night. A report already on
    /// disk for `today` (an earlier session) counts as done.
    pub(super) fn dream_tonight(
        &mut self,
        today: &str,
        hour: u32,
        now: u64,
    ) -> Option<std::thread::JoinHandle<()>> {
        if !self.amr_on()
            || hour < self.dream_hour()
            || self.dream_day.as_deref() == Some(today)
            || self.review_busy
            || self.heartbeat_busy()
        {
            return None;
        }
        self.dream_day = Some(today.to_string());
        if self.heartbeat_halted(now) {
            return None;
        }
        let dir = config::config_dir();
        if grokhub_core::amr::dream_report_path(&amr_store_at(&dir), today).is_file() {
            return None;
        }
        let store = self.amr_store(false);
        let today = today.to_string();
        Some(std::thread::spawn(move || {
            let opts = grokhub_core::amr::DreamOpts {
                date: Some(today),
                user_md: Some(config::read_memory("USER.md")),
                ..Default::default()
            };
            let _ = store.dream_once(now, &opts);
        }))
    }
}

#[cfg(test)]
mod amr_tests {
    use super::*;

    #[test]
    fn split_new_lines_keeps_known_lines_and_lifts_new_ones() {
        let disk = "harbor light\nkeel stays white\n";
        let edited = "harbor light\nprefer nvim\nKEEL STAYS WHITE\n\nprefer nvim\n";
        let (kept, added) = split_new_lines(disk, edited);
        assert_eq!(kept, "harbor light\nKEEL STAYS WHITE\n\n");
        assert_eq!(added, vec!["prefer nvim".to_string()]);
        let (kept, added) = split_new_lines("a line\n", "a line");
        assert_eq!(kept, "a line");
        assert!(added.is_empty());
        let (kept, added) = split_new_lines("", "");
        assert_eq!(kept, "");
        assert!(added.is_empty());
    }

    #[test]
    fn dual_read_drops_a_legacy_line_the_store_already_has() {
        let out = dual_read_hits(
            vec!["amr:mem-1: Harbor light".into(), "amr:mem-2: pier".into()],
            vec![
                "MEMORY.md:1: harbor light".into(),
                "MEMORY.md:2: old pier note".into(),
                "1 private note not searched. Locked.".into(),
            ],
        );
        assert_eq!(
            out,
            vec![
                "amr:mem-1: Harbor light".to_string(),
                "amr:mem-2: pier".to_string(),
                "MEMORY.md:2: old pier note".to_string(),
                "1 private note not searched. Locked.".to_string(),
            ]
        );
    }

    #[test]
    fn remembered_status_names_each_outcome() {
        assert_eq!(
            remembered_status(&Ok(Remembered::New("mem-1".into()))),
            "Remembered in the memory repo"
        );
        assert_eq!(
            remembered_status(&Ok(Remembered::Known("mem-1".into()))),
            "Already in the memory repo"
        );
        assert_eq!(
            remembered_status(&Err(AmrError::Scratch)),
            "Scratch — no memory writes"
        );
        assert_eq!(
            remembered_status(&Err(AmrError::Paused("Private data is locked".into()))),
            "Learning paused: Private data is locked. Nothing was written."
        );
    }
}
