//! Draft prep (LO-46): start recall while the message is still being typed.
//!
//! The cabin calls [`prepare`] on a worker thread once typing pauses. It reads
//! what memory recall needs for the draft and keeps it in one slot. At Enter,
//! recall takes the slot when the text and every memory input still match,
//! and reads afresh otherwise, so a hit gives the same pack as a fresh read.
//!
//! Local reads only: nothing here calls a model or sends anything. It never
//! asks the OS keyring (sealed nodes open only with a key already in memory)
//! and never prepares a library or installed-app ask, which reads the sealed
//! library index.
//!
//! Design note: memory recall is the first job. Later jobs (an intent guess,
//! a tool shortlist) would join [`Prepared`] under the same key.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::memory::Gathered;

/// Shortest draft worth preparing, in characters after trimming.
pub const MIN_CHARS: usize = 3;

/// A prepared read older than this is not used.
const MAX_AGE: Duration = Duration::from_secs(120);

/// What a prepared read was made for. Any difference at Enter is a miss.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Key {
    config_dir: PathBuf,
    workspace: PathBuf,
    text: String,
    stamps: Vec<String>,
}

impl Key {
    fn now(workspace: &Path, text: &str) -> Self {
        Self {
            config_dir: crate::perm::config_dir(),
            workspace: workspace.to_path_buf(),
            text: text.trim().to_string(),
            stamps: crate::memory::recall_stamps(workspace),
        }
    }
}

struct Prepared {
    key: Key,
    at: Instant,
    recall: Gathered,
}

static SLOT: Mutex<Option<Prepared>> = Mutex::new(None);

/// True when a draft is worth preparing: 3+ characters, not a slash
/// command, and not an ask whose recall could read the sealed library.
pub fn eligible(text: &str) -> bool {
    let text = text.trim();
    text.chars().count() >= MIN_CHARS
        && !text.starts_with('/')
        && !crate::memory::may_read_sealed(text)
}

/// Read recall for `text` in `workspace` and keep it in the slot, replacing
/// any earlier draft. Returns whether the slot was filled. Stamps are read
/// first, so a memory change during the read makes the slot a miss.
pub fn prepare(workspace: &Path, text: &str) -> bool {
    if !eligible(text) {
        return false;
    }
    let _lap = crate::timing::lap("draft:prepare");
    let key = Key::now(workspace, text);
    let vault = Arc::new(crate::harness::InMemoryVault::new(&key.config_dir));
    let recall = crate::memory::gather_ahead(workspace, &key.text, vault.clone());
    if vault.refused() {
        return false;
    }
    *SLOT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Prepared {
        key,
        at: Instant::now(),
        recall,
    });
    true
}

/// The prepared recall for `text`, once, when the text, workspace, config
/// dir and memory inputs all still match. `None` on any mismatch.
pub(crate) fn take(workspace: &Path, text: &str) -> Option<Gathered> {
    let prepared = SLOT.lock().unwrap_or_else(|e| e.into_inner()).take()?;
    let fresh = prepared.at.elapsed() < MAX_AGE;
    let hit = fresh && prepared.key == Key::now(workspace, text);
    crate::timing::record(if hit { "draft:hit" } else { "draft:miss" }, Duration::ZERO);
    hit.then_some(prepared.recall)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perm::ConfigGuard;
    use std::fs;

    fn pair(tag: &str) -> (PathBuf, PathBuf, ConfigGuard) {
        let root = std::env::temp_dir().join(format!(
            "gh-draft-prep-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let cfg = root.join("cfg");
        let ws = root.join("ws");
        fs::create_dir_all(&cfg).unwrap();
        fs::create_dir_all(&ws).unwrap();
        fs::write(cfg.join("app.json"), "{}\n").unwrap();
        fs::write(
            crate::memory::workspace_memory_path(&ws),
            "The save folder is D:/Loadout/saves\n",
        )
        .unwrap();
        let guard = ConfigGuard::set(&cfg);
        *SLOT.lock().unwrap_or_else(|e| e.into_inner()) = None;
        (cfg, ws, guard)
    }

    const ASK: &str = "where is the save folder";

    #[test]
    fn a_prepared_read_gives_the_same_pack_as_one_at_enter() {
        let (_cfg, ws, _guard) = pair("same");
        let at_enter = crate::memory::first_turn_injection(&ws, ASK).unwrap();
        assert!(prepare(&ws, ASK));
        let taken = take(&ws, ASK).unwrap();
        assert_eq!(taken.amr, Vec::<String>::new());
        assert_eq!(taken.scope, Vec::<String>::new());
        let bodies: Vec<String> = taken
            .legacy
            .unwrap()
            .into_iter()
            .map(|hit| hit.body)
            .collect();
        assert_eq!(
            bodies,
            vec!["The save folder is D:/Loadout/saves".to_string()]
        );
        assert!(take(&ws, ASK).is_none(), "a slot is used once");

        assert!(prepare(&ws, ASK));
        let from_slot = crate::memory::first_turn_injection(&ws, ASK).unwrap();
        assert!(take(&ws, ASK).is_none(), "recall took the slot");
        assert_eq!(from_slot, at_enter);
        assert!(from_slot.contains("The save folder is D:/Loadout/saves"));
    }

    #[test]
    fn an_edited_draft_or_a_new_note_is_a_miss() {
        let (_cfg, ws, _guard) = pair("miss");
        assert!(prepare(&ws, ASK));
        assert!(take(&ws, "where is the save folder now").is_none());

        assert!(prepare(&ws, ASK));
        crate::memory::remember(&ws, "the screenshots folder is D:/Loadout/shots").unwrap();
        assert!(take(&ws, ASK).is_none(), "remember() changed MEMORY.md");

        assert!(prepare(&ws, ASK));
        let other = ws.parent().unwrap().join("ws2");
        fs::create_dir_all(&other).unwrap();
        assert!(take(&other, ASK).is_none(), "another workspace");
    }

    #[test]
    fn a_followup_packs_the_prepared_read_minus_lines_already_sent() {
        let (_cfg, ws, _guard) = pair("followup");
        fs::write(
            crate::memory::workspace_memory_path(&ws),
            "The save folder is D:/Loadout/saves\n\nThe save backups go to E:/saves-old\n",
        )
        .unwrap();
        let already = vec!["The save folder is D:/Loadout/saves".to_string()];
        let at_enter = crate::memory::followup_injection(&ws, ASK, &already).unwrap();
        assert!(prepare(&ws, ASK));
        let from_slot = crate::memory::followup_injection(&ws, ASK, &already).unwrap();
        assert_eq!(from_slot, at_enter);
        assert!(from_slot.contains("E:/saves-old"), "{from_slot}");
        assert!(!from_slot.contains("D:/Loadout/saves"), "{from_slot}");
    }

    #[test]
    fn library_asks_and_short_or_slash_drafts_are_never_prepared() {
        let (_cfg, ws, _guard) = pair("library");
        for text in [
            "what games do I have installed on steam",
            "do I have Factorio",
            "is the app installed",
            "ab",
            "   ",
            "/help me",
        ] {
            assert!(!eligible(text), "{text}");
            assert!(!prepare(&ws, text), "{text}");
            assert!(take(&ws, text).is_none(), "{text}");
        }
        assert!(eligible("fix the parser test"));
    }

    /// Counts keyring reads, so a test can show prep never asks the keyring.
    #[derive(Default)]
    struct Counting {
        inner: crate::harness::MemoryKeyStore,
        loads: std::sync::atomic::AtomicUsize,
    }

    impl crate::harness::KeyStore for Counting {
        fn load(&self) -> Result<Option<zeroize::Zeroizing<String>>, String> {
            self.loads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inner.load()
        }
        fn create(&self, key_b64: &str) -> Result<(), String> {
            self.inner.create(key_b64)
        }
        fn may_block(&self) -> bool {
            false
        }
    }

    #[test]
    fn sealed_memory_opens_only_with_a_key_already_in_memory() {
        use grokhub_core::amr::{remember_line, LineWrite};
        use std::sync::atomic::Ordering;
        let (cfg, ws, _guard) = pair("sealed");
        let keys = Arc::new(Counting::default());
        crate::harness::use_key_store_for(&cfg, keys.clone());
        fs::write(cfg.join("app.json"), r#"{"memory_backend":"amr"}"#).unwrap();
        let line = LineWrite {
            text: "the save folder share is mailed to user@example.com",
            source: "user",
            tags: Vec::new(),
            confidence: 0.9,
            revive: false,
        };
        remember_line(&crate::memory::amr_store(), &line, 1).unwrap();
        let sealed = fs::read_dir(cfg.join("amr").join("nodes"))
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "sealed"))
            .count();
        assert_eq!(sealed, 1, "the personal line is sealed at rest");

        // The write left the key in memory: prep opens the node by itself.
        let at_enter = crate::memory::first_turn_injection(&ws, ASK).unwrap();
        assert!(at_enter.contains("save folder share"), "{at_enter}");
        let loads = keys.loads.load(Ordering::SeqCst);
        assert!(prepare(&ws, ASK));
        assert_eq!(keys.loads.load(Ordering::SeqCst), loads, "no keyring read");
        let from_slot = crate::memory::first_turn_injection(&ws, ASK).unwrap();
        assert_eq!(from_slot, at_enter);

        // With the cached key dropped, prep keeps nothing and asks nothing.
        crate::harness::recheck_keyring(&cfg);
        let loads = keys.loads.load(Ordering::SeqCst);
        assert!(!prepare(&ws, ASK));
        assert_eq!(keys.loads.load(Ordering::SeqCst), loads, "no keyring read");
        assert!(take(&ws, ASK).is_none());
    }
}
