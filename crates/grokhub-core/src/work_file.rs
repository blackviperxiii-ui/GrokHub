//! `workboard.json` on disk, shared by every writer: the cabin, the shell and
//! the self MCP process. One lock file and one merge rule, so no writer loses
//! another's change. Plain local file work; no model is ever involved.
//!
//! - [`lock_board`] takes `<file>.lock` with an exclusive create and breaks a
//!   lock older than five seconds (a writer that died).
//! - [`update_board`] is a short read-change-write under that lock for a writer
//!   that keeps no copy of its own (the shell). It merges through
//!   [`merge_cards`](crate::work_item::merge_cards) like the cabin's saves.
//! - [`read_board`] and [`write_board`] are the one reader and writer of the
//!   file; `work_run`'s board edits go through them too.
//! - [`read_work_log_tail`] reads only the end of a card's progress log.

use std::fs;
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::work_item::{
    board_base, merge_cards, migrate_cards, parse_work_log, stamp_changes, work_log_file_name,
    WorkLogEvent,
};
use crate::workboard::BoardCard;

/// A lock older than this was left by a writer that died; it is broken.
pub const LOCK_STALE: Duration = Duration::from_secs(5);
/// Give up on the lock after this long.
pub const LOCK_WAIT: Duration = Duration::from_secs(7);
/// The cabin's cap for its JSON stores; a bigger file is not a board.
pub const BOARD_FILE_MAX: u64 = 32 * 1024 * 1024;

/// Held while a writer re-reads, merges and writes. Removed on drop.
pub struct BoardLock(PathBuf);

impl Drop for BoardLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// `<file>.lock` beside the board file.
pub fn lock_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".lock");
    path.with_file_name(name)
}

/// Take `<file>.lock` with an exclusive create. A lock older than
/// [`LOCK_STALE`] is stale and broken.
pub fn lock_board(path: &Path) -> Result<BoardLock, String> {
    let lock = lock_path(path);
    if let Some(dir) = lock.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let start = Instant::now();
    loop {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
        {
            Ok(mut f) => {
                let _ = write!(f, "{}", std::process::id());
                return Ok(BoardLock(lock));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let stale = fs::metadata(&lock)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|age| age >= LOCK_STALE);
                if stale {
                    let _ = fs::remove_file(&lock);
                    continue;
                }
                if start.elapsed() >= LOCK_WAIT {
                    return Err("workboard.json is locked by another writer".into());
                }
                std::thread::sleep(Duration::from_millis(15));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// The cards on disk, migrated. A missing or empty file is an empty board; a
/// file that cannot be read or parsed is an error, so no writer replaces it.
pub fn read_board(path: &Path) -> Result<Vec<BoardCard>, String> {
    let len = match fs::metadata(path) {
        Ok(m) => m.len(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.to_string()),
    };
    if len > BOARD_FILE_MAX {
        return Err("workboard.json is too large to be a board.".into());
    }
    let raw = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw.as_str());
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut cards: Vec<BoardCard> =
        serde_json::from_str(raw).map_err(|e| format!("workboard.json: {e}"))?;
    migrate_cards(&mut cards, crate::now_ms());
    Ok(cards)
}

/// The board file's mtime in ms, 0 when there is none. A cheap "did another
/// writer save" check for a view that polls.
pub fn board_stamp(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Under the board lock: read the board, let `change` edit it, and when it
/// returns `Ok`, bump the rev of every card it changed and write the merge.
/// `change` returning `Err`, or changing nothing, writes nothing. Returns
/// `change`'s value and the cards now on disk.
pub fn update_board<R>(
    path: &Path,
    change: impl FnOnce(&mut Vec<BoardCard>) -> Result<R, String>,
) -> Result<(R, Vec<BoardCard>), String> {
    let _lock = lock_board(path)?;
    let disk = read_board(path)?;
    let base = board_base(&disk);
    let mut mine = disk.clone();
    let out = change(&mut mine)?;
    stamp_changes(&mut mine, &base, crate::now_ms());
    // Under the lock the disk cannot move, so this keeps `mine` as written,
    // through the same per-card rule every other writer uses.
    let merged = merge_cards(&base, &mine, &disk);
    let same = |a: &[BoardCard], b: &[BoardCard]| {
        serde_json::to_value(a)
            .ok()
            .is_some_and(|a| serde_json::to_value(b).ok() == Some(a))
    };
    if !same(&merged, &disk) {
        write_board(path, &merged)?;
    }
    Ok((out, merged))
}

/// Write `cards` as the board file: pretty JSON through a private temp file,
/// fsync, then a rename. Call it under [`lock_board`].
pub fn write_board(path: &Path, cards: &[BoardCard]) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(cards).map_err(|e| e.to_string())?;
    write_private_atomic(path, raw.as_bytes())
}

#[cfg(unix)]
fn create_private(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let _ = fs::remove_file(path);
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
}

#[cfg(windows)]
fn create_private(path: &Path) -> std::io::Result<fs::File> {
    crate::win_acl::create_user_only(path)
}

#[cfg(not(any(unix, windows)))]
fn create_private(path: &Path) -> std::io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
}

/// A private temp file, fsync, then a rename, like the cabin's `atomic_write`.
fn write_private_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("the board needs a folder")?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("the board needs a file name")?;
    static NEXT_TMP: AtomicU64 = AtomicU64::new(0);
    let n = NEXT_TMP.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".{name}.board-{}-{n}.tmp", std::process::id()));
    let mut f = create_private(&tmp).map_err(|e| e.to_string())?;
    f.write_all(bytes).map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())?;
    drop(f);
    // Windows refuses a replace while another reader holds the file for a moment.
    let mut renamed = fs::rename(&tmp, path);
    for _ in 0..5 {
        if renamed.is_ok() || !cfg!(windows) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        renamed = fs::rename(&tmp, path);
    }
    renamed.map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

/// The newest `lines` events of a card's progress log, reading at most the last
/// `max_bytes` of the file. A cut first line is dropped. Empty when there is no
/// log or the id could step outside `dir`.
pub fn read_work_log_tail(dir: &Path, id: &str, max_bytes: u64, lines: usize) -> Vec<WorkLogEvent> {
    let Some(name) = work_log_file_name(id) else {
        return Vec::new();
    };
    let Ok(mut f) = fs::File::open(dir.join(name)) else {
        return Vec::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max_bytes);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return Vec::new();
    }
    let mut buf = Vec::new();
    if f.take(max_bytes).read_to_end(&mut buf).is_err() {
        return Vec::new();
    }
    let body = String::from_utf8_lossy(&buf);
    let body = if start > 0 {
        body.split_once('\n').map(|(_, rest)| rest).unwrap_or("")
    } else {
        &body
    };
    let mut events = parse_work_log(body);
    let cut = events.len().saturating_sub(lines);
    events.drain(..cut);
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work_item::append_work_log;
    use crate::workboard::BoardStatus;

    fn dir(label: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "grokhub-work-file-{label}-{}-{}",
            std::process::id(),
            crate::now_ms()
        ));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn card(id: &str) -> BoardCard {
        let mut c = BoardCard::new(id, "", "");
        c.id = id.into();
        c.status = BoardStatus::Todo;
        c
    }

    #[test]
    fn update_bumps_only_what_changed_and_releases_the_lock() {
        let d = dir("update");
        let path = d.join("workboard.json");
        update_board(&path, |cards| {
            cards.extend([card("a"), card("b")]);
            Ok(())
        })
        .unwrap();
        let before = read_board(&path).unwrap();
        let ((), after) = update_board(&path, |cards| {
            cards[1].status = BoardStatus::Done;
            Ok(())
        })
        .unwrap();
        assert_eq!(after[0].rev, before[0].rev, "a untouched");
        assert_eq!(after[1].rev, before[1].rev + 1, "b bumped");
        assert_eq!(read_board(&path).unwrap()[1].status, BoardStatus::Done);
        assert!(!lock_path(&path).exists());

        let stamp = board_stamp(&path);
        std::thread::sleep(Duration::from_millis(20));
        let ((), same) = update_board(&path, |_| Ok(())).unwrap();
        assert_eq!(same.len(), 2);
        assert_eq!(board_stamp(&path), stamp, "no change, no write");

        let err = update_board(&path, |cards| -> Result<(), String> {
            cards.clear();
            Err("no".into())
        });
        assert!(err.is_err());
        assert_eq!(
            read_board(&path).unwrap().len(),
            2,
            "a refused change writes nothing"
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn an_unreadable_board_is_never_replaced() {
        let d = dir("bad");
        let path = d.join("workboard.json");
        fs::write(&path, "{ not json").unwrap();
        assert!(update_board(&path, |_| Ok(())).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ not json");
        assert!(read_board(&d.join("missing.json")).unwrap().is_empty());
        assert_eq!(board_stamp(&d.join("missing.json")), 0);
        assert!(board_stamp(&path) > 0);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn log_tail_reads_only_the_end() {
        let d = dir("tail");
        for i in 0..30 {
            append_work_log(&d, "c1", i, "step", &format!("line {i}")).unwrap();
        }
        let all = read_work_log_tail(&d, "c1", 1 << 20, 20);
        assert_eq!(all.len(), 20);
        assert_eq!(all[0].text, "line 10");
        assert_eq!(all[19].text, "line 29");
        // A small window cuts the first line it lands in and keeps whole lines only.
        let few = read_work_log_tail(&d, "c1", 120, 20);
        assert!(!few.is_empty() && few.len() < 5);
        assert_eq!(few.last().unwrap().text, "line 29");
        assert!(read_work_log_tail(&d, "../x", 1024, 20).is_empty());
        assert!(read_work_log_tail(&d, "none", 1024, 20).is_empty());
        let _ = fs::remove_dir_all(&d);
    }
}
