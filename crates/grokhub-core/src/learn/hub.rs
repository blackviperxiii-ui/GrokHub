//! The one engine a running app shares.
//!
//! An app calls [`start`] once with its config folder. From then on any code
//! in the process can [`note`] a signal without holding the engine, and a
//! small thread writes what is due every [`TICK_MS`]. Before `start` (in
//! tests, in tools) `note` does nothing, so the shared logging functions can
//! call it without a switch.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use super::decide::{self, Alarm, Area, Decision, Dial, Outcome, Proposal, Situation};
use super::habit;
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

/// Minutes local time is east of UTC, as the app last told us. UTC until then.
static UTC_OFFSET_MIN: AtomicI32 = AtomicI32::new(0);

/// The app's local clock offset, so habits are counted in local hours.
pub fn set_utc_offset_min(offset_min: i32) {
    UTC_OFFSET_MIN.store(offset_min.clamp(-840, 840), Ordering::Relaxed);
}

/// The local hour of `at_ms`, 0 to 23.
pub fn local_hour(at_ms: u64) -> u32 {
    crate::rhythm::local_slot(at_ms, UTC_OFFSET_MIN.load(Ordering::Relaxed)).1
}

/// The app's last word on what is going on right now.
static NOW: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// The app's picture of this moment. Every decider reads the same one.
pub fn set_now(now: super::now::Snapshot) {
    NOW.store(now.bits(), Ordering::Relaxed);
}

/// What is going on right now, as the app last said. All clear until then.
pub fn now() -> super::now::Snapshot {
    super::now::Snapshot::from_bits(NOW.load(Ordering::Relaxed))
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
    let state = engine().map(|e| decide::load(e.config_dir()));
    out.trust = view::trust(&state.unwrap_or_default(), now(), now_ms);
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

/// Auto setup's plan after the decision engine weighed it
/// ([`super::setup::weigh`]). Unchanged before [`start`].
pub fn weigh_setup(
    plan: crate::autotune::plan::Plan,
    moment: crate::autotune::plan::Moment,
    now_ms: u64,
) -> crate::autotune::plan::Plan {
    let Some(e) = engine() else {
        return plan;
    };
    let situation = now().situation();
    decide::with_state(e.config_dir(), |s| {
        super::setup::weigh(plan, moment, &mut |p| Some(s.decide(p, situation, now_ms)))
    })
}

/// What became of one of auto setup's changes or suggestions, by its rule.
pub fn setup_outcome(rule: &str, outcome: Outcome, now_ms: u64) {
    if let Some(rule) = super::setup::rule(rule) {
        decision_outcome(&rule, outcome, now_ms);
    }
}

/// The user handed one of auto setup's rules back to the app.
pub fn setup_forgive(rule: &str) {
    if let (Some(e), Some(rule)) = (engine(), super::setup::rule(rule)) {
        decide::with_state(e.config_dir(), |s| s.forgive(&rule));
    }
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
        Outcome::RolledBack => Kind::Failed,
    };
    if !paused() {
        e.record(Signal::new(now_ms, kind, rule.clone()));
    }
    decide::with_state(e.config_dir(), |s| s.outcome(rule, outcome, now_ms))
}

/// How far `area` may go. Act, the default, before [`start`].
pub fn dial(area: Area) -> Dial {
    engine().map_or(Dial::default(), |e| decide::load(e.config_dir()).dial(area))
}

/// Sets how far one area may go.
pub fn set_dial(area: Area, dial: Dial) {
    if let Some(e) = engine() {
        decide::with_state(e.config_dir(), |s| s.set_dial(area, dial));
    }
}

/// The user's "carry on" after too much was undone: the app may act again.
pub fn carry_on() {
    if let Some(e) = engine() {
        decide::with_state(e.config_dir(), |s| s.resume());
    }
}

/// Sets what matters most to the user, first to last.
pub fn set_goals(goals: &[decide::Goal]) {
    if let Some(e) = engine() {
        decide::with_state(e.config_dir(), |s| s.set_goals(goals));
    }
}

/// The dials and the newest decisions, for the cabin's chat.
pub fn decisions_text(now_ms: u64) -> String {
    let state = engine().map(|e| decide::load(e.config_dir()));
    decide::text(&state.unwrap_or_default(), now_ms)
}

/// The quick chips for what the user usually does around `now_ms`: each due
/// habit goes through the decision engine, and the ones it would offer or
/// act on become chips, at most [`habit::CHIPS_MAX`]. Empty before [`start`].
pub fn habit_chips(
    skills: &[crate::skill::SkillMd],
    now: Situation,
    score: f32,
    now_ms: u64,
) -> Vec<crate::chips::QuickChip> {
    let part = habit::Part::of_hour(local_hour(now_ms));
    let gaming = self::now().gaming;
    let due = read(|e| habit::due(e.beliefs(), part, gaming, now_ms)).unwrap_or_default();
    // A habit whose skill is gone has no chip, so it is not weighed either.
    let shown: Vec<(habit::Habit, crate::chips::QuickChip, Proposal)> = due
        .into_iter()
        .filter_map(|h| {
            let chip = habit::chip(&h, skills, score)?;
            let proposal = h.proposal()?;
            Some((h, chip, proposal))
        })
        .collect();
    if shown.is_empty() {
        return Vec::new();
    }
    let verdicts = decide_all(
        shown.iter().map(|(_, _, p)| p.clone()).collect(),
        now,
        now_ms,
    );
    shown
        .into_iter()
        .filter(|(_, _, p)| {
            verdicts.iter().any(|d| {
                d.rule == p.rule
                    && matches!(d.verdict, decide::Verdict::Act | decide::Verdict::Offer)
            })
        })
        .map(|(_, chip, _)| chip)
        .take(habit::CHIPS_MAX)
        .collect()
}
