//! KB-03: interrupted work picks up from its card's checkpoint, not from a
//! replayed transcript.
//!
//! - At startup every Doing card this cabin ran with no live run is
//!   Interrupted; so is a run whose worker went away, whose provider ended it
//!   with an error, or whose route moved it on or off the device mid-run.
//! - Resume sends `work_resume::resume_block` (at most 1,200 bytes) once, in the
//!   resume turn's input, never in the system prompt. On the same model with
//!   its session file intact it continues that session; else a fresh session
//!   gets only the block.
//! - Agent-owned automation and background cards resume on their own once per
//!   interruption, at most twice a day, then wait in Blocked with a bell
//!   notice. Work you started waits for Resume.
//! - A resume keeps the permission mode the run started in (never looser than
//!   now) and asks the game guard first when it may drive the desktop.

use std::time::{Duration, Instant};

use super::background::BgSession;
use super::work_log::log_line;
use super::*;
use grokhub_core::work_item::WorkKind;
use grokhub_core::work_resume::{self as res, AutoResume, InterruptCause, ResumeSession};
use grokhub_core::work_run as wr;
use grokhub_core::{BgOrigin, BG_TASK_MAX};

/// How often the auto-resume pass looks at the board.
const AUTO_TICK: Duration = Duration::from_secs(5);

/// The resume turn and the ticks. Memory only.
#[derive(Debug, Default)]
pub(super) struct WorkResumeView {
    /// The block the resume turn carries in its input. Cleared when that turn
    /// ends or a new turn is sent, so later turns add nothing.
    pub follow: Option<String>,
    /// The card the next chat turn resumes, until that turn starts.
    pub pending: Option<String>,
    /// The startup pass ran.
    pub booted: bool,
    /// The route moved the live turn on or off the device: cut it next frame.
    pub swap_cut: bool,
    pub next_auto: Option<Instant>,
}

fn perm_mode(mode: grokhub_core::wire::PermissionMode) -> grokhub_agent::PermMode {
    match mode {
        grokhub_core::wire::PermissionMode::Ask => grokhub_agent::PermMode::Ask,
        grokhub_core::wire::PermissionMode::Auto => grokhub_agent::PermMode::Auto,
        grokhub_core::wire::PermissionMode::AlwaysApprove => grokhub_agent::PermMode::Always,
    }
}

impl Cabin {
    /// Each frame, after `tick_board_notices`. Cheap: the startup pass runs once,
    /// the auto-resume pass every five seconds.
    pub(super) fn tick_work_resume(&mut self) {
        self.interrupt_boot_orphans();
        if std::mem::take(&mut self.work_resume.swap_cut) && self.running && self.inflight_open {
            self.interrupt_turn_card(InterruptCause::ModelSwap);
            self.halt_in_flight();
            self.status = "The model changed mid-run. Resume it from the Workboard.".into();
        }
        let now = Instant::now();
        if self.work_resume.next_auto.is_some_and(|at| now < at) {
            return;
        }
        self.work_resume.next_auto = Some(now + AUTO_TICK);
        self.tick_auto_resume();
    }

    /// Once, after the bell first looked at the board (so these cards ring):
    /// the cabin's own Doing cards with no live run were cut by a restart.
    pub(super) fn interrupt_boot_orphans(&mut self) {
        if self.work_resume.booted || self.board_view.paused_seen.is_none() {
            return;
        }
        self.work_resume.booted = true;
        let live = self.live_card_ids();
        let cut = res::interrupt_orphans(&mut self.board, res::HOST_CABIN, &live, now_ms());
        for id in &cut {
            log_line(
                id,
                wr::LOG_END,
                &format!("interrupted: {}", InterruptCause::Restart.text()),
            );
        }
        if !cut.is_empty() {
            self.flush_board();
        }
    }

    /// The live turn was cut short (worker gone, provider error, model swap),
    /// not stopped: its card is Interrupted where it was.
    pub(super) fn interrupt_turn_card(&mut self, cause: InterruptCause) {
        self.drop_pending_resume();
        if !self.inflight_open {
            return;
        }
        self.inflight_open = false;
        self.run_tools_logged.clear();
        let Some(thread) = self.chat_job_thread.clone() else {
            return;
        };
        let card = wr::run_card_index(&self.board, &thread).map(|i| self.board[i].id.clone());
        if grokhub_core::interrupt_inflight_card(&mut self.board, &thread, cause, now_ms()) {
            if let Some(id) = card {
                log_line(&id, wr::LOG_END, &format!("interrupted: {}", cause.text()));
            }
            self.flush_board();
        }
    }

    /// A resume turn that never started puts its card back in Blocked, and its
    /// block goes nowhere.
    pub(super) fn drop_pending_resume(&mut self) {
        self.work_resume.follow = None;
        let Some(id) = self.work_resume.pending.take() else {
            return;
        };
        let thread = self
            .board
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| c.thread_id.clone());
        if let Some(thread) = thread {
            if abandon_inflight_card(&mut self.board, &thread) {
                self.flush_board();
            }
        }
    }

    /// Resume `id` now. `auto`: the auto-resume pass, which only takes the
    /// agent's own automation and background cards.
    pub(super) fn resume_card_now(&mut self, id: &str, auto: bool) -> Result<(), String> {
        let card = self
            .board
            .iter()
            .find(|c| c.id == id)
            .cloned()
            .ok_or_else(|| "That card is gone".to_string())?;
        if !super::board_ui::card_can_resume(&card) {
            return Err("Only a paused or interrupted card resumes".into());
        }
        if let Some(why) = self.resume_guard_blocks(&card) {
            return Err(why);
        }
        if !self.agent_ready() {
            return Err(self.no_agent_note().into());
        }
        match card.kind {
            WorkKind::Automation | WorkKind::Background => self.resume_bg_card(&card, auto),
            _ if auto => Err("Work you started waits for Resume".into()),
            _ => {
                self.prepare_chat_resume(&card)?;
                self.kick_model(false);
                Ok(())
            }
        }
    }

    /// The hard no-cheat rule: a resumed run that may drive the desktop asks the
    /// game guard first, and waits while a game or an anti-cheat runs.
    fn resume_guard_blocks(&self, card: &BoardCard) -> Option<String> {
        if !res::resume_touches_desktop(card, self.cfg.desktop_control) {
            return None;
        }
        crate::desktop_mcp::game_guard()
            .blocks(grokhub_desktop::game_guard::Guarded::DesktopControl)
    }

    /// Set up a resume in the card's chat; the caller kicks the turn. The block
    /// rides in that one turn's input.
    pub(super) fn prepare_chat_resume(&mut self, card: &BoardCard) -> Result<(), String> {
        if self.running {
            return Err("Finish the open chat first".into());
        }
        let thread_id = match card
            .thread_id
            .clone()
            .filter(|tid| self.threads.iter().any(|t| &t.id == tid))
        {
            Some(tid) => tid,
            None => self.make_card_thread(&card.title, false),
        };
        let cp_session = card
            .checkpoint
            .as_ref()
            .map(|c| c.session.trim().to_string())
            .unwrap_or_default();
        let bound = self
            .threads
            .iter()
            .find(|t| t.id == thread_id)
            .and_then(|t| t.grok_session.clone())
            .unwrap_or_default();
        let intact = !cp_session.is_empty()
            && bound.trim() == cp_session
            && grokhub_agent::session_intact(&cp_session);
        let how = res::resume_session(card, &self.run_model(), intact);
        if how == ResumeSession::Fresh {
            // Only the block reaches the model: a new session, and no recap of
            // earlier turns rides along.
            if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
                t.grok_session = Some(format!("native-{}", grokhub_core::uid("n")));
                t.native_carry = false;
            }
        }
        let block = res::resume_block(card);
        let now = now_ms();
        if !grokhub_core::resume_inflight_card(&mut self.board, &card.id, &thread_id) {
            return Err("Only a paused or interrupted card resumes".into());
        }
        if let Some(c) = self.board.iter_mut().find(|c| c.id == card.id) {
            res::begin_resume(c, now);
        }
        log_line(&card.id, wr::LOG_NOTE, resume_note(&how));
        self.card_notes_follow = grokhub_core::take_card_notes_block(&mut self.board, &thread_id);
        self.bg.results_follow = None;
        self.board_view.note = None;
        self.work_resume.follow = Some(block);
        self.work_resume.pending = Some(card.id.clone());
        self.chat_job_thread = Some(thread_id);
        self.push_bound_msg("user", format!("Resume: {}", card.title.trim()));
        self.flush_board();
        self.persist();
        Ok(())
    }

    /// Resume an automation or background card as a background run on its own
    /// card, in the mode it started in.
    pub(super) fn resume_bg_card(&mut self, card: &BoardCard, auto: bool) -> Result<(), String> {
        let origin = if card.kind == WorkKind::Automation {
            BgOrigin::Scheduled
        } else {
            BgOrigin::Agent
        };
        if origin == BgOrigin::Scheduled && self.bg.scheduled_live() {
            return Err("A scheduled run is going. Resume after it ends".into());
        }
        if origin != BgOrigin::Scheduled && self.bg.live_count() >= BG_TASK_MAX {
            return Err(format!(
                "{BG_TASK_MAX} background tasks are already running — stop one first"
            ));
        }
        let thread_id = match card
            .thread_id
            .clone()
            .filter(|tid| self.threads.iter().any(|t| &t.id == tid))
        {
            Some(tid) => tid,
            None => {
                let idx = self.ensure_background_history_thread();
                self.threads[idx].id.clone()
            }
        };
        let cp_session = card
            .checkpoint
            .as_ref()
            .map(|c| c.session.trim().to_string())
            .unwrap_or_default();
        let intact = !cp_session.is_empty() && grokhub_agent::session_intact(&cp_session);
        let model = self.run_model();
        let how = res::resume_session(card, &model, intact);
        let mode = res::resume_mode(&card.run_state.mode, self.permission_mode);
        let mut gate = self.native_bg_gate();
        gate.mode = perm_mode(mode);
        let session = match &how {
            ResumeSession::Continue(id) => BgSession::Continue(id.clone()),
            ResumeSession::Fresh => BgSession::Fresh,
        };
        let block = res::resume_block(card);
        let (run_id, _, native) =
            self.start_native_bg_on(&block, &thread_id, origin, session, gate)?;
        if let Some(run) = self.bg.runs.iter_mut().find(|r| r.id == run_id) {
            run.title = card.title.clone();
            run.automation = card.automation.clone();
        }
        let now = now_ms();
        let mut attempt = 1;
        if let Some(c) = self.board.iter_mut().find(|c| c.id == card.id) {
            if auto {
                res::note_auto_resume(c, true, now);
            }
            res::begin_resume(c, now);
            let kind = c.kind;
            wr::begin_run(c, kind, &native, &model, now);
            res::claim_run(c, res::HOST_CABIN, mode);
            attempt = c.checkpoint.as_ref().map(|cp| cp.attempt).unwrap_or(1);
        }
        self.bg.cards.push((run_id, card.id.clone()));
        log_line(&card.id, wr::LOG_NOTE, resume_note(&how));
        log_line(&card.id, wr::LOG_START, &format!("attempt {attempt}"));
        self.flush_board();
        Ok(())
    }

    /// Every five seconds: the agent's own interrupted cards resume once per
    /// interruption, twice a day at most; past that they wait with a bell.
    pub(super) fn tick_auto_resume(&mut self) {
        let now = now_ms();
        let picks: Vec<(String, AutoResume)> = self
            .board
            .iter()
            .filter_map(|c| res::auto_resume_decision(c, now).map(|d| (c.id.clone(), d)))
            .collect();
        let mut changed = false;
        for (id, decision) in picks {
            match decision {
                AutoResume::GiveUp => {
                    self.give_up_auto_resume(&id, now);
                    changed = true;
                }
                AutoResume::Go => {
                    let Some(card) = self.board.iter().find(|c| c.id == id).cloned() else {
                        continue;
                    };
                    // Busy, signed out, or a game running: it waits, unspent.
                    let busy = if card.kind == WorkKind::Automation {
                        self.bg.scheduled_live()
                    } else {
                        self.bg.live_count() >= BG_TASK_MAX
                    };
                    if busy || !self.agent_ready() || self.resume_guard_blocks(&card).is_some() {
                        continue;
                    }
                    if let Err(why) = self.resume_card_now(&id, true) {
                        if let Some(c) = self.board.iter_mut().find(|c| c.id == id) {
                            res::note_auto_resume(c, false, now);
                            if let Some(cp) = c.checkpoint.as_mut() {
                                cp.blocker = grokhub_core::work_item::clip_line(
                                    &format!("Auto-resume failed: {why}"),
                                    grokhub_core::work_item::CHECKPOINT_BLOCKER_MAX,
                                );
                            }
                        }
                        log_line(&id, wr::LOG_NOTE, "auto-resume failed");
                        changed = true;
                    }
                }
            }
        }
        if changed {
            self.flush_board();
        }
    }

    /// Two auto-resumes today already: the card stays Blocked and the bell says so.
    fn give_up_auto_resume(&mut self, id: &str, now: u64) {
        let Some(c) = self.board.iter_mut().find(|c| c.id == id) else {
            return;
        };
        res::note_auto_resume(c, false, now);
        if let Some(cp) = c.checkpoint.as_mut() {
            cp.blocker = res::give_up_blocker();
        }
        let title = c.title.trim().to_string();
        log_line(id, wr::LOG_NOTE, "auto-resume stopped: twice today");
        // This bell replaces the plain "Interrupted" one for the card.
        if let Some(seen) = self.board_view.paused_seen.as_mut() {
            seen.insert(id.to_string());
        }
        self.notify(
            super::notices::NoticeKind::Info,
            format!("Auto-resume stopped: {title}. Resume it from the Workboard."),
            Some(super::notices::NoticeAction::OpenCard(id.to_string())),
        );
    }
}

fn resume_note(how: &ResumeSession) -> &'static str {
    match how {
        ResumeSession::Continue(_) => "resume: same session",
        ResumeSession::Fresh => "resume: fresh session",
    }
}
