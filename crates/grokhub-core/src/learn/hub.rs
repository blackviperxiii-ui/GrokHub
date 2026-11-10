//! The one engine a running app shares.
//!
//! An app calls [`start`] once with its config folder. From then on any code
//! in the process can [`note`] a signal without holding the engine, and a
//! small thread writes what is due every [`TICK_MS`]. Before `start` (in
//! tests, in tools) `note` does nothing, so the shared logging functions can
//! call it without a switch.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use super::decide::{self, Alarm, Area, Decision, Dial, Outcome, Proposal, Situation};
use super::view::{self, View};
use super::{Engine, Kind, Signal, Subject};

/// How often the writer thread looks for work. It writes only what
/// [`Engine::tick`] says is due, so most looks do nothing.
pub const TICK_MS: u64 = 15_000;

static HUB: OnceLock<Mutex<Engine>> = OnceLock::new();
/// The user's pause, read from the folder at [`start`] and again at every
/// look, so a pause in one app stops the other within [`TICK_MS`].
static PAUSED: AtomicBool = AtomicBool::new(false);

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
        PAUSED.store(super::store::paused(config_dir), Ordering::Relaxed);
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
                PAUSED.store(super::store::paused(e.config_dir()), Ordering::Relaxed);
                e.tick(crate::now_ms());
            }
        });
}

/// Whether [`start`] has run in this process.
pub fn started() -> bool {
    HUB.get().is_some()
}

/// Whether the user has learning paused. While it is, nothing is recorded.
pub fn paused() -> bool {
    PAUSED.load(Ordering::Relaxed)
}

/// The user's pause or resume. What was learned before stays as it is.
pub fn set_paused(on: bool) -> Result<(), String> {
    let Some(e) = engine() else {
        return Ok(());
    };
    super::store::set_paused(e.config_dir(), on)?;
    PAUSED.store(on, Ordering::Relaxed);
    Ok(())
}

/// Records one signal. Memory only; nothing before [`start`] or while
/// learning is paused.
pub fn note(signal: Signal) {
    if paused() {
        return;
    }
    if let Some(mut e) = engine() {
        e.record(signal);
    }
}

/// Records several signals under one lock.
pub fn note_all(signals: impl IntoIterator<Item = Signal>) {
    if paused() {
        return;
    }
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
    let mut out = read(|e| view::view(e.beliefs(), now_ms)).unwrap_or_default();
    out.paused = paused();
    out
}

/// The user's "always" on one belief. Saved at once. `false` when the line
/// is a measurement, which can't be pinned.
pub fn pin(key: &Subject) -> bool {
    engine().is_some_and(|mut e| e.pin(key, crate::now_ms()))
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
    if !paused() {
        e.record(Signal::new(now_ms, kind, rule.clone()));
    }
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
