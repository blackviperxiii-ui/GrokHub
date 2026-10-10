//! Background runs beside the chat, and steering a live reply.
//!
//! The composer owns one live turn (`grok_p_rx` / ACP). A background run is a
//! separate headless `grok -p` with its own receiver: `/bg <task>`, a
//! `BACKGROUND_TASK:` line in Grok's reply, or a live reply moved off the
//! composer. It forks the chat's session instead of writing into it, and its
//! reply is posted on the chat that started it once that chat is not mid-turn.
//!
//! Steering is a message typed while this chat's reply runs: the turn stops
//! where it is, what it said stays in the transcript, and the next turn carries
//! the new message with a note of the progress (`steer_follow_block`).

use super::*;
use grokhub_agent::harness as hx;
use grokhub_agent::Engine;
use grokhub_core::{
    bg_elapsed_label, bg_result_note, bg_result_post, bg_results_follow, bg_task_prompt,
    bg_task_title, can_detach_turn, chat_run_dot_alpha, extract_background_tasks, steer_follow_block,
    BgEnd, BgOrigin, BG_TASK_MAX,
};

/// `/bg` and a model `BACKGROUND_TASK:` line while Ask is on. A background run
/// has nobody to approve a tool, so it does not start.
/// Span tool for a scheduled run starting, and the trace `/loop` runs share.
pub(super) const AUTOMATION_TOOL: &str = "automation.run";
pub(super) const LOOP_TRACE: &str = "loops";

/// Span origin for a background run: scheduled jobs are automation.
pub(super) fn bg_span_origin(origin: BgOrigin) -> hx::Origin {
    if origin == BgOrigin::Scheduled {
        hx::Origin::Automation
    } else {
        hx::Origin::User
    }
}

pub(super) const BG_ASK_OFF: &str = "Background tasks are off while Ask is on, because they can't ask you for approval. Switch to Auto to use /bg.";

/// Session ids headless work reported from a worker thread. The UI files them
/// on the hidden Background chat (`file_hidden_sessions`) so they stay out of History.
static HIDDEN_SESSIONS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Keep a session that background work wrote out of the sidebar. Any thread may call this.
pub(super) fn hide_background_session(id: &str) {
    let id = id.trim();
    if id.is_empty() {
        return;
    }
    if let Ok(mut held) = HIDDEN_SESSIONS.lock() {
        if !held.iter().any(|s| s == id) {
            held.push(id.to_string());
        }
    }
}

/// One background `grok -p`. Not saved: the child dies with the cabin.
pub(super) struct BgRun {
    pub id: u64,
    pub thread_id: String,
    pub title: String,
    pub origin: BgOrigin,
    pub pid: Option<u32>,
    pub rx: Option<mpsc::Receiver<GrokPEvent>>,
    /// Reply text so far (not thoughts).
    pub say: String,
    /// Newest tool title, for the strip.
    pub action: String,
    pub started: Instant,
    pub end: Option<BgEnd>,
    /// Session the child reported at its end.
    pub session: String,
    /// Session the child resumed. A moved-off turn writes into it.
    pub resumed: Option<String>,
    /// This run set `grok_fork` on its chat so the next composer turn forks
    /// instead of writing the same session at the same time.
    pub fork_hold: bool,
    /// Native `/bg` session. Halt and delete cancel this engine. CLI runs leave it empty.
    pub native_session: Option<String>,
    /// The automation a scheduled run settles when it ends.
    pub automation: Option<String>,
}

impl BgRun {
    pub fn live(&self) -> bool {
        self.end.is_none()
    }

    fn stop_native(&self) {
        if let Some(id) = &self.native_session {
            grokhub_agent::cancel_session(id);
        }
    }
}

/// Background runs and the blocks the next turn carries. Not saved.
#[derive(Default)]
pub(super) struct BgWork {
    pub runs: Vec<BgRun>,
    pub next_id: u64,
    /// `(thread id, note)` for runs that ended since that chat's last turn.
    pub unread: Vec<(String, String)>,
    /// Background results for the turn being kicked. Set on each send.
    pub results_follow: Option<String>,
    /// Steer context for the turn being kicked. Set on each send.
    pub steer_follow: Option<String>,
    /// Alt+Enter (or `/queue`): this send waits for the live reply instead of steering it.
    pub queue_next: bool,
}

impl BgWork {
    /// Your live background tasks. Scheduled runs have their own slot and do not count.
    pub fn live_count(&self) -> usize {
        self.runs
            .iter()
            .filter(|r| r.live() && r.origin != BgOrigin::Scheduled)
            .count()
    }

    /// A scheduled automation is running in its own process.
    pub fn scheduled_live(&self) -> bool {
        self.runs.iter().any(|r| r.live() && r.origin == BgOrigin::Scheduled)
    }

    pub fn busy(&self) -> bool {
        !self.runs.is_empty()
    }
}

/// What a click on the live-work strip asks for. Applied after painting.
enum LiveWorkAct {
    Steer,
    Queue,
    SteerQueued(usize),
    DropQueued(usize),
    StopRun(u64),
}

impl Cabin {
    /// A message onto `thread_id`, visible tab or not. A new message is activity.
    pub(super) fn push_msg_on(&mut self, thread_id: &str, role: &str, content: String) {
        let content = self.scrub_transcript(take_ui_text(content, IMAGE_FILE_CAP));
        if thread_id == self.visible_thread_id() {
            self.live_mut().push((role.to_string(), content));
        } else if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
            t.messages_mut().push((role.to_string(), content));
        } else {
            return;
        }
        if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
            t.accessed_ms = now_ms();
        }
    }

    /// Start `task` as a background run beside `thread_id`. It forks that chat's
    /// Grok session, so it knows the conversation and never writes into it.
    /// Does not refuse under Ask — `/bg` and `BACKGROUND_TASK:` do. It still
    /// spawns, with deny args when Ask is on.
    pub(super) fn start_bg_task(
        &mut self,
        task: &str,
        thread_id: &str,
        origin: BgOrigin,
    ) -> Result<String, String> {
        let task = task.trim();
        if task.is_empty() {
            return Err("Nothing to run".into());
        }
        // A scheduled run has its own slot; `tick_night` starts one at a time.
        if origin != BgOrigin::Scheduled && self.bg.live_count() >= BG_TASK_MAX {
            return Err(format!(
                "{BG_TASK_MAX} background tasks are already running — stop one first"
            ));
        }
        if self.native_bg_target(thread_id) {
            return self.start_native_bg(task, thread_id, origin);
        }
        if !self.can_agent() {
            return Err("Install Grok Build (x.ai/cli) or Connect Grok in Settings".into());
        }
        let idx = self.threads.iter().position(|t| t.id == thread_id);
        let thread = idx.and_then(|i| self.threads.get(i));
        let cwd = thread
            .and_then(|t| t.grok_cwd.clone())
            .filter(|s| !s.trim().is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| self.grok_cwd());
        let resume = thread
            .and_then(|t| t.grok_session.clone())
            .filter(|s| !s.trim().is_empty());
        let resume_in_cabin = resume
            .as_deref()
            .is_some_and(grokhub_acp::cabin_has_session);
        let user_home = grokhub_acp::use_user_grok_home(
            thread.map(|t| t.grok_user_home).unwrap_or(false),
            resume_in_cabin,
        );
        let worktree = thread.map(|t| t.grok_worktree).unwrap_or(false);
        let (yolo, auto) = self.permission_mode.scheduled_flags();
        let model = grokhub_core::cabin_spawn_model(&self.cfg.model).to_string();
        let effort = self.bg_effort(origin);
        let prompt = bg_task_prompt(task);
        let (pid, rx) = grokhub_acp::spawn_grok_p_stream(
            &prompt,
            &cwd,
            resume.as_deref(),
            yolo,
            auto,
            Some(model.as_str()),
            effort.as_deref(),
            self.session_mode,
            grokhub_acp::GrokPAttach {
                image: None,
                learned: &grokhub_core::brief_for(&self.learning, "chat"),
                deny: self.permission_mode.needs_approval(),
                desktop: self.cfg.desktop_control,
                hard_deny: &grokhub_agent::harness::HEADLESS_DENY_RULES,
            },
            resume.is_some(),
            user_home,
            worktree,
        )?;
        let title = bg_task_title(task);
        self.bg.next_id += 1;
        self.bg.runs.push(BgRun {
            id: self.bg.next_id,
            thread_id: thread_id.to_string(),
            title: title.clone(),
            origin,
            pid: Some(pid),
            rx: Some(rx),
            say: String::new(),
            action: String::new(),
            started: Instant::now(),
            end: None,
            session: String::new(),
            resumed: resume,
            fork_hold: false,
            native_session: None,
            automation: None,
        });
        Ok(title)
    }

    /// A scheduled job or `/loop` run started (Spike-4c): one span with origin
    /// `automation`. The job id only, never its instructions.
    pub(super) fn automation_span(&self, trace: &str, job: &str) {
        let args = serde_json::json!({ "job": job }).to_string();
        let driver = if self.cfg.native_engine() { "native" } else { "grok_build" };
        let span = hx::Span::soft_allow(trace, AUTOMATION_TOOL, &args, "started", "scheduled run", self.access_mode(), driver)
            .from_origin(hx::Origin::Automation)
            .on_path("automation");
        let _ = hx::append_span(&crate::config::config_dir(), &span);
    }

    /// Reasoning effort for a background run. A scheduled automation runs
    /// unwatched, so it stays at low effort like every unattended run; your own
    /// `/bg` work starts at everyday chat's start (the router moves it from there).
    pub(super) fn bg_effort(&self, origin: BgOrigin) -> Option<String> {
        if origin == BgOrigin::Scheduled {
            Some(grokhub_core::BACKGROUND_EFFORT.to_string())
        } else {
            grokhub_agent::route::live::start_effort(grokhub_agent::route::live::DEFAULT_CLASS)
        }
    }

    fn native_bg_target(&self, thread_id: &str) -> bool {
        self.cfg.native_engine() && self.threads.iter().any(|t| t.id == thread_id)
    }

    fn native_bg_gate(&self) -> grokhub_agent::Gate {
        let readonly = matches!(
            self.session_mode,
            grokhub_acp::SessionMode::Plan | grokhub_acp::SessionMode::Ask
        );
        let mode = match self.permission_mode {
            grokhub_acp::PermissionMode::Ask => grokhub_agent::PermMode::Ask,
            grokhub_acp::PermissionMode::Auto => grokhub_agent::PermMode::Auto,
            grokhub_acp::PermissionMode::AlwaysApprove => grokhub_agent::PermMode::Always,
        };
        grokhub_agent::Gate {
            mode,
            readonly_session: readonly,
            attended: false,
            desktop: self.cfg.desktop_control,
        }
    }

    fn start_native_bg(
        &mut self,
        task: &str,
        thread_id: &str,
        origin: BgOrigin,
    ) -> Result<String, String> {
        let (cwd, parent) = {
            let thread = self.threads.iter().find(|t| t.id == thread_id);
            let cwd = thread
                .and_then(|t| t.grok_cwd.clone())
                .filter(|s| !s.trim().is_empty());
            let parent = thread
                .and_then(|t| t.grok_session.clone())
                .filter(|s| !s.trim().is_empty());
            (cwd, parent)
        };
        let workspace = cwd
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| self.grok_cwd());
        let child_id = if let Some(parent) = parent.as_deref() {
            grokhub_agent::fork_session(parent)
                .map(|info| info.id)
                .unwrap_or_else(|_| format!("native-{}", grokhub_core::uid("n")))
        } else {
            format!("native-{}", grokhub_core::uid("n"))
        };
        if let Some(parent) = parent.as_deref() {
            grokhub_agent::link_child(parent, &child_id);
        }
        let (client, auth_kind, bearer) = native_bg_model(self)?;
        let gate = self.native_bg_gate();
        let model = grokhub_core::cabin_spawn_model(&self.cfg.model).to_string();
        let effort = self.bg_effort(origin);
        let rules = grokhub_acp::cabin_rules_for(
            &grokhub_core::brief_for(&self.learning, "chat"),
            self.cfg.desktop_control,
        );
        let system = grokhub_agent::system_prompt(&rules, &workspace);
        let prompt = bg_task_prompt(task);
        let cancel = grokhub_agent::CancelToken::new();
        let _ = grokhub_agent::hub_for(&child_id);
        grokhub_agent::watch_cancel(&child_id, cancel.clone());
        let (tx, rx) = mpsc::channel();
        let session = child_id.clone();
        let started_by = bg_span_origin(origin);
        std::thread::spawn(move || {
            // The engine and its egress lines take this thread's origin.
            let _origin = grokhub_agent::harness::OriginScope::enter(started_by);
            run_native_bg(
                session, workspace, client, auth_kind, bearer, gate, model, effort, system, prompt,
                cancel, tx,
            );
        });
        let title = bg_task_title(task);
        self.bg.next_id += 1;
        self.bg.runs.push(BgRun {
            id: self.bg.next_id,
            thread_id: thread_id.to_string(),
            title: title.clone(),
            origin,
            pid: None,
            rx: Some(rx),
            say: String::new(),
            action: String::new(),
            started: Instant::now(),
            end: None,
            session: String::new(),
            resumed: parent,
            fork_hold: false,
            native_session: Some(child_id),
            automation: None,
        });
        Ok(title)
    }

    /// `BACKGROUND_TASK:` lines in a finished chat reply. Ones that do not fit
    /// are told to the next turn so Grok knows they never ran. Ask starts none:
    /// a background run cannot ask for approval.
    pub(super) fn start_agent_bg_tasks(&mut self, reply: &str, thread_id: &str) {
        let tasks = extract_background_tasks(reply, BG_TASK_MAX);
        if tasks.is_empty() {
            return;
        }
        if self.permission_mode.needs_approval() {
            for task in &tasks {
                let title = bg_task_title(task);
                self.bg.unread.push((
                    thread_id.to_string(),
                    format!("- {title} (not started: background tasks are off while Ask is on)"),
                ));
            }
            if thread_id == self.visible_thread_id() {
                self.status = BG_ASK_OFF.into();
            }
            return;
        }
        let mut started = 0usize;
        let mut refused = Vec::new();
        for task in tasks {
            match self.start_bg_task(&task, thread_id, BgOrigin::Agent) {
                Ok(_) => started += 1,
                Err(e) => refused.push((bg_task_title(&task), e)),
            }
        }
        for (title, why) in &refused {
            self.bg.unread.push((
                thread_id.to_string(),
                format!("- {title} (not started: {why})"),
            ));
        }
        if thread_id == self.visible_thread_id() {
            self.status = match (started, refused.len()) {
                (0, _) => "Background task not started — see the next reply".into(),
                (1, 0) => "Grok started a background task".into(),
                (n, 0) => format!("Grok started {n} background tasks"),
                (n, m) => format!("Grok started {n} background tasks · {m} not started"),
            };
        }
    }

    /// A plain headless chat turn that could keep running without the composer.
    /// Ask is not part of this: the pill hides the move, and the caller says why.
    fn headless_turn_can_detach(&self) -> bool {
        let headless = self.grok_p_rx.is_some() && self.grok_p_pid.is_some();
        let side_work = self.pending_kick.is_some()
            || self.kick_cap_rx.is_some()
            || self.verify_rx.is_some()
            || self.host_diff_rx.is_some()
            || self.background_tasks_open()
            || self.job_on_background_thread()
            || self.job_is_idea_talk();
        self.running
            && self.chat_job_thread.is_some()
            && self.bg.live_count() < BG_TASK_MAX
            && can_detach_turn(headless, self.acp.is_some(), self.scheduled_perm, side_work)
    }

    /// The live reply is a plain headless chat turn that can keep running without
    /// the composer, and there is room for one more background run. Ask cannot
    /// move it: a background run has nobody to approve a tool.
    pub(super) fn can_move_turn_to_background(&self) -> bool {
        self.headless_turn_can_detach() && !self.permission_mode.needs_approval()
    }

    /// Hand the live `grok -p` child to a background run without killing it and
    /// free the composer. Its reply posts on the same chat when it ends.
    pub(super) fn move_turn_to_background(&mut self) -> bool {
        if !self.can_move_turn_to_background() {
            return false;
        }
        let Some(thread_id) = self.chat_job_thread.clone() else {
            return false;
        };
        let (Some(rx), Some(pid)) = (self.grok_p_rx.take(), self.grok_p_pid.take()) else {
            return false;
        };
        let mut title = bg_task_title(&self.last_ask_on(&thread_id));
        if title.is_empty() {
            title = "Reply".into();
        }
        let action = self
            .turn_log
            .iter()
            .rev()
            .find(|b| b.kind == LiveKind::Tool && !b.tool_title.is_empty())
            .map(|b| b.tool_title.clone())
            .unwrap_or_default();
        let mut resumed = None;
        let mut fork_hold = false;
        if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
            resumed = t.grok_session.clone().filter(|s| !s.trim().is_empty());
            if resumed.is_some() && !t.grok_fork {
                t.grok_fork = true;
                fork_hold = true;
            }
        }
        self.bg.next_id += 1;
        self.bg.runs.push(BgRun {
            id: self.bg.next_id,
            thread_id: thread_id.clone(),
            title: title.clone(),
            origin: BgOrigin::Detached,
            pid: Some(pid),
            rx: Some(rx),
            say: std::mem::take(&mut self.stream_buf),
            action,
            started: Instant::now(),
            end: None,
            session: String::new(),
            resumed,
            fork_hold,
            native_session: None,
            automation: None,
        });
        // The Doing card stays up: the work goes on. The run settles it.
        self.inflight_open = false;
        self.running = false;
        self.turn_retried = false;
        self.speak_next = false;
        self.followup_step = 0;
        self.active_skill_follow = None;
        self.thought_buf.clear();
        self.turn_log.clear();
        self.thought_seam = false;
        self.say_seam = false;
        if thread_id == self.visible_thread_id() {
            self.tool_cards.clear();
            self.live_blocks.clear();
            if self.messages.last().is_some_and(|m| m.0 == "assistant") {
                self.live_mut().pop();
            }
        } else if let Some(t) = self.threads.iter_mut().find(|t| t.id == thread_id) {
            drop_trailing_assistant(t.messages_mut());
        }
        self.chat_job_thread = None;
        self.status = format!("Moved to background · {title}");
        self.persist();
        true
    }

    /// File the sessions headless work reported on the hidden Background chat.
    pub(super) fn file_hidden_sessions(&mut self) {
        let ids = match HIDDEN_SESSIONS.lock() {
            Ok(mut held) if !held.is_empty() => std::mem::take(&mut *held),
            _ => return,
        };
        if threads::file_background_sessions(&mut self.threads, &ids) {
            self.persist();
        }
    }

    /// The newest thing you asked in `thread_id`, or "".
    pub(super) fn last_ask_on(&self, thread_id: &str) -> String {
        if thread_id == self.visible_thread_id() {
            last_user_scan(self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())))
        } else {
            self.threads
                .iter()
                .find(|t| t.id == thread_id)
                .and_then(|t| last_user_scan(t.messages.iter().map(|m| (m.0.as_str(), m.1.as_str()))))
        }
        .unwrap_or_default()
    }

    /// A chat reply was killed from outside (exit 143) and the one automatic
    /// retry did not finish it: one card naming what you asked, with Open and
    /// Retry (`/retry` in that chat).
    pub(super) fn post_turn_crash(&mut self) {
        let thread = self
            .chat_job_thread
            .clone()
            .unwrap_or_else(|| self.visible_thread_id());
        let job = bg_task_title(&self.last_ask_on(&thread));
        let card = grokhub_core::crash_card(&thread, &job, &thread, "/retry", now_ms());
        self.post_feed_card(card);
    }

    /// Drain every run's events this frame, then post the ones that ended.
    pub(super) fn poll_bg_runs(&mut self) {
        self.file_hidden_sessions();
        if self.bg.runs.is_empty() {
            return;
        }
        let mut crashed: Vec<grokhub_core::UpdateCard> = Vec::new();
        for run in self.bg.runs.iter_mut() {
            let Some(rx) = run.rx.take() else {
                continue;
            };
            let mut keep = true;
            loop {
                match rx.try_recv() {
                    Ok(GrokPEvent::Text(d)) => {
                        let _ = push_stream_capped(&mut run.say, &d, TEXT_FILE_CAP as u64);
                    }
                    Ok(GrokPEvent::Tool(card)) => {
                        if !card.title.trim().is_empty() {
                            run.action = redact_held_secrets(&card.title, &self.secret_hold);
                        }
                    }
                    Ok(GrokPEvent::Recovering(_)) => run.action = "Retrying…".into(),
                    Ok(GrokPEvent::End(turn)) => {
                        if run.say.trim().is_empty() {
                            run.say = turn.text;
                        }
                        run.session = turn.session_id;
                        run.end = Some(BgEnd::Done);
                        keep = false;
                        break;
                    }
                    Ok(GrokPEvent::Err(e)) => {
                        // Stop drops the receiver first, so a SIGTERM that
                        // reaches here came from outside GrokHub.
                        run.end = Some(if grokhub_acp::is_sigterm_status(&e) {
                            crashed.push(bg_crash_card(run));
                            BgEnd::Stopped
                        } else {
                            BgEnd::Failed(rewrite_truncation_error(&e))
                        });
                        keep = false;
                        break;
                    }
                    Ok(_) => {}
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        run.end = Some(if run.say.trim().is_empty() {
                            BgEnd::Failed("Grok Build session missing".into())
                        } else {
                            BgEnd::Done
                        });
                        keep = false;
                        break;
                    }
                }
            }
            if keep {
                run.rx = Some(rx);
            } else {
                run.pid = None;
            }
        }
        for card in crashed {
            self.post_feed_card(card);
        }
        self.post_finished_bg_runs();
    }

    /// Post each ended run on its chat. A chat that is mid-turn waits: its live
    /// reply rewrites the last assistant message, so a post there would be lost.
    pub(super) fn post_finished_bg_runs(&mut self) {
        let busy = self.running.then(|| self.chat_job_thread.clone()).flatten();
        let ready: Vec<usize> = self
            .bg
            .runs
            .iter()
            .enumerate()
            .filter(|(_, r)| {
                r.end.is_some()
                    && busy.as_deref() != Some(r.thread_id.as_str())
                    && !self.scheduled_report_waits(r, busy.as_deref())
            })
            .map(|(i, _)| i)
            .collect();
        if ready.is_empty() {
            return;
        }
        let vis = self.visible_thread_id();
        let mut done = Vec::new();
        for i in ready.into_iter().rev() {
            done.push(self.bg.runs.remove(i));
        }
        done.reverse();
        for run in done {
            let end = run.end.clone().unwrap_or(BgEnd::Done);
            let reply = self.scrub_transcript(run.say.clone());
            if run.origin == BgOrigin::Scheduled {
                // Its report goes to Follow up and the home feed, never a chat
                // you can see. The hidden Background chat keeps a copy.
                self.push_msg_on(&run.thread_id, "assistant", bg_result_post(&run.title, &end, &reply));
                self.settle_scheduled_run(run.automation.as_deref(), &end, &reply);
                threads::file_background_sessions(
                    &mut self.threads,
                    std::slice::from_ref(&run.session),
                );
                continue;
            }
            if !self.threads.iter().any(|t| t.id == run.thread_id) {
                threads::file_background_sessions(
                    &mut self.threads,
                    std::slice::from_ref(&run.session),
                );
                continue;
            }
            if end != BgEnd::Stopped || !reply.trim().is_empty() {
                self.push_msg_on(&run.thread_id, "assistant", bg_result_post(&run.title, &end, &reply));
                self.bg.unread.push((
                    run.thread_id.clone(),
                    bg_result_note(&run.title, &end, &reply),
                ));
            }
            if run.origin == BgOrigin::Detached {
                self.settle_detached_run(&run, &end, &reply);
            }
            // A fork is its own session on disk. Unless a chat took it back
            // above, it is background work and not a History row.
            if !run.session.trim().is_empty() {
                threads::file_background_sessions(
                    &mut self.threads,
                    std::slice::from_ref(&run.session),
                );
            }
            let head = match end {
                BgEnd::Done => "Background task done",
                BgEnd::Failed(_) => "Background task failed",
                BgEnd::Stopped => "Background task stopped",
            };
            if run.thread_id == vis {
                self.status = format!("{head} · {}", run.title);
            }
            if end != BgEnd::Stopped && (!self.window_focused || run.thread_id != vis) {
                let clock = Self::local_clock();
                if !quiet_hours_active(&clock.hm(), &self.cfg.quiet_start, &self.cfg.quiet_end) {
                    crate::notify::ping("GrokHub", &format!("{head} · {}", run.title));
                }
            }
        }
        self.persist();
    }

    /// A scheduled run's report lands in its Follow up card's chat. While you
    /// are mid-reply in that chat, it waits: your live reply rewrites the last
    /// assistant message, so the report would be lost or spliced into it.
    fn scheduled_report_waits(&self, run: &BgRun, busy: Option<&str>) -> bool {
        let (Some(busy), Some(id)) = (busy, run.automation.as_deref()) else {
            return false;
        };
        run.origin == BgOrigin::Scheduled
            && self.board.iter().any(|c| {
                c.automation.as_deref() == Some(id)
                    && !matches!(c.status, BoardStatus::Done | BoardStatus::Dismissed)
                    && c.thread_id.as_deref() == Some(busy)
            })
    }

    /// A moved-off turn wrote its chat's own session. Give that session back to
    /// the chat when nothing newer took its place, and settle its Doing card:
    /// done when it finished, paused when it was stopped or failed.
    fn settle_detached_run(&mut self, run: &BgRun, end: &BgEnd, reply: &str) {
        let bound = self.grok_cwd().display().to_string();
        if let Some(t) = self.threads.iter_mut().find(|t| t.id == run.thread_id) {
            let open = t.grok_session.clone().filter(|s| !s.trim().is_empty());
            if run.fork_hold && t.grok_fork && open == run.resumed {
                t.grok_fork = false;
            }
            if open.is_none() && !run.session.trim().is_empty() {
                t.grok_session = Some(run.session.clone());
                if t.grok_cwd.as_deref().is_none_or(|s| s.trim().is_empty()) {
                    t.grok_cwd = Some(bound);
                }
            }
        }
        let mut changed = if *end == BgEnd::Done {
            settle_inflight_card(&mut self.board, &run.thread_id)
        } else {
            abandon_inflight_card(&mut self.board, &run.thread_id)
        };
        if apply_assistant_work_marks(&mut self.board, reply, &run.thread_id) {
            changed = true;
        }
        if changed {
            self.flush_board();
        }
    }

    /// Apply what native `scheduler_*` tools wrote to `automations.json` to the
    /// in-memory list, so the next persist keeps it instead of overwriting it.
    pub(super) fn poll_native_automations(&mut self) {
        let changes = grokhub_agent::take_automation_changes();
        if changes.is_empty() {
            return;
        }
        apply_automation_changes(&mut self.automations, changes);
        self.persist_automations();
    }

    /// Stop one run. What it said so far still posts.
    pub(super) fn stop_bg_run(&mut self, id: u64) {
        if let Some(run) = self.bg.runs.iter_mut().find(|r| r.id == id && r.live()) {
            run.stop_native();
            if let Some(pid) = run.pid.take() {
                kill_pid(pid);
            }
            run.rx = None;
            run.end = Some(BgEnd::Stopped);
        }
        self.post_finished_bg_runs();
    }

    pub(super) fn stop_all_bg_runs(&mut self) -> usize {
        let ids: Vec<u64> = self.bg.runs.iter().filter(|r| r.live()).map(|r| r.id).collect();
        for id in &ids {
            self.stop_bg_run(*id);
        }
        ids.len()
    }

    /// The chat is going away. Kill its background runs, drop their receivers,
    /// and free the slots. Nothing is posted. A moved-off turn also drops its
    /// Doing card.
    pub(super) fn drop_bg_runs_for(&mut self, thread_id: &str) -> usize {
        let mut n = 0usize;
        let mut detached = false;
        let mut i = 0;
        while i < self.bg.runs.len() {
            if self.bg.runs[i].thread_id != thread_id {
                i += 1;
                continue;
            }
            let mut run = self.bg.runs.remove(i);
            n += 1;
            run.stop_native();
            if let Some(pid) = run.pid.take() {
                kill_pid(pid);
            }
            run.rx = None;
            if run.origin == BgOrigin::Detached {
                detached = true;
            }
        }
        if detached && abandon_inflight_card(&mut self.board, thread_id) {
            self.flush_board();
        }
        self.bg.unread.retain(|(id, _)| id != thread_id);
        n
    }

    /// Quit: no child outlives the cabin, and nothing is posted.
    pub(super) fn kill_bg_runs(&mut self) {
        for run in self.bg.runs.iter_mut() {
            run.stop_native();
            if let Some(pid) = run.pid.take() {
                kill_pid(pid);
            }
            run.rx = None;
        }
        self.bg.runs.clear();
    }

    /// Results posted on `thread_id` since its last turn, as one block for the
    /// next prompt. Taken once.
    pub(super) fn take_bg_results_follow(&mut self, thread_id: &str) -> Option<String> {
        let mut notes = Vec::new();
        self.bg.unread.retain(|(id, note)| {
            if id == thread_id {
                notes.push(note.clone());
                false
            } else {
                true
            }
        });
        bg_results_follow(&notes)
    }

    /// Steering is for a chat turn on this tab: not a `/compact` or other
    /// cabin-wide Grok command.
    pub(super) fn can_steer_live_turn(&self) -> bool {
        self.running
            && self.chat_job_thread.as_deref() == Some(self.visible_thread_id().as_str())
            && (self.grok_p_rx.is_some() || self.acp.is_some())
            && !self.scheduled_perm
            && !self.job_is_idea_talk()
    }

    /// Stop this tab's live turn for a steering message. What it already said
    /// stays in the transcript. Returns the context block for the next turn.
    /// Native steer lands at the next tool boundary. The live turn keeps running.
    pub(super) fn steer_native_live(&mut self, text: String) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.live_mut().push(("user".into(), text.clone()));
        if let Some(handle) = &self.acp {
            let _ = handle.steer(&text);
        }
        self.status = "Steering…".into();
    }

    pub(super) fn stop_turn_for_steer(&mut self) -> String {
        let prev = last_user_scan(self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())))
            .unwrap_or_default();
        let tools: Vec<String> = self
            .turn_log
            .iter()
            .filter(|b| b.kind == LiveKind::Tool)
            .map(|b| b.tool_title.clone())
            .collect();
        let block = steer_follow_block(&prev, &self.stream_buf, &tools);
        // What the stopped turn showed, with tools that were still running marked
        // cancelled so the transcript does not hold a spinner forever.
        let kept = self
            .messages
            .last()
            .filter(|m| m.0 == "assistant" && !m.1.trim().is_empty())
            .map(|m| {
                let mut log = self.turn_log.clone();
                for b in log.iter_mut().filter(|b| {
                    b.kind == LiveKind::Tool
                        && grokhub_core::turn_timeline::tool_status_running(&b.tool_status)
                }) {
                    b.tool_status = "cancelled".into();
                }
                if turn_needs_timeline(&log) {
                    take_ui_text(encode_turn(&log), TEXT_FILE_CAP as u64)
                } else {
                    m.1.clone()
                }
            })
            .filter(|body| !body.trim().is_empty());
        // A steerable turn is never a scheduled one, so no automation settles here.
        // Parked cards and their spans outlive the steer (Spike-1a).
        let parks = self.take_parks_for_steer();
        self.halt_in_flight();
        self.restore_parks_after_steer(parks);
        if let Some(body) = kept {
            let body = self.scrub_transcript(body);
            self.live_mut().push(("assistant".into(), body));
        }
        self.status = "Steering…".into();
        block
    }

    /// `/bg`, `/bg stop`, `/bg <task>`.
    pub(super) fn run_bg_slash(&mut self, arg: &str) {
        let arg = arg.trim();
        if arg.is_empty() {
            if self.permission_mode.needs_approval() && self.headless_turn_can_detach() {
                self.status = BG_ASK_OFF.into();
                return;
            }
            if self.move_turn_to_background() {
                self.drain_followup_queue();
            } else if self.running {
                self.status = if self.bg.live_count() >= BG_TASK_MAX {
                    format!("{BG_TASK_MAX} background tasks are already running — stop one first")
                } else {
                    "This reply can't move to the background — /stop it or let it finish".into()
                };
            } else {
                self.status = match self.bg.live_count() {
                    0 if self.permission_mode.needs_approval() => BG_ASK_OFF.into(),
                    0 => "No background tasks — /bg <task> starts one".into(),
                    n => format!("{n} background running"),
                };
            }
            return;
        }
        if arg.eq_ignore_ascii_case("stop") {
            self.status = match self.stop_all_bg_runs() {
                0 => "No background tasks running".into(),
                1 => "Stopped 1 background task".into(),
                n => format!("Stopped {n} background tasks"),
            };
            return;
        }
        let visible = self.visible_thread_id();
        if self.permission_mode.needs_approval() && !self.native_bg_target(&visible) {
            self.status = BG_ASK_OFF.into();
            return;
        }
        match self.start_bg_task(arg, &visible, BgOrigin::User) {
            Ok(title) => self.status = format!("Background · {title}"),
            Err(e) => self.status = e,
        }
    }

    /// Enter in the composer. Alt+Enter queues instead of steering a live reply.
    pub(super) fn send_typed(&mut self, ui: &egui::Ui, text: String) {
        self.bg.queue_next = ui.input(|i| i.modifiers.alt);
        self.send_from_composer(text);
    }

    /// `/queue <message>`: wait for the live reply instead of steering it.
    pub(super) fn queue_or_send(&mut self, text: String) {
        self.bg.queue_next = true;
        self.send_chat(text);
    }

    /// Typed text during a run (Steer / Queue), queued messages, and this chat's
    /// background runs. Nothing paints when there is nothing to act on.
    pub(super) fn paint_live_work(&mut self, ui: &mut egui::Ui) {
        let vis = self.visible_thread_id();
        let typing = self.can_steer_live_turn() && !self.composer.trim().is_empty();
        let here: Vec<usize> = self
            .bg
            .runs
            .iter()
            .enumerate()
            .filter(|(_, r)| r.thread_id == vis && r.origin != BgOrigin::Scheduled)
            .map(|(i, _)| i)
            .collect();
        let elsewhere = self
            .bg
            .runs
            .iter()
            .filter(|r| r.thread_id != vis && r.live() && r.origin != BgOrigin::Scheduled)
            .count();
        let queued = if self.running { self.followup_queue.len() } else { 0 };
        if !typing && here.is_empty() && elsewhere == 0 && queued == 0 {
            return;
        }
        let steerable = self.can_steer_live_turn();
        let mut act = None;
        let t = ui.ctx().input(|i| i.time) as f32;
        let pulse = chat_run_dot_alpha(t);
        let live = crate::theme::live();
        let dot = Color32::from_rgba_unmultiplied(live.r(), live.g(), live.b(), (pulse * 255.0) as u8);
        let meta = |ui: &mut egui::Ui, s: &str, c: Color32| {
            ui.label(RichText::new(s).size(crate::theme::FONT_META).color(c));
        };
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            if typing {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    meta(ui, "Grok is still working.", crate::theme::muted());
                    if crate::cards::ghost_pill(ui, "Steer") {
                        act = Some(LiveWorkAct::Steer);
                    }
                    if crate::cards::ghost_pill(ui, "Queue") {
                        act = Some(LiveWorkAct::Queue);
                    }
                })
                .response
                .on_hover_text("Enter steers: the turn stops where it is and carries on with your message. Alt+Enter queues it for after this reply.");
            }
            for (i, q) in self.followup_queue.iter().enumerate().take(queued) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    meta(ui, "Queued", crate::theme::subtle());
                    let line: String = q.lines().next().unwrap_or("").chars().take(64).collect();
                    meta(ui, &line, crate::theme::muted());
                    if steerable && crate::cards::ghost_pill(ui, "Steer now") {
                        act = Some(LiveWorkAct::SteerQueued(i));
                    }
                    if crate::cards::ghost_pill(ui, "Remove") {
                        act = Some(LiveWorkAct::DropQueued(i));
                    }
                });
            }
            for i in &here {
                let Some(run) = self.bg.runs.get(*i) else {
                    continue;
                };
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    if run.live() {
                        ui.painter().circle_filled(rect.center(), 4.0, dot);
                    } else {
                        ui.painter().circle_filled(rect.center(), 4.0, crate::theme::subtle());
                    }
                    meta(ui, &format!("Background · {}", run.title), crate::theme::fg());
                    let mut line = bg_elapsed_label(run.started.elapsed().as_millis() as u64);
                    if !run.live() {
                        line = "waiting for this reply to finish".into();
                    } else if !run.action.is_empty() {
                        let action: String = run.action.chars().take(40).collect();
                        line = format!("{line} · {action}");
                    }
                    meta(ui, &line, crate::theme::subtle());
                    if run.live() && crate::cards::ghost_pill(ui, "Stop") {
                        act = Some(LiveWorkAct::StopRun(run.id));
                    }
                });
            }
            if elsewhere > 0 {
                let s = if elsewhere == 1 {
                    "1 background task running in another chat".to_string()
                } else {
                    format!("{elsewhere} background tasks running in other chats")
                };
                meta(ui, &s, crate::theme::subtle());
            }
        });
        ui.add_space(4.0);
        if !here.is_empty() || elsewhere > 0 {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        }
        match act {
            Some(LiveWorkAct::Steer) => {
                let text = std::mem::take(&mut self.composer);
                self.bg.queue_next = false;
                self.send_from_composer(text);
                self.composer_want_focus = true;
            }
            Some(LiveWorkAct::Queue) => {
                let text = std::mem::take(&mut self.composer);
                self.bg.queue_next = true;
                self.send_from_composer(text);
                self.composer_want_focus = true;
            }
            Some(LiveWorkAct::SteerQueued(i)) => {
                if i < self.followup_queue.len() {
                    let text = self.followup_queue.remove(i);
                    self.bg.queue_next = false;
                    self.send_from_composer(text);
                }
            }
            Some(LiveWorkAct::DropQueued(i)) => {
                if i < self.followup_queue.len() {
                    self.followup_queue.remove(i);
                    self.status = "Removed from the queue".into();
                }
            }
            Some(LiveWorkAct::StopRun(id)) => self.stop_bg_run(id),
            None => {}
        }
    }
}

/// The model client for a native `/bg` run, with the credential kind so usage is
/// labelled "SuperGrok pool" or "API credits" like the foreground engine.
type BgModel = (
    std::sync::Arc<dyn grokhub_agent::ModelClient + Send + Sync>,
    grokhub_agent::AuthKind,
    String,
);

fn native_bg_model(cabin: &mut Cabin) -> Result<BgModel, String> {
    #[cfg(test)]
    {
        let _ = cabin;
        Ok((
            std::sync::Arc::new(BgFake {
                n: std::sync::atomic::AtomicUsize::new(0),
            }),
            grokhub_agent::AuthKind::ApiKey,
            String::new(),
        ))
    }
    #[cfg(not(test))]
    {
        let (bearer, kind) = cabin.native_cred()?;
        Ok((
            std::sync::Arc::new(grokhub_agent::XaiClient::new(
                bearer.clone(),
                kind,
                std::time::Duration::from_secs(120),
            )),
            kind,
            bearer,
        ))
    }
}

fn run_native_bg(
    session: String,
    workspace: std::path::PathBuf,
    client: std::sync::Arc<dyn grokhub_agent::ModelClient + Send + Sync>,
    auth_kind: grokhub_agent::AuthKind,
    bearer: String,
    gate: grokhub_agent::Gate,
    model: String,
    effort: Option<String>,
    system: String,
    prompt: String,
    cancel: grokhub_agent::CancelToken,
    tx: mpsc::Sender<GrokPEvent>,
) {
    let hub = grokhub_agent::hub_for(&session);
    if hub.is_halted() || cancel.is_cancelled() {
        let _ = tx.send(GrokPEvent::Err("halted".into()));
        return;
    }
    let _guard = grokhub_agent::attach_run(&session, cancel.clone());
    let mut engine = grokhub_agent::NativeEngine::new(grokhub_agent::EngineParts {
        client,
        workspace,
        model,
        effort,
        system,
        conversation_id: session.clone(),
        auth_kind,
        max_turns: 0,
        cancel: cancel.clone(),
        steer: grokhub_agent::SteerQueue::new(),
        halt: Box::new(grokhub_agent::StampHalt {
            started_ms: grokhub_core::now_ms(),
            read: || crate::desktop_mcp::read_halt_stamp(),
        }),
        gate,
        desktop: None,
        permits: std::sync::Arc::new(grokhub_agent::ClosedPermits),
    });
    engine.set_imagine_bearer(&bearer);
    engine.set_reopen_tasks(false);
    if let Ok(info) = grokhub_agent::load_session(&session) {
        engine.resume(info.input(), info.usage);
    }
    let mut say = String::new();
    let _ = engine.prompt(&prompt, None, &mut |ev| match ev {
        AcpEvent::Text(text) => {
            say.push_str(&text);
            let _ = tx.send(GrokPEvent::Text(text));
        }
        AcpEvent::Thought(text) => {
            let _ = tx.send(GrokPEvent::Thought(text));
        }
        AcpEvent::Tool(card) => {
            let _ = tx.send(GrokPEvent::Tool(card));
        }
        AcpEvent::Err(err) => {
            let _ = tx.send(GrokPEvent::Err(err));
        }
        AcpEvent::Done { stop_reason } => {
            let _ = tx.send(GrokPEvent::End(grokhub_acp::SingleTurn {
                session_id: session.clone(),
                text: say.clone(),
                thought: String::new(),
                usage: empty_grok_usage(),
                stop_reason,
            }));
        }
        _ => {}
    });
}

fn apply_automation_changes(
    list: &mut Vec<grokhub_core::Automation>,
    changes: Vec<grokhub_agent::AutomationChange>,
) {
    for change in changes {
        match change {
            grokhub_agent::AutomationChange::Upsert(row) => {
                if let Some(slot) = list.iter_mut().find(|item| item.id == row.id) {
                    *slot = *row;
                } else {
                    list.push(*row);
                }
            }
            grokhub_agent::AutomationChange::Delete(id) => list.retain(|item| item.id != id),
        }
    }
}

fn empty_grok_usage() -> GrokUsage {
    GrokUsage {
        input_tokens: 0,
        output_tokens: 0,
        reasoning_tokens: 0,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
        total_tokens: 0,
        num_turns: 0,
        context_tokens_used: 0,
        context_window_tokens: 0,
        stop_reason: String::new(),
        cost_in_usd_ticks: 0,
        meter: String::new(),
    }
}


/// The crash card for a background run killed from outside. A moved-off chat
/// turn retries in its chat; any other run starts again with `/bg`.
pub(super) fn bg_crash_card(run: &BgRun) -> grokhub_core::UpdateCard {
    let retry = if run.origin == BgOrigin::Detached {
        "/retry".to_string()
    } else {
        format!("/bg {}", run.title)
    };
    let source = format!("bg:{}:{}", run.thread_id, run.title);
    grokhub_core::crash_card(&source, &run.title, &run.thread_id, &retry, now_ms())
}

#[cfg(test)]
struct BgFake {
    n: std::sync::atomic::AtomicUsize,
}

#[cfg(test)]
impl grokhub_agent::ModelClient for BgFake {
    fn stream(
        &self,
        _req: &grokhub_agent::ResponsesRequest,
        _cancel: &grokhub_agent::CancelToken,
        sink: &mut dyn FnMut(grokhub_agent::StreamEvent),
    ) -> Result<grokhub_agent::TurnOutput, grokhub_agent::ClientError> {
        use std::sync::atomic::Ordering;
        let n = self.n.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            Ok(grokhub_agent::TurnOutput {
                text: String::new(),
                reasoning: String::new(),
                calls: vec![grokhub_agent::FunctionCall {
                    call_id: "bg-write".into(),
                    name: "write".into(),
                    arguments: r#"{"path":"written-by-bg.txt","content":"nope"}"#.into(),
                }],
                usage: grokhub_agent::Usage::default(),
            })
        } else {
            sink(grokhub_agent::StreamEvent::TextDelta("bg finished".into()));
            Ok(grokhub_agent::TurnOutput {
                text: "bg finished".into(),
                reasoning: String::new(),
                calls: Vec::new(),
                usage: grokhub_agent::Usage::default(),
            })
        }
    }
}

#[cfg(test)]
mod automation_sync_tests {
    use super::apply_automation_changes;
    use grokhub_agent::AutomationChange;
    use grokhub_core::Automation;

    fn row(id: &str, every: u32) -> Automation {
        Automation {
            id: id.into(),
            name: id.into(),
            schedule: "heartbeat".into(),
            time: "09:00".into(),
            times: Vec::new(),
            instructions: String::new(),
            heartbeat_every_min: every,
            check_command: String::new(),
            enabled: true,
            last_run: None,
            next_run: None,
            run_count: 0,
            health: grokhub_core::AutoHealth::default(),
        }
    }

    #[test]
    fn scheduler_changes_reach_the_in_memory_list() {
        let mut list = vec![row("keep", 5), row("edit", 5), row("drop", 5)];
        apply_automation_changes(
            &mut list,
            vec![
                AutomationChange::Upsert(Box::new(row("edit", 30))),
                AutomationChange::Upsert(Box::new(row("new", 10))),
                AutomationChange::Delete("drop".into()),
            ],
        );
        let ids: Vec<&str> = list.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["keep", "edit", "new"]);
        assert_eq!(list[1].heartbeat_every_min, 30);
        assert_eq!(list[2].heartbeat_every_min, 10);
    }
}
