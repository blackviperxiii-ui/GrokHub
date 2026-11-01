//! `workboard.json` shared by several writers (the cabin, the shell, the self MCP
//! process). Each card's progress log lives under [`work_dir`]; its file work is
//! in `grokhub_core::work_item`, and the lock in `grokhub_core::work_file`, so
//! every writer shares them.
//!
//! A save takes `workboard.json.lock`, re-reads the file, merges card by card
//! (the higher `rev` wins) and writes through `atomic_write`, so one writer never
//! loses another's change. No model is involved in any of it.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use grokhub_core::work_file::lock_board;
use grokhub_core::work_item::{
    board_base, merge_cards, migrate_cards, remove_work_log, stamp_changes, BoardBase,
};
use grokhub_core::{now_ms, BoardCard};

use crate::config::{atomic_write, peek_json, JSON_STORE_CAP};

/// How often the cabin looks for another writer's save.
const RELOAD_EVERY: Duration = Duration::from_secs(2);
/// How often the cabin runs the sweep.
const SWEEP_EVERY_MS: u64 = 60 * 60 * 1000;

/// One writer's view of a board file.
#[derive(Debug, Default)]
pub struct BoardStore {
    /// Each card as this writer last saw it, to tell its own edits and deletes
    /// from another writer's.
    base: BoardBase,
    /// The file's mtime after this writer's last save or load, when the file
    /// then held exactly this writer's cards.
    mtime: Option<SystemTime>,
    checked: Option<Instant>,
    swept_ms: u64,
}

fn file_mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn read_disk(path: &Path) -> Option<Vec<BoardCard>> {
    let mut cards: Vec<BoardCard> = peek_json(path, JSON_STORE_CAP)?;
    migrate_cards(&mut cards, now_ms());
    Some(cards)
}

fn same_cards(a: &[BoardCard], b: &[BoardCard]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.id == y.id && x.rev == y.rev)
}

impl BoardStore {
    /// Remember `cards` as what this writer loaded from `path`.
    pub fn loaded(path: &Path, cards: &[BoardCard]) -> Self {
        Self {
            base: board_base(cards),
            mtime: file_mtime(path),
            checked: None,
            swept_ms: 0,
        }
    }

    /// Save `mine` merged with whatever another writer saved since. Returns
    /// the cards now on disk.
    pub fn save_merged(
        &mut self,
        path: &Path,
        mine: &[BoardCard],
    ) -> Result<Vec<BoardCard>, String> {
        let now = now_ms();
        let mut mine = mine.to_vec();
        stamp_changes(&mut mine, &self.base, now);
        let _lock = lock_board(path)?;
        // A missing or unreadable file is not "another writer deleted everything":
        // write this writer's cards as they are.
        let merged = match read_disk(path) {
            Some(disk) => merge_cards(&self.base, &mine, &disk),
            None => mine.clone(),
        };
        let s = serde_json::to_string_pretty(&merged).map_err(|e| e.to_string())?;
        atomic_write(path, s.as_bytes())?;
        self.base = board_base(&mine);
        // Another writer's cards came in: leave the mtime stale so the cabin reloads.
        self.mtime = if same_cards(&merged, &mine) {
            file_mtime(path)
        } else {
            None
        };
        Ok(merged)
    }

    /// When the file changed since this writer last saw it, merge it into
    /// `cards` (this writer's unsaved edits win ties). True when `cards` changed.
    pub fn reload_if_changed(&mut self, path: &Path, cards: &mut Vec<BoardCard>) -> bool {
        let mtime = file_mtime(path);
        if mtime.is_some() && mtime == self.mtime {
            return false;
        }
        let Some(disk) = read_disk(path) else {
            self.mtime = mtime;
            return false;
        };
        let mut mine = cards.clone();
        stamp_changes(&mut mine, &self.base, now_ms());
        let merged = merge_cards(&self.base, &mine, &disk);
        self.base = board_base(&disk);
        self.mtime = mtime;
        if same_cards(&merged, cards) {
            return false;
        }
        *cards = merged;
        true
    }
}

fn stores() -> &'static Mutex<HashMap<PathBuf, BoardStore>> {
    static STORES: OnceLock<Mutex<HashMap<PathBuf, BoardStore>>> = OnceLock::new();
    STORES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Migrate freshly loaded cards and remember them as this process's view of `path`.
pub fn note_loaded(path: &Path, cards: &mut [BoardCard]) {
    migrate_cards(cards, now_ms());
    if let Ok(mut s) = stores().lock() {
        s.insert(path.to_path_buf(), BoardStore::loaded(path, cards));
    }
}

/// Merged save for this process's writer of `path`.
pub fn save(path: &Path, cards: &[BoardCard]) -> Result<(), String> {
    let mut s = stores()
        .lock()
        .map_err(|_| "workboard store poisoned".to_string())?;
    s.entry(path.to_path_buf())
        .or_default()
        .save_merged(path, cards)
        .map(|_| ())
}

/// At most every two seconds, pick up another writer's save. Only for a board
/// this process loaded, and skipped while a save holds the store.
pub fn reload_if_changed(path: &Path, cards: &mut Vec<BoardCard>) -> bool {
    // The UI thread never waits on a save that is waiting on the file lock.
    let Ok(mut s) = stores().try_lock() else {
        return false;
    };
    let Some(store) = s.get_mut(path) else {
        return false;
    };
    if store.checked.is_some_and(|t| t.elapsed() < RELOAD_EVERY) {
        return false;
    }
    store.checked = Some(Instant::now());
    store.reload_if_changed(path, cards)
}

/// True once an hour for a board this process loaded.
pub fn sweep_due(path: &Path, now: u64) -> bool {
    let Ok(mut s) = stores().try_lock() else {
        return false;
    };
    let Some(store) = s.get_mut(path) else {
        return false;
    };
    if store.swept_ms != 0 && now.saturating_sub(store.swept_ms) < SWEEP_EVERY_MS {
        return false;
    }
    store.swept_ms = now;
    true
}

/// `{config_dir}/work`, one `<card id>.jsonl` per card.
pub fn work_dir() -> PathBuf {
    crate::config::config_dir().join("work")
}

/// Delete a card and its progress log.
pub fn delete_card(cards: &mut Vec<BoardCard>, id: &str) -> bool {
    let gone = grokhub_core::delete_work_card(cards, id);
    if gone {
        remove_work_log(&work_dir(), id);
    }
    gone
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::work_file::lock_path;
    use grokhub_core::BoardStatus;

    fn root(label: &str) -> PathBuf {
        let r = crate::config::test_config_root(label);
        let _ = fs::remove_dir_all(&r);
        fs::create_dir_all(&r).unwrap();
        r
    }

    fn card(id: &str) -> BoardCard {
        let mut c = BoardCard::new(id, "", "");
        c.id = id.into();
        c.status = BoardStatus::Todo;
        c
    }

    fn load(path: &Path) -> (BoardStore, Vec<BoardCard>) {
        let cards = read_disk(path).unwrap_or_default();
        (BoardStore::loaded(path, &cards), cards)
    }

    #[test]
    fn two_writers_with_interleaved_saves_keep_both_edits() {
        let dir = root("board-two-writers");
        let path = dir.join("workboard.json");
        let mut seed = BoardStore::default();
        seed.save_merged(&path, &[card("one"), card("two"), card("gone")])
            .unwrap();

        let (mut cabin, mut a) = load(&path);
        let (mut shell, mut b) = load(&path);
        a[0].status = BoardStatus::InProgress;
        a.retain(|c| c.id != "gone");
        b[1].notes = "check the cable".into();
        b.push(card("three"));

        cabin.save_merged(&path, &a).unwrap();
        let on_disk = shell.save_merged(&path, &b).unwrap();
        let ids: Vec<&str> = on_disk.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["one", "two", "three"]);
        assert_eq!(on_disk[0].status, BoardStatus::InProgress);
        assert_eq!(on_disk[1].notes, "check the cable");

        // The cabin picks up the shell's save and keeps its own unsaved edit.
        a[1].title = "Two, renamed".into();
        assert!(cabin.reload_if_changed(&path, &mut a));
        let ids: Vec<&str> = a.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["one", "two", "three"]);
        assert_eq!(
            a[1].title, "Two, renamed",
            "unsaved edit survives the reload"
        );
        cabin.save_merged(&path, &a).unwrap();
        let (_, last) = load(&path);
        assert_eq!(last[1].title, "Two, renamed");
        assert_eq!(
            last[1].notes, "",
            "both edited two: the later save wins that card"
        );
        assert_eq!(last.len(), 3);
        assert!(!lock_path(&path).exists(), "the lock is released");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrent_writers_lose_nothing() {
        let dir = root("board-concurrent");
        let path = dir.join("workboard.json");
        let handles: Vec<_> = ["a", "b"]
            .into_iter()
            .map(|w| {
                let path = path.clone();
                std::thread::spawn(move || {
                    let (mut store, mut cards) = load(&path);
                    for i in 0..15 {
                        cards.push(card(&format!("{w}-{i}")));
                        cards = store.save_merged(&path, &cards).unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let (_, cards) = load(&path);
        assert_eq!(cards.len(), 30, "every card from both writers is on disk");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn stale_lock_is_broken_and_a_missing_file_is_not_a_delete() {
        let dir = root("board-lock");
        let path = dir.join("workboard.json");
        let lock = lock_path(&path);
        fs::write(&lock, "999").unwrap();
        let old = SystemTime::now() - Duration::from_secs(30);
        fs::File::options()
            .write(true)
            .open(&lock)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let mut store = BoardStore::default();
        store.save_merged(&path, &[card("one")]).unwrap();
        assert!(!lock.exists());

        // The file vanished (or cannot be read): this writer's cards stay.
        fs::remove_file(&path).unwrap();
        let on_disk = store.save_merged(&path, &[card("one")]).unwrap();
        assert_eq!(on_disk.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
