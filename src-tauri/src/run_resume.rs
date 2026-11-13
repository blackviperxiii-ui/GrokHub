//! KB-03 on the shell: a paused or interrupted run card resumes from its
//! checkpoint, as the cabin's `work_resume.rs` did. The model gets one short
//! block built from the card's checkpoint and steps
//! ([`res::resume_block`], at most 1,200 bytes), once, as the resume turn's
//! input. The run continues its native session when the model is the same
//! and the session file is intact, else starts fresh with only the block. It
//! keeps the permission mode it started in, never looser than yours now.
//!
//! An automation's or a background run's card resumes as an unattended run on
//! that card; other cards resume in their chat (`workboard::resume`). The
//! agent's own interrupted cards resume by themselves once per interruption,
//! twice a day at most; past that they wait in Blocked and the bell says so.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use grokhub_agent::{CancelToken, ModelClient};
use grokhub_core::work_item::{append_work_log, clip_line, CHECKPOINT_BLOCKER_MAX};
use grokhub_core::work_resume::{self as res, AutoResume, ResumeSession};
use grokhub_core::work_run::{self as wr, RunEnd};
use grokhub_core::{BoardCard, WorkKind};
use serde::Serialize;

use crate::automations::{RunResult, Running};
use crate::unattended::{self, RunGate};

/// Tells the window an auto-resume stopped for the day.
pub const AUTO_RESUME_EVENT: &str = "workboard://auto-resume";

/// Resumed runs going now: card id to its title and the run's stop.
static LIVE: Mutex<BTreeMap<String, (String, CancelToken)>> = Mutex::new(BTreeMap::new());

fn live() -> MutexGuard<'static, BTreeMap<String, (String, CancelToken)>> {
    LIVE.lock().unwrap_or_else(|e| e.into_inner())
}

/// An automation's or a background run's card resumes unattended.
pub fn unattended_card(card: &BoardCard) -> bool {
    matches!(card.kind, WorkKind::Automation | WorkKind::Background)
}

/// Resumed runs going now.
pub fn live_count() -> usize {
    live().len()
}

/// `/bg`'s lines for resumed runs going now.
pub fn status_lines() -> Vec<String> {
    live()
        .values()
        .map(|(title, _)| format!("Running · Resumed: {title}"))
        .collect()
}

/// `/bg stop`: stops every resumed run; each card goes back to Blocked with
/// its checkpoint. How many it stopped.
pub fn stop_all() -> usize {
    let all = live();
    for (_, stop) in all.values() {
        stop.cancel();
    }
    all.len()
}

fn note(how: &ResumeSession) -> &'static str {
    match how {
        ResumeSession::Continue(_) => "resume: same session",
        ResumeSession::Fresh => "resume: fresh session",
    }
}

fn log(config_dir: &Path, id: &str, now: u64, kind: &str, text: &str) {
    let _ = append_work_log(&wr::work_dir(config_dir), id, now, kind, text);
}

fn read_card(config_dir: &Path, id: &str) -> Result<BoardCard, String> {
    wr::read_board_file(&wr::board_path(config_dir))?
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| crate::workboard::GONE.to_string())
}

/// A resume taken: the card is in Doing, and the run goes with [`run`].
pub struct Prepared {
    pub card_id: String,
    automation: Option<grokhub_core::Automation>,
    pub session_id: String,
    /// Continue that session's file.
    pub resume: bool,
    /// The one input the run gets.
    pub block: String,
    pub gate: RunGate,
    /// The attempt this resume started, so a late end leaves a newer one alone.
    attempt: u32,
    _running: Option<Running>,
    /// Held from the claim on, so the card can't resume twice and a second
    /// resume sees the room this one takes.
    live: Live,
}

/// Take card `id` from Blocked into Doing for an unattended resume on `model`.
/// `gate` is the gate a run would get now; the run keeps the mode it started
/// in when that is stricter. `intact` says whether a session file still reads.
/// `auto` spends the interruption's one auto-resume, and refuses a card whose
/// automation is turned off. `others` counts the `/bg` runs going now: with
/// resumed runs they share [`grokhub_core::BG_TASK_MAX`].
pub fn prepare(
    config_dir: &Path,
    id: &str,
    model: &str,
    gate: RunGate,
    auto: bool,
    intact: &dyn Fn(&str) -> bool,
    others: usize,
    now: u64,
) -> Result<Prepared, String> {
    let card = read_card(config_dir, id)?;
    if !unattended_card(&card) {
        return Err("Only an automation's or a background run's card resumes on its own.".into());
    }
    let automation = match card.automation.as_deref() {
        Some(job) => crate::automations::load(config_dir)?
            .into_iter()
            .find(|a| a.id == job),
        None => None,
    };
    if card.automation.is_some() && automation.is_none() {
        // Its tools came from the automation's instructions; without them it
        // would run with every tool.
        return Err("Its automation was removed, so its run can't resume.".into());
    }
    if auto && automation.as_ref().is_some_and(|a| !a.enabled) {
        return Err("Its automation is off.".into());
    }
    let live = Live::claim(id, &card.title, others)?;
    let running = match &automation {
        Some(job) => Some(Running::claim(&job.id)?),
        None => None,
    };
    let taken = wr::edit_board_file(&wr::board_path(config_dir), now, |cards| {
        let c = cards
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or_else(|| crate::workboard::GONE.to_string())?;
        if !crate::workboard::can_resume(c) {
            return Err("Only a paused or interrupted run can resume.".to_string());
        }
        // The block names the cut, so it is built before the resume clears it.
        let block = res::resume_block(c);
        let session = c
            .checkpoint
            .as_ref()
            .map(|cp| cp.session.trim().to_string())
            .unwrap_or_default();
        let how = res::resume_session(c, model, !session.is_empty() && intact(&session));
        let mode = res::resume_mode(&c.run_state.mode, gate.wire());
        let (session_id, resume) = match &how {
            ResumeSession::Continue(s) => (s.clone(), true),
            ResumeSession::Fresh => (format!("shell-resume-{}-{now}", c.id), false),
        };
        if auto {
            res::note_auto_resume(c, true, now);
        }
        res::begin_resume(c, now);
        let kind = c.kind;
        wr::begin_run(c, kind, &session_id, model, now);
        res::claim_run(c, res::HOST_SHELL, mode);
        let attempt = c.checkpoint.as_ref().map(|cp| cp.attempt).unwrap_or(1);
        Ok((block, session_id, resume, gate.in_mode(mode), how, attempt))
    })??;
    let (block, session_id, resume, gate, how, attempt) = taken;
    log(config_dir, id, now, wr::LOG_NOTE, note(&how));
    log(
        config_dir,
        id,
        now,
        wr::LOG_START,
        &format!("attempt {attempt}"),
    );
    Ok(Prepared {
        card_id: id.to_string(),
        automation,
        session_id,
        resume,
        block,
        gate,
        attempt,
        _running: running,
        live,
    })
}

/// Card `id`'s resumed run, held while it goes.
struct Live {
    id: String,
    stop: CancelToken,
}

impl Live {
    /// Claim card `id` for a resumed run, when it has none going and there is
    /// room beside `others` background runs.
    fn claim(id: &str, title: &str, others: usize) -> Result<Self, String> {
        let mut all = live();
        if all.contains_key(id) {
            return Err("That run is already going.".into());
        }
        if others + all.len() >= grokhub_core::BG_TASK_MAX {
            return Err(format!(
                "{} background tasks are already running. /bg stop stops them.",
                grokhub_core::BG_TASK_MAX
            ));
        }
        let stop = CancelToken::new();
        all.insert(id.to_string(), (clip_line(title.trim(), 80), stop.clone()));
        Ok(Self {
            id: id.to_string(),
            stop,
        })
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        live().remove(&self.id);
    }
}

/// Run a prepared resume to its end, and settle its card: done, or
/// Interrupted again with its checkpoint when it failed.
pub fn run(
    config_dir: &Path,
    job: Prepared,
    client: Arc<dyn ModelClient + Send + Sync>,
    model: &str,
    workspace: std::path::PathBuf,
) -> RunResult {
    let _config = grokhub_agent::perm::ConfigGuard::set(config_dir);
    let done = unattended::run(unattended::Job {
        client,
        model: model.to_string(),
        workspace,
        session_id: job.session_id.clone(),
        prompt: job.block.clone(),
        // An automation's run keeps the tools its instructions declare.
        tools_from: job.automation.as_ref().map(|a| a.instructions.clone()),
        gate: job.gate,
        cancel: job.live.stop.clone(),
        resume: job.resume,
    });
    let result = RunResult {
        text: done.text,
        error: done.error,
        stop: done.stop_reason,
    };
    let end = end_of(&result);
    match (&job.automation, &end) {
        (Some(auto), RunEnd::Done | RunEnd::Failed(_)) => {
            crate::automations::board_run_end(config_dir, auto, Some(&job.card_id), &result)
        }
        _ => settle(config_dir, &job.card_id, job.attempt, &end, &result.text),
    }
    result
}

/// How a resumed run ended: `/bg stop` or Halt stops it, so its card waits in
/// Blocked rather than resuming on its own.
fn end_of(done: &RunResult) -> RunEnd {
    match done.stop.as_str() {
        "cancelled" | "halted" => RunEnd::Stopped,
        _ if done.failed() => RunEnd::Failed(done.error.clone()),
        _ => RunEnd::Done,
    }
}

/// A background card's end, as its first run settles it. A card you moved
/// out of Doing on this attempt meanwhile keeps where you put it.
fn settle(config_dir: &Path, id: &str, attempt: u32, end: &RunEnd, text: &str) {
    let now = grokhub_core::now_ms();
    let settled = wr::edit_board_file(&wr::board_path(config_dir), now, |cards| {
        let c = cards.iter_mut().find(|c| {
            c.id == id
                && c.status == grokhub_core::BoardStatus::InProgress
                && c.checkpoint.as_ref().map(|cp| cp.attempt) == Some(attempt)
        })?;
        let cut = res::interrupt_failed(c, end, now);
        if !cut {
            wr::set_run_result(cards, id, text, None, now);
            wr::finish_run_card(cards, id, end, now);
        }
        Some(cut)
    })
    .ok()
    .flatten();
    let Some(cut) = settled else {
        log(
            config_dir,
            id,
            now,
            wr::LOG_NOTE,
            "resume ended after the card moved",
        );
        return;
    };
    let line = match end {
        RunEnd::Failed(why) if cut => format!("interrupted: {}", why.trim()),
        _ => end.log_text(),
    };
    log(config_dir, id, now, wr::LOG_END, &line);
}

/// The agent's own interrupted cards the tick looks at, with what to do.
pub fn auto_picks(config_dir: &Path, now: u64) -> Vec<(BoardCard, AutoResume)> {
    wr::read_board_file(&wr::board_path(config_dir))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|c| res::auto_resume_decision(&c, now).map(|d| (c, d)))
        .collect()
}

/// What the bell says when an auto-resume stops for the day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stopped {
    pub card_id: String,
    pub text: String,
}

/// Twice today already: the card stays Blocked with a blocker that says so.
/// Returns the bell's words; `None` when the card is gone or no longer waits
/// on an auto-resume (you resumed or moved it since the tick read it).
pub fn give_up(config_dir: &Path, id: &str, now: u64) -> Option<Stopped> {
    let title = wr::edit_board_file(&wr::board_path(config_dir), now, |cards| {
        let c = cards.iter_mut().find(|c| c.id == id && awaits_auto(c))?;
        res::note_auto_resume(c, false, now);
        if let Some(cp) = c.checkpoint.as_mut() {
            cp.blocker = res::give_up_blocker();
        }
        Some(c.title.trim().to_string())
    })
    .ok()
    .flatten()?;
    log(
        config_dir,
        id,
        now,
        wr::LOG_NOTE,
        "auto-resume stopped: twice today",
    );
    Some(Stopped {
        card_id: id.to_string(),
        text: format!("Auto-resume stopped: {title}. Resume it from the Workboard."),
    })
}

/// An auto-resume that could not start spends the interruption's try and
/// says why on the card.
pub fn auto_failed(config_dir: &Path, id: &str, why: &str, now: u64) {
    let spent = wr::edit_board_file(&wr::board_path(config_dir), now, |cards| {
        let Some(c) = cards.iter_mut().find(|c| c.id == id && awaits_auto(c)) else {
            return false;
        };
        res::note_auto_resume(c, false, now);
        if let Some(cp) = c.checkpoint.as_mut() {
            cp.blocker = clip_line(
                &format!("Auto-resume failed: {why}"),
                CHECKPOINT_BLOCKER_MAX,
            );
        }
        true
    })
    .unwrap_or(false);
    if spent {
        log(config_dir, id, now, wr::LOG_NOTE, "auto-resume failed");
    }
}

/// Still Blocked on an interruption whose auto-resume is unspent.
fn awaits_auto(c: &BoardCard) -> bool {
    c.status == grokhub_core::BoardStatus::Blocked
        && c.run_state
            .interrupted
            .as_ref()
            .is_some_and(|i| !i.auto_used)
}

/// Whether an auto-resume of `card` waits this tick, its try unspent: its
/// automation is running, background runs are full, or a game holds the
/// desktop a resumed step may drive.
pub fn waits(card: &BoardCard, bg_live: usize, desktop_control: bool, game: Option<&str>) -> bool {
    card.automation
        .as_deref()
        .is_some_and(crate::automations::is_running)
        || bg_live + live_count() >= grokhub_core::BG_TASK_MAX
        || (res::resume_touches_desktop(card, desktop_control) && game.is_some())
}

#[cfg(test)]
#[path = "run_resume_tests.rs"]
mod tests;
