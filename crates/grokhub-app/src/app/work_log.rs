//! KB-02: agent work logs itself on the board. A task turn, a background run,
//! a scheduled automation and a desktop session each open or update a card and
//! checkpoint it at every step: one progress log line per tool, the plan from
//! `todo_write`, the files a write changed, and the final checkpoint on settle
//! or Stop. Chats file no card: the turn hooks only follow a card that
//! `note_inflight_card` filed for a work ask.
//!
//! All of it is local file work in `grokhub_core::work_run`. No model is
//! involved and nothing here reaches a prompt (KB-04).

use super::*;
use grokhub_core::work_item::{append_work_log, WorkKind, WorkOrigin};
use grokhub_core::work_resume::{self as res, HOST_CABIN};
use grokhub_core::work_run::{self as wr, RunEnd};

/// Native tools that write files. Their checkpoint step's files become refs.
const FILE_TOOLS: &[&str] = &["write", "search_replace"];

pub(super) fn log_line(card_id: &str, kind: &str, text: &str) {
    let _ = append_work_log(
        &crate::work_store::work_dir(),
        card_id,
        now_ms(),
        kind,
        text,
    );
}

/// The progress log line of a run that ended Interrupted.
pub(super) fn interrupted_log_text(end: &RunEnd) -> String {
    match end {
        RunEnd::Failed(why) => format!("interrupted: {}", why.trim()),
        other => format!("interrupted: {}", other.log_text()),
    }
}

/// `completed` is ok, `failed` is not; anything else is still running.
fn tool_outcome(card: &ToolCard) -> Option<bool> {
    match card.status.as_str() {
        "completed" => Some(true),
        "failed" => Some(false),
        _ => None,
    }
}

fn tool_name(card: &ToolCard) -> String {
    if card.kind.trim().is_empty() {
        card.title.clone()
    } else {
        card.kind.clone()
    }
}

impl Cabin {
    /// The model a run starts on: the cabin's model, or the device's tag while
    /// the route falls back to Local AI. Resume compares against it (KB-03).
    pub(super) fn run_model(&self) -> String {
        res::run_model_tag(
            grokhub_core::cabin_spawn_model(&self.cfg.model),
            grokhub_agent::route::local::fallback_active(),
        )
    }

    /// The native session a chat turn runs on, for its todos and changed files.
    fn run_session(&self, thread_id: &str) -> String {
        self.threads
            .iter()
            .find(|t| t.id == thread_id)
            .and_then(|t| t.grok_session.clone())
            .filter(|s| !s.trim().is_empty())
            .or_else(|| self.acp.as_ref().map(|h| h.session_id.clone()))
            .unwrap_or_default()
    }

    /// One finished tool call on card `idx`: a log line, then the plan for
    /// `todo_write` and the refs for a file write. True when the card changed.
    fn checkpoint_tool(&mut self, idx: usize, card: &ToolCard, ok: bool, session: &str) -> bool {
        let name = tool_name(card);
        let id = self.board[idx].id.clone();
        log_line(&id, wr::LOG_TOOL, &wr::tool_log_text(&name, ok));
        if !ok || session.trim().is_empty() {
            return false;
        }
        let now = now_ms();
        if name == "todo_write" {
            let todos: Vec<(String, String)> = grokhub_agent::todos_for(session)
                .into_iter()
                .map(|t| (t.content, t.status))
                .collect();
            let changed = wr::mirror_todos(&mut self.board[idx], &todos, now);
            if changed {
                let next = self.board[idx]
                    .checkpoint
                    .as_ref()
                    .map(|c| c.next.clone())
                    .unwrap_or_default();
                log_line(&id, wr::LOG_STEP, &format!("next: {next}"));
            }
            return changed;
        }
        if FILE_TOOLS.contains(&name.as_str()) {
            let files = grokhub_agent::list_steps(session)
                .pop()
                .map(|s| s.files)
                .unwrap_or_default();
            return wr::note_refs(&mut self.board[idx], &files, now);
        }
        false
    }

    /// `note_inflight_card` filed or reused this chat's card: the run starts on
    /// it with its kind (a desktop session when an episode is open on the chat),
    /// session and model.
    pub(super) fn begin_run_card(&mut self, thread_id: &str) {
        self.run_tools_logged.clear();
        let Some(idx) = wr::run_card_index(&self.board, thread_id) else {
            return;
        };
        let kind = if self
            .harness
            .episode
            .as_ref()
            .is_some_and(|e| e.chat_id == thread_id)
        {
            WorkKind::Episode
        } else {
            WorkKind::Task
        };
        let session = self.run_session(thread_id);
        let model = self.run_model();
        wr::begin_run(&mut self.board[idx], kind, &session, &model, now_ms());
        res::claim_run(&mut self.board[idx], HOST_CABIN, self.permission_mode);
        let attempt = self.board[idx]
            .checkpoint
            .as_ref()
            .map(|c| c.attempt)
            .unwrap_or(1);
        log_line(
            &self.board[idx].id,
            wr::LOG_START,
            &format!("attempt {attempt}"),
        );
    }

    /// A tool card of the live turn. Only a finished call on a card this turn
    /// filed counts, once per call.
    pub(super) fn log_run_tool(&mut self, card: &ToolCard) {
        if !self.inflight_open {
            return;
        }
        let Some(ok) = tool_outcome(card) else {
            return;
        };
        if !card.id.is_empty() {
            if self.run_tools_logged.contains(&card.id) {
                return;
            }
            self.run_tools_logged.push(card.id.clone());
        }
        let Some(thread) = self.chat_job_thread.clone() else {
            return;
        };
        let Some(idx) = wr::run_card_index(&self.board, &thread) else {
            return;
        };
        if self.board[idx].status != BoardStatus::InProgress {
            return;
        }
        let session = self.run_session(&thread);
        let mut changed = self.checkpoint_tool(idx, card, ok, &session);
        changed |= self.check_run_swap(idx);
        if changed {
            self.flush_board();
        }
    }

    /// KB-03: the route moved this turn on or off the device since it started.
    /// The first finished call re-reads the route (the start reading can be the
    /// previous turn's); a later change cuts the turn on the next frame, so it
    /// resumes from the checkpoint instead of replaying the chat to another model.
    fn check_run_swap(&mut self, idx: usize) -> bool {
        let on_device = grokhub_agent::route::local::fallback_active();
        let started = self.board[idx]
            .checkpoint
            .as_ref()
            .map(|c| c.model.clone())
            .unwrap_or_default();
        if !res::model_swapped(&started, on_device) {
            return false;
        }
        if self.run_tools_logged.len() <= 1 {
            let model = self.run_model();
            if let Some(cp) = self.board[idx].checkpoint.as_mut() {
                cp.model = model;
            }
            return true;
        }
        self.work_resume.swap_cut = true;
        false
    }

    /// The final checkpoint on `thread_id`'s card while it is still in Doing,
    /// before settle or abandon moves it. The caller flushes.
    pub(super) fn end_run_card(&mut self, thread_id: &str, end: &RunEnd) {
        self.run_tools_logged.clear();
        let Some(idx) = wr::run_card_index(&self.board, thread_id) else {
            return;
        };
        if self.board[idx].status != BoardStatus::InProgress {
            return;
        }
        wr::end_run(&mut self.board[idx], end, now_ms());
        log_line(&self.board[idx].id, wr::LOG_END, &end.log_text());
    }

    /// A desktop session ended: one line on its chat's card.
    pub(super) fn log_episode_end(&mut self, chat_id: &str, why: &str) {
        if let Some(idx) = wr::run_card_index(&self.board, chat_id) {
            log_line(
                &self.board[idx].id,
                wr::LOG_END,
                &format!("desktop session: {why}"),
            );
        }
    }

    /// A background run started (`/bg` or a `BACKGROUND_TASK:` line): its own
    /// card in Doing.
    /// `yours`: you started it with `/bg`, so it never resumes on its own.
    pub(super) fn open_bg_card(&mut self, run_id: u64, title: &str, session: &str, yours: bool) {
        let model = self.run_model();
        let id = wr::open_run_card(
            &mut self.board,
            WorkKind::Background,
            title,
            session,
            &model,
            now_ms(),
        );
        if let Some(c) = self.board.iter_mut().find(|c| c.id == id) {
            res::claim_run(c, HOST_CABIN, self.permission_mode);
            if yours {
                c.origin = Some(WorkOrigin::User);
            }
        }
        log_line(&id, wr::LOG_START, "attempt 1");
        self.bg.cards.push((run_id, id));
        self.flush_board();
    }

    /// A scheduled run started: the automation's one card, back in Doing.
    pub(super) fn open_automation_card(&mut self, run_id: u64, automation_id: &str, name: &str) {
        let session = self
            .bg
            .runs
            .iter()
            .find(|r| r.id == run_id)
            .and_then(|r| r.native_session.clone())
            .unwrap_or_default();
        let model = self.run_model();
        let id = wr::begin_automation_card(
            &mut self.board,
            automation_id,
            name,
            &session,
            &model,
            now_ms(),
        );
        let mode = self.permission_mode;
        let attempt = self
            .board
            .iter_mut()
            .find(|c| c.id == id)
            .map(|c| {
                res::claim_run(c, HOST_CABIN, mode);
                c.checkpoint.as_ref().map(|cp| cp.attempt).unwrap_or(1)
            })
            .unwrap_or(1);
        log_line(&id, wr::LOG_START, &format!("attempt {attempt}"));
        self.bg.cards.push((run_id, id));
        self.flush_board();
    }

    /// Finished tool calls of background runs, as `poll_bg_runs` drained them.
    pub(super) fn log_bg_tools(&mut self, tools: Vec<(u64, String, ToolCard)>) {
        let mut changed = false;
        for (run_id, session, card) in tools {
            let Some(ok) = tool_outcome(&card) else {
                continue;
            };
            let Some(card_id) = self.bg_card_id(run_id) else {
                continue;
            };
            let Some(idx) = self.board.iter().position(|c| c.id == card_id) else {
                continue;
            };
            changed |= self.checkpoint_tool(idx, &card, ok, &session);
        }
        if changed {
            self.flush_board();
        }
    }

    pub(super) fn bg_card_id(&self, run_id: u64) -> Option<String> {
        self.bg
            .cards
            .iter()
            .find(|(r, _)| *r == run_id)
            .map(|(_, id)| id.clone())
    }

    /// A background run ended: the final checkpoint, and Done or Blocked. A
    /// scheduled run's card is the automation's; `settle_scheduled_run` moves it.
    /// The caller flushes.
    pub(super) fn finish_bg_card(&mut self, run_id: u64, end: &RunEnd, scheduled: bool) {
        let Some(at) = self.bg.cards.iter().position(|(r, _)| *r == run_id) else {
            return;
        };
        let (_, card_id) = self.bg.cards.remove(at);
        if scheduled {
            return;
        }
        let now = now_ms();
        let interrupted = self
            .board
            .iter_mut()
            .find(|c| c.id == card_id)
            .is_some_and(|c| res::interrupt_failed(c, end, now));
        if interrupted {
            log_line(&card_id, wr::LOG_END, &interrupted_log_text(end));
        } else if wr::finish_run_card(&mut self.board, &card_id, end, now) {
            log_line(&card_id, wr::LOG_END, &end.log_text());
        }
    }

    /// A scheduled run ended: the automation's card gets its final checkpoint.
    /// With a report it waits for `file_automation_follow_up`.
    pub(super) fn finish_automation_card(
        &mut self,
        automation_id: &str,
        end: &RunEnd,
        report: bool,
    ) {
        let follow_up = *end == RunEnd::Done && report;
        let now = now_ms();
        let cut = self
            .board
            .iter_mut()
            .rev()
            .find(|c| {
                c.automation.as_deref() == Some(automation_id)
                    && c.status == BoardStatus::InProgress
            })
            .and_then(|c| res::interrupt_failed(c, end, now).then(|| c.id.clone()));
        if let Some(id) = cut {
            log_line(&id, wr::LOG_END, &interrupted_log_text(end));
            self.flush_board();
            return;
        }
        if let Some(id) =
            wr::finish_automation_card(&mut self.board, automation_id, end, follow_up, now_ms())
        {
            log_line(&id, wr::LOG_END, &end.log_text());
            self.flush_board();
        }
    }

    /// Cards of runs that are still live: the sweep never archives them.
    pub(super) fn live_card_ids(&self) -> Vec<String> {
        self.chat_job_thread
            .iter()
            .cloned()
            .chain(self.bg.cards.iter().map(|(_, id)| id.clone()))
            .collect()
    }
}
