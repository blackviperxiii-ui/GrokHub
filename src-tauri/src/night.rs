//! The nightly passes the cabin ran on its review slot, while the cabin is
//! closed: once a local day at or after the dream hour (`dreamHour` in
//! `app.json`, the same hour as the Sunday self-review):
//! - the episode lessons' tidy (merge near-duplicates, drop stale ones), in
//!   any memory mode;
//! - the nightly chat review (`chat_review`): Suggested tiles, skill ideas
//!   and reviewed skill patches. Its own file says when it last ran, so it
//!   also catches up at any hour after a night it missed;
//! - the memory dream, in memory repo mode only. It merges repeat notes,
//!   retires unsure ones past Memory retention and proposes missing
//!   preferences, then writes `amr/dreams/<local day>.md`. With
//!   `"dream_model": true` it also runs the model pass and posts its
//!   "Memory dream" card.
//!
//! A running chat or a waiting decision puts the review and the dream off
//! to the next look. Halt skips the night. The report on disk marks the night as dreamed, so a
//! cabin or a relaunch the same night does not dream again.

use std::path::{Path, PathBuf};
use std::time::Duration;

use grokhub_agent::ModelClient;
use grokhub_core::amr::{dream_report_path, ModelDreamOpts, MODEL_DREAM_OUTPUT_TOKEN_CAP};
use grokhub_core::scheduler_lease::{read_lease, shell_may_fire};
use grokhub_core::{HeartbeatPace, HeartbeatThrottle, PACE_NORMAL};

use crate::chat_review::Reviewed;

/// How often the thread looks.
const TICK: Duration = Duration::from_secs(5 * 60);

/// What the thread remembers between looks: the local day each pass last
/// ran, so a pass runs once a night.
#[derive(Debug, Default)]
pub struct NightState {
    lessons_day: Option<String>,
    review_day: Option<String>,
    /// Too few new turns: the review looks again after this.
    review_after_ms: u64,
    dream_day: Option<String>,
}

/// How long a review with too few new turns waits before it counts again.
const REVIEW_RECOUNT_MS: u64 = 60 * 60 * 1000;

/// What tonight's dream did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dream {
    /// Before the dream hour, it already ran tonight, or the chat review
    /// took this look (the dream waits for the next one, which checks
    /// busy again).
    NotDue,
    /// Legacy memory mode: nothing to dream over, and no `amr/` is made.
    Legacy,
    /// Memory writes are off.
    WritesOff,
    /// Tonight's report is already on disk (an earlier run or the cabin).
    AlreadyDone,
    /// The dream ran; `card` when the model pass posted its Pulse card.
    Dreamed {
        card: bool,
    },
    Failed(String),
}

/// What one look did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Look {
    /// The cabin is open (its lease is fresh): it runs the passes.
    CabinOpen,
    /// The window hasn't reported its clock yet.
    NoClock,
    /// A chat is running or a decision waits: the dream tries again next look.
    Busy,
    /// Halt holds background work: tonight's dream is skipped.
    Halted,
    Ran(Night),
}

/// Tonight's passes. `lessons` is true when the lesson tidy ran this look;
/// `review` is `None` when the review did not look (busy, halted, or it
/// already ran or waits to count again).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Night {
    pub lessons: bool,
    pub review: Option<Reviewed>,
    pub dream: Dream,
}

/// A model client for the dream's model pass, asked for only when it runs.
pub type Model<'a> = dyn Fn() -> Result<Box<dyn ModelClient>, String> + 'a;

/// Start the thread. `busy` is a running chat or a waiting decision.
pub fn start<B, M>(config_dir: PathBuf, busy: B, model: M)
where
    B: Fn() -> bool + Send + 'static,
    M: Fn() -> Result<Box<dyn ModelClient>, String> + Send + 'static,
{
    let _ = std::thread::Builder::new()
        .name("night".into())
        .spawn(move || {
            let mut state = NightState::default();
            loop {
                std::thread::sleep(TICK);
                let now = grokhub_core::now_ms();
                let offset = crate::scheduler::utc_offset()
                    .or_else(|| crate::pulse_feed::state(&config_dir).utc_offset_min);
                let look = look(
                    &config_dir,
                    std::process::id(),
                    now,
                    offset,
                    busy(),
                    &model,
                    &mut state,
                );
                if let Look::Ran(night) = look {
                    if let Some(Reviewed::Held(why)) = night.review {
                        eprintln!("night: chat review held: {why}");
                    }
                    if let Dream::Failed(why) = night.dream {
                        eprintln!("night: dream failed: {why}");
                    }
                }
            }
        });
}

/// One look: the lease, the clock, then each pass once a night at or after
/// the dream hour. Lessons first, then the review, then the dream.
pub fn look(
    config_dir: &Path,
    pid: u32,
    now: u64,
    offset_min: Option<i32>,
    busy: bool,
    model: &Model<'_>,
    state: &mut NightState,
) -> Look {
    if !shell_may_fire(read_lease(config_dir).as_ref(), pid, now) {
        return Look::CabinOpen;
    }
    let Some(offset) = offset_min else {
        return Look::NoClock;
    };
    let _config = grokhub_agent::perm::ConfigGuard::set(config_dir);
    let clock = crate::scheduler::local_clock(now, offset);
    let hour = crate::self_review::dream_hour(config_dir);
    let due = clock.hour >= hour;
    let today = crate::self_review::local_day(now, offset);
    let mut lessons = false;
    if due && state.lessons_day.as_deref() != Some(today.as_str()) {
        state.lessons_day = Some(today.clone());
        lessons = grokhub_agent::episode::lessons::dream_store(config_dir, now).is_ok();
    }
    // A heartbeat pace that is off spends nothing proactive, as in the cabin.
    let review = (!busy
        && state.review_day.as_deref() != Some(today.as_str())
        && now >= state.review_after_ms
        && !halted(config_dir, now)
        && !pace(config_dir).is_off())
    .then(|| review_tonight(config_dir, &today, &clock, hour, now, model, state));
    // The review's call can take a long while (a batched request waits for
    // hours), so busy is stale by now: the dream waits for the next look.
    let reviewed = matches!(review, Some(Reviewed::Ran { .. } | Reviewed::Held(_)));
    if reviewed || !due || state.dream_day.as_deref() == Some(today.as_str()) {
        return Look::Ran(Night {
            lessons,
            review,
            dream: Dream::NotDue,
        });
    }
    if busy {
        return Look::Busy;
    }
    state.dream_day = Some(today.clone());
    if halted(config_dir, now) {
        return Look::Halted;
    }
    Look::Ran(Night {
        lessons,
        review,
        dream: dream_tonight(config_dir, &today, now, model),
    })
}

/// Tonight's chat review. The model is asked for only when the review calls.
fn review_tonight(
    config_dir: &Path,
    today: &str,
    clock: &grokhub_core::LocalClock,
    hour: u32,
    now: u64,
    model: &Model<'_>,
    state: &mut NightState,
) -> Reviewed {
    let call = |system: &str, digest: &str| {
        let client = model()?;
        crate::pulse_feed::ask_as(
            client.as_ref(),
            grokhub_core::model_for_mode("balanced"),
            "background:review",
            &[
                ("system".to_string(), system.to_string()),
                ("user".to_string(), digest.to_string()),
            ],
        )
    };
    let reviewed = crate::chat_review::review_step(config_dir, today, clock, hour, now, &call);
    match &reviewed {
        // Once a night, even when the file could not take the stamp.
        Reviewed::Ran { .. } | Reviewed::Held(_) => state.review_day = Some(today.to_string()),
        Reviewed::NotWorth { .. } => state.review_after_ms = now + REVIEW_RECOUNT_MS,
        Reviewed::NotDue => {}
    }
    reviewed
}

/// Tonight's dream over the pinned config folder's memory repo.
fn dream_tonight(config_dir: &Path, today: &str, now: u64, model: &Model<'_>) -> Dream {
    if !grokhub_agent::amr_enabled() {
        return Dream::Legacy;
    }
    if !grokhub_agent::memory_writes_enabled() {
        return Dream::WritesOff;
    }
    // Held through the model call too, so `/dream` says one is running
    // rather than merging the same notes under it.
    let _one = crate::memory::DREAM
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let store = grokhub_agent::amr_store();
    if dream_report_path(&store, today).is_file() {
        return Dream::AlreadyDone;
    }
    if let Err(e) = store.init() {
        return Dream::Failed(e.to_string());
    }
    let opts = crate::memory::dream_opts(Some(today));
    // Signed out, the model pass is skipped and the plain dream runs.
    let client = grokhub_agent::dream_model_enabled()
        .then(model)
        .and_then(Result::ok);
    let Some(client) = client else {
        return match store.dream_once(now, &opts) {
            Ok(_) => Dream::Dreamed { card: false },
            Err(e) => Dream::Failed(e.to_string()),
        };
    };
    let model_opts = ModelDreamOpts {
        enabled: true,
        output_token_cap: MODEL_DREAM_OUTPUT_TOKEN_CAP,
    };
    match grokhub_agent::sleep_dream(
        client.as_ref(),
        &grokhub_agent::CancelToken::new(),
        grokhub_agent::DEFAULT_MODEL,
        config_dir,
        &store,
        now,
        &opts,
        &model_opts,
    ) {
        Ok(out) => Dream::Dreamed {
            card: crate::pulse::add_post(config_dir, out.card).unwrap_or(false),
        },
        Err(e) => Dream::Failed(e),
    }
}

/// Your last send, written by `send_chat`. Only a message you send ends a
/// Halt's hold; turns that automations or a halted reply save do not.
pub(crate) const SENT_STAMP_FILE: &str = "desktop-sent.stamp";

/// Note that you sent a message at `now`.
pub(crate) fn note_send(config_dir: &Path, now: u64) {
    let _ = std::fs::write(config_dir.join(SENT_STAMP_FILE), now.to_string());
}

fn last_send(config_dir: &Path) -> Option<u64> {
    std::fs::read_to_string(config_dir.join(SENT_STAMP_FILE))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Halt holds background work as the cabin's heartbeat did: for
/// `haltHoldMin` minutes after the stamp (`heartbeat` in `app.json`, 15 by
/// default; 0 holds until you send), or until you send a message after it.
pub(crate) fn halted(config_dir: &Path, now: u64) -> bool {
    let Some(stamp) = grokhub_core::desktop_mcp::read_halt_stamp_in(config_dir) else {
        return false;
    };
    halt_holds(stamp, last_send(config_dir), &pace(config_dir), now)
}

/// The cabin's own hold: a Halt at `stamp`, then a send at `sent` ends it.
fn halt_holds(stamp: u64, sent: Option<u64>, pace: &HeartbeatPace, now: u64) -> bool {
    let mut hold = HeartbeatThrottle::default();
    hold.halt(stamp);
    if sent.is_some_and(|at| at > stamp) {
        hold.resume();
    }
    hold.halted(pace, now)
}

/// The cabin's `heartbeat` pace from `app.json`; the default when missing.
fn pace(config_dir: &Path) -> HeartbeatPace {
    grokhub_core::app_json::read(config_dir)
        .ok()
        .and_then(|m| m.get("heartbeat").cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or(PACE_NORMAL)
}

#[cfg(test)]
#[path = "night_tests.rs"]
mod tests;
