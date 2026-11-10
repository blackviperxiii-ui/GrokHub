//! The learning engine: one place every part of the app writes what it saw
//! and reads what the app has concluded.
//!
//! - [`signal`]: what happened, as small records of ids and numbers. Never
//!   chat text, file names or anything the user typed.
//! - [`belief`]: what the app holds to be true, each line with its evidence
//!   and a confidence from 0 to 1. Counts fade with a 30-day half-life, and
//!   the user can forget a line or mark it wrong.
//! - [`ladder`]: the one rule for what a confidence allows: watch, suggest
//!   or act.
//! - [`store`]: the files, under `{config}/learn`, capped.
//! - [`sources`]: what the app's existing logs mean as signals.
//! - [`hub`]: the one engine a running app shares.
//! - [`habit`]: what the user does at the same time of day, as timed chips.
//! - [`decide`]: the one place a change the app would make on its own is
//!   weighed: do it, offer it, wait or never.
//!
//! The budget is fixed. [`Engine::record`] touches memory only: no disk, no
//! model, no network. [`Engine::tick`] writes one batch at most every
//! [`signal::FLUSH_MS`], and nothing when nothing happened. On disk the
//! engine stays under [`store::DISK_BUDGET`].

pub mod belief;
pub mod decide;
pub mod habit;
pub mod hub;
pub mod ladder;
pub mod score;
pub mod setup;
pub mod signal;
pub mod sources;
pub mod store;
pub mod view;

#[cfg(test)]
mod decide_tests;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

pub use belief::{Belief, Beliefs, Shape, Standing};
pub use ladder::{step, Step, Undo};
pub use signal::{Kind, Signal, Stream, Subject};

/// How often the belief file is rewritten while beliefs keep changing. The
/// signals behind them are on disk sooner, so a crash loses counts only.
pub const BELIEF_SAVE_MS: u64 = 5 * 60 * 1_000;
/// Signals kept to replay onto the belief file at the next save. Past this
/// the save is due at once.
pub const UNSAVED_MAX: usize = 1_024;

/// The engine one app holds: the waiting signals, the beliefs and the files.
///
/// Two apps can run on one config folder. The belief file is the shared
/// truth: each save reads it, replays this app's signals since its last save
/// onto it and writes it back, under [`store::hold_lock`]. So neither app
/// loses what the other learned, and a Forget in one reaches the other at its
/// next save.
#[derive(Debug)]
pub struct Engine {
    config_dir: PathBuf,
    stream: Stream,
    /// The belief file as last read, plus every signal recorded since.
    beliefs: Beliefs,
    /// Signals the belief file has not seen yet.
    unsaved: Vec<Signal>,
    beliefs_saved_ms: u64,
    writes: u64,
}

impl Engine {
    /// Reads the beliefs kept under `config_dir`. A missing or unreadable
    /// file starts empty.
    pub fn open(config_dir: &Path) -> Self {
        Self {
            config_dir: config_dir.to_path_buf(),
            stream: Stream::default(),
            beliefs: store::load_beliefs(config_dir),
            unsaved: Vec::new(),
            beliefs_saved_ms: 0,
            writes: 0,
        }
    }

    /// Takes one signal: the beliefs learn from it now and the line waits in
    /// memory for the next [`Engine::tick`]. Never touches the disk.
    pub fn record(&mut self, signal: Signal) {
        if self.beliefs.apply(&signal) && self.unsaved.len() < UNSAVED_MAX {
            self.unsaved.push(signal.clone());
        }
        self.stream.record(signal);
    }

    /// Writes what is due and says whether anything was written. Call it
    /// from the app's heartbeat.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        let before = self.writes;
        if self.stream.due(now_ms) {
            self.write_signals(now_ms);
        }
        let waited = now_ms.saturating_sub(self.beliefs_saved_ms) >= BELIEF_SAVE_MS;
        if !self.unsaved.is_empty() && (waited || self.unsaved.len() >= UNSAVED_MAX) {
            self.sync_beliefs(now_ms, |_| {});
        }
        self.writes > before
    }

    /// Writes everything that waits, due or not. For a clean exit.
    pub fn flush(&mut self, now_ms: u64) {
        if self.stream.waiting() > 0 {
            self.write_signals(now_ms);
        }
        if !self.unsaved.is_empty() {
            self.sync_beliefs(now_ms, |_| {});
        }
    }

    pub fn beliefs(&self) -> &Beliefs {
        &self.beliefs
    }

    /// The user's Forget on one line. Saved at once.
    pub fn forget(&mut self, key: &Subject, now_ms: u64) {
        self.sync_beliefs(now_ms, |b| {
            b.forget(key, now_ms);
        });
    }

    /// The user's "That's wrong" on one line: it is never learned or acted
    /// on again. Saved at once.
    pub fn mark_wrong(&mut self, key: &Subject, now_ms: u64) {
        self.sync_beliefs(now_ms, |b| b.mark_wrong(key, now_ms));
    }

    /// The user's "always" on one line. Saved at once. Says whether the
    /// line could be pinned (a measurement can't).
    pub fn pin(&mut self, key: &Subject, now_ms: u64) -> bool {
        let mut pinned = false;
        self.sync_beliefs(now_ms, |b| pinned = b.pin(key, now_ms));
        pinned
    }

    /// The user's Forget everything: the beliefs, the waiting signals and
    /// the files.
    pub fn forget_all(&mut self) -> Result<(), String> {
        self.beliefs = Beliefs::default();
        self.stream = Stream::default();
        self.unsaved.clear();
        store::forget_all(&self.config_dir)?;
        decide::with_state(&self.config_dir, decide::State::forget_learned);
        Ok(())
    }

    /// The config folder this engine keeps its files under.
    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    /// Disk writes so far, for the budget tests.
    pub fn writes(&self) -> u64 {
        self.writes
    }

    /// Signals lost because the app never ticked while the ring was full.
    pub fn dropped(&self) -> u64 {
        self.stream.dropped()
    }

    fn write_signals(&mut self, now_ms: u64) {
        let batch = self.stream.drain(now_ms);
        // A full disk or a locked folder must not stall the app: the beliefs
        // already learned from these lines.
        if store::append_signals(&self.config_dir, &batch).is_ok() {
            self.writes += 1;
        }
    }

    /// Reads the belief file, replays this app's unsaved signals onto it,
    /// applies `change`, and writes it back, all under the lock.
    fn sync_beliefs(&mut self, now_ms: u64, change: impl FnOnce(&mut Beliefs)) {
        self.beliefs_saved_ms = now_ms;
        let _lock = store::hold_lock(&self.config_dir);
        let mut shared = store::load_beliefs(&self.config_dir);
        for s in &self.unsaved {
            shared.apply(s);
        }
        change(&mut shared);
        if store::save_beliefs(&self.config_dir, &shared).is_ok() {
            self.unsaved.clear();
            self.writes += 1;
        }
        self.beliefs = shared;
    }
}
