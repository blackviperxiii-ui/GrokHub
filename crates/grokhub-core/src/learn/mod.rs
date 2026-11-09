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
//!
//! The budget is fixed. [`Engine::record`] touches memory only: no disk, no
//! model, no network. [`Engine::tick`] writes one batch at most every
//! [`signal::FLUSH_MS`], and nothing when nothing happened. On disk the
//! engine stays under [`store::DISK_BUDGET`].

pub mod belief;
pub mod ladder;
pub mod signal;
pub mod store;

#[cfg(test)]
mod tests;

use std::path::Path;

pub use belief::{Belief, Beliefs, Shape, Standing};
pub use ladder::{step, Step, Undo};
pub use signal::{Kind, Signal, Stream, Subject};

/// How often the belief file is rewritten while beliefs keep changing. The
/// signals behind them are on disk sooner, so a crash loses counts only.
pub const BELIEF_SAVE_MS: u64 = 5 * 60 * 1_000;

/// The engine one app holds: the waiting signals, the beliefs and the files.
#[derive(Debug)]
pub struct Engine {
    stream: Stream,
    beliefs: Beliefs,
    log: store::Log,
    /// Beliefs changed since they were last saved.
    dirty: bool,
    beliefs_saved_ms: u64,
    writes: u64,
}

impl Engine {
    /// Reads the beliefs kept under `config_dir`. A missing or unreadable
    /// file starts empty.
    pub fn open(config_dir: &Path) -> Self {
        Self {
            stream: Stream::default(),
            beliefs: store::load_beliefs(config_dir),
            log: store::Log::new(config_dir),
            dirty: false,
            beliefs_saved_ms: 0,
            writes: 0,
        }
    }

    /// Takes one signal: the beliefs learn from it now and the line waits in
    /// memory for the next [`Engine::tick`]. Never touches the disk.
    pub fn record(&mut self, signal: Signal) {
        if self.beliefs.apply(&signal) {
            self.dirty = true;
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
        if self.dirty && now_ms.saturating_sub(self.beliefs_saved_ms) >= BELIEF_SAVE_MS {
            self.write_beliefs(now_ms);
        }
        self.writes > before
    }

    /// Writes everything that waits, due or not. For a clean exit.
    pub fn flush(&mut self, now_ms: u64) {
        if self.stream.waiting() > 0 {
            self.write_signals(now_ms);
        }
        if self.dirty {
            self.write_beliefs(now_ms);
        }
    }

    pub fn beliefs(&self) -> &Beliefs {
        &self.beliefs
    }

    /// The user's Forget on one line.
    pub fn forget(&mut self, key: &Subject, now_ms: u64) {
        if self.beliefs.forget(key, now_ms) {
            self.write_beliefs(now_ms);
        }
    }

    /// The user's "That's wrong" on one line: it is never learned or acted
    /// on again.
    pub fn mark_wrong(&mut self, key: &Subject, now_ms: u64) {
        self.beliefs.mark_wrong(key, now_ms);
        self.write_beliefs(now_ms);
    }

    /// The user's Forget everything: the beliefs, the waiting signals and
    /// the files.
    pub fn forget_all(&mut self) -> Result<(), String> {
        self.beliefs = Beliefs::default();
        self.stream = Stream::default();
        self.dirty = false;
        self.log.forget_all()
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
        if self.log.append(&batch).is_ok() {
            self.writes += 1;
        }
    }

    fn write_beliefs(&mut self, now_ms: u64) {
        self.beliefs_saved_ms = now_ms;
        if store::save_beliefs(self.log.config_dir(), &self.beliefs).is_ok() {
            self.dirty = false;
            self.writes += 1;
        }
    }
}
