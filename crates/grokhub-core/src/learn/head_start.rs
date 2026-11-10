//! A head start for someone who used the app before the engine existed.
//!
//! The run log and the card log were there long before the engine and hold
//! weeks of what the user did. The first time the engine starts in a config
//! folder it reads them once, as the signals their writers would have sent,
//! so habits and card tastes are known on day one and not a month later.
//! Each line keeps its own time, so old lines have already faded as if the
//! engine had seen them when they happened.
//!
//! It runs once per folder (the [`MARK`] file), never after the user's
//! Forget everything, and not while learning is paused (it waits for the
//! first start after the pause ends). Someone new to the app has empty logs
//! and starts from nothing, as before.

use std::fs;
use std::path::Path;

use super::signal::Signal;
use super::{sources, store, Engine};
use crate::card_signals::CardSignal;
use crate::run_metrics::RunMetrics;

/// The file that says this folder has had its head start, or needs none.
pub const MARK: &str = "head_start";
/// Signals taken in between two saves: under the stream's ring and the
/// engine's unsaved cap, so none is dropped.
pub const CHUNK: usize = 200;

/// Whether this start is the engine's first in `config_dir`: no mark and no
/// belief file. Writes the mark either way, so the answer is yes at most
/// once, in one app.
pub fn claim(config_dir: &Path) -> bool {
    let dir = store::dir(config_dir);
    let fresh = !dir.join(store::BELIEFS).exists();
    if fs::create_dir_all(&dir).is_err() {
        return false;
    }
    let made = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dir.join(MARK))
        .is_ok();
    made && fresh
}

/// What the old logs mean as signals, oldest first. Only lines from before
/// `before_ms` count: later ones reached the engine when they were written.
/// `hour` gives the local hour of a time, for the habit votes. A game is
/// never assumed: the old lines don't say.
pub fn signals(
    runs: &[RunMetrics],
    cards: &[CardSignal],
    before_ms: u64,
    hour: impl Fn(u64) -> u32,
) -> Vec<Signal> {
    let mut out = Vec::new();
    for m in runs.iter().filter(|m| m.at < before_ms) {
        out.extend(sources::run(m));
        out.extend(sources::run_habit(m, hour(m.at), false));
    }
    for c in cards.iter().filter(|c| c.ts < before_ms) {
        out.extend(sources::card(c));
    }
    out.sort_by_key(|s| s.at_ms);
    out
}

/// Feeds `signals` to the engine, saving every [`CHUNK`]. `with` runs one
/// step with the engine; the caller decides how it is reached (the hub locks
/// it per step, so the app is never held up for the whole read).
pub fn feed(signals: Vec<Signal>, now_ms: u64, mut with: impl FnMut(&mut dyn FnMut(&mut Engine))) {
    for chunk in signals.chunks(CHUNK) {
        with(&mut |e: &mut Engine| {
            for s in chunk {
                e.record(s.clone());
            }
            e.flush(now_ms);
        });
    }
}
