//! Backup and move: everything the app learned as one file the user can
//! carry to a reinstall or a second PC.
//!
//! - [`export`] writes `{config}/learned-backup.json`: the beliefs and the
//!   decision engine's state (dials, goals, rule counts, scoreboard). Ids
//!   and numbers only, like the files it is made from. It is written on this
//!   PC and nowhere else; moving it is the user's own act.
//! - [`import`] reads that file from the config folder and replaces what the
//!   app holds. What it held before is kept as one restore point.
//! - [`restore`] puts that restore point back, once.
//!
//! Forget everything removes the restore point with the rest. The backup
//! file is the user's and stays until they delete it.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::belief::Beliefs;
use super::{decide, store};

/// The backup, in the config folder itself so it is easy to find and copy.
pub const FILE: &str = "learned-backup.json";
/// What the app held before the last import. Under `{config}/learn`.
pub const RESTORE: &str = "restore.json";
/// The newest backup layout this build reads and the one it writes.
pub const VERSION: u32 = 1;

pub const NO_BACKUP: &str =
    "There is no backup to read. Put `learned-backup.json` in the app's config folder first.";
pub const BAD_BACKUP: &str = "That backup can't be read, so nothing was changed.";
pub const NEWER_BACKUP: &str =
    "That backup was made by a newer version of the app, so nothing was changed.";
pub const NO_RESTORE: &str = "There is nothing to put back.";

/// Everything learned, at one moment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Backup {
    pub version: u32,
    pub made_ms: u64,
    pub beliefs: Beliefs,
    pub decisions: decide::State,
}

pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE)
}

fn held(config_dir: &Path, now_ms: u64) -> Backup {
    Backup {
        version: VERSION,
        made_ms: now_ms,
        beliefs: store::load_beliefs(config_dir),
        decisions: decide::load(config_dir),
    }
}

fn write(path: &Path, backup: &Backup) -> Result<(), String> {
    let text = serde_json::to_string(backup).map_err(|e| e.to_string())?;
    crate::pulse_writer::write_atomic(path, &text)
}

fn read(path: &Path, missing: &str) -> Result<Backup, String> {
    let text = fs::read_to_string(path).map_err(|_| missing.to_string())?;
    // The version is read on its own first, so a newer layout is named as
    // newer and not as broken.
    #[derive(Deserialize)]
    struct Head {
        version: u32,
    }
    let head: Head = serde_json::from_str(&text).map_err(|_| BAD_BACKUP.to_string())?;
    if head.version > VERSION {
        return Err(NEWER_BACKUP.to_string());
    }
    serde_json::from_str(&text).map_err(|_| BAD_BACKUP.to_string())
}

/// Puts `backup` in place of what the folder holds. A write that fails is
/// an error, so a half-applied put never reads as done. The beliefs go last:
/// the engine holds them in memory and reads them again only after an `Ok`,
/// so on an error the folder still has the ones it holds.
fn put(config_dir: &Path, backup: Backup) -> Result<(), String> {
    decide::replace(config_dir, &backup.decisions)?;
    let _lock = store::hold_lock(config_dir);
    store::save_beliefs(config_dir, &backup.beliefs)
}

/// Writes the backup file and returns where it is. The caller saves what
/// waits in memory first.
pub fn export(config_dir: &Path, now_ms: u64) -> Result<PathBuf, String> {
    let to = path(config_dir);
    write(&to, &held(config_dir, now_ms))?;
    Ok(to)
}

/// Replaces what the folder holds with the backup file, keeping what was
/// there as the restore point. Returns how many lines came in. A backup that
/// can't be read changes nothing.
pub fn import(config_dir: &Path, now_ms: u64) -> Result<usize, String> {
    let backup = read(&path(config_dir), NO_BACKUP)?;
    let lines = backup.beliefs.len();
    write(
        &store::dir(config_dir).join(RESTORE),
        &held(config_dir, now_ms),
    )?;
    put(config_dir, backup)?;
    Ok(lines)
}

/// Puts back what the folder held before the last import, and uses the
/// restore point up. The point stays when putting it back fails.
pub fn restore(config_dir: &Path) -> Result<(), String> {
    let point = store::dir(config_dir).join(RESTORE);
    let before = read(&point, NO_RESTORE)?;
    put(config_dir, before)?;
    fs::remove_file(&point).map_err(|e| e.to_string())
}
