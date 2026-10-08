//! Spike-9 restore points: taken before a fix's first step.
//!
//! Every fix backs up each file its steps touch into the rewind store
//! (`{config_dir}/rewind/repair-<id>/`, see `grokhub_core::rewind`), so Undo
//! puts them back byte for byte. On top of that, a system snapshot when one is
//! there: snapper, Timeshift or a btrfs snapshot on Linux (found by binary),
//! a System Restore checkpoint on Windows. The snapshot is a step like any
//! other: it runs through Grok Build under the pill, and its admin prompt is
//! the user's to answer. Undo can't roll a snapshot back for you; it shows the
//! plain steps with the snapshot's name.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::harness::{content_hash, UndoAsk};

use super::fix::elevate;
use super::probes::Os;

/// A system snapshot tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotBackend {
    Snapper,
    Timeshift,
    Btrfs,
    SystemRestore,
}

impl SnapshotBackend {
    pub fn key(self) -> &'static str {
        match self {
            Self::Snapper => "snapper",
            Self::Timeshift => "timeshift",
            Self::Btrfs => "btrfs",
            Self::SystemRestore => "system_restore",
        }
    }
}

/// Snapshot tools on this computer, best first. Windows always has System
/// Restore (it may be switched off; the checkpoint step then fails and the
/// file backup still holds).
pub fn detect_backends(os: Os, has_bin: &dyn Fn(&str) -> bool) -> Vec<SnapshotBackend> {
    match os {
        Os::Windows => vec![SnapshotBackend::SystemRestore],
        Os::Linux => [("snapper", SnapshotBackend::Snapper), ("timeshift", SnapshotBackend::Timeshift), ("btrfs", SnapshotBackend::Btrfs)]
            .into_iter()
            .filter(|(bin, _)| has_bin(bin))
            .map(|(_, b)| b)
            .collect(),
    }
}

/// What every restore point can say about itself.
pub trait RestorePoint {
    /// `files`, `snapper`, `timeshift`, `btrfs`, `system_restore`.
    fn kind(&self) -> &'static str;
    /// What the restore-point span records.
    fn restore_ref(&self) -> String;
    /// The step that creates it through Grok Build, if it is a system snapshot.
    fn create_step(&self) -> Option<String>;
    /// Plain words for going back to it by hand, if Undo can't do it alone.
    fn undo_guidance(&self) -> Option<String>;
}

/// A system snapshot named `label`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub backend: SnapshotBackend,
    pub label: String,
}

impl RestorePoint for Snapshot {
    fn kind(&self) -> &'static str {
        self.backend.key()
    }

    fn restore_ref(&self) -> String {
        format!("{}:{}", self.backend.key(), self.label)
    }

    fn create_step(&self) -> Option<String> {
        let l = &self.label;
        Some(match self.backend {
            SnapshotBackend::Snapper => elevate(Os::Linux, &format!("snapper create --description {l}")),
            SnapshotBackend::Timeshift => elevate(Os::Linux, &format!("timeshift --create --comments {l}")),
            SnapshotBackend::Btrfs => elevate(Os::Linux, &format!("btrfs subvolume snapshot -r / /.snapshots/{l}")),
            SnapshotBackend::SystemRestore => {
                elevate(Os::Windows, &format!("Checkpoint-Computer -Description {l} -RestorePointType MODIFY_SETTINGS"))
            }
        })
    }

    fn undo_guidance(&self) -> Option<String> {
        let l = &self.label;
        Some(match self.backend {
            SnapshotBackend::Snapper => format!("I also saved a system snapshot called \"{l}\". To go back to it, open a terminal, run sudo snapper list, find {l}, run sudo snapper rollback with its number, then restart."),
            SnapshotBackend::Timeshift => format!("I also saved a system snapshot called \"{l}\". To go back to it, open Timeshift, pick {l} and choose Restore."),
            SnapshotBackend::Btrfs => format!("I also saved a read-only copy of your system at /.snapshots/{l}. Going back to it means booting from that copy; ask someone you trust if you haven't done it before."),
            SnapshotBackend::SystemRestore => format!("I also made a Windows restore point called \"{l}\". To go back to it, open Start, type \"Create a restore point\", choose System Restore and pick {l}."),
        })
    }
}

/// One backed-up file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupEntry {
    pub original: PathBuf,
    /// The copy's file name in the backup folder; `None` when the file didn't exist.
    pub saved: Option<String>,
    pub hash: String,
}

/// Copies of every file a fix touches, in the rewind store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileBackup {
    pub id: String,
    pub dir: PathBuf,
    pub entries: Vec<BackupEntry>,
}

impl RestorePoint for FileBackup {
    fn kind(&self) -> &'static str {
        "files"
    }

    fn restore_ref(&self) -> String {
        format!("files:{}", self.id)
    }

    fn create_step(&self) -> Option<String> {
        None
    }

    fn undo_guidance(&self) -> Option<String> {
        None
    }
}

/// The manifest inside each backup folder.
const MANIFEST: &str = "manifest.json";

/// `{config_dir}/rewind/repair-<id>`: the rewind store's layout
/// (`grokhub_core::rewind_dest`), built with `std::path` so Windows paths hold.
pub fn backup_dir(config_dir: &Path, id: &str) -> PathBuf {
    config_dir.join("rewind").join(format!("repair-{id}"))
}

/// Copy each path into the rewind store. A path that doesn't exist is
/// recorded as missing, so Undo removes what the fix created.
pub fn backup_files(config_dir: &Path, id: &str, paths: &[PathBuf]) -> Result<FileBackup, String> {
    let dir = backup_dir(config_dir, id);
    fs::create_dir_all(&dir).map_err(|e| format!("couldn't make the backup folder: {e}"))?;
    let mut entries = Vec::new();
    for (i, path) in paths.iter().enumerate() {
        match fs::read(path) {
            Ok(bytes) => {
                let saved = format!("{i}.bak");
                fs::write(dir.join(&saved), &bytes).map_err(|e| format!("couldn't back up {}: {e}", path.display()))?;
                entries.push(BackupEntry { original: path.clone(), saved: Some(saved), hash: content_hash(&bytes) });
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                entries.push(BackupEntry { original: path.clone(), saved: None, hash: String::new() });
            }
            Err(e) => return Err(format!("couldn't read {} to back it up: {e}", path.display())),
        }
    }
    let backup = FileBackup { id: id.into(), dir: dir.clone(), entries };
    let body = serde_json::to_string_pretty(&backup).map_err(|e| e.to_string())?;
    fs::write(dir.join(MANIFEST), body).map_err(|e| format!("couldn't write the backup list: {e}"))?;
    Ok(backup)
}

/// What Undo did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UndoReport {
    /// Files written back, byte for byte.
    pub restored: Vec<PathBuf>,
    /// Files the fix created, removed again.
    pub removed: Vec<PathBuf>,
    /// Files Undo couldn't touch (usually: they need admin rights), with the copy's path.
    pub failed: Vec<(PathBuf, PathBuf)>,
}

/// Put every backed-up file back. Needs an [`UndoAsk`]: only the user's click
/// or typing undoes a fix.
pub fn restore_files(backup: &FileBackup, _ask: UndoAsk) -> UndoReport {
    let mut report = UndoReport::default();
    for e in &backup.entries {
        match &e.saved {
            Some(saved) => {
                let copy = backup.dir.join(saved);
                let ok = fs::read(&copy)
                    .ok()
                    .filter(|b| content_hash(b) == e.hash)
                    .is_some_and(|b| fs::write(&e.original, b).is_ok());
                if ok {
                    report.restored.push(e.original.clone());
                } else {
                    report.failed.push((e.original.clone(), copy));
                }
            }
            None if e.original.exists() => match fs::remove_file(&e.original) {
                Ok(()) => report.removed.push(e.original.clone()),
                Err(_) => report.failed.push((e.original.clone(), PathBuf::new())),
            },
            None => {}
        }
    }
    report
}
