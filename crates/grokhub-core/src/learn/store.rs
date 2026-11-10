//! The engine's files, under `{config}/learn`:
//!
//! - `signals.jsonl`: the stream, one short line per signal. At
//!   [`SIGNAL_BYTES`] it becomes `signals.jsonl.1` (replacing the older one)
//!   and a new file starts.
//! - `beliefs.json`: every belief, replaced whole through a rename.
//! - `decisions.json`: the decision engine's dials, rule counts and log.
//! - `learn.lock`: held while the beliefs are read, changed and written, so
//!   two apps on one folder never write over each other.
//!
//! Local only. Nothing here is synced or sent anywhere.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use super::belief::Beliefs;
use super::signal::Signal;

pub const DIR: &str = "learn";
pub const SIGNALS: &str = "signals.jsonl";
pub const SIGNALS_OLD: &str = "signals.jsonl.1";
pub const BELIEFS: &str = "beliefs.json";
pub const LOCK: &str = "learn.lock";
/// Size of the active signal file at which it rotates, in bytes.
pub const SIGNAL_BYTES: u64 = 384 * 1024;
/// The most the engine's files may take together, in bytes.
pub const DISK_BUDGET: u64 = 2 * 1024 * 1024;

pub fn dir(config_dir: &Path) -> PathBuf {
    config_dir.join(DIR)
}

/// Appends the batch with one write, rotating first when the file has
/// reached [`SIGNAL_BYTES`]. The size is read from the file each time, so two
/// apps appending to one folder rotate at the same point.
pub fn append_signals(config_dir: &Path, batch: &[Signal]) -> std::io::Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    let dir = dir(config_dir);
    fs::create_dir_all(&dir)?;
    let path = dir.join(SIGNALS);
    if fs::metadata(&path).is_ok_and(|m| m.len() >= SIGNAL_BYTES) {
        let old = dir.join(SIGNALS_OLD);
        let _ = fs::remove_file(&old);
        fs::rename(&path, &old)?;
    }
    let mut text = String::with_capacity(batch.len() * 64);
    for s in batch {
        let line =
            serde_json::to_string(s).map_err(|e| std::io::Error::new(ErrorKind::InvalidData, e))?;
        text.push_str(&line);
        text.push('\n');
    }
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    file.write_all(text.as_bytes())
}

/// Removes every file the engine keeps.
pub fn forget_all(config_dir: &Path) -> Result<(), String> {
    let _lock = hold_lock(config_dir);
    let dir = dir(config_dir);
    for name in [SIGNALS, SIGNALS_OLD, BELIEFS] {
        match fs::remove_file(dir.join(name)) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

/// Holds the engine's lock file until the returned file is dropped. A folder
/// or filesystem without locks still works, just unserialised.
pub fn hold_lock(config_dir: &Path) -> Option<fs::File> {
    let dir = dir(config_dir);
    fs::create_dir_all(&dir).ok()?;
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(LOCK))
        .ok()?;
    f.lock().ok().map(|()| f)
}

/// Every signal on disk, oldest first. Unreadable lines are skipped.
pub fn read_signals(config_dir: &Path) -> Vec<Signal> {
    let dir = dir(config_dir);
    [SIGNALS_OLD, SIGNALS]
        .iter()
        .filter_map(|name| fs::read_to_string(dir.join(name)).ok())
        .flat_map(|text| {
            text.lines()
                .filter_map(|l| serde_json::from_str::<Signal>(l).ok())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The saved beliefs. A missing or unreadable file reads as none.
pub fn load_beliefs(config_dir: &Path) -> Beliefs {
    fs::read_to_string(dir(config_dir).join(BELIEFS))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_beliefs(config_dir: &Path, beliefs: &Beliefs) -> Result<(), String> {
    let text = serde_json::to_string(beliefs).map_err(|e| e.to_string())?;
    crate::pulse_writer::write_atomic(&dir(config_dir).join(BELIEFS), &text)
}

/// Bytes the engine's files take now.
pub fn disk_used(config_dir: &Path) -> u64 {
    let dir = dir(config_dir);
    [SIGNALS, SIGNALS_OLD, BELIEFS, super::decide::FILE]
        .iter()
        .filter_map(|name| fs::metadata(dir.join(name)).ok())
        .map(|m| m.len())
        .sum()
}
