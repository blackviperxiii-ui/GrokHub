//! Composer send and the grok -p / ACP kick.

use super::*;
use grokhub_core::{live_send, LiveSend};

impl Cabin {

    pub(super) fn send_chat(&mut self, text: String) {
        // Alt+Enter or `/queue`: wait for the live reply instead of steering it.
        let queue_asked = std::mem::take(&mut self.bg.queue_next);
        self.workflow_status_live = false;
        self.turn_retried = false;
        let mut text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        if self.running && self.job_is_idea_talk() {
            self.status = "The idea talk is still going".into();
            return;
        }
        if let Some(slash) = parse_slash(&text) {
            self.run_slash(slash);
            return;
        }
        if unknown_cabin_slash(&text) {
            self.status = "Unknown command — /help".into();
            return;
        }
        if self.try_diagnose_intent(&text) {
            return;
        }
        self.begin_turn_origin();
        let thread_native = self
            .threads
            .get(self.thread_idx)
            .is_some_and(|thread| thread.native);
        if self.apply_unparsed_native_slash(&text) {
            return;
        }
        if let Some(recipe) =
            grokhub_agent::native_deep_research_prompt(self.cfg.native_engine, thread_native, &text)
        {
            text = recipe;
        }
        if btw_queues_without_interrupt(self.session_mode == SessionMode::Ask, self.running) {
            self.side_ask_queue.push(text);
            self.status = format!(
                "btw queued ({}) — main run continues",
                self.side_ask_queue.len()
            );
            return;
        }
        let mut steer = None;
        match chat_send_kind(
            self.chat_job_thread.as_deref(),
            &self.visible_thread_id(),
            self.running,
        ) {
            ChatSendKind::Redirect => {
                if queue_asked || self.acp.is_some() || self.grok_p_rx.is_some() {
                    // Steer this chat's own reply. A cabin-wide Grok command
                    // (`/compact`), a `/loop` on the ACP session, or Grok's own
                    // background tasks on the turn still queue.
                    match live_send(
                        queue_asked || !self.can_steer_live_turn(),
                        self.background_tasks_open(),
                    ) {
                        LiveSend::Queue => {
                            self.followup_queue.push(text);
                            self.status = format!("Queued ({})", self.followup_queue.len());
                            return;
                        }
                        LiveSend::Steer => {
                            if self.native_engine_for_current() {
                                self.steer_native_live(text);
                                return;
                            }
                            steer = Some(self.stop_turn_for_steer());
                        }
                    }
                } else {
                    let prev =
                        last_user_scan(self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())))
                            .unwrap_or_default();
                    self.halt_work("Redirected");
                    text = redirect_prompt(&prev, &text);
                }
            }
            ChatSendKind::Fresh => {
                if self.running && self.chat_job_thread.is_some() {
                    if queue_asked || self.acp.is_some() || self.background_tasks_open() {
                        self.followup_queue.push(text);
                        self.status = format!("Queued ({})", self.followup_queue.len());
                        return;
                    }
                    // The other chat's reply keeps going in the background
                    // instead of being cut off by this send.
                    if !self.move_turn_to_background() {
                        self.halt_in_flight();
                        self.finish_hub_dispatch("Interrupted", false);
                    }
                }
            }
        }
        self.touch();
        remember_typed_prompt(
            &mut self.chip_memory,
            &text,
            now_ms(),
            Self::local_clock().hour as u8,
        );
        remember_home_surface(&mut self.chip_memory, "chat", now_ms());
        if !persist_user_turn(self.can_agent() || (self.cfg.native_engine && self.scheduled_perm)) {
            self.hands_attach = false;
            self.eyes_attach = false;
            self.speak_next = false;
            self.status = "Install Grok Build (x.ai/cli) or Connect Grok in Settings".into();
            return;
        }
        if let Some(name) = self.attach_name.clone() {
            if !name.trim().is_empty() {
                let kind = self.attach_kind.unwrap_or(AttachKind::Image);
                let path = self.attach_path.clone().unwrap_or_default();
                let line = if kind == AttachKind::Image {
                    attach_prompt_line(AttachKind::Image, &name)
                } else {
                    attach_send_line(kind, &name, &path)
                };
                text = append_composer(&text, &line);
            }
        }
        self.verify_ok_turn = verify_ok_after_user_turn(self.verify_ok_turn, true);
        self.active_skill_follow = None;
        // Notes typed on a workboard card linked to this chat go with this turn,
        // once per change. The chat pane shows your message, not the block.
        let notes_thread = self.visible_thread_id();
        self.card_notes_follow =
            grokhub_core::take_card_notes_block(&mut self.board, &notes_thread);
        if self.card_notes_follow.is_some() {
            self.flush_board();
        }
        // Background results that landed since this chat's last turn, and the
        // progress of a turn this message just steered.
        self.bg.results_follow = if self.scheduled_perm {
            None
        } else {
            self.take_bg_results_follow(&notes_thread)
        };
        self.bg.steer_follow = steer;
        let matched = match_skill(&text, &self.skill_list).map(|sk| {
            (
                sk.name.clone(),
                self.policy().injects_skill().then(|| skill_follow_block(sk)),
            )
        });
        if let Some((skill_ran, follow)) = matched {
            self.skill_name = skill_ran.clone();
            self.engine_note("skills", &format!("ran:{skill_ran}"), &skill_ran);
            self.status = format!("Skill {skill_ran}");
            if let Some(follow) = follow {
                self.active_skill_follow = Some(follow);
            }
        }
        self.eyes_attach = false;
        self.hands_attach = false;
        self.followup_step = 0;
        if self.scheduled_perm {
            let idx = self.ensure_background_history_thread();
            if let Some(t) = self.threads.get(idx) {
                self.chat_job_thread = Some(t.id.clone());
            }
        }
        let hidden = self.scheduled_perm
            && self
                .chat_job_thread
                .as_deref()
                .is_some_and(|id| id != self.visible_thread_id());
        if hidden {
            self.push_bound_msg("user", text.clone());
        } else {
            self.live_mut().push(("user".into(), text.clone()));
        }
        self.stamp_current_access();
        self.persist();
        self.kick_model(true);
    }

    /// Night, anticipate, and `/send` tasks enqueue through `send_chat` so they
    /// share the composer PermissionMode pill — not a separate always-yolo path.
    /// `scheduled_perm` makes `kick_model` skip ACP and honor `scheduled_flags`
    /// / `scheduled_args` (Ask is fail-closed, no `--always-approve`).
    pub(super) fn send_scheduled_chat(&mut self, text: String) {
        // kick_model honors scheduled_flags / scheduled_args while scheduled_perm.
        self.scheduled_perm = true;
        let _ = self.permission_mode.scheduled_args();
        let _ = self.permission_mode.scheduled_flags();
        self.send_chat(text);
        if !self.running && self.pending_kick.is_none() {
            self.scheduled_perm = false;
        }
    }

    pub(super) fn kick_model(&mut self, consume_attach: bool) {
        if !self.can_agent() && !(self.cfg.native_engine && self.scheduled_perm) {
            self.running = false;
            self.chat_job_thread = None;
            self.status = "Install Grok Build (x.ai/cli) or Connect Grok in Settings".into();
            return;
        }
        if !self.kick_skip
            && self.kick_frame.is_none()
            && (self.kick_cap_rx.is_some()
                || should_capture_before_chat(self.eyes_attach || self.hands_attach))
        {
            match self.poll_cabin_frame() {
                CabinFrame::Pending => {
                    self.pending_kick = Some(consume_attach);
                    if self.chat_job_thread.is_none() {
                        self.chat_job_thread = Some(self.visible_thread_id());
                    }
                    self.running = true;
                    if self.chrome_here() {
                        self.status = "Capturing…".into();
                    }
                    return;
                }
                CabinFrame::Ready(url) => {
                    self.kick_frame = Some(url);
                }
                CabinFrame::Skip => {}
            }
        }
        if self.verify_rx.is_some() {
            self.pending_kick = Some(consume_attach);
            if self.chat_job_thread.is_none() {
                self.chat_job_thread = Some(self.visible_thread_id());
            }
            self.running = true;
            if self.chrome_here() {
                self.status = "Verifying…".into();
            }
            return;
        }
        self.running = true;
        if self
            .chat_job_thread
            .as_deref()
            .is_none_or(|id| id == self.visible_thread_id())
        {
            self.status = "Thinking…".into();
        }
        if self.chat_job_thread.is_none() {
            self.chat_job_thread = Some(self.visible_thread_id());
        }
        let vis = self.visible_thread_id();
        let thread_label = self
            .threads
            .iter()
            .find(|t| self.chat_job_thread.as_deref() == Some(t.id.as_str()))
            .map(|t| t.title.clone())
            .unwrap_or_default();
        let raw_ask = {
            let job = self.chat_job_thread.as_deref();
            if job.is_none() || job == Some(vis.as_str()) {
                self.messages
                    .iter()
                    .rev()
                    .find(|m| m.0 == "user" && !is_workload_user(&m.1))
                    .map(|m| m.1.clone())
                    .unwrap_or_default()
            } else {
                self.threads
                    .iter()
                    .find(|t| Some(t.id.as_str()) == job)
                    .and_then(|t| {
                        t.messages
                            .iter()
                            .rev()
                            .find(|(role, content)| role == "user" && !is_workload_user(content))
                            .map(|(_, content)| content.clone())
                    })
                    .unwrap_or_else(|| {
                        self.messages
                            .iter()
                            .rev()
                            .find(|m| m.0 == "user" && !is_workload_user(&m.1))
                            .map(|m| m.1.clone())
                            .unwrap_or_default()
                    })
            }
        };
        let with_notes = apply_skill_follow(&raw_ask, self.card_notes_follow.as_deref());
        let with_bg = apply_skill_follow(&with_notes, self.bg.results_follow.as_deref());
        let with_steer = apply_skill_follow(&with_bg, self.bg.steer_follow.as_deref());
        let with_skill = apply_skill_follow(&with_steer, self.active_skill_follow.as_deref());
        let last_user = apply_skill_follow(&with_skill, self.idea_talk_brief().as_deref());
        if self.grok_p_rx.is_some() {
            return;
        }
        if self.acp_spawn_rx.is_some() {
            self.pending_kick = Some(consume_attach);
            return;
        }
        self.drop_stale_native_handle();
        if !self.scheduled_perm && self.permission_mode.uses_acp() && self.acp.is_none() {
            if let Err(e) = self.ensure_acp() {
                self.fail_ask_without_acp(&e);
                return;
            }
            if self.acp.is_none() {
                self.pending_kick = Some(consume_attach);
                return;
            }
        }
        let cabin = self.kick_frame.take();
        self.kick_skip = false;
        self.eyes_attach = false;
        self.hands_attach = false;
        self.stream_buf.clear();
        self.thought_buf.clear();
        self.turn_log.clear();
        self.thought_seam = false;
        self.say_seam = false;
        if self.stream_here() {
            self.tool_cards.clear();
            self.live_blocks.clear();
        }
        self.perm_ask = None;
        self.perm_queue.clear();
        self.perm_always_confirm = None;
        self.confirm = None;
        self.elicit_ask = None;
        self.elicit_draft.clear();
        let image = if consume_attach {
            let url =
                next_chat_image(self.attach_url.as_deref(), cabin.as_deref()).map(str::to_string);
            self.attach_url = None;
            self.attach_name = None;
            self.attach_path = None;
            self.attach_kind = None;
            url
        } else {
            None
        };
        if self.kick_native_turn(&last_user, image.as_deref(), &raw_ask, &thread_label) {
            return;
        }
        if !self.scheduled_perm && self.permission_mode.uses_acp() {
            self.side_ask_kick = false;
            let prompt_err = self
                .acp
                .as_ref()
                .map(|h| h.prompt_with_image(&last_user, image.as_deref()));
            match prompt_err {
                Some(Ok(())) => self.note_inflight_card(&raw_ask, &thread_label),
                Some(Err(e)) => {
                    self.acp = None;
                    self.fail_ask_without_acp(&e);
                }
                None => self.fail_ask_without_acp(""),
            }
            return;
        }
        let idx = self
            .chat_job_thread
            .as_deref()
            .and_then(|id| self.threads.iter().position(|t| t.id == id))
            .unwrap_or(self.thread_idx);
        let cwd = self
            .threads
            .get(idx)
            .and_then(|t| t.grok_cwd.clone())
            .filter(|s| !s.trim().is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| self.grok_cwd());
        // A follow-up resumes the open chat. Dropping --resume here started a
        // new Grok session and History gained a row per prompt.
        let resume = self.threads.get(idx).and_then(|t| {
            t.grok_session.clone().filter(|s| !s.trim().is_empty())
        });
        let (yolo, auto) = if self.scheduled_perm {
            self.permission_mode.scheduled_flags()
        } else {
            self.permission_mode.composer_headless_flags()
        };
        let model = grokhub_core::cabin_spawn_model(&self.cfg.model).to_string();
        // Automations, loops, and /send tasks run unwatched: always low effort.
        let effort = if self.scheduled_perm {
            Some(grokhub_core::BACKGROUND_EFFORT)
        } else {
            grokhub_core::parse_reasoning_effort(&self.cfg.reasoning_effort)
        };
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
        let fork = self.threads.get(idx).map(|t| t.grok_fork).unwrap_or(false);
        let mode = if self.side_ask_kick {
            SessionMode::Ask
        } else {
            self.session_mode
        };
        self.side_ask_kick = false;
        let worktree = self
            .threads
            .get(idx)
            .map(|t| t.grok_worktree)
            .unwrap_or(false);
        if self.cfg.native_engine && self.scheduled_perm {
            self.start_native_scheduled(
                &last_user,
                cwd,
                &model,
                effort.map(str::to_string),
                mode,
                image,
            );
            if self.grok_p_rx.is_some() {
                self.note_inflight_card(&raw_ask, &thread_label);
            }
            return;
        }
        match grokhub_acp::spawn_grok_p_stream(
            &last_user,
            &cwd,
            resume.as_deref(),
            yolo,
            auto,
            Some(model.as_str()),
            effort,
            mode,
            grokhub_acp::GrokPAttach {
                image: image.as_deref(),
                learned: &grokhub_core::brief_for(&self.learning, "chat"),
                deny: self.permission_mode.needs_approval(),
                desktop: self.cfg.desktop_control,
                hard_deny: grokhub_agent::harness::HEADLESS_DENY_RULES,
            },
            fork,
            user_home,
            worktree,
        ) {
            Ok((pid, rx)) => {
                self.grok_p_pid = Some(pid);
                self.grok_p_rx = Some(rx);
                if let Some(t) = self.threads.get_mut(idx) {
                    t.grok_user_home = user_home;
                }
                self.note_inflight_card(&raw_ask, &thread_label);
            }
            Err(e) => {
                self.abandon_turn_card();
                self.running = false;
                self.scheduled_perm = false;
                self.status = self.apply_job_fail(&e);
                self.chat_job_thread = None;
            }
        }
    }

    pub(super) fn kick_model_retry(&mut self, t: String) {
        self.try_again = false;
        // A retry re-runs the same ask: keep its steer and background notes,
        // which halting clears and `send_chat` will not set again.
        let steer = self.bg.steer_follow.take();
        let results = self.bg.results_follow.take();
        self.halt_in_flight();
        self.bg.steer_follow = steer;
        self.bg.results_follow = results;
        self.active_skill_follow = None;
        if let Some(sk) = match_skill(&t, &self.skill_list) {
            if self.policy().injects_skill() {
                self.active_skill_follow = Some(skill_follow_block(sk));
            }
        }
        self.kick_model(true);
    }

    pub(super) fn poll_pending_kick(&mut self) {
        let Some(consume) = self.pending_kick else {
            return;
        };
        if self.acp_spawn_rx.is_some() || self.grok_p_rx.is_some() {
            return;
        }
        if self.kick_frame.is_some()
            || (self.kick_cap_rx.is_none()
                && !should_capture_before_chat(self.eyes_attach || self.hands_attach))
        {
            self.pending_kick = None;
            self.kick_model(consume);
            return;
        }
        match self.poll_cabin_frame() {
            CabinFrame::Pending => {}
            CabinFrame::Ready(url) => {
                self.kick_frame = Some(url);
                self.pending_kick = None;
                self.kick_model(consume);
            }
            CabinFrame::Skip => {
                self.pending_kick = None;
                self.kick_model(consume);
            }
        }
    }
}
