//! The engine's files, under `{config}/learn`:
//!
//! - `signals.jsonl`: the stream, one short line per signal. At
//!   [`SIGNAL_CAP`] lines it becomes `signals.jsonl.1` (replacing the older
//!   one) and a new file starts.
//! - `beliefs.json`: every belief, replaced whole through a rename.
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
/// Lines in the active signal file before it rotates.
pub const SIGNAL_CAP: usize = 5_000;
/// The most the engine's files may take together, in bytes.
pub const DISK_BUDGET: u64 = 2 * 1024 * 1024;

pub fn dir(config_dir: &Path) -> PathBuf {
    config_dir.join(DIR)
}

/// The signal file and how many lines it holds. The count is read from disk
/// once and kept, so a write never re-reads the file.
#[derive(Debug)]
pub struct Log {
    config_dir: PathBuf,
    lines: Option<usize>,
}

impl Log {
    pub fn new(config_dir: &Path) -> Self {
        Self {
            config_dir: config_dir.to_path_buf(),
            lines: None,
        }
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// Appends the batch with one write, rotating first when it would pass
    /// [`SIGNAL_CAP`].
    pub fn append(&mut self, batch: &[Signal]) -> std::io::Result<()> {
        if batch.is_empty() {
            return Ok(());
        }
        let dir = dir(&self.config_dir);
        fs::create_dir_all(&dir)?;
        let path = dir.join(SIGNALS);
        let mut lines = match self.lines {
            Some(n) => n,
            None => count_lines(&path)?,
        };
        if lines > 0 && lines + batch.len() > SIGNAL_CAP {
            let old = dir.join(SIGNALS_OLD);
            let _ = fs::remove_file(&old);
            fs::rename(&path, &old)?;
            lines = 0;
        }
        let mut text = String::with_capacity(batch.len() * 64);
        for s in batch {
            let line = serde_json::to_string(s)
                .map_err(|e| std::io::Error::new(ErrorKind::InvalidData, e))?;
            text.push_str(&line);
            text.push('\n');
        }
        let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
        file.write_all(text.as_bytes())?;
        self.lines = Some(lines + batch.len());
        Ok(())
    }

    /// Removes every file the engine keeps.
    pub fn forget_all(&mut self) -> Result<(), String> {
        self.lines = Some(0);
        match fs::remove_dir_all(dir(&self.config_dir)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

fn count_lines(path: &Path) -> std::io::Result<usize> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text.lines().filter(|l| !l.trim().is_empty()).count()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(0),
        Err(e) => Err(e),
    }
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
    [SIGNALS, SIGNALS_OLD, BELIEFS]
        .iter()
        .filter_map(|name| fs::metadata(dir.join(name)).ok())
        .map(|m| m.len())
        .sum()
}
