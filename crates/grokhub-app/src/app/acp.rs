//! Grok Build ACP session spawn, poll, and `grok -p` stream.

use super::*;
use grokhub_acp::ask_denied_without_acp;


pub(super) type McpStatusMap = HashMap<String, grokhub_acp::McpDoctorStatus>;
pub(super) type McpDoctorMsg = (String, Option<McpStatusMap>);

pub(super) enum GrokSessMsg {
    Listed {
        gen: u64,
        rows: Vec<grokhub_acp::GrokSession>,
        done: Vec<String>,
        error: Option<String>,
    },
}


pub(super) fn grok_session_rows(listed: Vec<String>, cwd: PathBuf) -> Vec<grokhub_acp::GrokSession> {
    listed
        .into_iter()
        .map(|r| {
            let mut s = grokhub_acp::split_session_row(&r);
            s.cwd = Some(cwd.clone());
            s.cabin = false;
            s
        })
        .collect()
}


pub(super) fn hide_pending_grok_sessions(
    rows: Vec<grokhub_acp::GrokSession>,
    pending: &HashSet<String>,
) -> Vec<grokhub_acp::GrokSession> {
    if pending.is_empty() {
        return rows;
    }
    rows.into_iter()
        .filter(|s| !pending.contains(&s.id))
        .collect()
}

/// Skills must not sit on Loading… while catalog commands run.
/// A healthy load finishes under this. Past it the page settles (last catalog
/// kept) and Refresh can start again.
pub(super) const GROK_CATALOG_SETTLE: Duration = Duration::from_secs(18);

/// Debug builds honor `GROKHUB_CATALOG_SETTLE_MS` so a shot can force the timeout
/// without waiting the full 18s. Release stays on [`GROK_CATALOG_SETTLE`].
pub(super) fn grok_catalog_settle() -> Duration {
    #[cfg(debug_assertions)]
    if let Some(ms) = std::env::var("GROKHUB_CATALOG_SETTLE_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|ms| *ms > 0)
    {
        return Duration::from_millis(ms);
    }
    GROK_CATALOG_SETTLE
}

/// Status once [`GROK_CATALOG_SETTLE`] passes with no catalog reply.
pub(super) const GROK_CATALOG_TIMEOUT: &str = "Could not load Grok Build catalog (timed out)";

impl Cabin {

    pub(super) fn poll_acp_spawn(&mut self) {
        let Some(rx) = self.acp_spawn_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(h)) => {
                let sid = h.session_id.clone();
                let cwd = h.cwd.display().to_string();
                if !sid.trim().is_empty() {
                    let job = self.chat_job_thread.clone();
                    let idx = job
                        .as_deref()
                        .and_then(|id| self.threads.iter().position(|t| t.id == id))
                        .unwrap_or(self.thread_idx);
                    let title = self
                        .threads
                        .get(idx)
                        .map(|t| t.title.clone())
                        .unwrap_or_default();
                    let allow_fresh = self.threads.get(idx).is_some_and(|t| {
                        t.grok_session
                            .as_deref()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .is_none()
                            || t.messages.len() <= 1
                    });
                    let open = self.bind_reported_grok_session(idx, &sid, allow_fresh, false);
                    if let Some(t) = self.threads.get_mut(idx) {
                        if t.grok_cwd
                            .as_deref()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .is_none()
                        {
                            t.grok_cwd = Some(cwd.clone());
                        }
                    }
                    self.record_prompt_history(
                        idx,
                        open.as_deref(),
                        &sid,
                        &title,
                        Some(std::path::PathBuf::from(cwd)),
                    );
                    self.request_grok_sessions_refresh();
                }
                self.acp = Some(h);
                self.persist();
                if self.workflow_ctl_await_acp {
                    self.workflow_ctl_await_acp = false;
                }
            }
            Ok(Err(e)) => {
                if self.permission_mode.uses_acp() {
                    self.fail_ask_without_acp(&e);
                } else {
                    self.running = false;
                    self.pending_kick = None;
                    self.scheduled_perm = false;
                    self.status = self.apply_job_fail(&e);
                    self.abandon_turn_card();
                    self.chat_job_thread = None;
                    self.persist();
                }
                if self.workflow_ctl_await_acp {
                    self.workflow_ctl_await_acp = false;
                    self.workflow_ctl_queue.clear();
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.acp_spawn_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                if self.permission_mode.uses_acp() {
                    self.fail_ask_without_acp("Grok Build session missing");
                } else {
                    self.running = false;
                    self.pending_kick = None;
                    self.scheduled_perm = false;
                    self.status = self.apply_job_fail("Grok Build session missing");
                    self.abandon_turn_card();
                    self.chat_job_thread = None;
                    self.persist();
                }
                if self.workflow_ctl_await_acp {
                    self.workflow_ctl_await_acp = false;
                    self.workflow_ctl_queue.clear();
                }
            }
        }
    }

    pub(super) fn fail_ask_without_acp(&mut self, detail: &str) {
        self.abandon_turn_card();
        self.running = false;
        self.pending_kick = None;
        self.scheduled_perm = false;
        self.status = self.apply_job_fail(&ask_denied_without_acp(detail));
        self.chat_job_thread = None;
        self.persist();
        self.maybe_continue_ptt();
    }

    pub(super) fn ensure_acp(&mut self) -> Result<(), String> {
        if self.native_engine_for_current() {
            return self.ensure_native_engine();
        }
        if grokhub_acp::find_grok().is_none() {
            return Err("Grok Build CLI is not on PATH".into());
        }
        if self.background_tasks_open() && self.acp.is_some() {
            return Ok(());
        }
        let idx = self
            .chat_job_thread
            .as_deref()
            .and_then(|id| self.threads.iter().position(|t| t.id == id))
            .unwrap_or(self.thread_idx);
        let bound = self.grok_cwd();
        let cwd = self
            .threads
            .get(idx)
            .and_then(|t| t.grok_cwd.clone())
            .filter(|s| !s.trim().is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| bound.clone());
        let resume = self
            .threads
            .get(idx)
            .and_then(|t| t.grok_session.clone())
            .filter(|s| !s.trim().is_empty());
        if let Some(h) = &self.acp {
            if h.cwd == cwd {
                match resume.as_deref() {
                    Some(id) if h.session_id != id => {}
                    _ => return Ok(()),
                }
            }
        }
        if self.acp_spawn_rx.is_some() {
            return Ok(());
        }
        self.acp = None;
        let grok_login = grokhub_acp::grok_cli_key()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let console = self.console_key().trim();
        let console_env = if !console.is_empty() && !grokhub_acp::protocol::is_jwt_api_key(console)
        {
            Some(console.to_string())
        } else {
            None
        };
        // A leftover Settings console key must not override grok login on the child.
        let (auth_key, xai_env) = if grok_login.is_some() {
            (grok_login, None)
        } else {
            (console_env.clone(), console_env)
        };
        let perm = self.permission_mode;
        let mode = self.session_mode;
        let reasoning_effort =
            grokhub_core::parse_reasoning_effort(&self.cfg.reasoning_effort).map(|s| s.to_string());
        let foreign = self
            .threads
            .get(idx)
            .and_then(|t| t.grok_cwd.as_ref())
            .map(|p| *p != bound)
            .unwrap_or(false);
        let unknown_cwd = self
            .threads
            .get(idx)
            .map(|t| {
                t.grok_cwd
                    .as_ref()
                    .map(|s| s.trim().is_empty())
                    .unwrap_or(true)
            })
            .unwrap_or(true);
        // Agent stdio must not inherit ~/.grok. That home loads chrome-devtools
        // and the leader SIGTERMs the child. Imported CLI chats opt in.
        let user_home = self
            .threads
            .get(idx)
            .map(|t| t.grok_user_home)
            .unwrap_or(false);
        let worktree = self
            .threads
            .get(idx)
            .map(|t| t.grok_worktree)
            .unwrap_or(false);
        // The open chat already has dialogue. session/new would be another History row.
        let continuing = self.threads.get(idx).is_some_and(|t| {
            t.grok_session
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
                && t.messages.len() > 1
        });
        let (tx, rx) = mpsc::channel();
        self.acp_spawn_rx = Some(rx);
        std::thread::spawn(move || {
            if unknown_cwd && resume.as_ref().is_some() {
                let _ = tx.send(Err(grokhub_acp::explain_handshake_error(
                    "session/load refused: History session has no worktree",
                    &cwd,
                )));
                return;
            }
            let spawn = |resume: Option<String>| {
                build_agent::spawn_session(
                    cwd.clone(),
                    auth_key.clone(),
                    xai_env.clone(),
                    perm,
                    mode,
                    reasoning_effort.clone(),
                    resume,
                    user_home,
                    worktree,
                )
            };
            let out = match spawn(resume.clone()) {
                Ok(h) => Ok(h),
                Err(e) => {
                    let retry_fresh = resume.is_some()
                        && !foreign
                        && !unknown_cwd
                        && !continuing
                        && !grokhub_acp::is_session_cwd_error(&e);
                    if retry_fresh {
                        spawn(None).map_err(|e2| grokhub_acp::explain_handshake_error(&e2, &cwd))
                    } else {
                        Err(grokhub_acp::explain_handshake_error(&e, &cwd))
                    }
                }
            };
            let _ = tx.send(out);
        });
        Ok(())
    }

    pub(super) fn poll_acp(&mut self) {
        let evs = {
            let Some(h) = &self.acp else { return };
            let mut v = Vec::new();
            while let Ok(ev) = h.try_recv() {
                v.push(ev);
            }
            v
        };
        let native_session = self
            .acp
            .as_ref()
            .is_some_and(|handle| handle.session_id.starts_with("native-"));
        let auto_allow = self.permission_mode.auto_allows() && !native_session;
        for ev in evs {
            match ev {
                AcpEvent::Ready { session_id } => {
                    if !session_id.trim().is_empty() {
                        let job = self.chat_job_thread.clone();
                        let idx = job
                            .as_deref()
                            .and_then(|id| self.threads.iter().position(|t| t.id == id))
                            .unwrap_or(self.thread_idx);
                        let acp_cwd = self.acp.as_ref().map(|h| h.cwd.display().to_string());
                        let allow_fresh = self.threads.get(idx).is_some_and(|t| {
                            t.grok_session
                                .as_deref()
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .is_none()
                                || t.messages.len() <= 1
                        });
                        let open =
                            self.bind_reported_grok_session(idx, &session_id, allow_fresh, false);
                        if let Some(t) = self.threads.get_mut(idx) {
                            if let Some(cwd) = acp_cwd.clone() {
                                if t.grok_cwd
                                    .as_deref()
                                    .map(str::trim)
                                    .filter(|s| !s.is_empty())
                                    .is_none()
                                {
                                    t.grok_cwd = Some(cwd);
                                }
                            }
                        }
                        self.record_prompt_history(idx, open.as_deref(), &session_id, "", None);
                    }
                }
                AcpEvent::Thought(t) => {
                    if !self.running {
                        continue;
                    }
                    let changed = self.ingest_stream_chunk(LiveKind::Thought, &t);
                    if changed && self.stream_here() {
                        self.scrub_live_blocks();
                    }
                    if self.stream_here() {
                        self.status = self.thinking_status();
                    }
                    if changed {
                        self.upsert_stream_assistant();
                    }
                }
                AcpEvent::Text(t) => {
                    if !self.running {
                        continue;
                    }
                    let changed = self.ingest_stream_chunk(LiveKind::Say, &t);
                    if changed && self.stream_here() {
                        self.scrub_live_blocks();
                    }
                    if self.stream_here() {
                        self.status = self.thinking_status();
                    }
                    if changed {
                        self.upsert_stream_assistant();
                    }
                }
                AcpEvent::Tool(mut card) => {
                    if self.cfg.native_engine && card.status == "completed" {
                        let sid = self
                            .acp
                            .as_ref()
                            .map(|handle| handle.session_id.clone())
                            .unwrap_or_default();
                        if card.kind == "enter_plan_mode" && self.session_mode == SessionMode::Chat
                        {
                            self.set_session_mode(SessionMode::Plan);
                            if !sid.is_empty() {
                                grokhub_agent::set_plan_session(&sid, true);
                            }
                        } else if card.kind == "exit_plan_mode"
                            && self.session_mode == SessionMode::Plan
                        {
                            self.set_session_mode(SessionMode::Chat);
                            if !sid.is_empty() {
                                grokhub_agent::set_plan_session(&sid, false);
                            }
                        }
                    }
                    card.detail = redact_held_secrets(&card.detail, &self.secret_hold);
                    card.diff = redact_held_secrets(&card.diff, &self.secret_hold);
                    card.title = redact_held_secrets(&card.title, &self.secret_hold);
                    if let Some(url) = &card.image_data_url {
                        self.remember_last_frame(url);
                        self.store_hub_frame(url);
                    }
                    self.ingest_tool_card(&card);
                    if self.stream_here() {
                        self.scrub_live_blocks();
                        if let Some(url) = &card.image_data_url {
                            self.desk_frame = Some(url.clone());
                        }
                        if let Some(old) = self.tool_cards.iter_mut().find(|c| c.id == card.id) {
                            *old = merge_tool_card(old.clone(), card);
                        } else {
                            self.tool_cards.push(card);
                        }
                    }
                }
                AcpEvent::Plan(t) => {
                    // Plan text only. No approve / request-changes / comment RPC on this tip:
                    // session/request_permission is tool Allow/Deny, and /approve does not parse.
                    self.store_session_plan(&t, true);
                    if self.stream_here() {
                        self.status = format!("Plan · {t}");
                    }
                }
                AcpEvent::Usage(u) => self.merge_grok_usage(&u),
                AcpEvent::Commands(cmds) => self.apply_grok_commands(cmds),
                AcpEvent::Task { id, title, done } => self.apply_grok_task(id, title, done),
                AcpEvent::Compact {
                    started,
                    usage,
                    error,
                } => {
                    if self.stream_here() {
                        self.apply_compact_status(started, usage, error);
                    } else {
                        self.merge_grok_usage(&usage);
                    }
                }
                AcpEvent::Permission(p) => {
                    if !self.running {
                        if let Some(h) = &self.acp {
                            let _ = h.answer_permission(p.rpc_id, false);
                        }
                        continue;
                    }
                    // Harness pre-check before Grok Build's pill answers: tighten only.
                    let Some(p) = self.harness_precheck(p) else {
                        continue;
                    };
                    if self.permission_mode == PermissionMode::AlwaysApprove {
                        if let Some(h) = &self.acp {
                            let _ = h.answer_permission_always(p.rpc_id);
                        }
                    } else if auto_allow {
                        if let Some(h) = &self.acp {
                            let _ = h.answer_permission(p.rpc_id, true);
                        }
                    } else {
                        self.show_perm_ask(p);
                    }
                }
                AcpEvent::Elicit(p) => {
                    if !self.running {
                        if let Some(h) = &self.acp {
                            let _ = h.answer_elicit(p.rpc_id, "cancel", None);
                        }
                        continue;
                    }
                    if let Some(old) = self.elicit_ask.take() {
                        if let Some(h) = &self.acp {
                            let _ = h.answer_elicit(old.rpc_id, "cancel", None);
                        }
                    }
                    self.elicit_draft.clear();
                    if self.chrome_here() {
                        self.status = format!("{} wants input", p.server_name);
                    }
                    self.elicit_ask = Some(p);
                }
                AcpEvent::ElicitComplete {
                    elicitation_id,
                    server_name,
                } => {
                    let same = self.elicit_ask.as_ref().is_some_and(|e| {
                        (!elicitation_id.is_empty() && e.elicitation_id == elicitation_id)
                            || (!server_name.is_empty() && e.server_name == server_name)
                    });
                    if same {
                        self.elicit_ask = None;
                        self.elicit_draft.clear();
                    }
                }
                AcpEvent::Done { stop_reason } => {
                    self.sync_native_title_from_store();
                    if stop_reason.eq_ignore_ascii_case("cancelled") || !self.running {
                        continue;
                    }
                    let thought = std::mem::take(&mut self.thought_buf);
                    let stream = std::mem::take(&mut self.stream_buf);
                    let text = if thought.is_empty() {
                        stream
                    } else {
                        merge_thinking_capped(
                            &thought,
                            &strip_thinking(&stream),
                            IMAGE_FILE_CAP as usize,
                        )
                    };
                    self.finish_acp_turn(text);
                    self.drain_followup_queue();
                }
                AcpEvent::Err(e) => {
                    if e.contains("acp json") {
                        continue;
                    }
                    match classify_stream_error(&e) {
                        StreamErrorKind::Transient | StreamErrorKind::TruncationContinue => {
                            self.status = retry_status_line(&rewrite_truncation_error(&e));
                            continue;
                        }
                        StreamErrorKind::CreditLimit | StreamErrorKind::Fatal => {}
                    }
                    self.withdraw_perm_asks();
                    self.perm_always_confirm = None;
                    self.confirm = None;
                    if let Some(p) = self.elicit_ask.take() {
                        if let Some(h) = &self.acp {
                            let _ = h.answer_elicit(p.rpc_id, "cancel", None);
                        }
                    }
                    self.running = false;
                    self.acp = None;
                    if grokhub_acp::is_sigterm_status(&e) && !self.turn_retried {
                        self.turn_retried = true;
                        self.status = "Retrying…".into();
                        self.kick_model(false);
                        continue;
                    }
                    self.scheduled_perm = false;
                    let e = grokhub_acp::explain_handshake_error(&e, &self.grok_cwd());
                    self.status = self.apply_job_fail(&e);
                    self.abandon_turn_card();
                    self.chat_job_thread = None;
                    self.persist();
                    self.maybe_continue_ptt();
                    if !self.running {
                        self.release_workflow_ctl_queue();
                    }
                }
            }
        }
    }

    /// Put a new Ask on the card, or queue it behind the one already there. A
    /// parallel tool call can ask while a card is up; cancelling either would read
    /// to Grok as "User cancelled".
    pub(super) fn show_perm_ask(&mut self, p: grokhub_acp::PermissionAsk) {
        if self.perm_ask.is_some() {
            self.perm_queue.push_back(p);
            return;
        }
        self.perm_always_confirm = None;
        self.confirm = None;
        self.perm_ask = Some(p);
        if self.chrome_here() {
            self.status = "Grok wants permission".into();
        }
    }

    /// The turn is stopping: withdraw the Ask on screen and every Ask queued
    /// behind it, so no RPC is left hanging.
    pub(super) fn withdraw_perm_asks(&mut self) {
        self.withdraw_hard_parks();
        let asks: Vec<_> = self
            .perm_ask
            .take()
            .into_iter()
            .chain(self.perm_queue.drain(..))
            .collect();
        if let Some(h) = &self.acp {
            for p in asks {
                let _ = h.answer_permission(p.rpc_id, false);
            }
        }
    }

    /// The Ask on screen was answered. The next queued one takes the card.
    pub(super) fn next_perm_ask(&mut self) {
        self.perm_ask = self.perm_queue.pop_front();
        self.perm_always_confirm = None;
    }

    pub(super) fn finish_acp_turn(&mut self, text: String) {
        let text = self.scrub_transcript(take_ui_text(text, IMAGE_FILE_CAP));
        self.withdraw_perm_asks();
        self.perm_always_confirm = None;
        self.confirm = None;
        if let Some(p) = self.elicit_ask.take() {
            if let Some(h) = &self.acp {
                let _ = h.answer_elicit(p.rpc_id, "cancel", None);
            }
        }
        self.perm_ask = None;
        self.elicit_ask = None;
        self.elicit_draft.clear();
        let here =
            chat_stream_is_visible(self.chat_job_thread.as_deref(), &self.visible_thread_id());
        self.running = false;
        if !self.voice_is_on() {
            self.voice_orb = "idle".into();
        }
        if here {
            self.status.clear();
        }
        if self.background_tasks_open() {
            let n = self.grok_tasks.iter().filter(|t| !t.2).count();
            if here {
                self.status = format!("{n} still running");
            }
        }
        if !self.job_is_idea_talk() {
            remember_chip_outcome(&mut self.chip_memory, true, now_ms());
            record_turn(&mut self.learning);
            self.absorb_turn_learning(&text);
        }
        bump_usage(&mut self.usage, "message");
        if self.session_mode == SessionMode::Plan {
            self.store_session_plan(&text, false);
        }
        let stored = self.turn_transcript(text.clone(), IMAGE_FILE_CAP);
        self.apply_assistant_snapshot(stored);
        // The newest reply only. Every reply of a long turn joined into the
        // last bubble is what made a finished turn read as one wall of text.
        let prose = match last_say(&self.turn_log) {
            Some(say) => grokhub_core::assistant_prose(say),
            None => grokhub_core::assistant_prose(&text),
        };
        if here {
            if !prose.is_empty() {
                match self.live_blocks.last_mut() {
                    Some(b) if b.kind == LiveKind::Say => {
                        if b.body.trim().len() < prose.trim().len() {
                            b.body = prose;
                        }
                    }
                    _ => append_say(&mut self.live_blocks, &prose),
                }
            }
            self.scrub_live_blocks();
        }
        self.thought_buf.clear();
        self.stream_buf.clear();
        self.turn_log.clear();
        self.thought_seam = false;
        self.say_seam = false;
        self.settle_turn_card(&text);
        let origin = self.chat_job_thread.take();
        // Grok may hand separate long work to background runs. Unwatched runs
        // (night, loops, /send) and hidden background chats do not fan out.
        if let Some(id) = origin.as_deref() {
            if !self.scheduled_perm && !self.threads.iter().any(|t| t.id == id && t.background) {
                self.start_agent_bg_tasks(&strip_thinking(&text), id);
            }
        }
        self.bg.steer_follow = None;
        self.bg.results_follow = None;
        if here && self.speak_next {
            self.speak_next = false;
            self.speak_reply(&text);
        }
        if let Some(p) = extract_imagine_prompt(&text) {
            self.chat_job_thread = origin.clone();
            self.imagine_prompt = p;
            if here {
                self.nav = Nav::Imagine;
                self.imagine_want_focus = true;
            }
            self.kick_imagine();
        }
        self.maybe_continue_ptt();
        self.persist();
        if !self.running {
            self.scheduled_perm = false;
            self.finish_hub_dispatch(&text, hub_dispatch_ok(&text));
        }
        let vis = self.visible_thread_id();
        if leftover_empty_thread(
            self.threads
                .get(self.thread_idx)
                .map(|t| t.title.as_str())
                .unwrap_or(""),
            self.scratch(),
            self.messages.is_empty(),
        ) {
            let _ = vis;
        }
        let _ = origin;
    }

    pub(super) fn poll_single(&mut self) {
        let Some(rx) = self.grok_p_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(GrokPEvent::Thought(d)) => {
                // `append_thought` has no cap of its own, so it has to respect the buffer
                // cap or the rendered blocks grow past IMAGE_FILE_CAP unbounded.
                let paints = self.stream_here();
                if self.ingest_stream_chunk(LiveKind::Thought, &d) && paints {
                    self.scrub_live_blocks();
                }
                if paints {
                    self.status = self.thinking_status();
                }
                self.upsert_stream_assistant();
                self.grok_p_rx = Some(rx);
            }
            Ok(GrokPEvent::Text(d)) => {
                let paints = self.stream_here();
                if self.ingest_stream_chunk(LiveKind::Say, &d) && paints {
                    self.scrub_live_blocks();
                }
                if paints {
                    self.status = self.thinking_status();
                }
                self.upsert_stream_assistant();
                self.grok_p_rx = Some(rx);
            }
            Ok(GrokPEvent::Tool(mut card)) => {
                card.detail = redact_held_secrets(&card.detail, &self.secret_hold);
                card.diff = redact_held_secrets(&card.diff, &self.secret_hold);
                card.title = redact_held_secrets(&card.title, &self.secret_hold);
                if let Some(url) = &card.image_data_url {
                    self.remember_last_frame(url);
                    self.store_hub_frame(url);
                }
                self.harness_note_headless(&card);
                self.ingest_tool_card(&card);
                if self.stream_here() {
                    self.scrub_live_blocks();
                    if let Some(url) = &card.image_data_url {
                        self.desk_frame = Some(url.clone());
                    }
                    if let Some(old) = self.tool_cards.iter_mut().find(|c| c.id == card.id) {
                        *old = merge_tool_card(old.clone(), card);
                    } else {
                        self.tool_cards.push(card);
                    }
                }
                self.grok_p_rx = Some(rx);
            }
            Ok(GrokPEvent::Usage(u)) => {
                self.merge_grok_usage(&u);
                self.grok_p_rx = Some(rx);
            }
            Ok(GrokPEvent::Commands(cmds)) => {
                self.apply_grok_commands(cmds);
                self.grok_p_rx = Some(rx);
            }
            Ok(GrokPEvent::Task { id, title, done }) => {
                self.apply_grok_task(id, title, done);
                self.grok_p_rx = Some(rx);
            }
            Ok(GrokPEvent::Plan(t)) => {
                self.store_session_plan(&t, true);
                if self.stream_here() {
                    self.status = format!("Plan · {t}");
                }
                self.grok_p_rx = Some(rx);
            }
            Ok(GrokPEvent::Compact {
                started,
                usage,
                error,
            }) => {
                if self.stream_here() {
                    self.apply_compact_status(started, usage, error);
                } else {
                    self.merge_grok_usage(&usage);
                }
                self.grok_p_rx = Some(rx);
            }
            Ok(GrokPEvent::Recovering(msg)) => {
                if self.stream_here() {
                    self.status = retry_status_line(&msg);
                }
                self.grok_p_rx = Some(rx);
            }
            Ok(GrokPEvent::End(turn)) => {
                self.grok_p_pid = None;
                self.apply_single_turn(turn);
                self.harness_headless_end();
            }
            Ok(GrokPEvent::Err(e)) => {
                self.grok_p_pid = None;
                self.running = false;
                self.pending_kick = None;
                let paints = self.stream_here();
                if grokhub_acp::is_sigterm_status(&e) {
                    let empty = self.stream_buf.is_empty() && self.thought_buf.is_empty();
                    if empty && !self.turn_retried {
                        self.turn_retried = true;
                        if paints {
                            self.status = "Retrying…".into();
                        }
                        self.kick_model(false);
                    } else {
                        self.scheduled_perm = false;
                        if paints {
                            self.status.clear();
                        }
                        self.abandon_turn_card();
                        self.chat_job_thread = None;
                        self.persist();
                    }
                } else {
                    self.scheduled_perm = false;
                    let status = self.apply_job_fail(&rewrite_truncation_error(&e));
                    if self.cfg.native_engine {
                        self.finish_hub_dispatch(&status, false);
                    }
                    if paints || self.chat_job_thread.is_none() {
                        self.status = status;
                    }
                    self.abandon_turn_card();
                    self.chat_job_thread = None;
                    self.persist();
                }
                self.maybe_continue_ptt();
                if !self.running {
                    self.release_workflow_ctl_queue();
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.grok_p_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.grok_p_pid = None;
                self.running = false;
                self.pending_kick = None;
                let streamed = !self.stream_buf.is_empty() || !self.thought_buf.is_empty();
                if streamed {
                    let thought = std::mem::take(&mut self.thought_buf);
                    let stream = std::mem::take(&mut self.stream_buf);
                    let text = if thought.is_empty() {
                        stream
                    } else {
                        merge_thinking_capped(&thought, &stream, TEXT_FILE_CAP)
                    };
                    self.finish_acp_turn(text);
                } else {
                    self.scheduled_perm = false;
                    let paints = self.stream_here();
                    let status = self.apply_job_fail("Grok Build session missing");
                    if self.cfg.native_engine {
                        self.finish_hub_dispatch(&status, false);
                    }
                    if paints || self.chat_job_thread.is_none() {
                        self.status = status;
                    }
                    self.abandon_turn_card();
                    self.chat_job_thread = None;
                    self.persist();
                }
                if !self.running {
                    self.release_workflow_ctl_queue();
                }
            }
        }
    }

    pub(super) fn apply_single_turn(&mut self, turn: grokhub_acp::SingleTurn) {
        let job = self.chat_job_thread.clone();
        let idx = job
            .as_deref()
            .and_then(|id| self.threads.iter().position(|t| t.id == id))
            .unwrap_or(self.thread_idx);
        let bound = self.grok_cwd().display().to_string();
        let renaming = self.rename_idx == Some(idx);
        let hint = self.threads.get(idx).and_then(|t| {
            t.messages
                .iter()
                .rev()
                .find(|m| m.0 == "user")
                .map(|m| m.1.clone())
        });
        if !turn.usage.is_empty() {
            let u = turn.usage.clone();
            self.merge_grok_usage(&u);
        }
        let session_saved = !turn.session_id.trim().is_empty();
        let fork = self.threads.get(idx).map(|t| t.grok_fork).unwrap_or(false);
        let open = if session_saved {
            self.bind_reported_grok_session(idx, &turn.session_id, false, fork)
        } else {
            self.threads.get(idx).and_then(|t| t.grok_session.clone())
        };
        if let Some(t) = self.threads.get_mut(idx) {
            if t.grok_cwd
                .as_deref()
                .map(|s| s.trim().is_empty())
                .unwrap_or(true)
            {
                t.grok_cwd = Some(bound);
            }
            if let Some(hint) = hint.as_deref() {
                let mut tab = ThreadTab {
                    title: t.title.clone(),
                    pinned: t.pinned,
                    title_locked: t.title_locked,
                };
                if apply_auto_title_in(&mut tab, hint, renaming) {
                    t.title = tab.title;
                }
            }
        }
        if session_saved {
            let title = self
                .threads
                .get(idx)
                .map(|t| t.title.clone())
                .unwrap_or_default();
            let cwd = self
                .threads
                .get(idx)
                .and_then(|t| t.grok_cwd.clone())
                .map(std::path::PathBuf::from);
            self.record_prompt_history(idx, open.as_deref(), &turn.session_id, &title, cwd);
        }
        // A reply that only came in the end event still belongs after this turn's tools.
        if self.stream_buf.trim().is_empty() && !turn.text.trim().is_empty() {
            if self.thought_buf.trim().is_empty() && !turn.thought.trim().is_empty() {
                self.ingest_stream_chunk(LiveKind::Thought, &turn.thought);
            }
            self.ingest_stream_chunk(LiveKind::Say, &turn.text);
        }
        let streamed = if self.thought_buf.is_empty() {
            self.stream_buf.clone()
        } else {
            merge_thinking_capped(&self.thought_buf, &self.stream_buf, TEXT_FILE_CAP)
        };
        let finished = if turn.thought.is_empty() {
            turn.text
        } else {
            merge_thinking_capped(&turn.thought, &turn.text, TEXT_FILE_CAP)
        };
        let text = if streamed.trim().is_empty() {
            finished
        } else {
            streamed
        };
        let footer = turn_footer(&turn.stop_reason, &self.grok_usage);
        let here =
            chat_stream_is_visible(self.chat_job_thread.as_deref(), &self.visible_thread_id());
        self.finish_acp_turn(text);
        if here && !footer.is_empty() && self.status.is_empty() {
            self.status = footer;
        }
        self.drain_followup_queue();
    }

    pub(super) fn send_grok_slash(&mut self, cmd: &str) {
        if self.permission_mode.uses_acp() {
            if self.acp.is_none() {
                if let Err(e) = self.ensure_acp() {
                    self.fail_ask_without_acp(&e);
                    return;
                }
            }
            if let Some(h) = &self.acp {
                match h.prompt(cmd) {
                    Ok(()) => self.running = true,
                    Err(e) => {
                        self.acp = None;
                        self.fail_ask_without_acp(&e);
                    }
                }
                return;
            }
            // Handshake in flight or ACP still down — fail-closed, no yolo grok -p.
            self.fail_ask_without_acp("");
            return;
        }
        let idx = self.thread_idx;
        let cwd = self
            .threads
            .get(idx)
            .and_then(|t| t.grok_cwd.clone())
            .filter(|s| !s.trim().is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| self.grok_cwd());
        let resume = self
            .threads
            .get(idx)
            .and_then(|t| t.grok_session.clone())
            .filter(|s| !s.trim().is_empty());
        let resume_in_cabin = resume
            .as_deref()
            .is_some_and(grokhub_acp::cabin_has_session);
        let user_home = grokhub_acp::use_user_grok_home(
            self.threads
                .get(idx)
                .map(|t| t.grok_user_home)
                .unwrap_or(false),
            resume_in_cabin,
        );
        if let Some(t) = self.threads.get_mut(idx) {
            t.grok_user_home = user_home;
        }
        let worktree = self
            .threads
            .get(idx)
            .map(|t| t.grok_worktree)
            .unwrap_or(false);
        let (yolo, auto) = self.permission_mode.composer_headless_flags();
        match grokhub_acp::spawn_grok_p_stream(
            cmd,
            &cwd,
            resume.as_deref(),
            yolo,
            auto,
            None,
            None,
            self.session_mode,
            grokhub_acp::GrokPAttach {
                image: None,
                learned: &grokhub_core::brief_for(&self.learning, "chat"),
                deny: self.permission_mode.needs_approval(),
                desktop: self.cfg.desktop_control,
                hard_deny: grokhub_agent::harness::HEADLESS_DENY_RULES,
            },
            false,
            user_home,
            worktree,
        ) {
            Ok((pid, rx)) => {
                self.grok_p_pid = Some(pid);
                self.grok_p_rx = Some(rx);
                self.running = true;
            }
            Err(e) => {
                self.grok_p_pid = None;
                self.grok_p_rx = None;
                self.running = false;
                self.status = format!("Grok Build could not start: {e}");
            }
        }
    }

    pub(super) fn apply_grok_commands(&mut self, cmds: Vec<String>) {
        self.grok_commands = grok_command_hits(&cmds);
    }

    /// Background subagents emit cards after the parent turn has returned.
    /// The CLI path never queues these, and this poll runs only while the native engine is on.
    pub(super) fn poll_native_side_events(&mut self) {
        if !self.cfg.native_engine {
            return;
        }
        let here = self
            .acp
            .as_ref()
            .map(|handle| handle.session_id.clone())
            .unwrap_or_default();
        let mut later = Vec::new();
        for event in grokhub_agent::drain_side_events() {
            // Cards, rows and answers go through the open thread's handle. An event from
            // another session waits until that thread is open, so a card is never answered
            // through, or under the permission mode of, the wrong thread.
            if here.is_empty() || event.session() != here {
                later.push(event);
                continue;
            }
            match event {
                grokhub_agent::SideEvent::Task {
                    id, title, done, ..
                } => {
                    self.apply_grok_task(id, title, done);
                }
                grokhub_agent::SideEvent::Plan { text, .. } => {
                    self.store_session_plan(&text, true);
                    if self.stream_here() {
                        self.status = format!("Plan · {text}");
                    }
                }
                grokhub_agent::SideEvent::Permission(ask) => {
                    let Some(ask) = self.harness_precheck_on(ask, "E") else {
                        continue;
                    };
                    if self.permission_mode == PermissionMode::AlwaysApprove {
                        if let Some(handle) = &self.acp {
                            let _ = handle.answer_permission_always(ask.rpc_id);
                        }
                    } else {
                        self.show_perm_ask(ask);
                    }
                }
                grokhub_agent::SideEvent::Elicit(ask) => {
                    if let Some(old) = self.elicit_ask.take() {
                        if let Some(handle) = &self.acp {
                            let _ = handle.answer_elicit(old.rpc_id, "cancel", None);
                        }
                    }
                    self.elicit_draft.clear();
                    if self.chrome_here() {
                        self.status = format!("{} wants input", ask.server_name);
                    }
                    self.elicit_ask = Some(ask);
                }
            }
        }
        grokhub_agent::requeue_side_events(later);
    }

    pub(super) fn apply_grok_task(&mut self, id: String, title: String, done: bool) {
        if let Some(row) = self.grok_tasks.iter_mut().find(|t| t.0 == id) {
            row.1 = title;
            row.2 = done;
        } else {
            self.grok_tasks.push((id, title, done));
        }
    }

    /// One queued verb after the turn is gone, including Stop. Skips an ACP handshake.
    pub(super) fn release_workflow_ctl_if_idle(&mut self) {
        if self.running || self.workflow_ctl_queue.is_empty() || self.acp_spawn_rx.is_some() {
            return;
        }
        self.release_workflow_ctl_queue();
    }

    /// One queued workflow verb, before chat follow-ups. A live send stops the drain.
    /// Never pops during a handshake.
    pub(super) fn release_workflow_ctl_queue(&mut self) {
        if self.running || self.acp_spawn_rx.is_some() {
            return;
        }
        let Some(cmd) = self.workflow_ctl_queue.first().cloned() else {
            return;
        };
        self.workflow_ctl_queue.remove(0);
        let Some(Slash::WorkflowCtl { verb, target }) = parse_slash(&cmd) else {
            return;
        };
        // Ask with no agent re-queues at the back. Put this verb back at the front.
        let queued = self.workflow_ctl_queue.len();
        self.send_workflow_ctl(verb, &target);
        if self.workflow_ctl_await_acp
            && self.workflow_ctl_queue.len() == queued + 1
            && self.workflow_ctl_queue.last().is_some_and(|s| s == &cmd)
        {
            if let Some(held) = self.workflow_ctl_queue.pop() {
                self.workflow_ctl_queue.insert(0, held);
            }
        }
    }

    pub(super) fn drain_followup_queue(&mut self) {
        if self.running {
            return;
        }
        self.release_workflow_ctl_queue();
        if self.running {
            return;
        }
        if let Some(next) = self.side_ask_queue.first().cloned() {
            self.side_ask_queue.remove(0);
            self.send_queued_side_ask(next);
            return;
        }
        let Some(next) = self.followup_queue.first().cloned() else {
            return;
        };
        self.followup_queue.remove(0);
        self.send_chat(next);
    }

    /// Look-safe send after the live turn. Does not cancel that turn — it already ended.
    pub(super) fn send_queued_side_ask(&mut self, text: String) {
        self.side_ask_kick = true;
        self.send_chat(text);
        if !self.running && self.pending_kick.is_none() && self.acp_spawn_rx.is_none() {
            self.side_ask_kick = false;
        }
    }

    pub(super) fn thinking_status(&self) -> String {
        let ctx = grok_context_line(&self.grok_usage);
        if ctx.is_empty() {
            "Thinking…".into()
        } else {
            format!("Thinking… {ctx}")
        }
    }

    pub(super) fn upsert_stream_assistant(&mut self) {
        self.apply_live_assistant();
    }

    pub(super) fn sync_unlocked_titles_from_sessions(&mut self) {
        let mut changed = false;
        for t in &mut self.threads {
            if t.title_locked {
                continue;
            }
            let Some(id) = t.grok_session.as_deref() else {
                continue;
            };
            let Some(s) = self.grok_sessions.iter().find(|s| s.id == id) else {
                continue;
            };
            if grokhub_acp::is_placeholder_session_title(&s.title) || s.title == s.id {
                continue;
            }
            if t.title != s.title {
                t.title = s.title.clone();
                changed = true;
            }
        }
        if changed {
            self.persist();
        }
    }

    pub(super) fn forget_grok_build_session(&mut self, id: &str, also: &[String]) {
        let mut ids = Vec::new();
        let id = id.trim();
        if !id.is_empty() {
            ids.push(id.to_string());
        }
        for extra in also {
            let extra = extra.trim();
            if !extra.is_empty() && !ids.iter().any(|s| s == extra) {
                ids.push(extra.to_string());
            }
        }
        if ids.is_empty() {
            return;
        }
        for id in &ids {
            self.pending_grok_deletes.insert(id.clone());
            self.grok_sessions.retain(|s| s.id != *id);
        }
        self.grok_list_gen = self.grok_list_gen.wrapping_add(1);
        let gen = self.grok_list_gen;
        self.grok_sessions_inflight = self.grok_sessions_inflight.saturating_add(1);
        let bin = grokhub_acp::find_grok();
        let cwd = self.grok_cli_cwd();
        let tx = self.grok_sessions_tx.clone();
        std::thread::spawn(move || {
            let error = match bin.as_ref() {
                Some(bin) => {
                    let mut err = None;
                    for id in &ids {
                        if let Err(e) = grokhub_acp::delete_session(bin, &cwd, id) {
                            if err.is_none() {
                                err = Some(e);
                            }
                        }
                    }
                    err
                }
                None => Some("Grok Build CLI missing".into()),
            };
            let listed = match bin.as_ref() {
                Some(bin) => grokhub_acp::list_sessions(bin, &cwd).unwrap_or_default(),
                None => Vec::new(),
            };
            let rows = grok_session_rows(listed, cwd);
            let _ = tx.send(GrokSessMsg::Listed {
                gen,
                rows,
                done: ids,
                error,
            });
        });
    }

    pub(super) fn reload_grok_sessions(&mut self) {
        if self.grok_sessions_inflight > 0 {
            self.grok_sessions_refresh_pending = true;
            return;
        }
        self.grok_list_gen = self.grok_list_gen.wrapping_add(1);
        let gen = self.grok_list_gen;
        self.grok_sessions_inflight = self.grok_sessions_inflight.saturating_add(1);
        self.grok_sessions_refresh_pending = false;
        let bin = grokhub_acp::find_grok();
        let cwd = self.grok_cli_cwd();
        let tx = self.grok_sessions_tx.clone();
        std::thread::spawn(move || {
            let listed = if let Some(bin) = bin {
                grokhub_acp::list_sessions(&bin, &cwd).unwrap_or_default()
            } else {
                Vec::new()
            };
            let rows = grok_session_rows(listed, cwd);
            let _ = tx.send(GrokSessMsg::Listed {
                gen,
                rows,
                done: Vec::new(),
                error: None,
            });
        });
    }

    pub(super) fn poll_grok_sessions(&mut self) {
        loop {
            match self.grok_sessions_rx.try_recv() {
                Ok(msg) => self.apply_grok_sess_msg(msg),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
    }

    pub(super) fn apply_grok_sess_msg(&mut self, msg: GrokSessMsg) {
        self.grok_sessions_inflight = self.grok_sessions_inflight.saturating_sub(1);
        let GrokSessMsg::Listed {
            gen,
            rows,
            done,
            error,
        } = msg;
        for id in &done {
            self.pending_grok_deletes.remove(id);
        }
        let delete_failed = error.is_some();
        if let Some(e) = error {
            let e = e.trim();
            self.status = if e.is_empty() {
                "Could not delete session".into()
            } else {
                format!("Could not delete session: {e}")
            };
        } else if done.len() == 1 {
            self.status = "Deleted session".into();
        } else if done.len() > 1 {
            self.status = "Deleted sessions".into();
        }
        if gen != self.grok_list_gen {
            return;
        }
        self.grok_sessions = hide_pending_grok_sessions(rows, &self.pending_grok_deletes);
        let retired: Vec<String> = self
            .threads
            .iter()
            .flat_map(|t| t.retired_sessions.iter().cloned())
            .collect();
        self.grok_sessions
            .retain(|s| !retired.iter().any(|id| id == &s.id));
        self.grok_sessions_loaded = true;
        self.last_grok_list_at = Instant::now();
        self.sync_unlocked_titles_from_sessions();
        if self.nav == Nav::History && done.is_empty() && !delete_failed {
            self.status = format!("{} Grok sessions", self.grok_sessions.len());
        }
        if self.grok_sessions_refresh_pending && self.grok_sessions_inflight == 0 {
            self.request_grok_sessions_refresh();
        }
    }

    /// Keep the open id on a continuing follow-up. Fork and a fresh session/new
    /// replace it, and History must keep that new id instead of retiring it.
    fn bind_reported_grok_session(
        &mut self,
        idx: usize,
        reported: &str,
        allow_fresh: bool,
        clear_fork: bool,
    ) -> Option<String> {
        let open = self.threads.get(idx).and_then(|t| t.grok_session.clone());
        let fork = self.threads.get(idx).map(|t| t.grok_fork).unwrap_or(false);
        let adopt = threads::adopt_reported_session(open.as_deref(), reported, fork, !allow_fresh);
        let reported = reported.trim();
        let unbound = open
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_none();
        if let Some(t) = self.threads.get_mut(idx) {
            if (adopt || unbound) && !reported.is_empty() {
                t.grok_session = Some(reported.to_string());
            }
            if clear_fork {
                t.grok_fork = false;
            }
        }
        if adopt {
            Some(reported.to_string())
        } else {
            open
        }
    }

    /// Keep one History row for the open chat. A follow-up id is retired, not appended.
    pub(super) fn record_prompt_history(
        &mut self,
        idx: usize,
        open_session: Option<&str>,
        reported: &str,
        title: &str,
        cwd: Option<std::path::PathBuf>,
    ) {
        let plan = threads::prompt_history(
            &self
                .grok_sessions
                .iter()
                .map(|s| s.id.clone())
                .collect::<Vec<_>>(),
            open_session,
            reported,
        );
        if let Some(id) = plan.retire.clone() {
            if let Some(t) = self.threads.get_mut(idx) {
                if t.grok_session.as_deref() != Some(id.as_str())
                    && !t.retired_sessions.iter().any(|s| s == &id)
                {
                    t.retired_sessions.push(id);
                }
            }
        }
        let retired: Vec<String> = self
            .threads
            .iter()
            .flat_map(|t| t.retired_sessions.iter().cloned())
            .collect();
        self.grok_sessions.retain(|s| {
            plan.channels.iter().any(|id| id == &s.id) && !retired.iter().any(|id| id == &s.id)
        });
        self.note_live_grok_session(&plan.keep, title, cwd);
    }

    pub(super) fn note_live_grok_session(
        &mut self,
        id: &str,
        title: &str,
        cwd: Option<std::path::PathBuf>,
    ) {
        let id = id.trim();
        if id.is_empty() || threads::session_is_retired(&self.threads, id) {
            return;
        }
        if let Some(s) = self.grok_sessions.iter_mut().find(|s| s.id == id) {
            if !title.is_empty()
                && (s.title.is_empty()
                    || s.title == s.id
                    || grokhub_acp::is_placeholder_session_title(&s.title))
            {
                s.title = title.to_string();
            }
            if s.cwd.is_none() {
                s.cwd = cwd;
            }
            return;
        }
        self.grok_sessions.insert(
            0,
            grokhub_acp::GrokSession {
                id: id.to_string(),
                title: if title.trim().is_empty() {
                    id.to_string()
                } else {
                    title.to_string()
                },
                path: None,
                cwd,
                cabin: false,
            },
        );
    }

    pub(super) fn request_grok_sessions_refresh(&mut self) {
        self.grok_sessions_loaded = false;
        if self.grok_sessions_inflight > 0 {
            self.grok_list_gen = self.grok_list_gen.wrapping_add(1);
            self.grok_sessions_refresh_pending = true;
            return;
        }
        self.reload_grok_sessions();
    }

    pub(super) fn reload_grok_catalog(&mut self) {
        if self.grok_catalog_rx.is_some() {
            return;
        }
        let Some(bin) = grokhub_acp::find_grok() else {
            self.status = build_agent::grok_banner();
            self.grok_catalog_loaded = true;
            self.grok_catalog_started = None;
            return;
        };
        let cwd = self.grok_cwd();
        let (tx, rx) = mpsc::channel();
        self.grok_catalog_rx = Some(rx);
        self.grok_catalog_started = Some(Instant::now());
        self.status = "Loading Grok Build catalog…".into();
        std::thread::spawn(move || {
            let _ = tx.send(grokhub_acp::load_grok_catalog(&bin, &cwd));
        });
    }

    pub(super) fn poll_grok_catalog(&mut self) {
        let Some(rx) = self.grok_catalog_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(cat)) => {
                self.grok_catalog = cat;
                self.grok_catalog_loaded = true;
                self.grok_catalog_started = None;
                self.status = format!(
                    "{} skills · {} MCP · {} hooks · {} plugins · {} workflows",
                    self.grok_catalog.skills.len(),
                    self.grok_catalog.mcp.len(),
                    self.grok_catalog.hooks.len(),
                    self.grok_catalog.plugins.len(),
                    self.grok_catalog.workflows.len()
                );
            }
            Ok(Err(e)) => {
                self.grok_catalog_loaded = true;
                self.grok_catalog_started = None;
                self.status = e;
            }
            Err(mpsc::TryRecvError::Empty) => {
                let started = *self.grok_catalog_started.get_or_insert_with(Instant::now);
                if started.elapsed() >= grok_catalog_settle() {
                    self.grok_catalog_loaded = true;
                    self.grok_catalog_started = None;
                    self.status = GROK_CATALOG_TIMEOUT.into();
                } else {
                    self.grok_catalog_rx = Some(rx);
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.grok_catalog_loaded = true;
                self.grok_catalog_started = None;
            }
        }
    }

    pub(super) fn submit_mcp_line(&mut self, line: &str) {
        let t = line.trim();
        if t.is_empty() {
            return;
        }
        let lower = t.to_ascii_lowercase();
        if let Some(name) = lower
            .strip_prefix("remove ")
            .or_else(|| lower.strip_prefix("rm "))
        {
            let name = name.trim();
            if !name.is_empty() {
                self.run_grok_user_cmd(vec!["mcp".into(), "remove".into(), name.to_string()]);
            }
            return;
        }
        let mut parts = t.split_whitespace();
        let Some(name) = parts.next() else {
            return;
        };
        let rest: Vec<String> = parts.map(|s| s.to_string()).collect();
        let mut args = vec![
            "mcp".into(),
            "add".into(),
            "--scope".into(),
            "user".into(),
            name.to_string(),
        ];
        if !rest.is_empty() {
            args.push("--".into());
            args.extend(rest);
        }
        self.run_grok_user_cmd(args);
    }

    pub(super) fn run_grok_user_cmd(&mut self, args: Vec<String>) {
        if self.grok_ext_rx.is_some() {
            let line = format!("grok {}", args.join(" "));
            self.grok_ext_q.push(args);
            self.connector_note = format!("Queued {line}");
            return;
        }
        self.spawn_grok_user_cmd(args);
    }

    fn spawn_grok_user_cmd(&mut self, args: Vec<String>) {
        let Some(bin) = grokhub_acp::find_grok() else {
            self.status = build_agent::grok_banner();
            self.connector_note = build_agent::grok_banner();
            return;
        };
        let cwd = self.grok_cwd();
        let (tx, rx) = mpsc::channel();
        self.grok_ext_rx = Some(rx);
        let shown = format!("grok {}", args.join(" "));
        self.status = shown.clone();
        self.connector_note = shown;
        std::thread::spawn(move || {
            let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let text = grokhub_acp::grok_user_stdout_timeout(&bin, &cwd, &refs, 120)
                .unwrap_or_else(|e| e);
            let _ = tx.send(text);
        });
    }

    pub(super) fn poll_grok_ext(&mut self) {
        let Some(rx) = self.grok_ext_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(text) => {
                self.connector_note = text.clone();
                let clip: String = text.chars().take(160).collect();
                if !clip.is_empty() {
                    self.status = clip;
                }
                self.reload_grok_catalog();
                if !self.grok_ext_q.is_empty() {
                    let next = self.grok_ext_q.remove(0);
                    self.spawn_grok_user_cmd(next);
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.grok_ext_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    pub(super) fn run_mcp_doctor(&mut self) {
        if self.mcp_doctor_rx.is_some() {
            return;
        }
        let Some(bin) = grokhub_acp::find_grok() else {
            self.status = build_agent::grok_banner();
            self.connector_note = build_agent::grok_banner();
            return;
        };
        let cwd = self.grok_cwd();
        let (tx, rx) = mpsc::channel();
        self.mcp_doctor_rx = Some(rx);
        let shown = "grok mcp doctor --json".to_string();
        self.status = shown.clone();
        self.connector_note = shown;
        std::thread::spawn(move || {
            let text = grokhub_acp::grok_user_stdout_allow_fail(
                &bin,
                &cwd,
                &["mcp", "doctor", "--json"],
                120,
            )
            .unwrap_or_else(|e| e);
            let parsed = grokhub_acp::parse_mcp_doctor(&text);
            let _ = tx.send((text, parsed));
        });
    }

    pub(super) fn apply_mcp_doctor_result(
        &mut self,
        text: String,
        parsed: Option<McpStatusMap>,
    ) {
        self.connector_note = text.clone();
        let clip: String = text.chars().take(160).collect();
        if !clip.is_empty() {
            self.status = clip;
        }
        if let Some(map) = parsed {
            self.mcp_status = map;
        }
    }

    pub(super) fn poll_mcp_doctor(&mut self) {
        let Some(rx) = self.mcp_doctor_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok((text, parsed)) => {
                self.apply_mcp_doctor_result(text, parsed);
                self.reload_grok_catalog();
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.mcp_doctor_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    pub(super) fn poll_inspect(&mut self) {
        let Some(rx) = self.inspect_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(text) => {
                self.inspect_text = text.clone();
                if self.nav == Nav::Chat {
                    self.live_mut()
                        .push(("assistant".into(), mark_slash_result(&text)));
                    self.stamp_current_access();
                    self.persist_idle_key = self.persist_idle_now();
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.inspect_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    pub(super) fn poll_session_show(&mut self) {
        let Some((id, rx)) = self.session_show_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(text) => {
                if text.trim().is_empty() {
                    return;
                }
                let msgs = grokhub_acp::parse_session_markdown(&text);
                if msgs.is_empty() {
                    return;
                }
                let Some(i) = self
                    .threads
                    .iter()
                    .position(|t| t.grok_session.as_deref() == Some(id.as_str()))
                else {
                    return;
                };
                let filled = Arc::new(msgs);
                if let Some(t) = self.threads.get_mut(i) {
                    if t.messages.is_empty() {
                        t.messages = filled.clone();
                        t.grok_show_pending = false;
                    }
                }
                if i == self.thread_idx && self.messages.is_empty() {
                    self.messages = filled;
                    self.pin_chat_tail();
                }
                self.persist_bg();
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.session_show_rx = Some((id, rx));
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    pub(super) fn open_grok_session(&mut self, id: &str) {
        if let Some(i) = self
            .threads
            .iter()
            .position(|t| t.grok_session.as_deref() == Some(id))
        {
            if self.threads[i].messages.is_empty() {
                self.threads[i].grok_show_pending = true;
            }
            self.kick_session_show(id);
            self.switch_thread(i);
            self.nav = Nav::Chat;
            return;
        }
        let sess = self.grok_sessions.iter().find(|s| s.id == id).cloned();
        let title = sess
            .as_ref()
            .map(|s| s.title.clone())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| id.chars().take(24).collect());
        let mut t = ChatThread::new(&title, false);
        t.grok_session = Some(id.to_string());
        t.grok_user_home = sess.as_ref().is_none_or(|s| !s.cabin);
        t.grok_cwd = sess
            .as_ref()
            .and_then(|s| s.cwd.as_ref())
            .map(|p| p.display().to_string())
            .filter(|s| !s.is_empty());
        t.accessed_ms = now_ms();
        t.grok_show_pending = true;
        self.threads.push(t);
        self.kick_session_show(id);
        self.apply_switch_thread(self.threads.len() - 1);
        self.acp = None;
        self.nav = Nav::Chat;
        self.status = format!("Opened {title}");
        self.persist();
    }

    /// Fetch a Grok transcript into an empty cabin thread. Pin and rename can bind the
    /// session before it is opened; that empty row is not the transcript.
    pub(super) fn kick_session_show(&mut self, id: &str) {
        let id = id.trim();
        if id.is_empty() {
            return;
        }
        let empty = match self.thread_for_grok(id) {
            Some(i) if i == self.thread_idx => self.messages.is_empty(),
            Some(i) => self.threads.get(i).is_some_and(|t| t.messages.is_empty()),
            None => true,
        };
        if !empty {
            return;
        }
        if self
            .session_show_rx
            .as_ref()
            .is_some_and(|(sid, _)| sid == id)
        {
            return;
        }
        let sess = self.grok_sessions.iter().find(|s| s.id == id).cloned();
        let path = sess.as_ref().and_then(|s| s.path.clone());
        let cwd = sess
            .as_ref()
            .and_then(|s| s.cwd.clone())
            .or_else(|| {
                self.thread_for_grok(id).and_then(|i| {
                    self.threads.get(i).and_then(|t| {
                        t.grok_cwd
                            .as_ref()
                            .filter(|s| !s.is_empty())
                            .map(|s| std::path::PathBuf::from(s.as_str()))
                    })
                })
            })
            .unwrap_or_else(|| self.grok_cwd());
        let sid = id.to_string();
        let (tx, rx) = mpsc::channel();
        self.session_show_rx = Some((sid.clone(), rx));
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(path) = path {
                text = config::read_file_capped(&path, config::MEMORY_FILE_CAP);
            }
            if text.trim().is_empty() {
                if let Some(bin) = grokhub_acp::find_grok() {
                    text = grokhub_acp::show_session(&bin, &cwd, &sid).unwrap_or_default();
                }
            }
            let _ = tx.send(text);
        });
    }
}
