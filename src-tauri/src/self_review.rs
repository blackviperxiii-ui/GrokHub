//! The shell's Sunday self-review, the same weekly pass the cabin runs, on
//! the same profile. A light thread looks every few minutes. Two markers keep
//! the apps from both running it, and both are ones the cabin already
//! leaves:
//! - the cabin's scheduler lease: while its beat is fresh the cabin is open
//!   and runs the pass itself, so the shell does nothing;
//! - `last_day` in `self_review.json`: the pass claims tonight
//!   (`weekly_review::claim_week`) before it starts, so a cabin opened later
//!   sees the night as done.
//!
//! The pass itself is `grokhub_agent::harness::weekly_review`: revert and
//! draft cards first, then one routed model call for skill patches, chat
//! notes, code ideas and a Brain tweak, at most five cards in all. Quiet mode
//! (`selfImproveQuiet` in `app.json`) logs instead of posting. Cards land in
//! the shared Pulse store as Suggestion cards, so either app can apply them.

use std::path::{Path, PathBuf};
use std::time::Duration;

use grokhub_agent::harness::weekly_review::{self as wr, Budget, ProposalCard, TweakDone};
use grokhub_agent::harness::{self as hx, WeeklyPass};
use grokhub_agent::ModelClient;
use grokhub_core::scheduler_lease::{read_lease, shell_may_fire};
use grokhub_core::self_review as sr;
use grokhub_core::UpdateCard;

/// How often the thread looks.
const TICK: Duration = Duration::from_secs(5 * 60);
/// The cabin's dream hour when `app.json` has none.
const DREAM_HOUR_DEFAULT: u32 = 3;

/// What one look did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Look {
    /// The cabin is open (its lease is fresh): it runs the pass.
    CabinOpen,
    /// The window hasn't reported its clock yet.
    NoClock,
    /// Not Sunday night, or tonight's pass already started in either app.
    NotDue,
    /// The pass ran: how many cards it posted (quiet mode posts none).
    Ran { posted: usize },
}

/// The cabin's dream hour from `app.json` (`dreamHour`, 0–23).
pub fn dream_hour(config_dir: &Path) -> u32 {
    grokhub_core::app_json::read(config_dir)
        .ok()
        .and_then(|m| m.get("dreamHour").and_then(|v| v.as_u64()))
        .map(|h| h.min(23) as u32)
        .unwrap_or(DREAM_HOUR_DEFAULT)
}

/// The model the cabin uses for the weekly call.
fn weekly_model() -> String {
    grokhub_core::model_for_mode("balanced").to_string()
}

/// Start the thread. `client` is asked for a model client only when the
/// pass needs its call.
pub fn start<C>(config_dir: PathBuf, client: C)
where
    C: Fn() -> Result<Box<dyn ModelClient>, String> + Send + 'static,
{
    let _ = std::thread::Builder::new()
        .name("self-review".into())
        .spawn(move || loop {
            std::thread::sleep(TICK);
            let now = grokhub_core::now_ms();
            let offset = crate::scheduler::utc_offset()
                .or_else(|| crate::pulse_feed::state(&config_dir).utc_offset_min);
            let _ = look(
                &config_dir,
                std::process::id(),
                now,
                offset,
                &|pass_dir, stats, brain, now| {
                    let c = client()?;
                    hx::run_weekly(
                        pass_dir,
                        c.as_ref(),
                        &weekly_model(),
                        stats,
                        brain,
                        now,
                        &grokhub_agent::CancelToken::new(),
                    )
                    .map_err(|e| format!("{e:?}"))
                },
                &|f| crate::pulse::edit_cards(&config_dir, f),
            );
        });
}

/// The weekly model call, injected so tests run without a model.
pub type Call<'a> =
    dyn Fn(&Path, &[sr::SkillStats], Option<&hx::BrainAsk>, u64) -> Result<WeeklyPass, String> + 'a;
/// Change the Pulse store under its lock; returns how many cards landed.
pub type Store<'a> =
    dyn Fn(&mut dyn FnMut(&mut Vec<UpdateCard>) -> usize) -> Result<usize, String> + 'a;

/// One look: the lease gate, the clock, the last-run guard, then the pass.
pub fn look(
    config_dir: &Path,
    pid: u32,
    now: u64,
    offset_min: Option<i32>,
    call: &Call<'_>,
    store: &Store<'_>,
) -> Look {
    if !shell_may_fire(read_lease(config_dir).as_ref(), pid, now) {
        return Look::CabinOpen;
    }
    let Some(offset) = offset_min else {
        return Look::NoClock;
    };
    let clock = crate::scheduler::local_clock(now, offset);
    let today = crate::pulse_digests::local_day(now, offset);
    if !wr::claim_week(config_dir, &today, &clock, dream_hour(config_dir)) {
        return Look::NotDue;
    }
    let quiet = grokhub_core::app_json::bool_or(config_dir, crate::self_improve::QUIET_KEY, false);
    let posted = run_pass(config_dir, now, &today, quiet, call, store);
    Look::Ran { posted }
}

/// The weekly pass as of `now`, after the guard claimed it.
pub fn run_pass(
    config_dir: &Path,
    now: u64,
    today: &str,
    quiet: bool,
    call: &Call<'_>,
    store: &Store<'_>,
) -> usize {
    let outcomes = grokhub_core::outcome::read_outcomes(config_dir);
    let skills = wr::cabin_skills(&config_dir.join("skills"));
    let mut budget = Budget::new(quiet, today);
    let mut posted = post_all(
        config_dir,
        store,
        &mut budget,
        wr::first_cards(config_dir, &outcomes, &skills, now),
        Vec::new(),
        now,
    );
    let stats = sr::skill_stats(&outcomes, now, sr::WEEK_MS);
    let brain = hx::brain_ask(
        config_dir,
        &grokhub_core::run_metrics::read_runs(config_dir),
        now,
    );
    if !wr::wants_model(&budget, &stats, brain.is_some()) {
        return posted;
    }
    let pass = match call(config_dir, &stats, brain.as_ref(), now) {
        Ok(pass) => pass,
        Err(e) => {
            eprintln!("self-review: weekly call held: {e}");
            return posted;
        }
    };
    let mut notes = Vec::new();
    if let Some(tweak) = &pass.brain {
        match wr::brain_step(config_dir, tweak, &mut budget, now) {
            TweakDone::Trying(v) => notes.push(wr::trial_card(&v)),
            TweakDone::Refused(why) => eprintln!("self-review: {why}"),
            TweakDone::Logged | TweakDone::Skipped => {}
        }
    }
    posted += post_all(
        config_dir,
        store,
        &mut budget,
        wr::pass_cards(&pass, &skills),
        notes,
        now,
    );
    posted
}

/// Post `cards` inside the budget, plus trial `notes`, in one store write.
fn post_all(
    config_dir: &Path,
    store: &Store<'_>,
    budget: &mut Budget,
    cards: Vec<ProposalCard>,
    notes: Vec<UpdateCard>,
    now: u64,
) -> usize {
    if cards.is_empty() && notes.is_empty() {
        return 0;
    }
    let mut cards = Some(cards);
    let mut notes = Some(notes);
    store(&mut |feed: &mut Vec<UpdateCard>| {
        let mut n = 0;
        for note in notes.take().unwrap_or_default() {
            n += usize::from(grokhub_core::post_update(feed, note));
        }
        for card in cards.take().unwrap_or_default() {
            n += usize::from(wr::post_card(config_dir, feed, budget, card, now));
        }
        n
    })
    .unwrap_or(0)
}

#[cfg(test)]
#[path = "self_review_tests.rs"]
mod tests;
