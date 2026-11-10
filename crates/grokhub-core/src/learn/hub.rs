//! The one engine a running app shares.
//!
//! An app calls [`start`] once with its config folder. From then on any code
//! in the process can [`note`] a signal without holding the engine, and a
//! small thread writes what is due every [`TICK_MS`]. Before `start` (in
//! tests, in tools) `note` does nothing, so the shared logging functions can
//! call it without a switch.

use std::path::Path;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use super::decide::{self, Alarm, Area, Decision, Dial, Outcome, Proposal, Situation};
use super::view::{self, View};
use super::{Engine, Kind, Signal, Subject};

/// How often the writer thread looks for work. It writes only what
/// [`Engine::tick`] says is due, so most looks do nothing.
pub const TICK_MS: u64 = 15_000;

static HUB: OnceLock<Mutex<Engine>> = OnceLock::new();

fn engine() -> Option<MutexGuard<'static, Engine>> {
    HUB.get()
        .map(|m| m.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Opens the engine for this process and starts its writer thread. A second
/// call does nothing.
pub fn start(config_dir: &Path) {
    let mut first = false;
    HUB.get_or_init(|| {
        first = true;
        Mutex::new(Engine::open(config_dir))
    });
    if !first {
        return;
    }
    // The thread lives as long as the app. If it can't start, signals still
    // reach the beliefs in memory and `flush` still writes them.
    let _ = std::thread::Builder::new()
        .name("learn".into())
        .spawn(|| loop {
            std::thread::sleep(Duration::from_millis(TICK_MS));
            if let Some(mut e) = engine() {
                e.tick(crate::now_ms());
            }
        });
}

/// Whether [`start`] has run in this process.
pub fn started() -> bool {
    HUB.get().is_some()
}

/// Records one signal. Memory only; nothing before [`start`].
pub fn note(signal: Signal) {
    if let Some(mut e) = engine() {
        e.record(signal);
    }
}

/// Records several signals under one lock.
pub fn note_all(signals: impl IntoIterator<Item = Signal>) {
    if let Some(mut e) = engine() {
        for s in signals {
            e.record(s);
        }
    }
}

/// Writes everything that waits. Call it when the app exits.
pub fn flush() {
    if let Some(mut e) = engine() {
        e.flush(crate::now_ms());
    }
}

/// Reads the engine, e.g. to list the beliefs. `None` before [`start`].
pub fn read<R>(f: impl FnOnce(&Engine) -> R) -> Option<R> {
    engine().map(|e| f(&e))
}

/// What the app learned, as lines. Empty before [`start`].
pub fn view(now_ms: u64) -> View {
    read(|e| view::view(e.beliefs(), now_ms)).unwrap_or_default()
}

/// The user's Forget on one belief. Saved at once.
pub fn forget(key: &Subject) {
    if let Some(mut e) = engine() {
        e.forget(key, crate::now_ms());
    }
}

/// The user's "That's wrong" on one belief. Saved at once.
pub fn mark_wrong(key: &Subject) {
    if let Some(mut e) = engine() {
        e.mark_wrong(key, crate::now_ms());
    }
}

/// The user's Forget everything.
pub fn forget_all() -> Result<(), String> {
    match engine() {
        Some(mut e) => e.forget_all(),
        None => Ok(()),
    }
}

/// The usage counts auto setup reads. All zero before [`start`].
pub fn auto_usage(now_ms: u64) -> crate::autotune::rules::Usage {
    read(|e| super::setup::usage(e.beliefs(), now_ms)).unwrap_or_default()
}

/// Weighs what the learners would like to change, the most helpful first.
/// Nothing before [`start`], so nothing acts.
pub fn decide_all(proposals: Vec<Proposal>, now: Situation, now_ms: u64) -> Vec<Decision> {
    let Some(e) = engine() else {
        return Vec::new();
    };
    decide::with_state(e.config_dir(), |s| s.decide_all(proposals, now, now_ms))
}

/// What became of an offer or of a change the app made. The rule's counts
/// take it in, and the beliefs learn from it like any other signal.
pub fn decision_outcome(rule: &Subject, outcome: Outcome, now_ms: u64) -> Option<Alarm> {
    let mut e = engine()?;
    let kind = match outcome {
        Outcome::Accepted | Outcome::Kept => Kind::Accepted,
        Outcome::Declined => Kind::Declined,
        Outcome::Undone => Kind::Undone,
    };
    e.record(Signal::new(now_ms, kind, rule.clone()));
    decide::with_state(e.config_dir(), |s| s.outcome(rule, outcome, now_ms))
}

/// Sets how far one area may go.
pub fn set_dial(area: Area, dial: Dial) {
    if let Some(e) = engine() {
        decide::with_state(e.config_dir(), |s| s.set_dial(area, dial));
    }
}

/// The dials and the newest decisions, for the cabin's chat.
pub fn decisions_text(now_ms: u64) -> String {
    let state = engine().map(|e| decide::load(e.config_dir()));
    decide::text(&state.unwrap_or_default(), now_ms)
}
