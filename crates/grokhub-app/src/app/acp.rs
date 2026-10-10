//! The native engine's event stream: the live chat handle and one-shot
//! unattended receivers.

use super::*;

impl Cabin {

    pub(super) fn fail_turn_start(&mut self, detail: &str) {
        self.abandon_turn_card();
        self.running = false;
        self.pending_kick = None;
        self.scheduled_perm = false;
        self.status = self.apply_job_fail(detail);
        self.chat_job_thread = None;
        self.persist();
        self.maybe_continue_ptt();
    }

    pub(super) fn ensure_acp(&mut self) -> Result<(), String> {
        self.ensure_native_engine()
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
                    if card.status == "completed" {
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
                    let watched = card.clone();
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
                    // Path D: a stopped turn's later events find `running` off.
                    self.harness_watch_cu(&watched, true);
                }
                AcpEvent::Findings(body) => self.harness.pending_findings = Some(body),
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
                    self.episode_turn_done(&stop_reason);
                    let findings = self.harness.pending_findings.take();
                    if stop_reason.eq_ignore_ascii_case("cancelled") || !self.running {
                        continue;
                    }
                    let origin = self.chat_job_thread.clone();
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
                    let turn = self.turn_no();
                    self.harness_watch_end();
                    self.finish_acp_turn(text);
                    if let Some(body) = findings {
                        self.post_findings(origin, &body);
                    }
                    self.harness_turn_end_last_reply(turn);
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
                    if grokhub_core::proc_util::is_sigterm_status(&e) && !self.turn_retried {
                        self.turn_retried = true;
                        self.status = "Retrying…".into();
                        self.kick_model(false);
                        continue;
                    }
                    if grokhub_core::proc_util::is_sigterm_status(&e) {
                        self.post_turn_crash();
                    }
                    self.scheduled_perm = false;
                    self.status = self.apply_job_fail(&e);
                    self.abandon_turn_card();
                    self.chat_job_thread = None;
                    self.persist();
                    self.maybe_continue_ptt();
                }
            }
        }
    }

    /// Put a new Ask on the card, or queue it behind the one already there. A
    /// parallel tool call can ask while a card is up; cancelling either would read
    /// to Grok as "User cancelled".
    pub(super) fn show_perm_ask(&mut self, p: grokhub_core::wire::PermissionAsk) {
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
        // The audit reads the visible chat's spans, so only a reply on it is kept.
        self.harness.last_reply = here.then(|| prose.clone());
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
                let watched = card.clone();
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
                // Path D: a stopped turn keeps no stream to read.
                if !self.harness_watch_cu(&watched, false) {
                    self.grok_p_rx = Some(rx);
                }
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
                let turn_no = self.turn_no();
                self.apply_single_turn(turn);
                self.harness_watch_end();
                self.harness_headless_end();
                self.harness_turn_end_last_reply(turn_no);
                self.drain_followup_queue();
            }
            Ok(GrokPEvent::Err(e)) => {
                self.running = false;
                self.pending_kick = None;
                let paints = self.stream_here();
                if grokhub_core::proc_util::is_sigterm_status(&e) {
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
                        self.post_turn_crash();
                        self.abandon_turn_card();
                        self.chat_job_thread = None;
                        self.persist();
                    }
                } else {
                    self.scheduled_perm = false;
                    let status = self.apply_job_fail(&rewrite_truncation_error(&e));
                    self.finish_hub_dispatch(&status, false);
                    if paints || self.chat_job_thread.is_none() {
                        self.status = status;
                    }
                    self.abandon_turn_card();
                    self.chat_job_thread = None;
                    self.persist();
                }
                self.maybe_continue_ptt();
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.grok_p_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
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
                    let status = self.apply_job_fail("The engine stopped without a reply");
                    self.finish_hub_dispatch(&status, false);
                    if paints || self.chat_job_thread.is_none() {
                        self.status = status;
                    }
                    self.abandon_turn_card();
                    self.chat_job_thread = None;
                    self.persist();
                }
            }
        }
    }

    pub(super) fn apply_single_turn(&mut self, turn: grokhub_core::wire::SingleTurn) {
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

    /// A typed CLI slash (`/rewind`, `/workflow …`) goes to the native engine as a turn.
    pub(super) fn send_grok_slash(&mut self, cmd: &str) {
        if self.acp.is_none() {
            if let Err(e) = self.ensure_acp() {
                self.fail_turn_start(&e);
                return;
            }
        }
        let sent = self.acp.as_ref().map(|h| h.prompt(cmd));
        match sent {
            Some(Ok(())) => self.running = true,
            Some(Err(e)) => {
                self.acp = None;
                self.fail_turn_start(&e);
            }
            None => self.fail_turn_start("native engine is not running"),
        }
    }

    pub(super) fn apply_grok_commands(&mut self, cmds: Vec<String>) {
        self.grok_commands = grok_command_hits(&cmds);
    }

    /// Background subagents emit cards after the parent turn has returned.
    pub(super) fn poll_native_side_events(&mut self) {
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

    pub(super) fn drain_followup_queue(&mut self) {
        if self.running {
            return;
        }
        if let Some(next) = self.side_ask_queue.first().cloned() {
            self.side_ask_queue.remove(0);
            self.send_queued_side_ask(next);
            return;
        }
        let Some(next) = self.followup_queue.first().cloned() else {
            self.kick_repair();
            return;
        };
        self.followup_queue.remove(0);
        self.harness_user_queued();
        self.send_chat(next);
    }

    /// Look-safe send after the live turn. Does not cancel that turn — it already ended.
    pub(super) fn send_queued_side_ask(&mut self, text: String) {
        self.side_ask_kick = true;
        self.send_chat(text);
        if !self.running && self.pending_kick.is_none() {
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
                    || grokhub_core::cli_history::is_placeholder_session_title(&s.title))
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
            grokhub_core::cli_history::GrokSession {
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

    pub(super) fn poll_session_show(&mut self) {
        let Some((id, rx)) = self.session_show_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(text) => {
                if text.trim().is_empty() {
                    return;
                }
                let msgs = grokhub_core::cli_history::parse_session_markdown(&text);
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

    /// Load a CLI-era transcript into an empty cabin thread. Pin and rename can bind the
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
        let sid = id.to_string();
        let (tx, rx) = mpsc::channel();
        self.session_show_rx = Some((sid.clone(), rx));
        // Read-only: the CLI's saved transcript file, never the CLI itself.
        std::thread::spawn(move || {
            let text = path
                .map(|path| config::read_file_capped(&path, config::MEMORY_FILE_CAP))
                .unwrap_or_default();
            let _ = tx.send(text);
        });
    }
}
