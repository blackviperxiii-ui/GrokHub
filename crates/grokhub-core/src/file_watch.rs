//! Desktop MCP file watchers (card 12, Desktop parity): `watch_path`,
//! `watch_events` and `unwatch_path`. Pure std polling, no OS notify API:
//! a watch keeps a snapshot of the path (each file's size and modified time),
//! and `events` compares a fresh one with it and keeps the fresh one. A
//! folder is walked [`WATCH_DEPTH`] levels down, at most [`WATCH_ENTRIES_MAX`]
//! entries. Nothing here writes a file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Most watches one desktop server keeps.
pub const WATCH_MAX: usize = 16;
/// Most entries one snapshot holds; the rest are not watched.
pub const WATCH_ENTRIES_MAX: usize = 5_000;
/// How many folder levels below the watched path are walked.
pub const WATCH_DEPTH: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileEventKind {
    Created,
    Modified,
    Deleted,
}

impl FileEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Modified => "modified",
            Self::Deleted => "deleted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEvent {
    pub kind: FileEventKind,
    pub path: PathBuf,
}

/// Size and modified time (ns); a folder is `None` so only its coming and
/// going count.
type Stamp = Option<(u64, u128)>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Snapshot {
    entries: BTreeMap<PathBuf, Stamp>,
    truncated: bool,
}

fn stamp(meta: &std::fs::Metadata) -> Stamp {
    if meta.is_dir() {
        return None;
    }
    let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
    Some((meta.len(), modified))
}

fn snapshot(root: &Path) -> Snapshot {
    let mut snap = Snapshot::default();
    let Ok(meta) = std::fs::symlink_metadata(root) else {
        return snap;
    };
    snap.entries.insert(root.to_path_buf(), stamp(&meta));
    if !meta.is_dir() {
        return snap;
    }
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut children: Vec<PathBuf> = read.flatten().map(|e| e.path()).collect();
        children.sort();
        for path in children {
            if snap.entries.len() >= WATCH_ENTRIES_MAX {
                snap.truncated = true;
                return snap;
            }
            // Symlinks are listed, never followed, so a loop can't trap the walk.
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.is_dir() && depth + 1 < WATCH_DEPTH {
                stack.push((path.clone(), depth + 1));
            }
            snap.entries.insert(path, stamp(&meta));
        }
    }
    snap
}

fn diff(before: &Snapshot, after: &Snapshot) -> Vec<FileEvent> {
    let mut out = Vec::new();
    for (path, now) in &after.entries {
        match before.entries.get(path) {
            None => out.push(FileEvent { kind: FileEventKind::Created, path: path.clone() }),
            Some(was) if was != now => out.push(FileEvent { kind: FileEventKind::Modified, path: path.clone() }),
            Some(_) => {}
        }
    }
    for path in before.entries.keys() {
        if !after.entries.contains_key(path) {
            out.push(FileEvent { kind: FileEventKind::Deleted, path: path.clone() });
        }
    }
    out
}

/// What a new watch looks at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watching {
    pub id: String,
    pub path: PathBuf,
    /// Entries in the first snapshot, the path itself included.
    pub entries: usize,
    /// More than [`WATCH_ENTRIES_MAX`]: the rest are not watched.
    pub truncated: bool,
}

/// The watches of one desktop server, by id (`w1`, `w2`, …).
#[derive(Debug, Default)]
pub struct FileWatches {
    next: u64,
    watches: BTreeMap<String, (PathBuf, Snapshot)>,
}

impl FileWatches {
    /// Start watching an absolute path that is there. Watching it again gives
    /// back the same id.
    pub fn watch(&mut self, path: &str) -> Result<Watching, String> {
        let path = PathBuf::from(path.trim());
        if !path.is_absolute() {
            return Err(format!("\"{}\" is not a full path.", path.display()));
        }
        if std::fs::symlink_metadata(&path).is_err() {
            return Err(format!("{} is not there.", path.display()));
        }
        if let Some((id, (_, snap))) = self.watches.iter().find(|(_, (p, _))| *p == path) {
            return Ok(Watching { id: id.clone(), path, entries: snap.entries.len(), truncated: snap.truncated });
        }
        if self.watches.len() >= WATCH_MAX {
            return Err(format!("Already watching {WATCH_MAX} paths. Stop one with unwatch_path first."));
        }
        self.next += 1;
        let id = format!("w{}", self.next);
        let snap = snapshot(&path);
        let watching = Watching { id: id.clone(), path: path.clone(), entries: snap.entries.len(), truncated: snap.truncated };
        self.watches.insert(id, (path, snap));
        Ok(watching)
    }

    /// What was created, modified or deleted since the last look, oldest
    /// snapshot first. Each call starts the next interval.
    pub fn events(&mut self, id: &str) -> Result<Vec<FileEvent>, String> {
        let (path, snap) = self.watches.get_mut(id.trim()).ok_or_else(|| format!("No watch \"{}\".", id.trim()))?;
        let fresh = snapshot(path);
        let events = diff(snap, &fresh);
        *snap = fresh;
        Ok(events)
    }

    /// Stop a watch; returns the path it watched.
    pub fn unwatch(&mut self, id: &str) -> Result<PathBuf, String> {
        self.watches.remove(id.trim()).map(|(p, _)| p).ok_or_else(|| format!("No watch \"{}\".", id.trim()))
    }

    pub fn len(&self) -> usize {
        self.watches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.watches.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(label: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("grokhub-watch-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn kinds(events: &[FileEvent], root: &Path) -> Vec<(String, String)> {
        events
            .iter()
            .map(|e| (e.kind.as_str().to_string(), e.path.strip_prefix(root).unwrap().display().to_string()))
            .collect()
    }

    #[test]
    fn a_watch_sees_create_modify_and_delete_in_a_folder() {
        let root = tmp("crud");
        std::fs::write(root.join("keep.txt"), "a").unwrap();
        std::fs::write(root.join("gone.txt"), "b").unwrap();
        let mut w = FileWatches::default();
        let watching = w.watch(root.to_str().unwrap()).unwrap();
        assert_eq!((watching.id.as_str(), watching.entries, watching.truncated), ("w1", 3, false));
        assert_eq!(w.events("w1").unwrap(), vec![], "nothing changed yet");

        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub").join("new.txt"), "c").unwrap();
        std::fs::write(root.join("keep.txt"), "a longer body").unwrap();
        std::fs::remove_file(root.join("gone.txt")).unwrap();
        let got = kinds(&w.events("w1").unwrap(), &root);
        assert_eq!(
            got,
            vec![
                ("modified".to_string(), "keep.txt".to_string()),
                ("created".to_string(), "sub".to_string()),
                ("created".to_string(), "sub/new.txt".replace('/', std::path::MAIN_SEPARATOR_STR)),
                ("deleted".to_string(), "gone.txt".to_string()),
            ]
        );
        assert_eq!(w.events("w1").unwrap(), vec![], "each change is reported once");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_watched_file_and_its_deletion_and_unwatch() {
        let root = tmp("file");
        let file = root.join("notes.md");
        std::fs::write(&file, "x").unwrap();
        let mut w = FileWatches::default();
        assert_eq!(w.watch(file.to_str().unwrap()).unwrap().entries, 1);
        assert_eq!(w.watch(file.to_str().unwrap()).unwrap().id, "w1", "the same path keeps its id");
        std::fs::remove_file(&file).unwrap();
        assert_eq!(w.events("w1").unwrap(), vec![FileEvent { kind: FileEventKind::Deleted, path: file.clone() }]);
        assert_eq!(w.unwatch("w1").unwrap(), file);
        assert_eq!(w.events("w1"), Err("No watch \"w1\".".into()));
        assert!(w.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_watch_needs_a_full_path_that_is_there_and_stops_at_the_cap() {
        let root = tmp("cap");
        let mut w = FileWatches::default();
        assert_eq!(w.watch("notes.md").unwrap_err(), "\"notes.md\" is not a full path.");
        let missing = root.join("nope");
        assert_eq!(w.watch(missing.to_str().unwrap()).unwrap_err(), format!("{} is not there.", missing.display()));
        for i in 0..WATCH_MAX {
            let p = root.join(format!("f{i}"));
            std::fs::write(&p, "").unwrap();
            w.watch(p.to_str().unwrap()).unwrap();
        }
        let extra = root.join("extra");
        std::fs::write(&extra, "").unwrap();
        assert_eq!(w.watch(extra.to_str().unwrap()).unwrap_err(), "Already watching 16 paths. Stop one with unwatch_path first.");
        assert_eq!(w.len(), 16);
        let _ = std::fs::remove_dir_all(&root);
    }
}
